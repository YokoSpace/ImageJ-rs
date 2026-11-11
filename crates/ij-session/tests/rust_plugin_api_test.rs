use ij_core::{ImageDocument, Lut256, MeasurementSummary, PixelPlane};
use ij_plugin_api::{ImageMetadata, PixelKind, PluginContext, RustImagePlugin};
use ij_session::{Command, Session};

struct EditAndMeasure {
    fail: bool,
    row: Option<MeasurementSummary>,
}

impl RustImagePlugin for EditAndMeasure {
    fn name(&self) -> &str {
        "Edit and measure"
    }

    fn supports(&self, meta: &ImageMetadata) -> Result<(), String> {
        if meta.pixel_kind != PixelKind::Gray8 {
            return Err("requires 8-bit grayscale".into());
        }
        Ok(())
    }

    fn run(&self, ctx: &mut PluginContext) -> Result<(), String> {
        let PixelPlane::Gray8(pixels) = ctx.current_plane_mut() else {
            return Err("unexpected pixel type".into());
        };
        pixels[0] = pixels[0].wrapping_add(1);
        if let Some(row) = &self.row {
            ctx.append_result(row.clone());
        }
        if self.fail {
            Err("intentional failure after editing".into())
        } else {
            Ok(())
        }
    }
}

#[test]
fn rust_plugin_commits_pixels_results_and_undo_only_after_success() {
    let mut session = Session::new();
    let doc = ImageDocument::new("plugin", 2, 2, PixelPlane::Gray8(vec![10, 20, 30, 40])).unwrap();
    let row = doc.measure().unwrap();
    let id = session.insert_doc(doc);
    let revision = session.image_revision(id);
    session
        .run_rust_plugin(&EditAndMeasure {
            fail: false,
            row: Some(row.clone()),
        })
        .unwrap();

    assert_eq!(
        session.active_doc().unwrap().plane(),
        &PixelPlane::Gray8(vec![11, 20, 30, 40])
    );
    assert_eq!(session.results.rows, vec![row]);
    assert!(session.is_dirty(id));
    assert!(session.image_revision(id) > revision);
    assert!(session.active_doc().unwrap().snapshot.is_some());
    session.dispatch(Command::Undo).unwrap();
    assert_eq!(
        session.active_doc().unwrap().plane(),
        &PixelPlane::Gray8(vec![10, 20, 30, 40])
    );
}

#[test]
fn rust_plugin_error_rolls_back_pixels_results_and_session_state() {
    let mut session = Session::new();
    let doc = ImageDocument::new("plugin", 2, 2, PixelPlane::Gray8(vec![10, 20, 30, 40])).unwrap();
    let row = doc.measure().unwrap();
    let id = session.insert_doc(doc.clone());
    let revision = session.image_revision(id);
    let error = session
        .run_rust_plugin(&EditAndMeasure {
            fail: true,
            row: Some(row),
        })
        .unwrap_err();
    assert!(error.to_string().contains("intentional failure"));
    assert_eq!(session.get_image(id), Some(&doc));
    assert!(session.results.rows.is_empty());
    assert!(!session.is_dirty(id));
    assert_eq!(session.image_revision(id), revision);
}

#[test]
fn rust_plugin_rejects_unsupported_or_invalid_output_without_mutation() {
    let mut session = Session::new();
    let doc =
        ImageDocument::new("unsupported", 2, 2, PixelPlane::Gray16(vec![1, 2, 3, 4])).unwrap();
    let id = session.insert_doc(doc.clone());
    assert!(
        session
            .run_rust_plugin(&EditAndMeasure {
                fail: false,
                row: None
            })
            .unwrap_err()
            .to_string()
            .contains("requires 8-bit grayscale")
    );
    assert_eq!(session.get_image(id), Some(&doc));

    struct BadLength;
    impl RustImagePlugin for BadLength {
        fn name(&self) -> &str {
            "Bad length"
        }
        fn supports(&self, _: &ImageMetadata) -> Result<(), String> {
            Ok(())
        }
        fn run(&self, ctx: &mut PluginContext) -> Result<(), String> {
            *ctx.current_plane_mut() = PixelPlane::Gray16(vec![9]);
            Ok(())
        }
    }
    let error = session.run_rust_plugin(&BadLength).unwrap_err();
    assert!(error.to_string().contains("pixel count"));
    assert_eq!(session.get_image(id), Some(&doc));
}

#[test]
fn rust_plugin_requires_active_image() {
    let mut session = Session::new();
    assert!(
        session
            .run_rust_plugin(&EditAndMeasure {
                fail: false,
                row: None
            })
            .is_err()
    );
}

#[test]
fn results_only_rust_plugin_keeps_image_clean_and_undo_state_untouched() {
    struct AnalyzeOnly(MeasurementSummary);
    impl RustImagePlugin for AnalyzeOnly {
        fn name(&self) -> &str {
            "Analyze only"
        }
        fn supports(&self, _: &ImageMetadata) -> Result<(), String> {
            Ok(())
        }
        fn run(&self, ctx: &mut PluginContext) -> Result<(), String> {
            ctx.append_result(self.0.clone());
            Ok(())
        }
    }

    let mut session = Session::new();
    let doc = ImageDocument::new("analysis", 2, 2, PixelPlane::Gray8(vec![1, 2, 3, 4])).unwrap();
    let row = doc.measure().unwrap();
    let id = session.insert_doc(doc);
    let revision = session.image_revision(id);
    session.run_rust_plugin(&AnalyzeOnly(row.clone())).unwrap();
    assert_eq!(session.results.rows, vec![row]);
    assert!(!session.is_dirty(id));
    assert_eq!(session.image_revision(id), revision);
    assert!(session.active_doc().unwrap().snapshot.is_none());
}

#[test]
fn rust_plugin_rejects_malformed_palette_output_without_commit() {
    struct BadPalette;
    impl RustImagePlugin for BadPalette {
        fn name(&self) -> &str {
            "Bad palette"
        }
        fn supports(&self, _: &ImageMetadata) -> Result<(), String> {
            Ok(())
        }
        fn run(&self, ctx: &mut PluginContext) -> Result<(), String> {
            let PixelPlane::Color256 { lut, .. } = ctx.current_plane_mut() else {
                unreachable!()
            };
            lut.r.clear();
            Ok(())
        }
    }
    let mut session = Session::new();
    let doc = ImageDocument::new(
        "palette",
        2,
        2,
        PixelPlane::Color256 {
            pixels: vec![0, 1, 2, 3],
            lut: Box::new(Lut256::default()),
        },
    )
    .unwrap();
    let id = session.insert_doc(doc.clone());
    let error = session.run_rust_plugin(&BadPalette).unwrap_err();
    assert!(error.to_string().contains("palette"));
    assert_eq!(session.get_image(id), Some(&doc));
}
