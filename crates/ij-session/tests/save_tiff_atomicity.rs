//! `Command::SaveTiff` must only clear the session's dirty flag after the file
//! has actually been replaced, and a failed save must leave any pre-existing
//! destination intact. The ij-io unit tests cover partial write/flush/rename
//! failures; these tests cover the session-visible contract.

use ij_core::{ImageDocument, PixelPlane};
use ij_io::{load_tiff, save_tiff};
use ij_session::{Command, CommandOutcome, Session};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TEST_SEQ: AtomicU64 = AtomicU64::new(0);

fn test_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "ij_session_save_atomic_{name}_{}_{}",
        std::process::id(),
        TEST_SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn dir_entries(dir: &Path) -> Vec<String> {
    let mut entries: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    entries.sort();
    entries
}

#[test]
fn successful_save_overwrites_existing_file_and_clears_dirty_state() {
    let dir = test_dir("success");
    let destination = dir.join("ramp.tif");
    fs::write(&destination, b"OLD SENTINEL").unwrap();

    let mut session = Session::new();
    let id = match session
        .dispatch(Command::NewRamp8Bit {
            title: "ramp".into(),
            width: 8,
            height: 6,
        })
        .unwrap()
    {
        CommandOutcome::ImageOpened(id) => id,
        other => panic!("unexpected outcome: {other:?}"),
    };
    assert!(session.is_dirty(id), "a new image starts dirty");

    let outcome = session
        .dispatch(Command::SaveTiff {
            path: destination.clone(),
        })
        .unwrap();

    assert_eq!(outcome, CommandOutcome::Saved(destination.clone()));
    assert!(!session.is_dirty(id), "a successful save clears dirty");
    let expected = session.get_image(id).unwrap();
    let loaded = load_tiff(&destination).unwrap();
    assert_eq!(loaded.plane(), expected.plane());
    let expected_macro = format!("saveAs(\"Tiff\", \"{}\");", destination.display());
    assert_eq!(
        session.macro_recorder.last().map(String::as_str),
        Some(expected_macro.as_str())
    );
    assert_eq!(dir_entries(&dir), vec!["ramp.tif".to_string()]);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn failed_save_keeps_existing_file_and_dirty_state() {
    let dir = test_dir("unsupported_state");
    let source = dir.join("rgb_source.tif");
    // A non-default display range on an RGB document is a genuine save-time
    // rejection (ImageJ TIFF cannot persist it), reached through dispatch only.
    let rgb = ImageDocument::new("rgb", 2, 2, PixelPlane::ColorRgb(vec![0xff00_0000; 4])).unwrap();
    save_tiff(&source, &rgb).unwrap();

    let mut session = Session::new();
    let id = match session
        .dispatch(Command::OpenTiff {
            path: source.clone(),
        })
        .unwrap()
    {
        CommandOutcome::ImageOpened(id) => id,
        other => panic!("unexpected outcome: {other:?}"),
    };
    session
        .dispatch(Command::SetDisplayRange {
            min: 10.0,
            max: 200.0,
        })
        .unwrap();
    assert!(session.is_dirty(id));

    let destination = dir.join("dest.tif");
    let sentinel = b"existing destination sentinel".to_vec();
    fs::write(&destination, &sentinel).unwrap();
    let recorder_len = session.macro_recorder.len();

    let error = session
        .dispatch(Command::SaveTiff {
            path: destination.clone(),
        })
        .unwrap_err();

    assert!(
        error.to_string().contains("display range"),
        "unexpected error: {error}"
    );
    assert_eq!(
        fs::read(&destination).unwrap(),
        sentinel,
        "a rejected save must not truncate the existing file"
    );
    assert!(
        session.is_dirty(id),
        "a rejected save must leave the image dirty"
    );
    assert_eq!(
        session.macro_recorder.len(),
        recorder_len,
        "a failed save must not be recorded as saveAs"
    );
    assert_eq!(
        dir_entries(&dir),
        vec!["dest.tif".to_string(), "rgb_source.tif".to_string()],
        "a failed save must not leave temporary files"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn io_failure_keeps_dirty_state() {
    let dir = test_dir("io_failure");
    let destination = dir.join("is_a_directory");
    fs::create_dir(&destination).unwrap();

    let mut session = Session::new();
    let id = match session
        .dispatch(Command::NewRamp8Bit {
            title: "ramp".into(),
            width: 4,
            height: 4,
        })
        .unwrap()
    {
        CommandOutcome::ImageOpened(id) => id,
        other => panic!("unexpected outcome: {other:?}"),
    };

    session
        .dispatch(Command::SaveTiff {
            path: destination.clone(),
        })
        .unwrap_err();

    assert!(
        session.is_dirty(id),
        "an I/O failure must keep the image dirty"
    );
    assert!(destination.is_dir());
    assert_eq!(dir_entries(&dir), vec!["is_a_directory".to_string()]);
    let _ = fs::remove_dir_all(&dir);
}
