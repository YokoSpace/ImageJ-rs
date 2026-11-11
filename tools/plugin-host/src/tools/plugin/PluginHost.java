package tools.plugin;

import ij.plugin.PlugIn;
import ij.plugin.filter.ExtendedPlugInFilter;
import ij.plugin.filter.PlugInFilter;
import java.io.*;
import java.lang.reflect.InvocationTargetException;
import java.net.URL;
import java.net.URLClassLoader;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Paths;
import java.util.ArrayList;
import java.util.List;

/**
 * Protocol host runner for executing ImageJ 1.x plugins in an isolated child JVM.
 *
 * Implements narrow no-image PlugIn compatibility plus a narrow PlugInFilter
 * image path, informed by original_imagej/ij/IJ.java and
 * original_imagej/ij/plugin/filter/PlugInFilterRunner.java.
 *
 * Captures execution stdout/stderr and emits structured JSON reporting:
 * - success
 * - class_not_found
 * - exception_thrown
 * - unsupported_plugin_type (no-image filter, ExtendedPlugInFilter, ambiguous
 *   PlugIn+PlugInFilter, or a non-plugin class)
 * - unsupported_filter (image depth/stack/ROI/metadata/flags outside the
 *   supported subset; no output image is written)
 * - not_processed (the runner returned without calling run(ip), e.g. DONE or
 *   a failed image capability check; no output image is written and no pixel
 *   processing is claimed)
 * - io_error (the host could not read the input or publish the output)
 *
 * A single standard GenericDialog is exchanged as JSON via ParameterDialog.
 * Custom GUI, hyperstacks, preview/ExtendedPlugInFilter, CONVERT_TO_FLOAT and non-rectangular-ROI compatibility are explicitly not
 * claimed. Plain Z-stacks are processed by the unmodified ImageJ runner:
 * every slice with DOES_STACKS, otherwise only the current slice transferred
 * with --current-slice.
 *
 * Lifecycle & Sandboxing notice:
 * The plugin host provides process lifecycle management and JVM isolation for legacy ImageJ plugins,
 * terminating non-daemon background threads upon plugin return. It makes NO security claims of
 * sandboxing untrusted plugin code.
 */
public class PluginHost {

    private static volatile boolean completed = false;

