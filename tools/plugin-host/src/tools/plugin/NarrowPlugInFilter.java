package tools.plugin;

import ij.ImagePlus;
import ij.plugin.filter.PlugInFilter;
import ij.process.ImageProcessor;
import java.util.ArrayList;
import java.util.List;

/**
 * Wraps a real third-party PlugInFilter so that the host can observe the flags
 * returned by setup() and preflight them before PlugInFilterRunner acts on
 * them.
 *
 * setup() is still called by the runner, and run(ip) is delegated unchanged.
 * ImageJ calls setup("final", imp) a second time for FINAL_PROCESSING filters
 * (PlugInFilterRunner.java 136-137) and ignores that return value, so the
 * wrapper must not overwrite the initial flags or validate the final return.
 *
 * Depth preflight mirrors PlugInFilterRunner.checkImagePlus
 * (PlugInFilterRunner.java 379-427): the flag required by the active image
 * type must be present, otherwise the run is refused with the same
 * "requires an image of type" summary instead of processing partially.
 * Plain Z-stacks are also accepted; PlugInFilterRunner itself decides between
 * the whole stack (DOES_STACKS) and the single current slice.
 */
final class NarrowPlugInFilter implements PlugInFilter {

    private static final int KNOWN_FLAGS =
            DOES_8G | DOES_8C | DOES_16 | DOES_32 | DOES_RGB
            | DOES_STACKS | SUPPORTS_MASKING | NO_CHANGES | NO_UNDO
            | NO_IMAGE_REQUIRED | ROI_REQUIRED | STACK_REQUIRED
            | DONE | CONVERT_TO_FLOAT | SNAPSHOT | PARALLELIZE_STACKS
            | FINAL_PROCESSING | KEEP_THRESHOLD | PARALLELIZE_IMAGES
            | NO_UNDO_RESET;

    private static final int UNSUPPORTED_FLAGS =
            NO_IMAGE_REQUIRED
            | CONVERT_TO_FLOAT | PARALLELIZE_STACKS | PARALLELIZE_IMAGES;

    private final PlugInFilter delegate;
    private int flags;
    private boolean setupCalled;
    private boolean finalSetupCalled;
    private int runInvocations;

    NarrowPlugInFilter(PlugInFilter delegate) {
        this.delegate = delegate;
    }

    @Override
    public int setup(String arg, ImagePlus imp) {
        if (setupCalled) {
            // FINAL_PROCESSING: ImageJ passes arg="final" after processing and
            // discards the returned flags. Delegate the call, but leave the
            // observed initial flags untouched.
            finalSetupCalled = true;
            return delegate.setup(arg, imp);
        }

        flags = delegate.setup(arg, imp);
        setupCalled = true;

        // PlugInFilterRunner returns immediately for DONE before checking the
        // image type or any other capability (PlugInFilterRunner.java 55-56).
        if ((flags & DONE) != 0) {
            return flags;
        }

        List<String> problems = new ArrayList<>();

        int unsupported = flags & UNSUPPORTED_FLAGS;
        if (unsupported != 0) {
            problems.add("unsupported flags " + flagNames(unsupported));
        }

        int unknown = flags & ~KNOWN_FLAGS;
        if (unknown != 0) {
            problems.add(String.format("unknown flag bits 0x%x", unknown));
        }

        if (imp != null && imp.getProcessor() != null) {
            // checkImagePlus (PlugInFilterRunner.java 386-408) extracts the
            // flag required by the active image type and refuses the run
            // through wrongType (PlugInFilterRunner.java 419-427).
            int type = imp.getType();
            boolean supported;
            switch (type) {
                case ImagePlus.GRAY8:
                    supported = (flags & DOES_8G) != 0;
                    break;
                case ImagePlus.COLOR_256:
                    supported = (flags & DOES_8C) != 0;
                    break;
                case ImagePlus.GRAY16:
                    supported = (flags & DOES_16) != 0;
                    break;
                case ImagePlus.GRAY32:
                    supported = (flags & DOES_32) != 0;
                    break;
                case ImagePlus.COLOR_RGB:
                    supported = (flags & DOES_RGB) != 0;
                    break;
                default:
                    supported = false;
                    break;
            }
            if (!supported) {
                problems.add(wrongTypeMessage(type));
            }
        }

        if ((flags & ROI_REQUIRED) != 0 && (imp == null || imp.getRoi() == null)) {
            problems.add("ROI_REQUIRED was returned but the image has no selection");
        }

        if ((flags & STACK_REQUIRED) != 0 && (imp == null || imp.getStackSize() <= 1)) {
            problems.add("STACK_REQUIRED was returned but the image is not a stack");
        }

        if (!problems.isEmpty()) {
            throw new UnsupportedFilterException(
                    "PlugInFilter setup() requested capabilities outside the narrow "
                    + "image filter subset: " + String.join("; ", problems),
                    flags);
        }
        return flags;
    }

    /**
     * The same summary PlugInFilterRunner.wrongType would show in a dialog
     * (PlugInFilterRunner.java 419-427), naming the image types the filter
     * actually declared.
     */
    private String wrongTypeMessage(int type) {
        String commandWord = "This command";
        String s = "the active image type (" + typeName(type) + ") is not among the "
            + "types declared by setup() - it requires an image of type: ";
        boolean any = false;
        if ((flags & DOES_8G) != 0) { s += "8-bit grayscale"; any = true; }
        if ((flags & DOES_8C) != 0) { s += (any ? ", " : "") + "8-bit color"; any = true; }
        if ((flags & DOES_16) != 0) { s += (any ? ", " : "") + "16-bit grayscale"; any = true; }
        if ((flags & DOES_32) != 0) { s += (any ? ", " : "") + "32-bit (float) grayscale"; any = true; }
        if ((flags & DOES_RGB) != 0) { s += (any ? ", " : "") + "RGB color"; any = true; }
        if (!any) {
            s += "(setup() declared no image type flags)";
        }
        return commandWord + ": " + s;
    }

    private static String typeName(int type) {
        switch (type) {
            case ImagePlus.GRAY8:
                return "8-bit grayscale";
            case ImagePlus.GRAY16:
                return "16-bit grayscale";
            case ImagePlus.GRAY32:
                return "32-bit float grayscale";
            case ImagePlus.COLOR_256:
                return "8-bit indexed color";
            case ImagePlus.COLOR_RGB:
                return "RGB color";
            default:
                return "type " + type;
        }
    }

    @Override
    public void run(ImageProcessor ip) {
        runInvocations++;
        delegate.run(ip);
    }

    int flags() {
        return flags;
    }

    boolean setupCalled() {
        return setupCalled;
    }

    boolean finalSetupCalled() {
        return finalSetupCalled;
    }

    int runInvocations() {
        return runInvocations;
    }

    boolean done() {
        return (flags & DONE) != 0;
    }

    private static String flagNames(int value) {
        StringBuilder sb = new StringBuilder();
        appendFlag(sb, value, NO_IMAGE_REQUIRED, "NO_IMAGE_REQUIRED");
        appendFlag(sb, value, CONVERT_TO_FLOAT, "CONVERT_TO_FLOAT");
        appendFlag(sb, value, PARALLELIZE_STACKS, "PARALLELIZE_STACKS");
        appendFlag(sb, value, PARALLELIZE_IMAGES, "PARALLELIZE_IMAGES");
        return sb.toString();
    }

    private static void appendFlag(StringBuilder sb, int value, int flag, String name) {
        if ((value & flag) == 0) {
            return;
        }
        if (sb.length() > 0) {
            sb.append('|');
        }
        sb.append(name);
    }
}
