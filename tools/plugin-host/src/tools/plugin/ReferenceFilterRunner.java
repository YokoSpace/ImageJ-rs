package tools.plugin;

import ij.IJ;
import ij.ImagePlus;
import ij.gui.Roi;

/**
 * Differential oracle for the narrow PlugInFilter spike.
 *
 * This runner does not contain the spike's validation or transfer logic. It
 * loads the input image, installs the requested rectangular ROI, and invokes
 * the plugin through ImageJ's own public IJ.runPlugIn(ImagePlus, ...) path,
 * which constructs the real ImageJ 1.54u PlugInFilterRunner. The test compares
 * the complete output plane against the PluginHost path.
 *
 * The fixture class is discovered through the standard ImageJ plugin class
 * loader; the shell wrapper sets -Dplugins.dir to the directory that contains
 * the fixture JAR.
 */
public class ReferenceFilterRunner {

    public static void main(String[] args) throws Exception {
        String className = null;
        String arg = "";
        String inputImage = null;
        String outputImage = null;
        String roiSpec = null;

        for (int i = 0; i < args.length; i++) {
            if ("--class".equals(args[i]) && i + 1 < args.length) {
                className = args[++i];
            } else if ("--arg".equals(args[i]) && i + 1 < args.length) {
                arg = args[++i];
            } else if ("--input-image".equals(args[i]) && i + 1 < args.length) {
                inputImage = args[++i];
            } else if ("--output-image".equals(args[i]) && i + 1 < args.length) {
                outputImage = args[++i];
            } else if ("--roi".equals(args[i]) && i + 1 < args.length) {
                roiSpec = args[++i];
            }
        }

        if (className == null || inputImage == null || outputImage == null) {
            System.err.println("usage: ReferenceFilterRunner --class C --arg A "
                    + "--input-image IN --output-image OUT [--roi x,y,w,h]");
            System.exit(2);
        }

        ImagePlus imp = IJ.openImage(inputImage);
        if (imp == null) {
            System.err.println("reference oracle could not open " + inputImage);
            System.exit(2);
        }
        if (roiSpec != null) {
            String[] parts = roiSpec.split(",");
            if (parts.length != 4) {
                System.err.println("invalid --roi " + roiSpec);
                System.exit(2);
            }
            imp.setRoi(new Roi(
                    Integer.parseInt(parts[0].trim()),
                    Integer.parseInt(parts[1].trim()),
                    Integer.parseInt(parts[2].trim()),
                    Integer.parseInt(parts[3].trim())));
        }

        try {
            Object plugin = IJ.runPlugIn(imp, className, arg);
            if (plugin == null) {
                System.err.println("reference oracle could not load/run " + className);
                System.exit(3);
            }
            if (!IJ.saveAsTiff(imp, outputImage)) {
                System.err.println("reference oracle could not save " + outputImage);
                System.exit(4);
            }
            System.out.println("{\"status\":\"success\",\"class_name\":\"" + className
                    + "\",\"arg\":\"" + arg + "\"}");
            System.exit(0);
        } catch (Throwable t) {
            t.printStackTrace(System.err);
            System.exit(1);
        }
    }
}
