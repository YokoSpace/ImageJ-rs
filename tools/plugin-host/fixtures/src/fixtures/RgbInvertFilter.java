package fixtures;

import ij.ImagePlus;
import ij.plugin.filter.PlugInFilter;
import ij.process.ImageProcessor;
import java.awt.Rectangle;

/**
 * RGB counterpart of RectInvertFilter. ImageJ hands RGB filters a
 * ColorProcessor whose get(x,y) returns the packed 24-bit pixel, so inverting
 * the rectangle reported by ip.getRoi() exercises the COLOR_RGB branch of the
 * depth-extended narrow PlugInFilter host.
 */
public class RgbInvertFilter implements PlugInFilter {

    @Override
    public int setup(String arg, ImagePlus imp) {
        return DOES_RGB | SUPPORTS_MASKING;
    }

    @Override
    public void run(ImageProcessor ip) {
        Rectangle r = ip.getRoi();
        for (int y = r.y; y < r.y + r.height; y++) {
            for (int x = r.x; x < r.x + r.width; x++) {
                ip.set(x, y, ~ip.get(x, y) & 0xffffff);
            }
        }
    }
}
