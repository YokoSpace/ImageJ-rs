//! Ticket and validation tests for the narrow Java `PlugInFilter` path.
//!
//! A run must be bound to one image identity and one exact pre-run snapshot.
//! Anything that changes while the child JVM is running invalidates the
//! ticket, and malformed or wrong-image host output must be rejected before
//! the session document is touched.

use ij_bridge::plugin::{PluginBridgeError, PluginFilterRequest, PluginFilterRun};
use ij_core::{DisplayRange, ImageDocument, PixelPlane, RectRoi, Roi};
use ij_session::{Command, CommandOutcome, ImageId, Session, SessionError};
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("locate repo root")
        .to_path_buf()
}

fn fixture_jar_path() -> PathBuf {
    repo_root().join("tools/plugin-host/fixtures/fixtures.jar")
}

fn gray8(doc: &ImageDocument) -> &[u8] {
    match doc.plane() {
        PixelPlane::Gray8(pixels) => pixels,
        other => panic!("expected 8-bit grayscale, got {other:?}"),
    }
}

fn session_with_ramp(title: &str, width: u32, height: u32) -> (Session, ImageId) {
    let mut session = Session::new();
    let id = match session
        .dispatch(Command::NewRamp8Bit {
            title: title.into(),
            width,
            height,
        })
        .unwrap()
    {
        CommandOutcome::ImageOpened(id) => id,
        other => panic!("expected ImageOpened, got {other:?}"),
    };
    session.set_dirty(id, false);
    (session, id)
}

fn prepare(session: &Session, id: ImageId) -> ij_session::PreparedPluginFilter {
    session
        .prepare_plugin_filter(id, "fixtures.RectInvertFilter", &[fixture_jar_path()], "")
        .expect("prepare plugin filter")
}

/// Builds a host-shaped success result from the request snapshot. The caller
/// can then corrupt one identity/shape field to exercise validation.
fn fake_run(request: &PluginFilterRequest, invert: bool) -> PluginFilterRun {
    let mut image = request.image.clone();
    if invert {
        if let PixelPlane::Gray8(pixels) = &mut image.slices[0] {
            for pixel in pixels {
                *pixel = 255 - *pixel;
            }
        }
    }
    image.snapshot = None;
    PluginFilterRun {
        image,
        class_name: request.class_name.clone(),
        arg: request.arg.clone(),
        command: request.command.clone(),
        filter_flags: 0,
        run_invocations: 1,
        final_setup_called: false,
        stdout: String::new(),
        stderr: String::new(),
    }
}

fn fake_run_from_image(image: &ImageDocument) -> PluginFilterRun {
    let mut image = image.clone();
    if let PixelPlane::Gray8(pixels) = &mut image.slices[0] {
        for pixel in pixels {
            *pixel = 255 - *pixel;
        }
    }
    image.snapshot = None;
    PluginFilterRun {
        image,
        class_name: "fixtures.RectInvertFilter".into(),
        arg: String::new(),
        command: String::new(),
        filter_flags: 0,
        run_invocations: 1,
        final_setup_called: false,
        stdout: String::new(),
        stderr: String::new(),
    }
}

#[derive(Debug, PartialEq)]
struct State {
    image: ImageDocument,
    dirty: bool,
    results: String,
    macro_recorder: Vec<String>,
}

fn state(session: &Session, id: ImageId) -> State {
    State {
        image: session.get_image(id).expect("image").clone(),
        dirty: session.is_dirty(id),
        results: session.results.to_tsv_basic(),
        macro_recorder: session.macro_recorder.clone(),
    }
}

