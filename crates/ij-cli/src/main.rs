use ij_bridge::JavaOracleBridge;
use ij_session::{Command, Session};
use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

mod benchmark;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        print_usage();
        return ExitCode::from(2);
    }

    match args[0].as_str() {
        "doctor" => run_doctor(),
        "slice1" => run_slice1(&args[1..]),
        "parity-check" => run_parity_check(),
        "bench-worker" => match benchmark::run(&args[1..]) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("benchmark failed: {error}");
                ExitCode::from(1)
            }
        },
        _ => {
            eprintln!("unknown subcommand: {}", args[0]);
            print_usage();
            ExitCode::from(2)
        }
    }
}

fn print_usage() {
    eprintln!(
        "Usage:
  ij-cli doctor
  ij-cli slice1 --out-dir <dir>
  ij-cli parity-check
  ij-cli bench-worker <case> <width> <height> <slices> <iterations> <warmup>
    [type=8|16|32] [input=noise|impulse|ramp] [sigma=N] [seed=N] [threads=N]"
    );
}

fn run_doctor() -> ExitCode {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .to_path_buf();
    let bridge = match JavaOracleBridge::locate_from_repo_root(&repo_root) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("doctor failed: {e}");
            return ExitCode::from(1);
        }
    };
    match bridge.eval_macro("print('IJ_ORACLE_VERSION=' + getVersion());") {
        Ok(out) => {
            print!("{}", out.stdout);
            println!("IJ_RS_DOCTOR=OK");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("doctor failed: {e}");
            ExitCode::from(1)
        }
    }
}

fn run_slice1(args: &[String]) -> ExitCode {
    let mut out_dir: Option<PathBuf> = None;
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--out-dir" && i + 1 < args.len() {
            out_dir = Some(PathBuf::from(&args[i + 1]));
            i += 2;
        } else {
            eprintln!("unexpected arg: {}", args[i]);
            return ExitCode::from(2);
        }
    }
    let out_dir = match out_dir {
        Some(d) => d,
        None => {
            eprintln!("missing --out-dir <dir>");
            return ExitCode::from(2);
        }
    };
    if let Err(e) = std::fs::create_dir_all(&out_dir) {
        eprintln!("failed to create out-dir: {e}");
        return ExitCode::from(1);
    }

    let tiff_path = out_dir.join("ramp_input.tif");
    let saved_tiff_path = out_dir.join("ramp_saved.tif");
    let results_path = out_dir.join("Results.tsv");
    let macro_path = out_dir.join("recorded_macro.ijm");

    let mut setup_session = Session::new();
    setup_session
        .dispatch(Command::NewRamp8Bit {
            title: "ramp_input.tif".into(),
            width: 16,
            height: 16,
        })
        .unwrap();
    setup_session
        .dispatch(Command::SaveTiff {
            path: tiff_path.clone(),
        })
        .unwrap();

    // Vertical Slice 1: Open TIFF -> Adjust display range -> Make Rect ROI -> Measure -> Save Results & TIFF
    let mut session = Session::new();
    session
        .dispatch(Command::OpenTiff { path: tiff_path })
        .unwrap();
    session
        .dispatch(Command::SetDisplayRange {
            min: 16.0,
            max: 240.0,
        })
        .unwrap();
    session
        .dispatch(Command::MakeRectRoi {
            x: 2,
            y: 3,
            width: 8,
            height: 6,
        })
        .unwrap();
    session.dispatch(Command::Measure).unwrap();
    session
        .dispatch(Command::SaveResultsTsv {
            path: results_path.clone(),
        })
        .unwrap();
    session
        .dispatch(Command::SaveTiff {
            path: saved_tiff_path.clone(),
        })
        .unwrap();

    std::fs::write(&macro_path, session.macro_recorder.join("\n") + "\n").unwrap();

    println!("SLICE1_RESULTS={}", results_path.display());
    println!("SLICE1_TIFF={}", saved_tiff_path.display());
    println!("SLICE1_MACRO={}", macro_path.display());
    print!("{}", session.results.to_tsv_basic());
    ExitCode::SUCCESS
}

fn run_parity_check() -> ExitCode {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .to_path_buf();
    let bridge = match JavaOracleBridge::locate_from_repo_root(&repo_root) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("parity-check failed: {e}");
            return ExitCode::from(1);
        }
    };

    let mut session = Session::new();
    session
        .dispatch(Command::NewRamp8Bit {
            title: "test".into(),
            width: 16,
            height: 16,
        })
        .unwrap();
    session
        .dispatch(Command::MakeRectRoi {
            x: 2,
            y: 3,
            width: 8,
            height: 6,
        })
        .unwrap();
    session.dispatch(Command::Measure).unwrap();
    let rust_tsv = session.results.to_tsv_basic();

    let oracle_out = match bridge.eval_macro(
        "newImage('test', '8-bit ramp', 16, 16, 1); makeRectangle(2, 3, 8, 6); run('Measure');",
    ) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("oracle execution failed: {e}");
            return ExitCode::from(1);
        }
    };

    if rust_tsv.trim() != oracle_out.stdout.trim() {
        eprintln!(
            "PARITY MISMATCH:\n--- Rust ---\n{rust_tsv}\n--- Java Oracle ---\n{}",
            oracle_out.stdout
        );
        return ExitCode::from(1);
    }

    println!("PARITY_OK");
    print!("{rust_tsv}");
    ExitCode::SUCCESS
}
