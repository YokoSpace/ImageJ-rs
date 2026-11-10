package fixtures;

import ij.ImagePlus;
import ij.WindowManager;
import ij.plugin.filter.PlugInFilter;
import ij.process.ImageProcessor;

/** Fixture that changes the image LUT while leaving the pixel plane intact. */
public class OutputLutFilter implements PlugInFilter {

    @Override
    public int setup(String arg, ImagePlus imp) {
        return DOES_8G;
    }

    @Override
    public void run(ImageProcessor ip) {
        WindowManager.getCurrentImage().getProcessor().invertLut();
    }
}
