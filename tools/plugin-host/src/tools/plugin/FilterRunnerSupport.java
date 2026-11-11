package tools.plugin;

import ij.IJ;
import ij.ImagePlus;
import ij.WindowManager;
import ij.gui.Roi;
import ij.measure.Calibration;
import ij.plugin.filter.PlugInFilter;
import ij.plugin.filter.PlugInFilterRunner;
import ij.process.LUT;
import java.io.File;
import java.io.IOException;
import java.nio.file.AtomicMoveNotSupportedException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.nio.file.StandardCopyOption;
import java.util.Arrays;

/**
 * The deliberately narrow image-processing subset of the plugin host.
 *
 * Supported: 8-bit grayscale, 16-bit grayscale, 32-bit float and RGB images,
 * optionally as a plain single-channel Z-stack, with an optional in-bounds
 * rectangular ROI, default spatial calibration, and display range metadata
 * that survives the TIFF round trip unchanged. PlugInFilterRunner itself
 * decides between the whole stack (DOES_STACKS) and the single current slice.
 * Everything else is rejected before an output file is published. The actual
 * processing call is the unmodified ImageJ 1.54u PlugInFilterRunner.
 */
final class FilterRunnerSupport {

    private FilterRunnerSupport() {
    }

    static final class Result {
        final int flags;
        final int width;
        final int height;
        final int runInvocations;
        final boolean finalSetupCalled;

        Result(
                int flags,
                int width,
                int height,
                int runInvocations,
                boolean finalSetupCalled) {
            this.flags = flags;
            this.width = width;
            this.height = height;
            this.runInvocations = runInvocations;
            this.finalSetupCalled = finalSetupCalled;
        }

        boolean processed() {
            return runInvocations > 0;
        }
    }

    static Result run(
            Class<?> clazz,
            String command,
            String arg,
            String inputImagePath,
            String outputImagePath,
            String roiSpec,
            String currentSliceSpec) throws Exception {

        if (inputImagePath == null) {
            throw new UnsupportedFilterException(
                    "PlugInFilter execution requires --input-image; no-image PlugInFilter "
                    + "semantics are not supported by this spike");
        }
        if (outputImagePath == null) {
            throw new UnsupportedFilterException(
                    "PlugInFilter execution requires --output-image in this spike");
        }

        ImagePlus imp = IJ.openImage(inputImagePath);
        if (imp == null) {
            throw new FilterIoException(
                    "ImageJ could not open the input image: " + inputImagePath);
        }

        validateInputImage(imp);
        configureRoi(imp, roiSpec);
        configureCurrentSlice(imp, currentSliceSpec);

        Roi inputRoi = copyRoi(imp.getRoi());
        int width = imp.getWidth();
        int height = imp.getHeight();
        int type = imp.getType();
        int stackSize = imp.getStackSize();
        double displayMin = imp.getDisplayRangeMin();
        double displayMax = imp.getDisplayRangeMax();
        byte[] inputLut = lutBytes(imp);

        Object instance = clazz.getDeclaredConstructor().newInstance();
        if (!(instance instanceof PlugInFilter)) {
            throw new UnsupportedFilterException(
                    "class '" + clazz.getName() + "' does not implement ij.plugin.filter.PlugInFilter");
        }

        NarrowPlugInFilter filter = new NarrowPlugInFilter((PlugInFilter) instance);
        WindowManager.setTempCurrentImage(imp);
        try {
            // The unmodified ImageJ 1.54u runner calls setup(), checks the
            // flags/image type, installs the ROI on the processor, and calls
            // run(ip) for the current slice or for every slice of a stack
            // when DOES_STACKS is set. The wrapper only observes setup()'s
            // flags and run calls.
            new PlugInFilterRunner(filter, command == null ? "" : command, arg == null ? "" : arg);
        } finally {
            WindowManager.setTempCurrentImage(null);
        }

        if (!filter.setupCalled()) {
            throw new UnsupportedFilterException(
                    "PlugInFilterRunner did not call setup(); refusing to claim filter compatibility");
        }

        Result result = new Result(
                filter.flags(),
                width,
                height,
                filter.runInvocations(),
                filter.finalSetupCalled());

        // DONE, a failed capability check, or a failed image lock makes the
        // runner return without ever calling run(ip). That is not pixel
        // processing, so it must not be reported or saved as a success.
        if (!result.processed()) {
            return result;
        }

        validateOutputImage(
                imp, type, width, height, stackSize, displayMin, displayMax, inputRoi, inputLut);

        publishOutputImage(imp, outputImagePath);
        return result;
    }

