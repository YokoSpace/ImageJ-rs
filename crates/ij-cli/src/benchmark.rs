//! `ij-cli bench-worker`: one benchmark sample for the Java/Rust comparison
//! driven by `tools/benchmark/compare.py`.
//!
//! The worker prints exactly one `BENCH_RESULT=<json>` line. Everything the
//! comparison needs in order to prove that both engines processed the same
//! input lives in that line: the resolved configuration, the elapsed time and
//! a fingerprint over the raw pixel representation.
//!
//! The input generator is duplicated in `tools/benchmark/BenchRunner.java`
//! with identical arithmetic, because ImageJ's own `noise` fill draws from an
//! unseeded `java.util.Random` and therefore cannot be reproduced.

use ij_core::new_image::{NewImageFill, NewImageType};
use ij_core::{ImageDocument, PixelPlane};
use ij_session::{Command, Session};
use std::error::Error;
use std::time::Instant;

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Seed used when `seed=` is absent. Any value works; this one is the
/// xorshift64* multiplier so the first draw is not degenerate.
const DEFAULT_SEED: u64 = 0x2545_f491_4f6c_dd1d;

/// `ij-core` rejects sigma above this because ImageJ's kernel radius
/// `(int)Math.ceil(sigma*sqrt(-2*ln(accuracy)))+1` overflows an int there
/// (GaussianBlur.java 536-574).
const MAX_SIGMA: f64 = 5.0e8;

/// One impulse per 512 pixels for `input=impulse`.
const IMPULSE_MASK: u64 = 0x1ff;

/// Same ceiling `compare.py` enforces, so a worker never allocates a stack the
/// harness would have rejected.
const MAX_PIXELS: u64 = 100_000_000;

const USAGE: &str = "usage: ij-cli bench-worker <case> <width> <height> <slices> <iterations> <warmup> \
[type=8|16|32] [sigma=<positive>] [input=noise|ramp|impulse] [seed=<u64>] [threads=<n>]";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BenchType {
    Gray8,
    Gray16,
    Gray32,
}

impl BenchType {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "8" | "gray8" => Ok(Self::Gray8),
            "16" | "gray16" => Ok(Self::Gray16),
            "32" | "gray32" => Ok(Self::Gray32),
            other => Err(format!("unknown type: {other} (expected 8, 16 or 32)")),
        }
    }

    fn bits(self) -> u32 {
        match self {
            Self::Gray8 => 8,
            Self::Gray16 => 16,
            Self::Gray32 => 32,
        }
    }

    fn new_image_type(self) -> NewImageType {
        match self {
            Self::Gray8 => NewImageType::Gray8,
            Self::Gray16 => NewImageType::Gray16,
            Self::Gray32 => NewImageType::Gray32,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BenchInput {
    Noise,
    Ramp,
    Impulse,
}

impl BenchInput {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "noise" => Ok(Self::Noise),
            "ramp" => Ok(Self::Ramp),
            "impulse" => Ok(Self::Impulse),
            other => Err(format!(
                "unknown input: {other} (expected noise, ramp or impulse)"
            )),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Noise => "noise",
            Self::Ramp => "ramp",
            Self::Impulse => "impulse",
        }
    }
}

#[derive(Debug, Clone)]
struct Config {
    case: String,
    width: u32,
    height: u32,
    slices: u32,
    iterations: u32,
    warmup: u32,
    image_type: BenchType,
    sigma: f64,
    input: BenchInput,
    seed: u64,
    threads: Option<usize>,
}

/// xorshift64*, duplicated bit-for-bit in `BenchRunner.java`. Java `long`
/// arithmetic wraps and `>>>` is a logical shift, so both sides produce the
/// same stream.
struct BenchRng {
    state: u64,
}

impl BenchRng {
    fn new(seed: u64) -> Self {
        Self { state: seed | 1 }
    }

