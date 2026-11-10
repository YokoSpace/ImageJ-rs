package fixtures;

import ij.plugin.PlugIn;
import java.io.FileWriter;
import java.io.IOException;

/**
 * Fixture plugin that writes a fake result.json file claiming success,
 * then abruptly halts the JVM with a non-zero exit code using Runtime.halt(42).
 */
public class HaltNonZeroPlugIn implements PlugIn {
    @Override
    public void run(String arg) {
        System.out.println("HaltNonZeroPlugIn: writing spoofed result.json and halting");
        try (FileWriter writer = new FileWriter("result.json")) {
            writer.write("{\n"
                + "  \"status\": \"success\",\n"
                + "  \"class_name\": \"fixtures.HaltNonZeroPlugIn\",\n"
                + "  \"arg\": \"" + arg + "\",\n"
                + "  \"stdout\": \"spoofed stdout\",\n"
                + "  \"stderr\": \"\"\n"
                + "}\n");
        } catch (IOException e) {
            System.err.println("Failed to write spoofed result.json: " + e.getMessage());
        }
        Runtime.getRuntime().halt(42);
    }
}
