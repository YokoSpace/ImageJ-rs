package fixtures;

import ij.ImagePlus;
import ij.plugin.filter.PlugInFilter;
import ij.process.ImageProcessor;
import java.io.FileWriter;
import java.io.IOException;

/**
 * Inspection fixture that records every lifecycle phase it reaches.
 *
 * The marker files are created relative to the host's working directory. A
 * read-only inspection must load this class without initializing it, so none of
 * the markers may appear; executing the plugin would create them.
 */
public class InspectionProbeFilter implements PlugInFilter {

    static {
        mark("inspection-static-initializer.marker");
    }

    @Override
    public int setup(String arg, ImagePlus imp) {
        mark("inspection-setup.marker");
        return DOES_8G;
    }

    @Override
    public void run(ImageProcessor ip) {
        mark("inspection-run.marker");
    }

    private static void mark(String name) {
        try (FileWriter writer = new FileWriter(name)) {
            writer.write("executed");
        } catch (IOException e) {
            throw new RuntimeException(e);
        }
    }
}
