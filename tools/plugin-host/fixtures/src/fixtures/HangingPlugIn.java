package fixtures;

import ij.plugin.PlugIn;

public class HangingPlugIn implements PlugIn {
    @Override
    public void run(String arg) {
        System.out.println("HangingPlugIn sleeping indefinitely for arg: " + arg);
        try {
            Thread.sleep(60_000);
        } catch (InterruptedException e) {
            // JVM terminated
        }
    }
}
