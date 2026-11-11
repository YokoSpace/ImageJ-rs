package fixtures;

import ij.ImagePlus;
import ij.plugin.filter.PlugInFilter;
import ij.process.ImageProcessor;
import java.awt.Rectangle;

/**
 * Thirty-two-bit float fixture filter for the depth extension of the narrow
 * PlugInFilter host. Doubles every pixel in the rectangle reported by
 * ip.getRoi(); multiplying a finite f32 by 2 is exact, so the differential
 * oracle comparison can require bit-equal results.
 */
public class FloatInvertFilter implements PlugInFilter {

    @Override
    public int setup(String arg, ImagePlus imp) {
        return DOES_32 | SUPPORTS_MASKING;
    }

    @Override
    public void run(ImageProcessor ip) {
        Rectangle r = ip.getRoi();
        for (int y = r.y; y < r.y + r.height; y++) {
            for (int x = r.x; x < r.x + r.width; x++) {
                ip.setf(x, y, ip.getf(x, y) * 2.0f);
            }
        }
    }
}