    public static void main(String[] args) {
        String pluginCp = null;
        String className = null;
        String pluginArg = "";
        String resultFile = null;
        String runId = "";
        String commandName = "";
        String inputImage = null;
        String outputImage = null;
        String roiSpec = null;
        String currentSliceSpec = null;
        boolean inspect = false;
        String macroOptions = null;

        for (int i = 0; i < args.length; i++) {
            if ("--macro-options".equals(args[i]) && i + 1 < args.length) {
                macroOptions = args[++i];
            } else if ("--inspect".equals(args[i])) {
                inspect = true;
            } else if ("--plugin-cp".equals(args[i]) && i + 1 < args.length) {
                pluginCp = args[++i];
            } else if ("--class".equals(args[i]) && i + 1 < args.length) {
                className = args[++i];
            } else if ("--arg".equals(args[i]) && i + 1 < args.length) {
                pluginArg = args[++i];
            } else if ("--result-file".equals(args[i]) && i + 1 < args.length) {
                resultFile = args[++i];
            } else if ("--run-id".equals(args[i]) && i + 1 < args.length) {
                runId = args[++i];
            } else if ("--command".equals(args[i]) && i + 1 < args.length) {
                commandName = args[++i];
            } else if ("--input-image".equals(args[i]) && i + 1 < args.length) {
                inputImage = args[++i];
            } else if ("--output-image".equals(args[i]) && i + 1 < args.length) {
                outputImage = args[++i];
            } else if ("--roi".equals(args[i]) && i + 1 < args.length) {
                roiSpec = args[++i];
            } else if ("--current-slice".equals(args[i]) && i + 1 < args.length) {
                currentSliceSpec = args[++i];
            }
        }

        if (className == null) {
            System.err.println("Error: --class <className> is required.");
            System.exit(2);
        }

        // Prepare stdout / stderr interception
        ByteArrayOutputStream capturedOutStream = new ByteArrayOutputStream();
        ByteArrayOutputStream capturedErrStream = new ByteArrayOutputStream();
        PrintStream origOut = System.out;
        PrintStream origErr = System.err;

        PrintStream interceptedOut = new PrintStream(capturedOutStream, true, StandardCharsets.UTF_8);
        PrintStream interceptedErr = new PrintStream(capturedErrStream, true, StandardCharsets.UTF_8);

        System.setOut(interceptedOut);
        System.setErr(interceptedErr);

        // Register shutdown hook to flush captured diagnostics on abrupt termination (e.g. IJ.getImage() -> System.exit(0))
        final String finalRunId = runId;
        Runtime.getRuntime().addShutdownHook(new Thread(() -> {
            if (!completed) {
                try {
                    interceptedOut.flush();
                    interceptedErr.flush();
                    String outStr = capturedOutStream.toString(StandardCharsets.UTF_8);
                    String errStr = capturedErrStream.toString(StandardCharsets.UTF_8);
                    if (!outStr.isEmpty()) {
                        origErr.println("Plugin host process aborted prematurely before protocol completion. Intercepted stdout:\n" + outStr);
                    }
                    if (!errStr.isEmpty()) {
                        origErr.println("Intercepted stderr:\n" + errStr);
                    }
                    origErr.flush();
                } catch (Throwable ignored) {
                }
            }
        }, "plugin-host-shutdown-hook"));

        try {
            // Build class loader for plugin classpath entries
            ClassLoader loader = buildClassLoader(pluginCp);

            if (inspect) {
                inspectClass(loader, className, resultFile, runId, origOut,
                    capturedOutStream, capturedErrStream, interceptedOut, interceptedErr);
                completed = true;
                System.exit(0);
                return;
            }

            ParameterDialog.options = macroOptions;
            if (macroOptions != null) {
                Thread.currentThread().setName("Run$_plugin");
                ij.Macro.setOptions(macroOptions.isEmpty() ? " " : macroOptions);
            }
            Class<?> clazz;
            try {
                clazz = Class.forName(className, true, loader);
            } catch (ClassNotFoundException | NoClassDefFoundError e) {
                interceptedOut.flush();
                interceptedErr.flush();
                String outStr = capturedOutStream.toString(StandardCharsets.UTF_8);
                String errStr = capturedErrStream.toString(StandardCharsets.UTF_8);
                writeResult(resultFile, origOut, formatClassNotFoundJson(finalRunId, className, pluginArg, e.toString(), outStr, errStr));
                completed = true;
                System.exit(0);
                return;
            }

            boolean isPlugIn = PlugIn.class.isAssignableFrom(clazz);
            boolean isFilter = PlugInFilter.class.isAssignableFrom(clazz);

            if (isPlugIn && isFilter) {
                interceptedOut.flush();
                interceptedErr.flush();
                String outStr = capturedOutStream.toString(StandardCharsets.UTF_8);
                String errStr = capturedErrStream.toString(StandardCharsets.UTF_8);
                String msg = "Class '" + className + "' implements both ij.plugin.PlugIn and "
                    + "ij.plugin.filter.PlugInFilter. The narrow host refuses this ambiguous case "
                    + "because ImageJ's dispatch would depend on the invocation path. GUI and stack "
                    + "compatibility are not supported in this spike.";
                writeResult(resultFile, origOut, formatUnsupportedTypeJson(finalRunId, className, pluginArg, msg, outStr, errStr));
                completed = true;
                System.exit(0);
                return;
            }

            // Typed image/stack transfer with an optional rectangular ROI.
            if (isFilter) {
                if (ExtendedPlugInFilter.class.isAssignableFrom(clazz)) {
                    interceptedOut.flush();
                    interceptedErr.flush();
                    String outStr = capturedOutStream.toString(StandardCharsets.UTF_8);
                    String errStr = capturedErrStream.toString(StandardCharsets.UTF_8);
                    String msg = "Class '" + className + "' implements ExtendedPlugInFilter. "
                        + "Dialogs, preview and stack progress support are outside this spike; "
                        + "only plain ij.plugin.filter.PlugInFilter is supported.";
                    writeResult(resultFile, origOut, formatUnsupportedTypeJson(finalRunId, className, pluginArg, msg, outStr, errStr));
                    completed = true;
                    System.exit(0);
                    return;
                }
                if (inputImage == null) {
                    interceptedOut.flush();
                    interceptedErr.flush();
                    String outStr = capturedOutStream.toString(StandardCharsets.UTF_8);
                    String errStr = capturedErrStream.toString(StandardCharsets.UTF_8);
                    String msg = "Class '" + className + "' implements ij.plugin.filter.PlugInFilter. "
                        + "The narrow compatibility host requires --input-image; no-image "
                        + "PlugInFilter execution (NO_IMAGE_REQUIRED) is not supported. "
                        + "GUI and stack compatibility are not supported in this spike.";
                    writeResult(resultFile, origOut, formatUnsupportedTypeJson(finalRunId, className, pluginArg, msg, outStr, errStr));
                    completed = true;
                    System.exit(0);
                    return;
                }
                try {
                    FilterRunnerSupport.Result filterResult = FilterRunnerSupport.run(
                        clazz, commandName, pluginArg, inputImage, outputImage, roiSpec,
                        currentSliceSpec);
                    interceptedOut.flush();
                    interceptedErr.flush();
                    String outStr = capturedOutStream.toString(StandardCharsets.UTF_8);
                    String errStr = capturedErrStream.toString(StandardCharsets.UTF_8);
                    if (filterResult.processed()) {
                        writeResult(resultFile, origOut, formatFilterSuccessJson(
                            finalRunId, className, pluginArg, filterResult.flags,
                            filterResult.width, filterResult.height,
                            filterResult.runInvocations, filterResult.finalSetupCalled,
                            outStr, errStr));
                    } else {
                        writeResult(resultFile, origOut, formatNotProcessedJson(
                            finalRunId, className, pluginArg, filterResult.flags,
                            notProcessedMessage(filterResult), outStr, errStr));
                    }
                } catch (ParameterDialog.Required required) {
                    writeResult(resultFile, origOut, "{\"status\":\"dialog_required\",\"run_id\":"
                        + ParameterDialog.quote(runId) + ",\"class_name\":" + ParameterDialog.quote(className)
                        + ",\"arg\":" + ParameterDialog.quote(pluginArg)
                        + ",\"schema\":" + required.dialog.json() + "}");
                } catch (NoSuchMethodError | VerifyError e) {
                    writeResult(resultFile, origOut, formatUnsupportedFilterJson(
                        finalRunId, className, pluginArg,
                        "Unsupported GenericDialog API or custom UI: " + e.toString(), 0,
                        capturedOutStream.toString(StandardCharsets.UTF_8),
                        capturedErrStream.toString(StandardCharsets.UTF_8)));
                } catch (FilterIoException e) {
                    interceptedOut.flush();
                    interceptedErr.flush();
                    String outStr = capturedOutStream.toString(StandardCharsets.UTF_8);
                    String errStr = capturedErrStream.toString(StandardCharsets.UTF_8);
                    writeResult(resultFile, origOut, formatIoErrorJson(
                        finalRunId, className, pluginArg, e.getMessage(), outStr, errStr));
                } catch (UnsupportedFilterException e) {
                    interceptedOut.flush();
                    interceptedErr.flush();
                    String outStr = capturedOutStream.toString(StandardCharsets.UTF_8);
                    String errStr = capturedErrStream.toString(StandardCharsets.UTF_8);
                    writeResult(resultFile, origOut, formatUnsupportedFilterJson(
                        finalRunId, className, pluginArg, e.getMessage(), e.flags(), outStr, errStr));
                } catch (Throwable t) {
                    Throwable cause = unwrapThrowable(t);
                    interceptedOut.flush();
                    interceptedErr.flush();
                    String outStr = capturedOutStream.toString(StandardCharsets.UTF_8);
                    String errStr = capturedErrStream.toString(StandardCharsets.UTF_8);
                    writeResult(resultFile, origOut, formatExceptionThrownJson(
                        finalRunId, className, pluginArg, cause.getClass().getName(),
                        cause.getMessage() != null ? cause.getMessage() : cause.toString(),
                        getStackTraceString(cause), outStr, errStr
                    ));
                }
                completed = true;
                System.exit(0);
                return;
            }

            if (!isPlugIn) {
                interceptedOut.flush();
                interceptedErr.flush();
                String outStr = capturedOutStream.toString(StandardCharsets.UTF_8);
                String errStr = capturedErrStream.toString(StandardCharsets.UTF_8);
                String msg = "Class '" + className + "' does not implement ij.plugin.PlugIn "
                    + "or ij.plugin.filter.PlugInFilter.";
                writeResult(resultFile, origOut, formatUnsupportedTypeJson(finalRunId, className, pluginArg, msg, outStr, errStr));
                completed = true;
                System.exit(0);
                return;
            }

            if (inputImage != null) {
                interceptedOut.flush();
                interceptedErr.flush();
                String outStr = capturedOutStream.toString(StandardCharsets.UTF_8);
                String errStr = capturedErrStream.toString(StandardCharsets.UTF_8);
                String msg = "Class '" + className + "' implements ij.plugin.PlugIn and does not "
                    + "consume an image. Supplying --input-image is only supported for "
                    + "ij.plugin.filter.PlugInFilter.";
                writeResult(resultFile, origOut, formatUnsupportedTypeJson(finalRunId, className, pluginArg, msg, outStr, errStr));
                completed = true;
                System.exit(0);
                return;
            }

            // Instantiate plugin
            Object pluginInstance;
            try {
                pluginInstance = clazz.getDeclaredConstructor().newInstance();
            } catch (Throwable t) {
                Throwable cause = unwrapThrowable(t);
                interceptedOut.flush();
                interceptedErr.flush();
                String outStr = capturedOutStream.toString(StandardCharsets.UTF_8);
                String errStr = capturedErrStream.toString(StandardCharsets.UTF_8);
                writeResult(resultFile, origOut, formatExceptionThrownJson(
                    finalRunId, className, pluginArg, cause.getClass().getName(),
                    cause.getMessage() != null ? cause.getMessage() : cause.toString(),
                    getStackTraceString(cause), outStr, errStr
                ));
                completed = true;
                System.exit(0);
                return;
            }

            // Execute plugin.run(arg)
            PlugIn plugIn = (PlugIn) pluginInstance;
            try {
                plugIn.run(pluginArg);
            } catch (Throwable t) {
                Throwable cause = unwrapThrowable(t);
                interceptedOut.flush();
                interceptedErr.flush();
                String outStr = capturedOutStream.toString(StandardCharsets.UTF_8);
                String errStr = capturedErrStream.toString(StandardCharsets.UTF_8);
                writeResult(resultFile, origOut, formatExceptionThrownJson(
                    finalRunId, className, pluginArg, cause.getClass().getName(),
                    cause.getMessage() != null ? cause.getMessage() : cause.toString(),
                    getStackTraceString(cause), outStr, errStr
                ));
                completed = true;
                System.exit(0);
                return;
            }

            // Execution succeeded
            interceptedOut.flush();
            interceptedErr.flush();
            String outStr = capturedOutStream.toString(StandardCharsets.UTF_8);
            String errStr = capturedErrStream.toString(StandardCharsets.UTF_8);
            writeResult(resultFile, origOut, formatSuccessJson(finalRunId, className, pluginArg, outStr, errStr));
            completed = true;
            // Explicitly exit cleanly to terminate any non-daemon background threads spawned by plugin
            System.exit(0);

        } catch (Throwable t) {
            interceptedOut.flush();
            interceptedErr.flush();
            String outStr = capturedOutStream.toString(StandardCharsets.UTF_8);
            String errStr = capturedErrStream.toString(StandardCharsets.UTF_8);
            Throwable cause = unwrapThrowable(t);
            writeResult(resultFile, origOut, formatExceptionThrownJson(
                finalRunId, className, pluginArg, cause.getClass().getName(),
                cause.getMessage() != null ? cause.getMessage() : cause.toString(),
                getStackTraceString(cause), outStr, errStr
            ));
            completed = true;
            System.exit(0);
        } finally {
            System.setOut(origOut);
            System.setErr(origErr);
        }
    }