    fn next(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
}

pub fn run(args: &[String]) -> Result<(), Box<dyn Error>> {
    let config = parse_config(args).map_err(|message| -> Box<dyn Error> { message.into() })?;

    let mut warmup_session = new_session(&config)?;
    for _ in 0..config.warmup {
        run_case(&mut warmup_session, &config)?;
    }
    drop(warmup_session);

    let mut session = new_session(&config)?;
    let started = Instant::now();
    for _ in 0..config.iterations {
        run_case(&mut session, &config)?;
    }
    let elapsed_ns = started.elapsed().as_nanos();
    let doc = session.active_doc()?;
    let fingerprint = fingerprint(doc, config.image_type)?;
    let (resolved_threads, thread_policy) = executor_budget(&config);
    let result = serde_json::json!({
        "case": config.case,
        "type": config.image_type.bits(),
        "sigma": config.sigma,
        "input": config.input.name(),
        "seed": config.seed,
        "width": doc.width,
        "height": doc.height,
        "slices": doc.stack_size(),
        "iterations": config.iterations,
        "elapsed_ns": elapsed_ns,
        "fingerprint": fingerprint,
        "threads": {
            "requested": config.threads,
            "resolved": resolved_threads,
            "policy": thread_policy,
        },
        "machine_threads": machine_threads(),
    });
    println!("BENCH_RESULT={result}");
    Ok(())
}

/// Reports the worker budget the Rust session actually used.
///
/// `ij-core` resolves a budget of `0` to `std::thread::available_parallelism`
/// (`filters.rs::resolve_workers`), which is the same default
/// `Prefs.getThreads()` picks in Java (Prefs.java 690-698). An explicit
/// `threads=` value is passed straight through, so the reported budget is the
/// one the blur was given rather than a guess.
///
/// The budget is not the number of OS threads that ran. `blur_one_direction`
/// caps it by the line count and by ImageJ's task-size estimate
/// (GaussianBlur.java 273-277), so a small image runs with fewer threads than
/// the budget on both engines. That cap is not observable from outside
/// `ij-core`, so it is documented rather than reported as a number.
fn executor_budget(config: &Config) -> (Option<usize>, &'static str) {
    if config.case != "gaussian" {
        // Invert and Rotate 90 run sequentially in both engines.
        return (None, "sequential");
    }
    match config.threads {
        Some(workers) => (Some(workers), "explicit-workers"),
        None => (Some(machine_threads()), "session-default"),
    }
}

fn machine_threads() -> usize {
    std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1)
}

fn parse_config(args: &[String]) -> Result<Config, String> {
    if args.len() < 6 {
        return Err(USAGE.to_string());
    }
    let case = args[0].clone();
    if !matches!(case.as_str(), "invert" | "gaussian" | "rotate90") {
        return Err(format!("unknown benchmark case: {case}"));
    }
    let width = parse_u32(&args[1], "width")?;
    let height = parse_u32(&args[2], "height")?;
    let slices = parse_u32(&args[3], "slices")?;
    let iterations = parse_u32(&args[4], "iterations")?;
    let warmup = parse_u32(&args[5], "warmup")?;
    if width == 0 || height == 0 || slices == 0 || iterations == 0 {
        return Err("dimensions and iterations must be positive".into());
    }
    if (width as u64) * (height as u64) > MAX_PIXELS / slices as u64 {
        return Err(format!("input exceeds {MAX_PIXELS} pixels"));
    }

    let mut image_type = BenchType::Gray8;
    let mut sigma = 2.0_f64;
    let mut input = BenchInput::Noise;
    let mut seed = DEFAULT_SEED;
    let mut threads = None;
    for option in &args[6..] {
        let (key, value) = option
            .split_once('=')
            .ok_or_else(|| format!("expected key=value option, got {option}"))?;
        match key {
            "type" => image_type = BenchType::parse(value)?,
            "sigma" => {
                sigma = value
                    .parse::<f64>()
                    .map_err(|_| format!("sigma must be a number, got {value}"))?;
            }
            "input" => input = BenchInput::parse(value)?,
            "seed" => {
                seed = value
                    .parse::<u64>()
                    .map_err(|_| format!("seed must be an unsigned 64-bit integer, got {value}"))?;
            }
            "threads" => {
                let count = value
                    .parse::<usize>()
                    .map_err(|_| format!("threads must be a positive integer, got {value}"))?;
                if count == 0 {
                    return Err("threads must be a positive integer".into());
                }
                threads = Some(count);
            }
            other => return Err(format!("unknown option: {other}")),
        }
    }
    if !sigma.is_finite() || sigma <= 0.0 || sigma > MAX_SIGMA {
        return Err(format!(
            "sigma must be finite, positive and at most {MAX_SIGMA}, got {sigma}"
        ));
    }

    Ok(Config {
        case,
        width,
        height,
        slices,
        iterations,
        warmup,
        image_type,
        sigma,
        input,
        seed,
        threads,
    })
}

fn parse_u32(value: &str, name: &str) -> Result<u32, String> {
    value
        .parse::<u32>()
        .map_err(|_| format!("{name} must be a non-negative integer, got {value}"))
}

