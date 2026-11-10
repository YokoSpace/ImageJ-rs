package fixtures;

import ij.ImagePlus;
import ij.WindowManager;
import ij.plugin.filter.PlugInFilter;
import ij.process.ImageProcessor;

/** Fixture that adds spatial calibration the narrow TIFF bridge cannot carry. */
public class OutputCalibrationFilter implements PlugInFilter {

    @Override
    public int setup(String arg, ImagePlus imp) {
        return DOES_8G;
    }

    @Override
    public void run(ImageProcessor ip) {
        WindowManager.getCurrentImage().getCalibration().pixelWidth = 2.0;
    }
}