    /**
     * Read-only class inspection: load with initialization disabled, derive the
     * interface category with isAssignableFrom, and never instantiate or run the
     * plugin. Loading errors are reported as structured status objects.
     */
    private static void inspectClass(ClassLoader loader, String className, String resultFile,
            String runId, PrintStream origOut, ByteArrayOutputStream capturedOutStream,
            ByteArrayOutputStream capturedErrStream, PrintStream interceptedOut,
            PrintStream interceptedErr) {
        Class<?> clazz;
        try {
            clazz = Class.forName(className, false, loader);
        } catch (ClassNotFoundException e) {
            interceptedOut.flush();
            interceptedErr.flush();
            writeResult(resultFile, origOut, formatInspectionClassNotFoundJson(
                runId, className, e.toString(),
                capturedOutStream.toString(StandardCharsets.UTF_8),
                capturedErrStream.toString(StandardCharsets.UTF_8)));
            return;
        } catch (NoClassDefFoundError e) {
            interceptedOut.flush();
            interceptedErr.flush();
            writeResult(resultFile, origOut, formatInspectionLoadFailedJson(
                runId, className, e.getClass().getName(), e.toString(),
                capturedOutStream.toString(StandardCharsets.UTF_8),
                capturedErrStream.toString(StandardCharsets.UTF_8)));
            return;
        } catch (LinkageError e) {
            interceptedOut.flush();
            interceptedErr.flush();
            writeResult(resultFile, origOut, formatInspectionLoadFailedJson(
                runId, className, e.getClass().getName(), e.toString(),
                capturedOutStream.toString(StandardCharsets.UTF_8),
                capturedErrStream.toString(StandardCharsets.UTF_8)));
            return;
        } catch (Throwable t) {
            Throwable cause = unwrapThrowable(t);
            interceptedOut.flush();
            interceptedErr.flush();
            writeResult(resultFile, origOut, formatInspectionLoadFailedJson(
                runId, className, cause.getClass().getName(),
                cause.getMessage() != null ? cause.getMessage() : cause.toString(),
                capturedOutStream.toString(StandardCharsets.UTF_8),
                capturedErrStream.toString(StandardCharsets.UTF_8)));
            return;
        }

        boolean isPlugIn = PlugIn.class.isAssignableFrom(clazz);
        boolean isFilter = PlugInFilter.class.isAssignableFrom(clazz);
        boolean isExtended = ExtendedPlugInFilter.class.isAssignableFrom(clazz);

        String kind;
        String message;
        if (isPlugIn && isFilter) {
            kind = "ambiguous";
            message = "Class '" + className + "' implements both ij.plugin.PlugIn and "
                + "ij.plugin.filter.PlugInFilter; the narrow image filter path cannot "
                + "choose an unambiguous invocation.";
        } else if (isExtended) {
            kind = "extended_plugin_filter";
            message = "Class '" + className + "' implements ExtendedPlugInFilter; dialogs, "
                + "preview and stack progress support are outside the narrow image filter path.";
        } else if (isFilter) {
            kind = "plugin_filter";
            message = "Class '" + className + "' implements the supported plain "
                + "ij.plugin.filter.PlugInFilter interface.";
        } else if (isPlugIn) {
            kind = "plugin";
            message = "Class '" + className + "' implements ij.plugin.PlugIn, which is not "
                + "supported by the narrow image filter path.";
        } else {
            kind = "not_a_plugin";
            message = "Class '" + className + "' implements neither ij.plugin.PlugIn nor "
                + "ij.plugin.filter.PlugInFilter.";
        }

        interceptedOut.flush();
        interceptedErr.flush();
        writeResult(resultFile, origOut, formatInspectedJson(
            runId, className, kind, message,
            capturedOutStream.toString(StandardCharsets.UTF_8),
            capturedErrStream.toString(StandardCharsets.UTF_8)));
    }

