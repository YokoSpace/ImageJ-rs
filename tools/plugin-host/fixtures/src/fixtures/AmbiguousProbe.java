package fixtures;

import ij.ImagePlus;
import ij.plugin.PlugIn;
import ij.plugin.filter.PlugInFilter;
import ij.process.ImageProcessor;

/** Inspection fixture that implements both plug-in interfaces. */
public class AmbiguousProbe implements PlugIn, PlugInFilter {

    @Override
    public void run(String arg) {
    }

    @Override
    public int setup(String arg, ImagePlus imp) {
        return DOES_8G;
    }

    @Override
    public void run(ImageProcessor ip) {
    }
}
