package tools.benchmark;

import ij.IJ;
import ij.ImagePlus;
import ij.ImageStack;

/** Runs the same ImageJ 1.54u commands as ij-cli bench-worker. */
public final class BenchRunner {
    private static final long FNV_OFFSET = 0xcbf29ce484222325L;
    private static final long FNV_PRIME = 0x100000001b3L;

    private BenchRunner() {}

    public static void main(String[] args) {
        if (args.length != 6) {
            System.err.println("Usage: BenchRunner <case> <width> <height> <slices> <iterations> <warmup>");
            System.exit(2);
        }
        try {
            String benchmarkCase = args[0];
            int width = Integer.parseInt(args[1]);
            int height = Integer.parseInt(args[2]);
            int slices = Integer.parseInt(args[3]);
            int iterations = Integer.parseInt(args[4]);
            int warmup = Integer.parseInt(args[5]);
            if (width < 1 || height < 1 || slices < 1 || iterations < 1 || warmup < 0)
                throw new IllegalArgumentException("dimensions and iterations must be positive, warmup nonnegative");

            ImagePlus warmupImage = IJ.createImage("warmup", "8-bit ramp", width, height, slices);
            for (int i = 0; i < warmup; i++)
                runCase(warmupImage, benchmarkCase);
            ImagePlus image = IJ.createImage("bench", "8-bit ramp", width, height, slices);
            long started = System.nanoTime();
            for (int i = 0; i < iterations; i++)
                runCase(image, benchmarkCase);
            long elapsed = System.nanoTime() - started;
            String fingerprint = fingerprint(image);
            System.out.printf(
                "BENCH_RESULT={\"case\":\"%s\",\"width\":%d,\"height\":%d,\"slices\":%d,\"iterations\":%d,\"elapsed_ns\":%d,\"fingerprint\":\"%s\"}%n",
                benchmarkCase, image.getWidth(), image.getHeight(), image.getStackSize(),
                iterations, elapsed, fingerprint);
            System.exit(0);
        } catch (Exception e) {
            e.printStackTrace(System.err);
            System.exit(1);
        }
    }

    private static void runCase(ImagePlus image, String benchmarkCase) {
        switch (benchmarkCase) {
            case "invert": IJ.run(image, "Invert", "stack"); break;
            case "gaussian": IJ.run(image, "Gaussian Blur...", "sigma=2 stack"); break;
            case "rotate90": IJ.run(image, "Rotate 90 Degrees Right", ""); break;
            default: throw new IllegalArgumentException("unknown case: " + benchmarkCase);
        }
    }

    private static long addInt(long hash, int value) {
        for (int shift = 0; shift < 32; shift += 8)
            hash = (hash ^ ((value >>> shift) & 0xff)) * FNV_PRIME;
        return hash;
    }

    private static String fingerprint(ImagePlus image) {
        long hash = FNV_OFFSET;
        hash = addInt(hash, image.getWidth());
        hash = addInt(hash, image.getHeight());
        ImageStack stack = image.getStack();
        hash = addInt(hash, stack.getSize());
        for (int slice = 1; slice <= stack.getSize(); slice++) {
            byte[] pixels = (byte[])stack.getPixels(slice);
            for (byte pixel : pixels)
                hash = (hash ^ (pixel & 0xff)) * FNV_PRIME;
        }
        return String.format("%016x", hash);
    }
}