#[test]
fn any_document_change_during_a_run_invalidates_the_ticket() {
    let mutations: Vec<(&str, Box<dyn Fn(&mut Session, ImageId)>)> = vec![
        (
            "pixels",
            Box::new(|s: &mut Session, _id: ImageId| {
                s.dispatch(Command::Invert).unwrap();
            }),
        ),
        (
            "ROI",
            Box::new(|s: &mut Session, _id: ImageId| {
                s.dispatch(Command::MakeRectRoi {
                    x: 2,
                    y: 2,
                    width: 4,
                    height: 4,
                })
                .unwrap();
            }),
        ),
        (
            "display range",
            Box::new(|s: &mut Session, _id: ImageId| {
                s.dispatch(Command::SetDisplayRange {
                    min: 10.0,
                    max: 200.0,
                })
                .unwrap();
            }),
        ),
        (
            "threshold",
            Box::new(|s: &mut Session, _id: ImageId| {
                s.dispatch(Command::AutoThreshold {
                    method: "IsoData".into(),
                    dark_background: false,
                    histogram_16: false,
                })
                .unwrap();
            }),
        ),
        (
            "dimensions",
            Box::new(|s: &mut Session, id: ImageId| {
                s.get_image_mut(id).unwrap().n_channels = 2;
            }),
        ),
        (
            "position",
            Box::new(|s: &mut Session, _id: ImageId| {
                s.dispatch(Command::SetPosition {
                    channel: 1,
                    slice: 1,
                    frame: 1,
                })
                .unwrap();
            }),
        ),
        (
            "undo state",
            Box::new(|s: &mut Session, _id: ImageId| {
                s.dispatch(Command::Invert).unwrap();
                s.dispatch(Command::Undo).unwrap();
            }),
        ),
    ];

    for (label, mutate) in mutations {
        let (mut session, id) = session_with_ramp("ticket", 12, 10);
        let prepared = prepare(&session, id);
        let before = state(&session, id);
        mutate(&mut session, id);
        let after_mutation = state(&session, id);

        let run = fake_run_from_image(&before.image);
        let result = session.apply_plugin_filter_run(prepared.ticket, run);
        match result {
            Err(SessionError::StalePluginFilterResult { .. }) => {}
            other => panic!("{label}: expected stale rejection, got {other:?}"),
        }
        assert_eq!(
            state(&session, id),
            after_mutation,
            "{label}: stale rejection must not mutate state further"
        );
    }
}

#[test]
fn malformed_or_wrong_identity_output_is_rejected_atomically() {
    let (mut session, id) = session_with_ramp("malformed", 12, 10);

    let corruptions: Vec<(&str, Box<dyn Fn(&mut PluginFilterRun)>)> = vec![
        (
            "class",
            Box::new(|r: &mut PluginFilterRun| {
                r.class_name = "fixtures.OtherFilter".into();
            }),
        ),
        (
            "arg",
            Box::new(|r: &mut PluginFilterRun| {
                r.arg = "unexpected".into();
            }),
        ),
        (
            "title",
            Box::new(|r: &mut PluginFilterRun| {
                r.image.title = "another-image".into();
            }),
        ),
        (
            "width",
            Box::new(|r: &mut PluginFilterRun| {
                r.image.width += 1;
            }),
        ),
        (
            "pixel type",
            Box::new(|r: &mut PluginFilterRun| {
                r.image.slices = vec![PixelPlane::Gray16(vec![0; 12 * 10])];
            }),
        ),
        (
            "slice count",
            Box::new(|r: &mut PluginFilterRun| {
                r.image.slices.push(PixelPlane::Gray8(vec![0; 12 * 10]));
            }),
        ),
        (
            "ROI",
            Box::new(|r: &mut PluginFilterRun| {
                r.image.roi = Some(Roi::Rect(ij_core::RectRoi::new(1, 1, 3, 3).unwrap()));
            }),
        ),
        (
            "display range",
            Box::new(|r: &mut PluginFilterRun| {
                r.image.display_range = DisplayRange { min: 1.0, max: 2.0 };
            }),
        ),
        (
            "threshold",
            Box::new(|r: &mut PluginFilterRun| {
                r.image.threshold = Some(ij_core::ThresholdLevels {
                    lower: 1.0,
                    upper: 2.0,
                });
            }),
        ),
        (
            "run invocations",
            Box::new(|r: &mut PluginFilterRun| {
                r.run_invocations = 0;
            }),
        ),
    ];

    for (label, corrupt) in corruptions {
        let prepared = prepare(&session, id);
        let before = state(&session, id);
        let mut run = fake_run(&prepared.request, true);
        corrupt(&mut run);

        let result = session.apply_plugin_filter_run(prepared.ticket, run);
        match result {
            Err(SessionError::InvalidPluginFilterOutput(_)) => {}
            other => panic!("{label}: expected InvalidPluginFilterOutput, got {other:?}"),
        }
        assert_eq!(
            state(&session, id),
            before,
            "{label}: malformed output mutated the session"
        );
    }
}

