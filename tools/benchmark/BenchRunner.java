package tools.benchmark;

import ij.IJ;
import ij.ImagePlus;
import ij.ImageStack;
import ij.Prefs;

/**
 * Runs the same ImageJ 1.54u commands as {@code ij-cli bench-worker}.
 *
 * <p>The input generator is duplicated in {@code crates/ij-cli/src/benchmark.rs}
 * with identical arithmetic, because ImageJ's own {@code noise} fill draws from
 * an unseeded {@code java.util.Random} and cannot be reproduced.
 */
public final class BenchRunner {
    private static final long FNV_OFFSET = 0xcbf29ce484222325L;
    private static final long FNV_PRIME = 0x100000001b3L;
    private static final long DEFAULT_SEED = 0x2545F4914F6CDD1DL;
    private static final double MAX_SIGMA = 5.0e8;
    private static final long IMPULSE_MASK = 0x1ffL;
    private static final long MAX_PIXELS = 100_000_000L;
    private static final String USAGE =
        "Usage: BenchRunner <case> <width> <height> <slices> <iterations> <warmup>"
        + " [type=8|16|32] [sigma=<positive>] [input=noise|ramp|impulse] [seed=<u64>] [threads=<n>]";

    private BenchRunner() {}

    /** xorshift64*, duplicated bit-for-bit in the Rust worker. */
    private static final class Rng {
        private long state;

        Rng(long seed) {
            state = seed | 1L;
        }

        long next() {
            long x = state;
            x ^= x >>> 12;
            x ^= x << 25;
            x ^= x >>> 27;
            state = x;
            return x * 0x2545F4914F6CDD1DL;
        }
    }

    private static final class Config {
        String benchmarkCase;
        int width;
        int height;
        int slices;
        int iterations;
        int warmup;
        int type = 8;
        double sigma = 2.0;
        String input = "noise";
        long seed = DEFAULT_SEED;
        Integer threads;
    }

    public static void main(String[] args) {
        try {
            Config config = parseConfig(args);
            // IJ.run lazily loads Prefs and the command table (IJ.java 410-426).
            // Resolve thread preferences only after that initialization.
            ImagePlus initializationImage = IJ.createImage("init", "8-bit black", 1, 1, 1);
            IJ.run(initializationImage, "Select None", "");
            initializationImage.flush();
            if (config.threads != null)
                Prefs.setThreads(config.threads);
            // The budget GaussianBlur is given, not the number of threads that
            // ran: blur1Direction caps it by the line count and by a task-size
            // estimate (GaussianBlur.java 273-277), so small images use fewer.
            int resolvedThreads = Prefs.getThreads();

            ImagePlus warmupImage = createImage(config);
            for (int i = 0; i < config.warmup; i++)
                runCase(warmupImage, config);
            warmupImage.flush();
            ImagePlus image = createImage(config);
            long started = System.nanoTime();
            for (int i = 0; i < config.iterations; i++)
                runCase(image, config);
            long elapsed = System.nanoTime() - started;
            String fingerprint = fingerprint(image, config.type);
            boolean gaussian = config.benchmarkCase.equals("gaussian");
            String threads = String.format(
                "{\"requested\":%s,\"resolved\":%s,\"policy\":\"%s\"}",
                config.threads == null ? "null" : Integer.toString(config.threads),
                gaussian ? Integer.toString(resolvedThreads) : "null",
                gaussian ? "prefs" : "sequential");
            System.out.printf(
                "BENCH_RESULT={\"case\":\"%s\",\"type\":%d,\"sigma\":%s,\"input\":\"%s\",\"seed\":%s,"
                + "\"width\":%d,\"height\":%d,\"slices\":%d,\"iterations\":%d,\"elapsed_ns\":%d,"
                + "\"fingerprint\":\"%s\",\"threads\":%s,\"machine_threads\":%d}%n",
                config.benchmarkCase, config.type, Double.toString(config.sigma), config.input,
                Long.toUnsignedString(config.seed), image.getWidth(), image.getHeight(),
                image.getStackSize(), config.iterations, elapsed, fingerprint, threads,
                Runtime.getRuntime().availableProcessors());
            System.exit(0);
        } catch (Exception e) {
            e.printStackTrace(System.err);
            System.exit(1);
        }
    }

