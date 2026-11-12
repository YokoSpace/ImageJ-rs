use ij_core::PixelPlane;
use ij_core::new_image::{NewImageFill, NewImageType};
use ij_session::{Command, Session};
use std::error::Error;
use std::time::Instant;

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

pub fn run(args: &[String]) -> Result<(), Box<dyn Error>> {
    if args.len() != 6 {
        return Err(
            "usage: ij-cli bench-worker <case> <width> <height> <slices> <iterations> <warmup>"
                .into(),
        );
    }
    let case = args[0].as_str();
    if !matches!(case, "invert" | "gaussian" | "rotate90") {
        return Err(format!("unknown benchmark case: {case}").into());
    }
    let width: u32 = args[1].parse()?;
    let height: u32 = args[2].parse()?;
    let slices: u32 = args[3].parse()?;
    let iterations: u32 = args[4].parse()?;
    let warmup: u32 = args[5].parse()?;
    if width == 0 || height == 0 || slices == 0 || iterations == 0 {
        return Err("dimensions and iterations must be positive".into());
    }

    let mut warmup_session = new_session(width, height, slices)?;
    for _ in 0..warmup {
        run_case(&mut warmup_session, case)?;
    }
    drop(warmup_session);

    let mut session = new_session(width, height, slices)?;
    let started = Instant::now();
    for _ in 0..iterations {
        run_case(&mut session, case)?;
    }
    let elapsed_ns = started.elapsed().as_nanos();
    let doc = session.active_doc()?;
    let fingerprint = fingerprint(doc)?;
    let result = serde_json::json!({
        "case": case,
        "width": doc.width,
        "height": doc.height,
        "slices": doc.stack_size(),
        "iterations": iterations,
        "elapsed_ns": elapsed_ns,
        "fingerprint": fingerprint,
    });
    println!("BENCH_RESULT={result}");
    Ok(())
}

fn new_session(width: u32, height: u32, slices: u32) -> Result<Session, Box<dyn Error>> {
    let doc = ij_core::ImageDocument::new_image(
        "bench",
        NewImageType::Gray8,
        NewImageFill::Ramp,
        width,
        height,
        slices,
        1,
    )?;
    let mut session = Session::new();
    session.insert_doc(doc);
    Ok(session)
}

fn run_case(session: &mut Session, case: &str) -> Result<(), Box<dyn Error>> {
    let command = match case {
        "invert" => Command::InvertStack,
        "gaussian" => Command::GaussianBlurStack { sigma: 2.0 },
        "rotate90" => Command::Rotate90Right,
        _ => unreachable!(),
    };
    session.dispatch(command)?;
    Ok(())
}

fn add_int(mut hash: u64, value: u32) -> u64 {
    for shift in (0..32).step_by(8) {
        hash = (hash ^ ((value >> shift) & 0xff) as u64).wrapping_mul(FNV_PRIME);
    }
    hash
}

fn fingerprint(doc: &ij_core::ImageDocument) -> Result<String, Box<dyn Error>> {
    let mut hash = FNV_OFFSET;
    hash = add_int(hash, doc.width);
    hash = add_int(hash, doc.height);
    hash = add_int(hash, doc.stack_size() as u32);
    for plane in &doc.slices {
        let PixelPlane::Gray8(pixels) = plane else {
            return Err("benchmark requires 8-bit grayscale".into());
        };
        for &pixel in pixels {
            hash = (hash ^ pixel as u64).wrapping_mul(FNV_PRIME);
        }
    }
    Ok(format!("{hash:016x}"))
}
