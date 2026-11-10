package fixtures;

import ij.ImagePlus;
import ij.plugin.filter.PlugInFilter;
import ij.process.ImageProcessor;

/**
 * Fixture for FINAL_PROCESSING. ImageJ calls setup("final", imp) after run(ip)
 * and ignores the return value; returning DONE here must not overwrite the
 * initial DOES_8G|FINAL_PROCESSING flags or be validated as a new request.
 */
public class FinalProcessingFilter implements PlugInFilter {

    @Override
    public int setup(String arg, ImagePlus imp) {
        if ("final".equals(arg)) {
            System.out.println("FinalProcessingFilter: final setup");
            return DONE;
        }
        return DOES_8G | FINAL_PROCESSING;
    }

    @Override
    public void run(ImageProcessor ip) {
        System.out.println("FinalProcessingFilter: run");
        ip.set(0, 0, 255 - ip.get(0, 0));
    }
}
