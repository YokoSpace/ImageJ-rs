package tools.oracle;

import ij.IJ;
import ij.ImageJ;
import ij.ImagePlus;
import ij.WindowManager;
import ij.gui.ImageWindow;
import ij.macro.Interpreter;
import ij.plugin.ZProjector;
import ij.measure.ResultsTable;
import ij.plugin.filter.Analyzer;
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

    /** Creates real ImageWindow instances and observes ZProjector title uniquification. */
    public static String zProjectRealWindowTitles(String unused) {
        try {
            String result = zProjectRealWindowTitlesOrThrow();
            System.out.println("TITLES=" + result);
            return result;
        } catch (Throwable t) {
            return "ERROR=" + t.getClass().getName() + ":" + t.getMessage();
        }
    }

    private static String zProjectRealWindowTitlesOrThrow() {
        boolean savedBatchMode = Interpreter.batchMode;
        java.util.List<ImageWindow> windows = new java.util.ArrayList<ImageWindow>();
        try {
            ImagePlus input = WindowManager.getCurrentImage();
            if (input == null)
                throw new IllegalStateException("No current image");
            Interpreter.batchMode = false;
            if (IJ.getInstance() == null)
                new ImageJ();
            windows.add(new ImageWindow(input));
            ImagePlus blocker = new ImagePlus(
                "MAX_input.tif",
                new ij.process.ByteProcessor(input.getWidth(), input.getHeight())
            );
            windows.add(new ImageWindow(blocker));

            ImagePlus avg = ZProjector.run(input, "avg");
            ImageWindow avgWindow = new ImageWindow(avg);
            windows.add(avgWindow);
            ImagePlus max1 = ZProjector.run(input, "max");
            ImageWindow max1Window = new ImageWindow(max1);
            windows.add(max1Window);
            ImagePlus max2 = ZProjector.run(input, "max");
            ImageWindow max2Window = new ImageWindow(max2);
            windows.add(max2Window);

            return avg.getTitle() + ";" + max1.getTitle() + ";" + max2.getTitle()
                + "|visible=" + (avgWindow.isVisible() && max1Window.isVisible() && max2Window.isVisible())
                + "|windows=" + WindowManager.getWindowCount();
        } finally {
            for (ImageWindow window : windows) {
                window.setVisible(false);
                window.dispose();
            }
            Interpreter.batchMode = savedBatchMode;
        }
    }


    /** Reports the current processor LUT inversion state and selected entries. */
    public static String lutState(String indicesCsv) {
        ImagePlus image = currentImage();
        ij.process.ImageProcessor ip = image.getProcessor();
        if (!(ip.getColorModel() instanceof java.awt.image.IndexColorModel))
            return "cm=none";
        java.awt.image.IndexColorModel cm = (java.awt.image.IndexColorModel)ip.getColorModel();
        StringBuilder out = new StringBuilder();
        out.append("inv=").append(ip.isInvertedLut())
            .append(";color=").append(ip.isColorLut())
            .append(";min=").append(ip.getMin())
            .append(";max=").append(ip.getMax())
            .append(";changes=").append(image.changes)
            .append(";entries=");
        boolean first = true;
        for (String part : indicesCsv.split(",")) {
            int index = Integer.parseInt(part.trim());
            if (!first) out.append(',');
            first = false;
            out.append(index).append(':')
                .append(cm.getRed(index)).append(',')
                .append(cm.getGreen(index)).append(',')
                .append(cm.getBlue(index));
        }
        return out.toString();
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

    /**
     * Drives the real Analyzer/ResultsTable lifecycle that a headless
     * run("Measure") masks: Analyzer.displayResults deletes the rows it cannot
     * show when no Results window exists (Analyzer.java 845-865), and
     * Analyzer.setOptions only clears the summary and updates columns when
     * IJ.isResultsWindow() is true (Analyzer.java 243-249).
     *
     * The probe installs a real TextPanel as IJ.textPanel so those branches
     * run, then executes ';'-separated ops against the system Results table:
     *   measure                  Analyzer.saveResults on the current image/ROI
     *   measurePoint:x,y         Analyzer.measurePoint (Analyzer.java 379-425)
     *   measureAngle:x1,y1,...   Analyzer.measureAngle (Analyzer.java 427-440)
     *   measureLine:x1,y1,x2,y2  Analyzer.measureLength (Analyzer.java 442-525)
     *   summarize                Analyzer.summarize (Analyzer.java 888-927)
     *   clearSummary             Analyzer.clearSummary (Analyzer.java 726-732)
     *   set:flags:precision      Set Measurements OK (Analyzer.java 205-255)
     *   update:flags             ResultsTable.update(flags, imp, null)
     *   disableLabels            ResultsTable.disableRowLabels
     *   clear                    Analyzer.resetCounter
     * Returns "size=&lt;n&gt;;labels=&lt;0|1&gt;;resultsWindow=&lt;0|1&gt;".
     */
    public static String resultsLifecycle(String script) {
        ImagePlus image = currentImage();
        ResultsTable table = Analyzer.getResultsTable();
        try {
            installResultsPanel();
            for (String raw : script.split(";")) {
                String op = raw.trim();
                if (op.isEmpty()) continue;
                // ImageJ instantiates a fresh Analyzer for every command, and
                // its constructor refreshes rt.setPrecision and
                // rt.setNaNEmptyCells from the current options (Analyzer.java
                // 57-64, 81-90). Allocation-time flags therefore come from the
                // options in force before the command runs.
                Analyzer analyzer = new Analyzer(image);
                if (op.equals("measure")) {
                    measureArea(image, table);
                } else if (op.startsWith("measurePoint:")) {
                    double[] p = numbers(op.substring("measurePoint:".length()));
                    ij.gui.Roi roi = new ij.gui.PointRoi(p[0], p[1]);
                    image.setRoi(roi);
                    callAnalyzer(analyzer, "measurePoint", roi);
                } else if (op.startsWith("measureAngle:")) {
                    double[] p = numbers(op.substring("measureAngle:".length()));
                    // An angle ROI is a three-point PolygonRoi (PolygonRoi.java 91-92).
                    int[] xs = {(int) p[0], (int) p[2], (int) p[4]};
                    int[] ys = {(int) p[1], (int) p[3], (int) p[5]};
                    ij.gui.Roi roi = new ij.gui.PolygonRoi(xs, ys, 3, ij.gui.Roi.ANGLE);
                    image.setRoi(roi);
                    callAnalyzer(analyzer, "measureAngle", roi);
                } else if (op.startsWith("measureLine:")) {
                    double[] p = numbers(op.substring("measureLine:".length()));
                    ij.gui.Roi roi = new ij.gui.Line(p[0], p[1], p[2], p[3]);
                    image.setRoi(roi);
                    callAnalyzer(analyzer, "measureLength", roi);
                } else if (op.equals("summarize")) {
                    analyzer.summarize();
                } else if (op.equals("clearSummary")) {
                    callAnalyzer(analyzer, "clearSummary", null);
                } else if (op.startsWith("set:")) {
                    String[] parts = op.substring("set:".length()).split(":");
                    setMeasurements(analyzer, table, Integer.parseInt(parts[0]), Integer.parseInt(parts[1]));
                } else if (op.startsWith("update:")) {
                    table.update(Integer.parseInt(op.substring("update:".length())), image, null);
                } else if (op.equals("disableLabels")) {
                    table.disableRowLabels();
                } else if (op.equals("clear")) {
                    Analyzer.resetCounter();
                } else {
                    throw new IllegalArgumentException("Unknown resultsLifecycle op: " + op);
                }
            }
            Method labels = ResultsTable.class.getDeclaredMethod("getRowLabels");
            labels.setAccessible(true);
            return "size=" + table.size()
                + ";labels=" + (labels.invoke(table) != null ? 1 : 0)
                + ";resultsWindow=" + (IJ.isResultsWindow() ? 1 : 0);
        } catch (ReflectiveOperationException e) {
            throw new IllegalStateException("Could not drive the Results lifecycle", e);
        }
    }

    private static void installResultsPanel() throws ReflectiveOperationException {
        Field field = IJ.class.getDeclaredField("textPanel");
        field.setAccessible(true);
        if (field.get(null) == null)
            field.set(null, new ij.text.TextPanel("Results"));
    }

    private static void measureArea(ImagePlus image, ResultsTable table) {
        int measurements = Analyzer.getMeasurements();
        ij.process.ImageStatistics stats = image.getStatistics(measurements);
        new Analyzer(image, measurements, table).saveResults(stats, image.getRoi());
    }

    private static void callAnalyzer(Analyzer analyzer, String name, ij.gui.Roi roi)
            throws ReflectiveOperationException {
        Method method = roi == null
            ? Analyzer.class.getDeclaredMethod(name)
            : Analyzer.class.getDeclaredMethod(name, ij.gui.Roi.class);
        method.setAccessible(true);
        if (roi == null) method.invoke(analyzer);
        else method.invoke(analyzer, roi);
    }

    private static double[] numbers(String text) {
        String[] parts = text.split(",");
        double[] values = new double[parts.length];
        for (int i = 0; i < parts.length; i++) values[i] = Double.parseDouble(parts[i].trim());
        return values;
    }

    /**
     * Reproduces Analyzer.doSetDialog's effect on the table (Analyzer.java
     * 205-255): setOptions(gd) followed by the decimal-place block. The dialog
     * is built with the same two checkbox groups so setOptions reads the real
     * fields in the order of Analyzer.list (Analyzer.java 29-32).
     */
    private static void setMeasurements(Analyzer analyzer, ResultsTable table, int flags, int precision)
            throws ReflectiveOperationException {
        int oldMeasurements = Analyzer.getMeasurements();
        ij.gui.GenericDialog dialog = new ij.gui.GenericDialog("Set Measurements");
        int[] first = {Analyzer.AREA, Analyzer.MEAN, Analyzer.STD_DEV, Analyzer.MODE, Analyzer.MIN_MAX,
            Analyzer.CENTROID, Analyzer.CENTER_OF_MASS, Analyzer.PERIMETER, Analyzer.RECT, Analyzer.ELLIPSE,
            Analyzer.SHAPE_DESCRIPTORS, Analyzer.FERET, Analyzer.INTEGRATED_DENSITY, Analyzer.MEDIAN,
            Analyzer.SKEWNESS, Analyzer.KURTOSIS, Analyzer.AREA_FRACTION, Analyzer.STACK_POSITION};
        String[] labels = new String[first.length];
        boolean[] states = new boolean[first.length];
        for (int i = 0; i < first.length; i++) {
            labels[i] = "option" + i;
            states[i] = (flags & first[i]) != 0;
        }
        dialog.addCheckboxGroup(10, 2, labels, states);
        int[] second = {Analyzer.LIMIT, Analyzer.LABELS, Analyzer.INVERT_Y, Analyzer.SCIENTIFIC_NOTATION,
            Analyzer.ADD_TO_OVERLAY, Analyzer.NaN_EMPTY_CELLS};
        labels = new String[second.length];
        states = new boolean[second.length];
        for (int i = 0; i < second.length; i++) {
            labels[i] = "option" + (first.length + i);
            states[i] = (flags & second[i]) != 0;
        }
        dialog.addCheckboxGroup(3, 2, labels, states);
        dialog.addChoice("Redirect to:", new String[] {"None"}, "None");
        dialog.addNumericField("Decimal places (0-9):", precision, 0, 2, "");
        Method setOptions = Analyzer.class.getDeclaredMethod("setOptions", ij.gui.GenericDialog.class);
        setOptions.setAccessible(true);
        setOptions.invoke(analyzer, dialog);
        dialog.getNextChoiceIndex();
        int places = (int) dialog.getNextNumber();
        if (places < 0) places = 0;
        if (places > 9) places = 9;
        boolean notationChanged = (oldMeasurements & Analyzer.SCIENTIFIC_NOTATION)
            != (Analyzer.getMeasurements() & Analyzer.SCIENTIFIC_NOTATION);
        if (places != Analyzer.getPrecision() || notationChanged) {
            Analyzer.setPrecision(places);
            table.setPrecision((Analyzer.getMeasurements() & Analyzer.SCIENTIFIC_NOTATION) != 0 ? -places : places);
            if (table.size() > 0) table.show("Results");
        }
    }
}