fn new_session(config: &Config) -> Result<Session, Box<dyn Error>> {
    let fill = match config.input {
        BenchInput::Ramp => NewImageFill::Ramp,
        // Noise and impulse overwrite every pixel below, so the fill only has
        // to leave the display range ImageJ's `black` creation would.
        BenchInput::Noise | BenchInput::Impulse => NewImageFill::Black,
    };
    let mut doc = ImageDocument::new_image(
        "bench",
        config.image_type.new_image_type(),
        fill,
        config.width,
        config.height,
        config.slices,
        1,
    )?;
    if config.input != BenchInput::Ramp {
        fill_deterministic(&mut doc, config)?;
    }
    let mut session = Session::new();
    session.insert_doc(doc);
    Ok(session)
}

/// Writes the seeded input in slice-major, row-major order. `BenchRunner.java`
/// draws in the same order, so both engines start from identical pixels.
fn fill_deterministic(doc: &mut ImageDocument, config: &Config) -> Result<(), Box<dyn Error>> {
    let mut rng = BenchRng::new(config.seed);
    for plane in &mut doc.slices {
        match plane {
            PixelPlane::Gray8(pixels) => {
                for pixel in pixels.iter_mut() {
                    *pixel = match config.input {
                        BenchInput::Noise => (rng.next() >> 56) as u8,
                        BenchInput::Impulse => impulse_value(rng.next(), u8::MAX as u64) as u8,
                        BenchInput::Ramp => unreachable!("ramp keeps the NewImage fill"),
                    };
                }
            }
            PixelPlane::Gray16(pixels) => {
                for pixel in pixels.iter_mut() {
                    *pixel = match config.input {
                        BenchInput::Noise => (rng.next() >> 48) as u16,
                        BenchInput::Impulse => impulse_value(rng.next(), u16::MAX as u64) as u16,
                        BenchInput::Ramp => unreachable!("ramp keeps the NewImage fill"),
                    };
                }
            }
            PixelPlane::Gray32(pixels) => {
                for pixel in pixels.iter_mut() {
                    *pixel = match config.input {
                        BenchInput::Noise => {
                            // `(value & 0xffff) / 256` is exact in binary
                            // floating point, so Java and Rust agree bit for
                            // bit without any rounding question.
                            ((rng.next() >> 40) & 0xffff) as f32 / 256.0
                        }
                        BenchInput::Impulse => impulse_value(rng.next(), 255) as f32,
                        BenchInput::Ramp => unreachable!("ramp keeps the NewImage fill"),
                    };
                }
            }
            PixelPlane::Color256 { .. } | PixelPlane::ColorRgb(_) => {
                return Err("benchmark supports grayscale images only".into());
            }
        }
    }
    Ok(())
}

fn impulse_value(draw: u64, peak: u64) -> u64 {
    if draw & IMPULSE_MASK == 0 { peak } else { 0 }
}

fn run_case(session: &mut Session, config: &Config) -> Result<(), Box<dyn Error>> {
    match config.case.as_str() {
        "invert" => {
            session.dispatch(Command::InvertStack)?;
        }
        "gaussian" => match config.threads {
            // `Command::GaussianBlurStack` carries only sigma, so an explicit
            // worker budget has to go through the core entry point. Both paths
            // run the same blur; only the scheduling budget differs.
            Some(workers) => session
                .active_doc_mut()?
                .gaussian_blur_stack_with_workers(config.sigma, workers)?,
            None => {
                session.dispatch(Command::GaussianBlurStack {
                    sigma: config.sigma,
                })?;
            }
        },
        "rotate90" => {
            session.dispatch(Command::Rotate90Right)?;
        }
        _ => unreachable!("case validated by parse_config"),
    }
    Ok(())
}

fn add_int(mut hash: u64, value: u32) -> u64 {
    for shift in (0..32).step_by(8) {
        hash = (hash ^ ((value >> shift) & 0xff) as u64).wrapping_mul(FNV_PRIME);
    }
    hash
}

