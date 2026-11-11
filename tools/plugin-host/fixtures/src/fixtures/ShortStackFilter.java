package fixtures;

import ij.ImagePlus;
import ij.plugin.filter.PlugInFilter;
import ij.process.ImageProcessor;
import java.awt.Rectangle;

/**
 * Combined depth/stack fixture: DOES_16|DOES_STACKS|SUPPORTS_MASKING. Exercised
 * on 16-bit stacks where PlugInFilterRunner both applies checkImagePlus for
 * the GRAY16 type and processes every slice because of DOES_STACKS.
 */
public class ShortStackFilter implements PlugInFilter {

    @Override
    public int setup(String arg, ImagePlus imp) {
        return DOES_16 | DOES_STACKS | SUPPORTS_MASKING;
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
