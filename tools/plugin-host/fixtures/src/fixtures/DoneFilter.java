package fixtures;

import ij.ImagePlus;
import ij.plugin.filter.PlugInFilter;
import ij.process.ImageProcessor;

/**
 * Fixture that returns DONE without DOES_8G. ImageJ returns from the runner
 * before checking image capabilities, so run(ip) is never called and no pixel
 * processing has happened.
 */
public class DoneFilter implements PlugInFilter {

    @Override
    public int setup(String arg, ImagePlus imp) {
        return DONE;
    }

    @Override
    public void run(ImageProcessor ip) {
        throw new AssertionError("DoneFilter.run must not be called");
    }
}
