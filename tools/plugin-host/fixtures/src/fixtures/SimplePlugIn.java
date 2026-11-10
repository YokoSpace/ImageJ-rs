package fixtures;

import ij.plugin.PlugIn;
import java.io.FileWriter;
import java.io.IOException;

public class SimplePlugIn implements PlugIn {
    @Override
    public void run(String arg) {
        System.out.println("SimplePlugIn executed with arg: " + arg);
        if (arg != null && arg.startsWith("write:")) {
            String targetPath = arg.substring("write:".length());
            try (FileWriter writer = new FileWriter(targetPath)) {
                writer.write("executed:" + arg);
            } catch (IOException e) {
                throw new RuntimeException("Failed writing marker file", e);
            }
        }
    }
}