    private static ClassLoader buildClassLoader(String pluginCp) {
        if (pluginCp == null || pluginCp.trim().isEmpty()) {
            return PluginHost.class.getClassLoader();
        }
        String[] parts = pluginCp.split(File.pathSeparator);
        List<URL> urls = new ArrayList<>();
        for (String part : parts) {
            String trimmed = part.trim();
            if (!trimmed.isEmpty()) {
                File f = new File(trimmed).getAbsoluteFile();
                try {
                    urls.add(f.toURI().toURL());
                } catch (Exception e) {
                    System.err.println("Warning: failed to convert classpath entry to URL: " + trimmed);
                }
            }
        }
        return new DialogClassLoader(urls.toArray(new URL[0]), PluginHost.class.getClassLoader());
    }

    private static Throwable unwrapThrowable(Throwable t) {
        if (t instanceof InvocationTargetException && t.getCause() != null) {
            return unwrapThrowable(t.getCause());
        }
        return t;
    }

    private static String getStackTraceString(Throwable t) {
        StringWriter sw = new StringWriter();
        PrintWriter pw = new PrintWriter(sw);
        t.printStackTrace(pw);
        return sw.toString();
    }

    private static void writeResult(String resultFile, PrintStream out, String json) {
        if (resultFile != null) {
            try {
                Files.writeString(Paths.get(resultFile), json, StandardCharsets.UTF_8);
            } catch (IOException e) {
                System.err.println("Failed to write result file: " + e.getMessage());
            }
        }
        out.println(json);
    }

