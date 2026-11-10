package fixtures;

import ij.ImagePlus;
import ij.plugin.filter.PlugInFilter;
import ij.process.ImageProcessor;

/**
 * Fixture that mutates the image and then throws, proving that failed filter
 * runs never surface a partially processed output image.
 */
public class FailingFilter implements PlugInFilter {

    @Override
    public int setup(String arg, ImagePlus imp) {
        return DOES_8G;
    }

    @Override
    public void run(ImageProcessor ip) {
        ip.set(0, 0, 0);
        throw new IllegalStateException("fixture-failure-after-mutation");
    }
}