/// FNV-1a over a defined raw representation: dimensions and bit depth as
/// little-endian `u32`, then every pixel as little-endian bytes of its stored
/// value (`u8`, `u16`, or the raw bits of the `f32`).
///
/// Floats are hashed through `f32::to_bits`, matching Java's
/// `Float.floatToRawIntBits`, so `-0.0` and `0.0` differ and two NaNs with
/// different payloads differ. The benchmark never generates NaN or infinity
/// (`input=noise` produces exact multiples of 1/256 in `[0, 256)`), and a
/// Gaussian blur of finite values stays finite, so a NaN in the output is
/// itself a parity failure rather than a hashing ambiguity.
fn fingerprint(doc: &ImageDocument, image_type: BenchType) -> Result<String, Box<dyn Error>> {
    let mut hash = FNV_OFFSET;
    hash = add_int(hash, doc.width);
    hash = add_int(hash, doc.height);
    hash = add_int(hash, doc.stack_size() as u32);
    hash = add_int(hash, image_type.bits());
    for plane in &doc.slices {
        match (image_type, plane) {
            (BenchType::Gray8, PixelPlane::Gray8(pixels)) => {
                for &pixel in pixels {
                    hash = (hash ^ pixel as u64).wrapping_mul(FNV_PRIME);
                }
            }
            (BenchType::Gray16, PixelPlane::Gray16(pixels)) => {
                for &pixel in pixels {
                    hash = (hash ^ (pixel & 0xff) as u64).wrapping_mul(FNV_PRIME);
                    hash = (hash ^ (pixel >> 8) as u64).wrapping_mul(FNV_PRIME);
                }
            }
            (BenchType::Gray32, PixelPlane::Gray32(pixels)) => {
                for &pixel in pixels {
                    hash = add_int(hash, pixel.to_bits());
                }
            }
            _ => return Err("benchmark pixel type changed during the run".into()),
        }
    }
    Ok(format!("{hash:016x}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    fn base() -> Vec<String> {
        args(&["gaussian", "64", "48", "2", "3", "1"])
    }

    #[test]
    fn defaults_match_the_documented_baseline() {
        let config = parse_config(&base()).unwrap();
        assert_eq!(config.image_type, BenchType::Gray8);
        assert_eq!(config.sigma, 2.0);
        assert_eq!(config.input, BenchInput::Noise);
        assert_eq!(config.seed, DEFAULT_SEED);
        assert_eq!(config.threads, None);
    }

    #[test]
    fn options_override_every_knob() {
        let mut values = base();
        values.extend(args(&[
            "type=32",
            "sigma=1.5",
            "input=impulse",
            "seed=7",
            "threads=4",
        ]));
        let config = parse_config(&values).unwrap();
        assert_eq!(config.image_type, BenchType::Gray32);
        assert_eq!(config.sigma, 1.5);
        assert_eq!(config.input, BenchInput::Impulse);
        assert_eq!(config.seed, 7);
        assert_eq!(config.threads, Some(4));
    }

    #[test]
    fn invalid_options_are_rejected() {
        for option in [
            "type=24",
            "sigma=0",
            "sigma=-1",
            "sigma=nan",
            "sigma=1e9",
            "input=random",
            "seed=-1",
            "threads=0",
            "threads=x",
            "colour=red",
            "type",
        ] {
            let mut values = base();
            values.push(option.to_string());
            assert!(
                parse_config(&values).is_err(),
                "expected {option} to be rejected"
            );
        }
    }

    #[test]
    fn invalid_positional_arguments_are_rejected() {
        assert!(parse_config(&args(&["gaussian", "64", "48", "2", "3"])).is_err());
        assert!(parse_config(&args(&["blur", "64", "48", "2", "3", "1"])).is_err());
        assert!(parse_config(&args(&["gaussian", "0", "48", "2", "3", "1"])).is_err());
        assert!(parse_config(&args(&["gaussian", "64", "48", "2", "0", "1"])).is_err());
        assert!(parse_config(&args(&["gaussian", "64", "48", "2", "3", "-1"])).is_err());
        assert!(parse_config(&args(&["gaussian", "100000", "1000", "2", "3", "1"])).is_err());
        assert!(
            parse_config(&args(&[
                "gaussian",
                "4294967295",
                "4294967295",
                "4294967295",
                "1",
                "0"
            ]))
            .is_err()
        );
    }

    #[test]
    fn seeded_noise_is_reproducible_and_seed_specific() {
        let mut first = BenchRng::new(DEFAULT_SEED);
        let mut second = BenchRng::new(DEFAULT_SEED);
        for _ in 0..8 {
            assert_eq!(first.next(), second.next());
        }
        assert_ne!(BenchRng::new(1).next(), BenchRng::new(2).next());
        // A zero seed must not collapse the stream.
        assert_ne!(BenchRng::new(0).next(), 0);
    }

    #[test]
    fn impulse_input_is_sparse_and_peaks_at_the_type_maximum() {
        let mut config = parse_config(&base()).unwrap();
        config.input = BenchInput::Impulse;
        let session = new_session(&config).unwrap();
        let doc = session.active_doc().unwrap();
        let PixelPlane::Gray8(pixels) = &doc.slices[0] else {
            panic!("expected 8-bit plane");
        };
        let peaks = pixels.iter().filter(|&&pixel| pixel == u8::MAX).count();
        assert!(peaks > 0, "impulse input must contain peaks");
        assert!(
            peaks * 32 < pixels.len(),
            "impulse input must stay sparse, got {peaks} peaks"
        );
        assert!(pixels.iter().all(|&pixel| pixel == 0 || pixel == u8::MAX));
    }

    #[test]
    fn noise_input_covers_the_full_8bit_range() {
        let config = parse_config(&base()).unwrap();
        let session = new_session(&config).unwrap();
        let doc = session.active_doc().unwrap();
        let PixelPlane::Gray8(pixels) = &doc.slices[0] else {
            panic!("expected 8-bit plane");
        };
        assert!(pixels.iter().any(|&pixel| pixel < 8));
        assert!(pixels.iter().any(|&pixel| pixel > 247));
    }

    #[test]
    fn fingerprint_covers_dimensions_bit_depth_and_pixels() {
        let config = parse_config(&base()).unwrap();
        let session = new_session(&config).unwrap();
        let doc = session.active_doc().unwrap();
        let baseline = fingerprint(doc, BenchType::Gray8).unwrap();
        assert_eq!(baseline, fingerprint(doc, BenchType::Gray8).unwrap());

        // The bit depth is part of the hashed header, so the same pixels
        // declared as another type cannot collide.
        let mut wide = doc.clone();
        wide.slices = vec![PixelPlane::Gray16(vec![0; 64 * 48]); 2];
        assert_ne!(baseline, fingerprint(&wide, BenchType::Gray16).unwrap());

        let mut changed = doc.clone();
        if let PixelPlane::Gray8(pixels) = &mut changed.slices[0] {
            pixels[0] = pixels[0].wrapping_add(1);
        }
        assert_ne!(baseline, fingerprint(&changed, BenchType::Gray8).unwrap());
    }

    #[test]
    fn float_fingerprint_uses_raw_bits() {
        let mut doc = ImageDocument::new_image(
            "bits",
            NewImageType::Gray32,
            NewImageFill::Black,
            2,
            1,
            1,
            1,
        )
        .unwrap();
        doc.slices[0] = PixelPlane::Gray32(vec![0.0, 1.0]);
        let positive = fingerprint(&doc, BenchType::Gray32).unwrap();
        doc.slices[0] = PixelPlane::Gray32(vec![-0.0, 1.0]);
        assert_ne!(positive, fingerprint(&doc, BenchType::Gray32).unwrap());
        doc.slices[0] = PixelPlane::Gray32(vec![f32::NAN, 1.0]);
        let quiet_nan = fingerprint(&doc, BenchType::Gray32).unwrap();
        doc.slices[0] = PixelPlane::Gray32(vec![f32::from_bits(0x7fc0_0001), 1.0]);
        assert_ne!(quiet_nan, fingerprint(&doc, BenchType::Gray32).unwrap());
    }

    #[test]
    fn gaussian_runs_on_every_grayscale_type() {
        for image_type in [BenchType::Gray8, BenchType::Gray16, BenchType::Gray32] {
            let mut config = parse_config(&base()).unwrap();
            config.image_type = image_type;
            let mut session = new_session(&config).unwrap();
            run_case(&mut session, &config).unwrap();
            let doc = session.active_doc().unwrap();
            assert_eq!(doc.stack_size(), 2);
            assert!(fingerprint(doc, image_type).is_ok());
        }
    }

    #[test]
    fn executor_budget_names_the_worker_budget() {
        let mut config = parse_config(&base()).unwrap();
        assert_eq!(
            executor_budget(&config),
            (Some(machine_threads()), "session-default")
        );
        config.threads = Some(1);
        assert_eq!(executor_budget(&config), (Some(1), "explicit-workers"));
        config.case = "invert".into();
        assert_eq!(executor_budget(&config), (None, "sequential"));
    }

    #[test]
    fn worker_budget_does_not_change_pixels() {
        let mut config = parse_config(&base()).unwrap();
        let mut fingerprints = Vec::new();
        for threads in [None, Some(1), Some(4)] {
            config.threads = threads;
            let mut session = new_session(&config).unwrap();
            run_case(&mut session, &config).unwrap();
            fingerprints
                .push(fingerprint(session.active_doc().unwrap(), BenchType::Gray8).unwrap());
        }
        assert_eq!(fingerprints[0], fingerprints[1]);
        assert_eq!(fingerprints[0], fingerprints[2]);
    }
}
