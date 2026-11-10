package fixtures;

import ij.ImagePlus;
import ij.WindowManager;
import ij.gui.Roi;
import ij.plugin.filter.PlugInFilter;
import ij.process.ImageProcessor;

/** Fixture that replaces the input selection with a different rectangle. */
public class OutputRoiFilter implements PlugInFilter {

    @Override
    public int setup(String arg, ImagePlus imp) {
        return DOES_8G;
    }

    @Override
    public void run(ImageProcessor ip) {
        WindowManager.getCurrentImage().setRoi(new Roi(1, 1, 2, 2));
    }
}
