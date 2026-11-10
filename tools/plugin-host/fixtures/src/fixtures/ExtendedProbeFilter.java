package fixtures;

import ij.ImagePlus;
import ij.plugin.filter.ExtendedPlugInFilter;
import ij.plugin.filter.PlugInFilterRunner;
import ij.process.ImageProcessor;

/** Inspection fixture for the unsupported ExtendedPlugInFilter category. */
public class ExtendedProbeFilter implements ExtendedPlugInFilter {

    @Override
    public int setup(String arg, ImagePlus imp) {
        return DOES_8G;
    }

    @Override
    public void run(ImageProcessor ip) {
    }

    @Override
    public int showDialog(ImagePlus imp, String command, PlugInFilterRunner pfr) {
        return DOES_8G;
    }

    @Override
    public void setNPasses(int nPasses) {
    }
}
