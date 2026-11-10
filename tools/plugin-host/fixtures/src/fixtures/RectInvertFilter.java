package fixtures;

import ij.ImagePlus;
import ij.plugin.filter.PlugInFilter;
import ij.process.ImageProcessor;
import java.awt.Rectangle;

/**
 * Third-party-style fixture filter used to exercise the narrow PlugInFilter
 * compatibility spike. It intentionally follows the usual ImageJ filter
 * pattern of processing only the rectangle reported by ip.getRoi().
 */
public class RectInvertFilter implements PlugInFilter {

    @Override
    public int setup(String arg, ImagePlus imp) {
        return DOES_8G | SUPPORTS_MASKING;
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
