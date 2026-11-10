package fixtures;

import ij.plugin.PlugIn;

public class FailingPlugIn implements PlugIn {
    @Override
    public void run(String arg) {
        System.err.println("FailingPlugIn intentional failure triggered for arg: " + arg);
        throw new IllegalStateException("Intentional test exception from FailingPlugIn: " + arg);
    }
}
