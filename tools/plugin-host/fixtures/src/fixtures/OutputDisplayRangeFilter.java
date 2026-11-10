package fixtures;

import ij.ImagePlus;
import ij.WindowManager;
import ij.plugin.filter.PlugInFilter;
import ij.process.ImageProcessor;

/** Fixture that changes the display range while leaving the pixels intact. */
public class OutputDisplayRangeFilter implements PlugInFilter {

    @Override
    public int setup(String arg, ImagePlus imp) {
        return DOES_8G;
    }

    @Override
    public void run(ImageProcessor ip) {
        WindowManager.getCurrentImage().setDisplayRange(0, 100);
    }
}
