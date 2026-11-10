package fixtures;

import ij.plugin.PlugIn;

/**
 * Fixture plugin that starts a non-daemon background thread before run() returns.
 * In a standard JVM without explicit System.exit, non-daemon threads prevent the JVM
 * from terminating even after main() completes.
 */
public class BackgroundThreadPlugIn implements PlugIn {
    @Override
    public void run(String arg) {
        System.out.println("BackgroundThreadPlugIn: starting non-daemon background thread");
        Thread bgThread = new Thread(() -> {
            try {
                Thread.sleep(60_000);
            } catch (InterruptedException ignored) {
            }
        }, "fixtures-bg-worker");
        bgThread.setDaemon(false);
        bgThread.start();
        System.out.println("BackgroundThreadPlugIn: run() completed");
    }
}
