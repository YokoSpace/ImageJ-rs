package fixtures;

import ij.ImagePlus;
import ij.plugin.filter.PlugInFilter;
import ij.process.ImageProcessor;
import java.awt.Rectangle;

/**
 * Stack counterpart of RectInvertFilter: DOES_8G|DOES_STACKS|SUPPORTS_MASKING
 * with a full-plane rectangle invert. PlugInFilterRunner sets doStack for
 * images with getStackSize()>1 and calls run(ip) once per slice, so this
 * fixture makes whole-stack processing observable on every plane (unlike
 * StackFilter, which keys on a single pixel).
 */
public class StackInvertFilter implements PlugInFilter {

    @Override
    public int setup(String arg, ImagePlus imp) {
        return DOES_8G | DOES_STACKS | SUPPORTS_MASKING;
    }

    @Override
    public void run(ImageProcessor ip) {
        Rectangle r = ip.getRoi();
        for (int y = r.y; y < r.y + r.height; y++) {
            for (int x = r.x; x < r.x + r.width; x++) {
                ip.set(x, y, 255 - ip.get(x, y));
            }
        }
    }
}