    /**
     * Pins the one-based current slice. A TIFF does not carry ImageJ's
     * current-slice state, so the Rust side transfers it explicitly. A filter
     * without DOES_STACKS then processes exactly this slice, like the
     * interactive Path>Filter behavior.
     */
    private static void configureCurrentSlice(ImagePlus imp, String currentSliceSpec) {
        int currentSlice = 1;
        if (currentSliceSpec != null) {
            try {
                currentSlice = Integer.parseInt(currentSliceSpec.trim());
            } catch (NumberFormatException e) {
                throw new UnsupportedFilterException(
                        "invalid --current-slice '" + currentSliceSpec + "'; expected an integer");
            }
        }
        int stackSize = imp.getStackSize();
        if (currentSlice < 1 || currentSlice > stackSize) {
            throw new UnsupportedFilterException(
                    "unsupported --current-slice '" + currentSliceSpec + "': the stack has "
                    + stackSize + " slice(s)");
        }
        if (currentSlice != imp.getCurrentSlice()) {
            imp.setSlice(currentSlice);
        }
    }

    private static void validateInputImage(ImagePlus imp) {
        if (imp.getProcessor() == null) {
            throw new UnsupportedFilterException("input image has no processor");
        }
        int type = imp.getType();
        if (type != ImagePlus.GRAY8
                && type != ImagePlus.GRAY16
                && type != ImagePlus.GRAY32
                && type != ImagePlus.COLOR_RGB) {
            throw new UnsupportedFilterException(
                    "the narrow PlugInFilter host supports 8-bit/16-bit/32-bit grayscale and "
                    + "RGB images; got " + typeName(type));
        }
        // Only plain single-channel Z-stacks can be transferred; a C or T
        // hyperstack dimension could not round-trip through the runner's
        // plain-stack path.
        if (imp.isHyperStack()
                || imp.getNChannels() != 1
                || imp.getNFrames() != 1
                || imp.getNSlices() != imp.getStackSize()) {
            throw new UnsupportedFilterException(
                    "the narrow PlugInFilter host does not support hyperstacks; got "
                    + imp.getNChannels() + "x" + imp.getNSlices() + "x" + imp.getNFrames());
        }
        requireSupportedMetadata(imp, "input");
        // FileSaver stores the display range of 16/32-bit images in the
        // ImageDescription (FileSaver.java 775-784); it must round-trip.
        if (!Double.isFinite(imp.getDisplayRangeMin()) || !Double.isFinite(imp.getDisplayRangeMax())) {
            throw new UnsupportedFilterException(
                    "input image has a non-finite display range (" + imp.getDisplayRangeMin()
                    + ".." + imp.getDisplayRangeMax() + ")");
        }
        if (type == ImagePlus.GRAY8 && !isDefaultGrayLut(imp)) {
            throw new UnsupportedFilterException(
                    "input image has a non-default 8-bit LUT that this spike cannot preserve");
        }
    }