#[test]
fn prepare_uses_the_bridge_preflight_validator_before_any_child_can_start() {
    let mut cases: Vec<(&str, ImageDocument)> = Vec::new();
    // Pre-P3.10 this preflight rejected 16-bit and plain stacks too. Those
    // now pass pre-flight (the bit-depth and DOES_STACKS decisions live in
    // the Java host's setup() flag check), so the transfer-shaped rejections
    // below are the ones that must still block before any child can start.
    cases.push((
        "8-bit indexed color",
        ImageDocument::new(
            "indexed",
            16,
            12,
            PixelPlane::Color256 {
                pixels: vec![0; 16 * 12],
                lut: Box::new(ij_core::Lut256::default()),
            },
        )
        .expect("8-bit indexed color"),
    ));
    cases.push((
        "hyperstack",
        ImageDocument::new_hyperstack(
            "hyper",
            8,
            8,
            vec![
                PixelPlane::Gray8(vec![0; 64]),
                PixelPlane::Gray8(vec![0; 64]),
            ],
            2,
            1,
            1,
        )
        .expect("hyperstack"),
    ));

    let mut oval = ImageDocument::new_ramp_8bit("oval", 16, 12).expect("oval");
    oval.set_oval_roi(2, 2, 6, 4).unwrap();
    cases.push(("oval ROI", oval));

    let mut clipped = ImageDocument::new_ramp_8bit("clipped", 16, 12).expect("clipped");
    clipped.set_rect_roi(12, 8, 8, 8).unwrap();
    cases.push(("out-of-bounds ROI", clipped));

    // i32::MAX + 4 overflows the UI's former `x + width` check. The bridge
    // validator uses i64, so this must be a clean rejection, not a panic.
    let mut overflow = ImageDocument::new_ramp_8bit("overflow", 16, 12).expect("overflow");
    overflow.roi = Some(Roi::Rect(RectRoi {
        x: i32::MAX - 1,
        y: 0,
        width: 4,
        height: 1,
    }));
    cases.push(("overflowing ROI", overflow));

    let mut calibration = ImageDocument::new_ramp_8bit("cal", 16, 12).expect("cal");
    calibration.calibration = ij_core::Calibration {
        pixel_width: 0.5,
        pixel_height: 0.5,
        pixel_depth: 1.0,
        unit: "um".to_string(),
        invert_y: false,
        x_origin: 0.0,
        y_origin: 0.0,
    };
    cases.push(("non-default calibration", calibration));

    let mut display_range = ImageDocument::new_ramp_8bit("range", 16, 12).expect("range");
    display_range.display_range = DisplayRange {
        min: 10.0,
        max: 200.0,
    };
    cases.push(("non-default display range", display_range));

    for (label, doc) in cases {
        let mut session = Session::new();
        let id = session.insert_doc(doc);
        match session.prepare_plugin_filter(
            id,
            "fixtures.RectInvertFilter",
            &[fixture_jar_path()],
            "",
        ) {
            Err(SessionError::Plugin(PluginBridgeError::UnsupportedFilterInput(_))) => {}
            other => panic!("{label}: expected bridge preflight rejection, got {other:?}"),
        }
    }

    // The active threshold keeps its session-specific error, but is still
    // rejected before any JVM is spawned.
    let mut threshold = ImageDocument::new_ramp_8bit("threshold", 16, 12).expect("threshold");
    threshold.threshold = Some(ij_core::ThresholdLevels {
        lower: 10.0,
        upper: 200.0,
    });
    let mut session = Session::new();
    let id = session.insert_doc(threshold);
    assert!(matches!(
        session.prepare_plugin_filter(id, "fixtures.RectInvertFilter", &[], ""),
        Err(SessionError::UnsupportedPluginFilterState(_))
    ));
}

