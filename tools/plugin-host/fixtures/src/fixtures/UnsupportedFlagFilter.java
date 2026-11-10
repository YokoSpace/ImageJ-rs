package fixtures;

import ij.ImagePlus;
import ij.plugin.filter.PlugInFilter;
import ij.process.ImageProcessor;

/**
 * Fixture with a flag combination the narrow host genuinely cannot preserve.
 * CONVERT_TO_FLOAT hands run(ip) a FloatProcessor and is outside the 8-bit
 * return subset.
 */
public class UnsupportedFlagFilter implements PlugInFilter {

    @Override
    public int setup(String arg, ImagePlus imp) {
        return DOES_8G | CONVERT_TO_FLOAT;
    }

    @Override
    public void run(ImageProcessor ip) {
    }
}