    private static void requireSupportedMetadata(ImagePlus imp, String stage) {
        Calibration cal = imp.getCalibration();
        if (cal.pixelWidth != 1.0
                || cal.pixelHeight != 1.0
                || cal.pixelDepth != 1.0
                || !"pixel".equals(cal.getUnit())
                || !"pixel".equals(cal.getXUnit())
                || !"pixel".equals(cal.getYUnit())
                || !"pixel".equals(cal.getZUnit())
                || cal.getInvertY()
                || cal.getFunction() != Calibration.NONE
                || cal.getCoefficients() != null
                || cal.info != null
                || cal.xOrigin != 0.0
                || cal.yOrigin != 0.0
                || cal.zOrigin != 0.0
                || cal.frameInterval != 0.0
                || cal.fps != 0.0
                || cal.loop
                || !Calibration.DEFAULT_VALUE_UNIT.equals(cal.getValueUnit())
                || !"sec".equals(cal.getTimeUnit())) {
            throw new UnsupportedFilterException(
                    stage + " image has spatial/density calibration metadata that this spike "
                    + "cannot preserve");
        }
        if (imp.getProperty("Info") != null) {
            throw new UnsupportedFilterException(
                    stage + " image has an ImageJ 'Info' metadata property that this spike "
                    + "cannot preserve");
        }
        if (imp.getOverlay() != null) {
            throw new UnsupportedFilterException(
                    stage + " image has an overlay that this spike cannot preserve");
        }
    }

    private static boolean isDefaultGrayLut(ImagePlus imp) {
        LUT lut = imp.getProcessor().getLut();
        if (lut == null || lut.getMapSize() != 256) {
            return false;
        }
        byte[] bytes = lut.getBytes();
        if (bytes == null || bytes.length != 256 * 3) {
            return false;
        }
        for (int i = 0; i < 256; i++) {
            byte value = (byte) i;
            if (bytes[i] != value || bytes[256 + i] != value || bytes[512 + i] != value) {
                return false;
            }
        }
        return true;
    }

    private static byte[] lutBytes(ImagePlus imp) {
        LUT lut = imp.getProcessor().getLut();
        return lut == null ? null : lut.getBytes();
    }

    private static void configureRoi(ImagePlus imp, String roiSpec) {
        Roi existing = imp.getRoi();
        if (roiSpec == null) {
            if (existing != null) {
                throw new UnsupportedFilterException(
                        "input image already contains a selection; the narrow host only "
                        + "accepts an explicit rectangular --roi");
            }
            return;
        }
        if (existing != null) {
            imp.deleteRoi();
        }

        String[] parts = roiSpec.split(",");
        if (parts.length != 4) {
            throw new UnsupportedFilterException(
                    "invalid --roi '" + roiSpec + "'; expected x,y,width,height");
        }
        int x;
        int y;
        int width;
        int height;
        try {
            x = Integer.parseInt(parts[0].trim());
            y = Integer.parseInt(parts[1].trim());
            width = Integer.parseInt(parts[2].trim());
            height = Integer.parseInt(parts[3].trim());
        } catch (NumberFormatException e) {
            throw new UnsupportedFilterException(
                    "invalid --roi '" + roiSpec + "'; expected integer x,y,width,height");
        }
        if (width <= 0 || height <= 0) {
            throw new UnsupportedFilterException(
                    "invalid --roi '" + roiSpec + "'; width and height must be positive");
        }
        if (x < 0 || y < 0 || x + width > imp.getWidth() || y + height > imp.getHeight()) {
            throw new UnsupportedFilterException(
                    "unsupported --roi '" + roiSpec + "': the narrow host requires a rectangle "
                    + "fully inside the " + imp.getWidth() + "x" + imp.getHeight() + " image");
        }
        imp.setRoi(new Roi(x, y, width, height));
        Roi installed = imp.getRoi();
        if (installed == null || installed.getType() != Roi.RECTANGLE) {
            throw new UnsupportedFilterException(
                    "the narrow host supports only rectangular selections");
        }
    }

    private static Roi copyRoi(Roi roi) {
        if (roi == null) {
            return null;
        }
        return new Roi(roi.getBounds());
    }