#[test]
fn ticket_targets_one_image_and_ignores_the_active_window() {
    let (mut session, id1) = session_with_ramp("first", 12, 10);
    let id2 = match session
        .dispatch(Command::NewRamp8Bit {
            title: "second".into(),
            width: 10,
            height: 8,
        })
        .unwrap()
    {
        CommandOutcome::ImageOpened(id) => id,
        other => panic!("expected ImageOpened, got {other:?}"),
    };
    session.set_dirty(id1, false);
    session.set_dirty(id2, false);

    let first = prepare(&session, id1);
    let first_before = state(&session, id1);
    let second_before = state(&session, id2);

    // Switch the UI/active path to the other image while the run is pending.
    session.set_active_image(id2).unwrap();
    let outcome = session
        .apply_plugin_filter_run(first.ticket, fake_run(&first.request, true))
        .expect("ticket must apply to its own image");
    assert_eq!(outcome.class_name, "fixtures.RectInvertFilter");

    assert_ne!(
        gray8(session.get_image(id1).unwrap()),
        gray8(&first_before.image)
    );
    let second_after = state(&session, id2);
    assert_eq!(
        second_after.image, second_before.image,
        "active image was touched"
    );
    assert_eq!(second_after.dirty, second_before.dirty);
    assert_eq!(second_after.results, second_before.results);
    assert!(session.is_dirty(id1));
    assert!(!session.is_dirty(id2));
}

#[test]
fn concurrent_pending_tickets_apply_to_their_own_images() {
    let (mut session, id1) = session_with_ramp("one", 8, 8);
    let id2 = match session
        .dispatch(Command::NewRamp8Bit {
            title: "two".into(),
            width: 9,
            height: 7,
        })
        .unwrap()
    {
        CommandOutcome::ImageOpened(id) => id,
        other => panic!("expected ImageOpened, got {other:?}"),
    };
    session.set_dirty(id1, false);
    session.set_dirty(id2, false);

    let first = prepare(&session, id1);
    let second = prepare(&session, id2);
    let first_before = state(&session, id1);
    let second_before = state(&session, id2);

    // Apply in the reverse order of preparation; both tickets remain valid
    // until their own target changes.
    let second_run = fake_run(&second.request, true);
    let first_run = fake_run(&first.request, true);
    session
        .apply_plugin_filter_run(second.ticket, second_run)
        .expect("second ticket applies");
    session
        .apply_plugin_filter_run(first.ticket, first_run)
        .expect("first ticket still applies after the other image ran");

    assert_ne!(
        gray8(session.get_image(id1).unwrap()),
        gray8(&first_before.image)
    );
    assert_ne!(
        gray8(session.get_image(id2).unwrap()),
        gray8(&second_before.image)
    );
}

#[test]
fn a_ticket_is_single_use_and_reapplying_it_is_stale() {
    let (mut session, id) = session_with_ramp("single-use", 10, 10);
    let prepared = prepare(&session, id);
    let reused_ticket = prepared.ticket.clone();

    session
        .apply_plugin_filter_run(prepared.ticket, fake_run(&prepared.request, true))
        .expect("first apply");
    let after_first = state(&session, id);

    let result = session.apply_plugin_filter_run(reused_ticket, fake_run(&prepared.request, false));
    assert!(matches!(
        result,
        Err(SessionError::StalePluginFilterResult { .. })
    ));
    assert_eq!(state(&session, id), after_first);
}

#[test]
fn apply_uses_ticket_snapshot_for_undo_not_a_caller_snapshot() {
    let (mut session, id) = session_with_ramp("undo-source", 10, 10);
    let before = state(&session, id);
    let prepared = prepare(&session, id);

    session
        .apply_plugin_filter_run(prepared.ticket, fake_run(&prepared.request, true))
        .expect("apply ticket");
    assert_ne!(gray8(session.get_image(id).unwrap()), gray8(&before.image));

    session.dispatch(Command::Undo).unwrap();
    assert_eq!(
        gray8(session.get_image(id).unwrap()),
        gray8(&before.image),
        "undo must restore the exact ticket pre-run plane"
    );
}

#[test]
fn output_validation_failure_leaves_undo_dirty_results_and_macro_unchanged() {
    let (mut session, id) = session_with_ramp("atomic-failure", 12, 10);
    session.dispatch(Command::Invert).unwrap();
    session.dispatch(Command::Measure).unwrap();
    let prepared = prepare(&session, id);
    let before = state(&session, id);

    let mut run = fake_run(&prepared.request, true);
    run.image.slices = vec![PixelPlane::Gray8(vec![1, 2, 3])];
    assert!(matches!(
        session.apply_plugin_filter_run(prepared.ticket, run),
        Err(SessionError::InvalidPluginFilterOutput(_))
    ));
    assert_eq!(state(&session, id), before);
}