    private static Config parseConfig(String[] args) {
        if (args.length < 6)
            throw new IllegalArgumentException(USAGE);
        Config config = new Config();
        config.benchmarkCase = args[0];
        if (!config.benchmarkCase.equals("invert") && !config.benchmarkCase.equals("gaussian")
                && !config.benchmarkCase.equals("rotate90"))
            throw new IllegalArgumentException("unknown benchmark case: " + config.benchmarkCase);
        config.width = parsePositive(args[1], "width");
        config.height = parsePositive(args[2], "height");
        config.slices = parsePositive(args[3], "slices");
        config.iterations = parsePositive(args[4], "iterations");
        config.warmup = parseNonNegative(args[5], "warmup");
        if ((long) config.width * config.height > MAX_PIXELS / config.slices)
            throw new IllegalArgumentException("input exceeds " + MAX_PIXELS + " pixels");

        for (int i = 6; i < args.length; i++) {
            String option = args[i];
            int separator = option.indexOf('=');
            if (separator < 0)
                throw new IllegalArgumentException("expected key=value option, got " + option);
            String key = option.substring(0, separator);
            String value = option.substring(separator + 1);
            switch (key) {
                case "type":
                    if (value.equals("8") || value.equals("gray8"))
                        config.type = 8;
                    else if (value.equals("16") || value.equals("gray16"))
                        config.type = 16;
                    else if (value.equals("32") || value.equals("gray32"))
                        config.type = 32;
                    else
                        throw new IllegalArgumentException(
                            "unknown type: " + value + " (expected 8, 16 or 32)");
                    break;
                case "sigma":
                    try {
                        config.sigma = Double.parseDouble(value);
                    } catch (NumberFormatException e) {
                        throw new IllegalArgumentException("sigma must be a number, got " + value);
                    }
                    break;
                case "input":
                    if (!value.equals("noise") && !value.equals("ramp") && !value.equals("impulse"))
                        throw new IllegalArgumentException(
                            "unknown input: " + value + " (expected noise, ramp or impulse)");
                    config.input = value;
                    break;
                case "seed":
                    try {
                        config.seed = Long.parseUnsignedLong(value);
                    } catch (NumberFormatException e) {
                        throw new IllegalArgumentException(
                            "seed must be an unsigned 64-bit integer, got " + value);
                    }
                    break;
                case "threads":
                    int count = parsePositive(value, "threads");
                    config.threads = count;
                    break;
                default:
                    throw new IllegalArgumentException("unknown option: " + key);
            }
        }
        if (Double.isNaN(config.sigma) || Double.isInfinite(config.sigma)
                || config.sigma <= 0.0 || config.sigma > MAX_SIGMA)
            throw new IllegalArgumentException("sigma must be finite, positive and at most "
                + MAX_SIGMA + ", got " + config.sigma);
        return config;
    }

    private static int parsePositive(String value, String name) {
        int parsed = parseNonNegative(value, name);
        if (parsed < 1)
            throw new IllegalArgumentException(name + " must be positive, got " + value);
        return parsed;
    }

    private static int parseNonNegative(String value, String name) {
        try {
            int parsed = Integer.parseInt(value);
            if (parsed < 0)
                throw new IllegalArgumentException(name + " must be non-negative, got " + value);
            return parsed;
        } catch (NumberFormatException e) {
            throw new IllegalArgumentException(name + " must be an integer, got " + value);
        }
    }

    private static ImagePlus createImage(Config config) {
        String typeName = config.type == 8 ? "8-bit" : config.type == 16 ? "16-bit" : "32-bit";
        String fillName = config.input.equals("ramp") ? "ramp" : "black";
        ImagePlus image = IJ.createImage(
            "bench", typeName + " " + fillName, config.width, config.height, config.slices);
        if (image == null)
            throw new IllegalStateException("ImageJ could not allocate the benchmark image");
        if (!config.input.equals("ramp"))
            fillDeterministic(image, config);
        return image;
    }