    private static void validateOutputImage(
            ImagePlus imp,
            int expectedType,
            int expectedWidth,
            int expectedHeight,
            int expectedStackSize,
            double expectedDisplayMin,
            double expectedDisplayMax,
            Roi inputRoi,
            byte[] inputLut) {
        if (imp.getType() != expectedType) {
            throw new UnsupportedFilterException(
                    "filter changed the image type to " + typeName(imp.getType())
                    + " (expected " + typeName(expectedType)
                    + "); the narrow host cannot return it");
        }
        if (imp.getWidth() != expectedWidth || imp.getHeight() != expectedHeight) {
            throw new UnsupportedFilterException(
                    "filter changed the image dimensions to "
                    + imp.getWidth() + "x" + imp.getHeight());
        }
        if (imp.getStackSize() != expectedStackSize) {
            throw new UnsupportedFilterException(
                    "filter changed the stack size to " + imp.getStackSize()
                    + " (expected " + expectedStackSize + "); the narrow host cannot return it");
        }
        if (imp.getDisplayRangeMin() != expectedDisplayMin
                || imp.getDisplayRangeMax() != expectedDisplayMax) {
            throw new UnsupportedFilterException(
                    "filter changed the display range to "
                    + imp.getDisplayRangeMin() + ".." + imp.getDisplayRangeMax()
                    + "; this spike cannot preserve it");
        }
        requireSupportedMetadata(imp, "output");
        if (expectedType == ImagePlus.GRAY8 && !Arrays.equals(inputLut, lutBytes(imp))) {
            throw new UnsupportedFilterException(
                    "filter changed the image LUT; this spike cannot preserve it");
        }
        if (!sameRoi(inputRoi, imp.getRoi())) {
            throw new UnsupportedFilterException(
                    "filter changed the image selection; the narrow host cannot preserve it");
        }
    }

    private static boolean sameRoi(Roi a, Roi b) {
        if (a == null || b == null) {
            return a == b;
        }
        return a.getType() == b.getType() && a.getBounds().equals(b.getBounds());
    }

    /**
     * Save to a sibling temporary file and atomically move it into place only
     * after ImageJ reports success and the file is non-empty. A failure must
     * neither leave a partial output nor delete a pre-existing user file.
     */
    private static void publishOutputImage(ImagePlus imp, String outputImagePath) {
        Path outputPath = Paths.get(outputImagePath).toAbsolutePath();
        Path parent = outputPath.getParent();
        Path tempPath = null;
        try {
            tempPath = Files.createTempFile(parent, ".filter-output-", ".tif");
            if (!IJ.saveAsTiff(imp, tempPath.toString())) {
                throw new FilterIoException(
                        "ImageJ failed to save the filter output image: " + outputImagePath);
            }
            File outputFile = tempPath.toFile();
            if (!outputFile.isFile() || outputFile.length() == 0) {
                throw new FilterIoException(
                        "filter output image was not written: " + outputImagePath);
            }
            try {
                Files.move(
                        tempPath,
                        outputPath,
                        StandardCopyOption.REPLACE_EXISTING,
                        StandardCopyOption.ATOMIC_MOVE);
            } catch (AtomicMoveNotSupportedException e) {
                Files.move(tempPath, outputPath, StandardCopyOption.REPLACE_EXISTING);
            }
        } catch (IOException e) {
            throw new FilterIoException(
                    "failed to publish filter output image '" + outputImagePath
                    + "': " + e.getMessage(), e);
        } finally {
            if (tempPath != null) {
                try {
                    Files.deleteIfExists(tempPath);
                } catch (IOException ignored) {
                }
            }
        }
    }

    private static String typeName(int type) {
        switch (type) {
            case ImagePlus.GRAY8:
                return "8-bit grayscale";
            case ImagePlus.GRAY16:
                return "16-bit grayscale";
            case ImagePlus.GRAY32:
                return "32-bit float grayscale";
            case ImagePlus.COLOR_256:
                return "8-bit indexed color";
            case ImagePlus.COLOR_RGB:
                return "RGB color";
            default:
                return "type " + type;
        }
    }
}