    private static String quote(String s) {
        if (s == null) return "null";
        StringBuilder sb = new StringBuilder("\"");
        for (int i = 0; i < s.length(); i++) {
            char c = s.charAt(i);
            switch (c) {
                case '"': sb.append("\\\""); break;
                case '\\': sb.append("\\\\"); break;
                case '\b': sb.append("\\b"); break;
                case '\f': sb.append("\\f"); break;
                case '\n': sb.append("\\n"); break;
                case '\r': sb.append("\\r"); break;
                case '\t': sb.append("\\t"); break;
                default:
                    if (c < ' ') {
                        sb.append(String.format("\\u%04x", (int) c));
                    } else {
                        sb.append(c);
                    }
            }
        }
        sb.append("\"");
        return sb.toString();
    }

    private static String notProcessedMessage(FilterRunnerSupport.Result result) {
        if ((result.flags & PlugInFilter.DONE) != 0) {
            return "PlugInFilter setup() returned DONE; ImageJ's PlugInFilterRunner returned "
                + "without calling run(ip), so the image was not processed";
        }
        return "ImageJ's PlugInFilterRunner returned without calling run(ip) (for example, a "
            + "failed image capability check or image lock); the image was not processed";
    }

    private static String formatInspectedJson(String runId, String className, String kind,
            String message, String stdout, String stderr) {
        return "{\n"
            + "  \"status\": \"inspected\",\n"
            + "  \"run_id\": " + quote(runId) + ",\n"
            + "  \"class_name\": " + quote(className) + ",\n"
            + "  \"kind\": " + quote(kind) + ",\n"
            + "  \"message\": " + quote(message) + ",\n"
            + "  \"stdout\": " + quote(stdout) + ",\n"
            + "  \"stderr\": " + quote(stderr) + "\n"
            + "}";
    }

