package tools.plugin;

import ij.ImagePlus;
import ij.plugin.filter.PlugInFilter;
import ij.process.ImageProcessor;
import java.util.ArrayList;
import java.util.List;

/**
 * Wraps a real third-party PlugInFilter so that the host can observe the flags
 * returned by setup() before the real ImageJ PlugInFilterRunner acts on them.
 *
 * setup() is still called by the runner, and run(ip) is delegated unchanged.
 * ImageJ calls setup("final", imp) a second time for FINAL_PROCESSING filters
 * (PlugInFilterRunner.java 136-137) and ignores that return value, so the
 * wrapper must not overwrite the initial flags or validate the final return.
 *
 * Only flag combinations outside the explicitly supported subset are refused,
 * and refusal happens before the first run(ip) call. DOES_STACKS is supported
 * on a single-plane image because ImageJ only uses it to set doStack when
 * imp.getStackSize() > 1 (PlugInFilterRunner.java 72).
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
            NO_IMAGE_REQUIRED | STACK_REQUIRED
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

        if ((flags & DOES_8G) == 0) {
            problems.add("missing DOES_8G (the spike only supports 8-bit grayscale images)");
        }

        int unsupported = flags & UNSUPPORTED_FLAGS;
        if (unsupported != 0) {
            problems.add("unsupported flags " + flagNames(unsupported));
        }

        int unknown = flags & ~KNOWN_FLAGS;
        if (unknown != 0) {
            problems.add(String.format("unknown flag bits 0x%x", unknown));
        }

        if ((flags & ROI_REQUIRED) != 0 && (imp == null || imp.getRoi() == null)) {
            problems.add("ROI_REQUIRED was returned but the image has no selection");
        }

        if (!problems.isEmpty()) {
            throw new UnsupportedFilterException(
                    "PlugInFilter setup() requested capabilities outside the narrow "
                    + "8-bit, single-image, rectangular-ROI spike: " + String.join("; ", problems),
                    flags);
        }
        return flags;
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
        appendFlag(sb, value, STACK_REQUIRED, "STACK_REQUIRED");
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
