package fixtures;

import ij.ImagePlus;
import ij.plugin.filter.PlugInFilter;
import ij.process.ImageProcessor;

/**
 * Fixture declaring stack support. On a single-plane image ImageJ sets
 * doStack=false because getStackSize()==1, so run(ip) is called once and the
 * spike must accept DOES_STACKS rather than reject a capability Java ignores.
 */
public class StackFilter implements PlugInFilter {

    @Override
    public int setup(String arg, ImagePlus imp) {
        return DOES_8G | DOES_STACKS;
    }

    @Override
    public void run(ImageProcessor ip) {
        ip.set(0, 0, 255 - ip.get(0, 0));
    }
}