    private static String formatInspectionClassNotFoundJson(String runId, String className,
            String message, String stdout, String stderr) {
        return "{\n"
            + "  \"status\": \"class_not_found\",\n"
            + "  \"run_id\": " + quote(runId) + ",\n"
            + "  \"class_name\": " + quote(className) + ",\n"
            + "  \"message\": " + quote(message) + ",\n"
            + "  \"stdout\": " + quote(stdout) + ",\n"
            + "  \"stderr\": " + quote(stderr) + "\n"
            + "}";
    }

    private static String formatInspectionLoadFailedJson(String runId, String className,
            String exceptionType, String message, String stdout, String stderr) {
        return "{\n"
            + "  \"status\": \"load_failed\",\n"
            + "  \"run_id\": " + quote(runId) + ",\n"
            + "  \"class_name\": " + quote(className) + ",\n"
            + "  \"exception_type\": " + quote(exceptionType) + ",\n"
            + "  \"message\": " + quote(message) + ",\n"
            + "  \"stdout\": " + quote(stdout) + ",\n"
            + "  \"stderr\": " + quote(stderr) + "\n"
            + "}";
    }

    private static String formatSuccessJson(String runId, String className, String arg, String stdout, String stderr) {
        return "{\n"
            + "  \"status\": \"success\",\n"
            + "  \"run_id\": " + quote(runId) + ",\n"
            + "  \"class_name\": " + quote(className) + ",\n"
            + "  \"arg\": " + quote(arg) + ",\n"
            + "  \"stdout\": " + quote(stdout) + ",\n"
            + "  \"stderr\": " + quote(stderr) + "\n"
            + "}";
    }

    private static String formatFilterSuccessJson(String runId, String className, String arg,
            int flags, int width, int height, int runInvocations, boolean finalSetupCalled,
            String stdout, String stderr) {
        return "{\n"
            + "  \"status\": \"success\",\n"
            + "  \"run_id\": " + quote(runId) + ",\n"
            + "  \"class_name\": " + quote(className) + ",\n"
            + "  \"arg\": " + quote(arg) + ",\n"
            + "  \"filter_flags\": " + flags + ",\n"
            + "  \"image_width\": " + width + ",\n"
            + "  \"image_height\": " + height + ",\n"
            + "  \"run_invocations\": " + runInvocations + ",\n"
            + "  \"final_setup_called\": " + finalSetupCalled + ",\n"
            + "  \"stdout\": " + quote(stdout) + ",\n"
            + "  \"stderr\": " + quote(stderr) + "\n"
            + "}";
    }