    /**
     * Writes the seeded input in slice-major, row-major order. The Rust worker
     * draws in the same order, so both engines start from identical pixels.
     */
    private static void fillDeterministic(ImagePlus image, Config config) {
        int n = image.getWidth() * image.getHeight();
        boolean noise = config.input.equals("noise");
        Rng rng = new Rng(config.seed);
        for (int slice = 1; slice <= image.getStackSize(); slice++) {
            if (config.type == 8) {
                byte[] pixels = new byte[n];
                for (int i = 0; i < n; i++) {
                    long draw = rng.next();
                    pixels[i] = noise ? (byte) (draw >>> 56) : (byte) impulse(draw, 255);
                }
                setSlicePixels(image, slice, pixels);
            } else if (config.type == 16) {
                short[] pixels = new short[n];
                for (int i = 0; i < n; i++) {
                    long draw = rng.next();
                    pixels[i] = noise ? (short) (draw >>> 48) : (short) impulse(draw, 65535);
                }
                setSlicePixels(image, slice, pixels);
            } else {
                float[] pixels = new float[n];
                for (int i = 0; i < n; i++) {
                    long draw = rng.next();
                    // (value & 0xffff) / 256 is exact in binary floating point,
                    // so Java and Rust agree bit for bit.
                    pixels[i] = noise
                        ? (float) ((draw >>> 40) & 0xFFFFL) / 256.0f
                        : (float) impulse(draw, 255);
                }
                setSlicePixels(image, slice, pixels);
            }
        }
    }

    private static long impulse(long draw, long peak) {
        return (draw & IMPULSE_MASK) == 0 ? peak : 0;
    }

    private static void setSlicePixels(ImagePlus image, int slice, Object pixels) {
        if (image.getStackSize() == 1)
            image.getProcessor().setPixels(pixels);
        else
            image.getStack().setPixels(pixels, slice);
    }

    private static Object slicePixels(ImagePlus image, int slice) {
        return image.getStackSize() == 1
            ? image.getProcessor().getPixels()
            : image.getStack().getPixels(slice);
    }

    private static void runCase(ImagePlus image, Config config) {
        switch (config.benchmarkCase) {
            case "invert": IJ.run(image, "Invert", "stack"); break;
            case "gaussian":
                IJ.run(image, "Gaussian Blur...", "sigma=" + config.sigma + " stack");
                break;
            case "rotate90": IJ.run(image, "Rotate 90 Degrees Right", ""); break;
            default: throw new IllegalArgumentException("unknown case: " + config.benchmarkCase);
        }
    }

    private static long addInt(long hash, int value) {
        for (int shift = 0; shift < 32; shift += 8)
            hash = (hash ^ ((value >>> shift) & 0xff)) * FNV_PRIME;
        return hash;
    }

    /**
     * FNV-1a over the same raw representation as the Rust worker: dimensions
     * and bit depth as little-endian ints, then every pixel as little-endian
     * bytes of its stored value. Floats go through
     * {@link Float#floatToRawIntBits}, so {@code -0.0} and {@code 0.0} differ
     * and two NaNs with different payloads differ.
     */
    private static String fingerprint(ImagePlus image, int type) {
        long hash = FNV_OFFSET;
        hash = addInt(hash, image.getWidth());
        hash = addInt(hash, image.getHeight());
        hash = addInt(hash, image.getStackSize());
        hash = addInt(hash, type);
        for (int slice = 1; slice <= image.getStackSize(); slice++) {
            Object pixels = slicePixels(image, slice);
            if (type == 8) {
                for (byte pixel : (byte[]) pixels)
                    hash = (hash ^ (pixel & 0xff)) * FNV_PRIME;
            } else if (type == 16) {
                for (short pixel : (short[]) pixels) {
                    int value = pixel & 0xffff;
                    hash = (hash ^ (value & 0xff)) * FNV_PRIME;
                    hash = (hash ^ ((value >>> 8) & 0xff)) * FNV_PRIME;
                }
            } else {
                for (float pixel : (float[]) pixels)
                    hash = addInt(hash, Float.floatToRawIntBits(pixel));
            }
        }
        return String.format("%016x", hash);
    }
}
