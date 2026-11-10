//! Per-image TIFF destination tracking used by the close-save route.
//!
//! `FileSaver.save()` re-saves a TIFF opened from disk in place, and a
//! successful Save As makes the new file the re-save destination
//! (original_imagej/ij/io/FileSaver.java 42-69 and 676-698).

use ij_core::ImageDocument;
use ij_io::{load_tiff, save_tiff};
use ij_session::{Command, CommandOutcome, ImageId, Session};
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn write_ramp(dir: &TempDir, name: &str) -> PathBuf {
    let path = dir.path().join(name);
    let doc = ImageDocument::new_ramp_8bit(name, 16, 16).unwrap();
    save_tiff(&path, &doc).unwrap();
    path
}

fn open(session: &mut Session, path: &Path) -> ImageId {
    match session
        .dispatch(Command::OpenTiff {
            path: path.to_path_buf(),
        })
        .unwrap()
    {
        CommandOutcome::ImageOpened(id) => id,
        other => panic!("unexpected outcome: {other:?}"),
    }
}

#[test]
fn opened_tiff_remembers_its_source_path() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_ramp(&dir, "source.tif");
    let mut session = Session::new();
    let id = open(&mut session, &path);

    assert_eq!(session.image_path(id), Some(path.as_path()));
    assert!(!session.is_dirty(id));
}

#[test]
fn relative_tiff_paths_are_remembered_as_absolute_destinations() {
    let cwd = std::env::current_dir().unwrap();
    let dir = tempfile::Builder::new()
        .prefix("ij_relative_tiff_")
        .tempdir_in(&cwd)
        .unwrap();
    let source = write_ramp(&dir, "relative_source.tif");
    let relative_source = source.strip_prefix(&cwd).unwrap();
    let mut session = Session::new();
    let id = open(&mut session, relative_source);
    assert_eq!(session.image_path(id), Some(source.as_path()));

    let destination = dir.path().join("relative_save_as.tif");
    let relative_destination = destination.strip_prefix(&cwd).unwrap();
    session.save_image(id, relative_destination).unwrap();
    assert_eq!(session.image_path(id), Some(destination.as_path()));
}

#[test]
fn new_image_has_no_remembered_path() {
    let mut session = Session::new();
    let id = match session
        .dispatch(Command::NewRamp8Bit {
            title: "untitled".into(),
            width: 8,
            height: 8,
        })
        .unwrap()
    {
        CommandOutcome::ImageOpened(id) => id,
        other => panic!("unexpected outcome: {other:?}"),
    };

    assert_eq!(session.image_path(id), None);
    assert!(session.is_dirty(id));
}

#[test]
fn successful_save_as_updates_the_remembered_path() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("first.tif");
    let second = dir.path().join("second.tif");

    let mut session = Session::new();
    let id = match session
        .dispatch(Command::NewRamp8Bit {
            title: "save_as".into(),
            width: 8,
            height: 8,
        })
        .unwrap()
    {
        CommandOutcome::ImageOpened(id) => id,
        other => panic!("unexpected outcome: {other:?}"),
    };

    let outcome = session
        .dispatch(Command::SaveTiff {
            path: first.clone(),
        })
        .unwrap();
    assert_eq!(outcome, CommandOutcome::Saved(first.clone()));
    assert_eq!(session.image_path(id), Some(first.as_path()));
    assert!(!session.is_dirty(id));

    session.set_dirty(id, true);
    session
        .dispatch(Command::SaveTiff {
            path: second.clone(),
        })
        .unwrap();
    assert_eq!(
        session.image_path(id),
        Some(second.as_path()),
        "a successful Save As must become the next re-save destination"
    );
    assert!(!session.is_dirty(id));
    assert_eq!(
        session.macro_recorder.last().map(String::as_str),
        Some(format!("saveAs(\"Tiff\", \"{}\");", second.display()).as_str())
    );
}

#[test]
fn failed_save_keeps_previous_path_dirty_state_and_recorder() {
    let dir = tempfile::tempdir().unwrap();
    let source = write_ramp(&dir, "source.tif");
    let mut session = Session::new();
    let id = open(&mut session, &source);

    session.dispatch(Command::Invert).unwrap();
    assert!(session.is_dirty(id));
    let macros_before = session.macro_recorder.clone();

    let bad_path = dir.path().join("missing_parent").join("impossible.tif");
    let err = session
        .dispatch(Command::SaveTiff { path: bad_path })
        .unwrap_err();

    assert!(matches!(err, ij_session::SessionError::Io(_)));
    assert_eq!(
        session.image_path(id),
        Some(source.as_path()),
        "a failed save must not forget the existing destination"
    );
    assert!(session.is_dirty(id), "a failed save must keep dirty state");
    assert_eq!(
        session.macro_recorder, macros_before,
        "a failed save must not record a save macro"
    );
    assert!(session.get_image(id).is_some());
}

#[test]
fn closing_an_image_forgets_its_remembered_path() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_ramp(&dir, "closing.tif");
    let mut session = Session::new();
    let id = open(&mut session, &path);
    assert!(session.image_path(id).is_some());

    assert!(session.close_image(id));
    assert_eq!(session.image_path(id), None);

    let other = open(&mut session, &path);
    assert!(session.image_path(other).is_some());
    session.close_all_images();
    assert_eq!(session.image_path(other), None);
}

#[test]
fn save_leaves_the_file_readable_at_the_remembered_path() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_ramp(&dir, "roundtrip.tif");
    let mut session = Session::new();
    let id = open(&mut session, &path);
    session.dispatch(Command::Invert).unwrap();
    session
        .dispatch(Command::SaveTiff { path: path.clone() })
        .unwrap();

    let reloaded = load_tiff(&path).unwrap();
    assert_eq!(
        reloaded.plane().sample_f64(0),
        session.get_image(id).unwrap().plane().sample_f64(0)
    );
    assert_eq!(session.image_path(id), Some(path.as_path()));
}
