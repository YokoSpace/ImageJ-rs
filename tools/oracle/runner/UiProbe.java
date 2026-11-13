package tools.oracle;

import ij.ImagePlus;
import ij.WindowManager;
import ij.gui.ImageWindow;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import sun.misc.Unsafe;

/** Exposes ImageJ 1.54u UI strings to the batch macro oracle. */
public final class UiProbe {
    private UiProbe() {}

    /** Reads real non-macro numeric fields without displaying a dialog. */
    public static String zProjectNumbers(String start, String stop, String stackSize) {
        String savedMacroOptions = ij.Macro.getOptions();
        boolean savedRecord = ij.plugin.frame.Recorder.record;
        ij.gui.GenericDialog dialog = null;
        boolean disposed = false;
        try {
            Field options = ij.plugin.frame.Recorder.class.getDeclaredField("commandOptions");
            options.setAccessible(true);
            Object savedOptions = options.get(null);
            try {
                ij.Macro.setOptions(null);
                ij.plugin.frame.Recorder.record = true;
                dialog = new ij.gui.GenericDialog("ZProjection");
                dialog.addNumericField("Start slice:", 1, 0);
                dialog.addNumericField("Stop slice:", Integer.parseInt(stackSize), 0);
                ((java.awt.TextField)dialog.getNumericFields().get(0)).setText(start);
                ((java.awt.TextField)dialog.getNumericFields().get(1)).setText(stop);
                // dispose enables recording just as clicking OK does, without showDialog.
                dialog.dispose();
                disposed = true;
                ij.plugin.frame.Recorder.resetCommandOptions();
                dialog.setSmartRecording(true);
                double first = dialog.getNextNumber();
                double last = dialog.getNextNumber();
                String recorded = ij.plugin.frame.Recorder.getCommandOptions();
                return first + "," + last + "|" + (recorded == null ? "" : recorded);
            } finally {
                options.set(null, savedOptions);
            }
        } catch (ReflectiveOperationException e) {
            throw new IllegalStateException("Could not inspect Z Project numeric fields", e);
        } finally {
            ij.plugin.frame.Recorder.record = savedRecord;
            if (dialog != null && !disposed) dialog.dispose();
            ij.Macro.setOptions(savedMacroOptions);
        }
    }

    private static ImagePlus currentImage() {
        ImagePlus image = WindowManager.getCurrentImage();
        if (image == null)
            throw new IllegalStateException("No current image");
        return image;
    }

    public static String status(String x, String y) {
        ImagePlus image = currentImage();
        int pixelX = Integer.parseInt(x);
        int pixelY = Integer.parseInt(y);
        try {
            Method valueMethod = ImagePlus.class.getDeclaredMethod("getValueAsString", int.class, int.class);
            valueMethod.setAccessible(true);
            return image.getLocationAsString(pixelX, pixelY)
                + (String) valueMethod.invoke(image, pixelX, pixelY);
        } catch (ReflectiveOperationException e) {
            throw new IllegalStateException("Could not read ImageJ pixel status", e);
        }
    }

    public static String subtitle() {
        ImagePlus image = currentImage();
        try {
            Field unsafeField = Unsafe.class.getDeclaredField("theUnsafe");
            unsafeField.setAccessible(true);
            Unsafe unsafe = (Unsafe) unsafeField.get(null);
            ImageWindow window = (ImageWindow) unsafe.allocateInstance(ImageWindow.class);
            Field imageField = ImageWindow.class.getDeclaredField("imp");
            imageField.setAccessible(true);
            imageField.set(window, image);
            return window.createSubtitle();
        } catch (ReflectiveOperationException e) {
            throw new IllegalStateException("Could not read ImageJ subtitle", e);
        }
    }

    public static String imageSize() {
        return ImageWindow.getImageSize(currentImage());
    }

    private static int keyNameToCode(String key) {
        if ("left".equalsIgnoreCase(key) || "vk_left".equalsIgnoreCase(key))
            return java.awt.event.KeyEvent.VK_LEFT;
        if ("right".equalsIgnoreCase(key) || "vk_right".equalsIgnoreCase(key))
            return java.awt.event.KeyEvent.VK_RIGHT;
        if ("up".equalsIgnoreCase(key) || "vk_up".equalsIgnoreCase(key))
            return java.awt.event.KeyEvent.VK_UP;
        if ("down".equalsIgnoreCase(key) || "vk_down".equalsIgnoreCase(key))
            return java.awt.event.KeyEvent.VK_DOWN;
        throw new IllegalArgumentException("Unknown arrow key: " + key);
    }

    public static String nudge(String key) {
        ImagePlus image = currentImage();
        ij.gui.Roi roi = image.getRoi();
        if (roi == null) return "";
        roi.nudge(keyNameToCode(key));
        if (roi instanceof ij.gui.Line) {
            ij.gui.Line l = (ij.gui.Line) roi;
            return l.x1 + "," + l.y1 + "," + l.x2 + "," + l.y2;
        }
        java.awt.Rectangle r = roi.getBounds();
        return r.x + "," + r.y + "," + r.width + "," + r.height;
    }

    public static String nudgeCorner(String key) {
        ImagePlus image = currentImage();
        ij.gui.Roi roi = image.getRoi();
        if (roi == null) return "";
        roi.nudgeCorner(keyNameToCode(key));
        if (roi instanceof ij.gui.Line) {
            ij.gui.Line l = (ij.gui.Line) roi;
            return l.x1 + "," + l.y1 + "," + l.x2 + "," + l.y2;
        }
        java.awt.Rectangle r = roi.getBounds();
        return r.x + "," + r.y + "," + r.width + "," + r.height;
    }
}
