package java.applet;

import java.awt.Panel;
import java.net.URL;

/**
 * Compile-time and runtime shim for JDK 26+ where java.applet.Applet was removed.
 * Original ImageJ 1.54u references java.applet.Applet in fields and signatures
 * even when running as a standalone desktop or headless batch application.
 */
public class Applet extends Panel {
    public URL getDocumentBase() {
        return null;
    }

    public URL getCodeBase() {
        return null;
    }

    public String getParameter(String name) {
        return null;
    }

    public void init() {}

    public void destroy() {}
}