    private static String formatNotProcessedJson(String runId, String className, String arg,
            int flags, String message, String stdout, String stderr) {
        return "{\n"
            + "  \"status\": \"not_processed\",\n"
            + "  \"run_id\": " + quote(runId) + ",\n"
            + "  \"class_name\": " + quote(className) + ",\n"
            + "  \"arg\": " + quote(arg) + ",\n"
            + "  \"filter_flags\": " + flags + ",\n"
            + "  \"run_invocations\": 0,\n"
            + "  \"message\": " + quote(message) + ",\n"
            + "  \"stdout\": " + quote(stdout) + ",\n"
            + "  \"stderr\": " + quote(stderr) + "\n"
            + "}";
    }

    private static String formatIoErrorJson(String runId, String className, String arg,
            String message, String stdout, String stderr) {
        return "{\n"
            + "  \"status\": \"io_error\",\n"
            + "  \"run_id\": " + quote(runId) + ",\n"
            + "  \"class_name\": " + quote(className) + ",\n"
            + "  \"arg\": " + quote(arg) + ",\n"
            + "  \"message\": " + quote(message) + ",\n"
            + "  \"stdout\": " + quote(stdout) + ",\n"
            + "  \"stderr\": " + quote(stderr) + "\n"
            + "}";
    }


    private static String formatUnsupportedFilterJson(String runId, String className, String arg,
            String message, int flags, String stdout, String stderr) {
        return "{\n"
            + "  \"status\": \"unsupported_filter\",\n"
            + "  \"run_id\": " + quote(runId) + ",\n"
            + "  \"class_name\": " + quote(className) + ",\n"
            + "  \"arg\": " + quote(arg) + ",\n"
            + "  \"filter_flags\": " + flags + ",\n"
            + "  \"message\": " + quote(message) + ",\n"
            + "  \"stdout\": " + quote(stdout) + ",\n"
            + "  \"stderr\": " + quote(stderr) + "\n"
            + "}";
    }

    private static String formatClassNotFoundJson(String runId, String className, String arg, String message, String stdout, String stderr) {
        return "{\n"
            + "  \"status\": \"class_not_found\",\n"
            + "  \"run_id\": " + quote(runId) + ",\n"
            + "  \"class_name\": " + quote(className) + ",\n"
            + "  \"arg\": " + quote(arg) + ",\n"
            + "  \"message\": " + quote(message) + ",\n"
            + "  \"stdout\": " + quote(stdout) + ",\n"
            + "  \"stderr\": " + quote(stderr) + "\n"
            + "}";
    }

    private static String formatUnsupportedTypeJson(String runId, String className, String arg, String message, String stdout, String stderr) {
        return "{\n"
            + "  \"status\": \"unsupported_plugin_type\",\n"
            + "  \"run_id\": " + quote(runId) + ",\n"
            + "  \"class_name\": " + quote(className) + ",\n"
            + "  \"arg\": " + quote(arg) + ",\n"
            + "  \"message\": " + quote(message) + ",\n"
            + "  \"stdout\": " + quote(stdout) + ",\n"
            + "  \"stderr\": " + quote(stderr) + "\n"
            + "}";
    }

    private static String formatExceptionThrownJson(String runId, String className, String arg, String exceptionType, String message, String stackTrace, String stdout, String stderr) {
        return "{\n"
            + "  \"status\": \"exception_thrown\",\n"
            + "  \"run_id\": " + quote(runId) + ",\n"
            + "  \"class_name\": " + quote(className) + ",\n"
            + "  \"arg\": " + quote(arg) + ",\n"
            + "  \"exception_type\": " + quote(exceptionType) + ",\n"
            + "  \"message\": " + quote(message) + ",\n"
            + "  \"stack_trace\": " + quote(stackTrace) + ",\n"
            + "  \"stdout\": " + quote(stdout) + ",\n"
            + "  \"stderr\": " + quote(stderr) + "\n"
            + "}";
    }
}
