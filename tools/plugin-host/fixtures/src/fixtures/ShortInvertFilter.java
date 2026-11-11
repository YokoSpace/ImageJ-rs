package fixtures;

import ij.ImagePlus;
import ij.plugin.filter.PlugInFilter;
import ij.process.ImageProcessor;
import java.awt.Rectangle;

/**
 * Sixteen-bit counterpart of RectInvertFilter. Declares DOES_16 so a 16-bit
 * input passes PlugInFilterRunner.checkImagePlus, and inverts the rectangle
 * reported by ip.getRoi() in 0..65535 unsigned values. Used to exercise the
 * 16-bit extension of the narrow PlugInFilter host.
 */
public class ShortInvertFilter implements PlugInFilter {

    @Override
    public int setup(String arg, ImagePlus imp) {
        return DOES_16 | SUPPORTS_MASKING;
    }

    @Override
    public void run(ImageProcessor ip) {
        Rectangle r = ip.getRoi();
        for (int y = r.y; y < r.y + r.height; y++) {
            for (int x = r.x; x < r.x + r.width; x++) {
                int v = ip.get(x, y) & 0xffff;
                ip.set(x, y, (short) (0xffff - v));
            }
        }
    }
}
