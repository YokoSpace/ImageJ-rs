package fixtures;

import ij.IJ;
import ij.plugin.PlugIn;

/**
 * Fixture plugin that calls IJ.getImage() when no image is open.
 * In headless ImageJ without an open image, IJ.getImage() calls IJ.noImage()
 * and terminates the JVM via System.exit(0).
 */
public class GetImageExitPlugIn implements PlugIn {
    @Override
    public void run(String arg) {
        System.out.println("GetImageExitPlugIn: invoking IJ.getImage()");
        IJ.getImage();
        System.out.println("GetImageExitPlugIn: should never reach here");
    }
}
