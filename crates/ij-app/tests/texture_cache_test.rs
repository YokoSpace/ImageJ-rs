use egui::{CentralPanel, Context, RawInput};
use ij_app::ImageJApp;
use ij_core::{ImageDocument, PixelPlane};
use ij_session::{Command, ImageId};

fn init_egui_fonts(ctx: &Context) {
    let _ = ctx.run(RawInput::default(), |ctx| {
        CentralPanel::default().show(ctx, |ui| {
            ui.label("prime fonts");
        });
    });
}

fn render_canvas(ctx: &Context, app: &mut ImageJApp, image_id: ImageId) -> egui::FullOutput {
    ctx.run(RawInput::default(), |ctx| {
        CentralPanel::default().show(ctx, |ui| {
            app.render_image_canvas_content(ui, image_id);
        });
    })
}

#[test]
fn test_second_unchanged_frame_no_upload() {
    let ctx = Context::default();
    init_egui_fonts(&ctx);

    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("ramp", 64, 64).unwrap();

    // Frame 1: Initial upload
    let out1 = render_canvas(&ctx, &mut app, id);
    let doc_tex_id = app.cached_texture_id(id).expect("texture should be cached");
    assert_eq!(
        out1.textures_delta.set.len(),
        1,
        "Frame 1 must upload exactly one texture"
    );
    assert!(
        out1.textures_delta
            .set
            .iter()
            .any(|(tid, _)| *tid == doc_tex_id),
        "Uploaded texture must match cached texture id"
    );
    assert_eq!(app.texture_upload_count(), 1);
    assert_eq!(app.texture_cache_hit_count(), 0);
    assert!(app.is_texture_cached(id));

    // Frame 2: Second unchanged frame - must NOT upload anything
    let out2 = render_canvas(&ctx, &mut app, id);
    assert!(
        out2.textures_delta.set.is_empty(),
        "Second unchanged frame must NOT upload any texture"
    );
    assert_eq!(
        app.texture_upload_count(),
        1,
        "Upload counter must remain 1 on unchanged frame"
    );
    assert_eq!(
        app.texture_cache_hit_count(),
        1,
        "Cache hit counter must increment to 1"
    );

    // Frame 3: Third unchanged frame - still no upload
    let out3 = render_canvas(&ctx, &mut app, id);
    assert!(
        out3.textures_delta.set.is_empty(),
        "Third unchanged frame must NOT upload any texture"
    );
    assert_eq!(app.texture_upload_count(), 1);
    assert_eq!(app.texture_cache_hit_count(), 2);
}

#[test]
fn test_filter_regenerates_texture_and_subsequent_frame_is_cached() {
    let ctx = Context::default();
    init_egui_fonts(&ctx);

    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("filter_test", 64, 64).unwrap();

    // Warm cache
    let _ = render_canvas(&ctx, &mut app, id);
    let out_cached = render_canvas(&ctx, &mut app, id);
    assert!(out_cached.textures_delta.set.is_empty());
    assert_eq!(app.texture_upload_count(), 1);

    // Apply Invert filter
    app.session.dispatch(Command::Invert).unwrap();

    // Next frame must regenerate and upload new texture
    let out_filter = render_canvas(&ctx, &mut app, id);
    let new_tex_id = app.cached_texture_id(id).expect("new texture cached");
    assert_eq!(
        out_filter.textures_delta.set.len(),
        1,
        "Filter must trigger texture upload"
    );
    assert!(
        out_filter
            .textures_delta
            .set
            .iter()
            .any(|(tid, _)| *tid == new_tex_id),
        "Filter upload must match new cached texture id"
    );
    assert_eq!(
        app.texture_upload_count(),
        2,
        "Upload count must increment after filter"
    );

    // Subsequent frame is cached
    let out_next = render_canvas(&ctx, &mut app, id);
    assert!(
        out_next.textures_delta.set.is_empty(),
        "Subsequent frame after filter must be cached"
    );
    assert_eq!(app.texture_upload_count(), 2);
    assert_eq!(app.texture_cache_hit_count(), 2);
}

#[test]
fn test_undo_regenerates_texture_and_subsequent_frame_is_cached() {
    let ctx = Context::default();
    init_egui_fonts(&ctx);

    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("undo_test", 64, 64).unwrap();

    // Warm cache
    let _ = render_canvas(&ctx, &mut app, id);

    // Filter, then render
    app.session.dispatch(Command::Invert).unwrap();
    let _ = render_canvas(&ctx, &mut app, id);
    assert_eq!(app.texture_upload_count(), 2);

    // Undo filter
    app.session.dispatch(Command::Undo).unwrap();

    // Undo must regenerate texture
    let out_undo = render_canvas(&ctx, &mut app, id);
    assert_eq!(
        out_undo.textures_delta.set.len(),
        1,
        "Undo must trigger texture upload"
    );
    assert_eq!(
        app.texture_upload_count(),
        3,
        "Upload count must increment after Undo"
    );

    // Next frame is cached
    let out_after_undo = render_canvas(&ctx, &mut app, id);
    assert!(
        out_after_undo.textures_delta.set.is_empty(),
        "Frame after Undo must be cached"
    );
    assert_eq!(app.texture_upload_count(), 3);
}

#[test]
fn test_slice_navigation_regenerates_texture_and_subsequent_frame_is_cached() {
    let ctx = Context::default();
    init_egui_fonts(&ctx);

    let mut app = ImageJApp::new();
    let planes = vec![
        PixelPlane::Gray8(vec![10; 64]),
        PixelPlane::Gray8(vec![50; 64]),
        PixelPlane::Gray8(vec![100; 64]),
    ];
    let doc = ImageDocument::new_stack("stack_cache_test", 8, 8, planes).unwrap();
    let id = app.session.insert_doc(doc);
    app.set_active_window(id).unwrap();

    // Slice 1: initial render & cache
    let out_s1 = render_canvas(&ctx, &mut app, id);
    assert_eq!(out_s1.textures_delta.set.len(), 1);
    assert_eq!(app.texture_upload_count(), 1);

    let out_s1_cached = render_canvas(&ctx, &mut app, id);
    assert!(out_s1_cached.textures_delta.set.is_empty());
    assert_eq!(app.texture_upload_count(), 1);

    // Navigate to slice 2
    app.set_slice_active(2).unwrap();
    let out_s2 = render_canvas(&ctx, &mut app, id);
    assert_eq!(
        out_s2.textures_delta.set.len(),
        1,
        "Navigating to slice 2 must trigger texture upload"
    );
    assert_eq!(app.texture_upload_count(), 2);

    let out_s2_cached = render_canvas(&ctx, &mut app, id);
    assert!(out_s2_cached.textures_delta.set.is_empty());
    assert_eq!(app.texture_upload_count(), 2);

    // Navigate back to slice 1
    app.set_slice_active(1).unwrap();
    let out_s1_again = render_canvas(&ctx, &mut app, id);
    assert_eq!(
        out_s1_again.textures_delta.set.len(),
        1,
        "Navigating back to slice 1 must trigger texture upload"
    );
    assert_eq!(app.texture_upload_count(), 3);

    let out_s1_again_cached = render_canvas(&ctx, &mut app, id);
    assert!(out_s1_again_cached.textures_delta.set.is_empty());
    assert_eq!(app.texture_upload_count(), 3);
}

#[test]
fn test_contrast_display_range_regenerates_texture() {
    let ctx = Context::default();
    init_egui_fonts(&ctx);

    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("contrast_test", 64, 64).unwrap();

    // Warm cache
    let _ = render_canvas(&ctx, &mut app, id);
    let _ = render_canvas(&ctx, &mut app, id);
    assert_eq!(app.texture_upload_count(), 1);

    // Change display range / contrast
    app.set_display_range_active(20.0, 180.0).unwrap();

    // Must regenerate
    let out = render_canvas(&ctx, &mut app, id);
    assert_eq!(
        out.textures_delta.set.len(),
        1,
        "Display range change must regenerate texture"
    );
    assert_eq!(app.texture_upload_count(), 2);

    // Next frame cached
    let out_cached = render_canvas(&ctx, &mut app, id);
    assert!(out_cached.textures_delta.set.is_empty());
    assert_eq!(app.texture_upload_count(), 2);
}

#[test]
fn test_threshold_regenerates_texture_and_reset_regenerates_texture() {
    let ctx = Context::default();
    init_egui_fonts(&ctx);

    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("thresh_test", 64, 64).unwrap();

    // Warm cache
    let _ = render_canvas(&ctx, &mut app, id);
    assert_eq!(app.texture_upload_count(), 1);

    // Apply auto threshold (highlights pixels in red)
    app.auto_threshold_active("IsoData", false).unwrap();

    // Must regenerate
    let out_thresh = render_canvas(&ctx, &mut app, id);
    assert_eq!(
        out_thresh.textures_delta.set.len(),
        1,
        "Threshold application must regenerate texture"
    );
    assert_eq!(app.texture_upload_count(), 2);

    // Next frame cached
    let out_cached = render_canvas(&ctx, &mut app, id);
    assert!(out_cached.textures_delta.set.is_empty());
    assert_eq!(app.texture_upload_count(), 2);

    // Reset threshold
    app.reset_threshold_active().unwrap();

    // Must regenerate without threshold highlight
    let out_reset = render_canvas(&ctx, &mut app, id);
    assert_eq!(
        out_reset.textures_delta.set.len(),
        1,
        "Reset threshold must regenerate texture"
    );
    assert_eq!(app.texture_upload_count(), 3);

    // Next frame cached
    let out_reset_cached = render_canvas(&ctx, &mut app, id);
    assert!(out_reset_cached.textures_delta.set.is_empty());
    assert_eq!(app.texture_upload_count(), 3);
}

#[test]
fn test_closing_releases_cache_and_frees_texture() {
    let ctx = Context::default();
    init_egui_fonts(&ctx);

    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("close_test", 64, 64).unwrap();

    // Render frame
    let _ = render_canvas(&ctx, &mut app, id);
    assert!(app.is_texture_cached(id));

    // Close window
    let closed = app.close_window(id);
    assert!(closed);
    assert!(
        !app.is_texture_cached(id),
        "Closing window must remove image from texture cache"
    );

    // Next frame: egui will report the dropped texture in textures_delta.free
    let out_free = ctx.run(RawInput::default(), |_ctx| {});
    assert!(
        !out_free.textures_delta.free.is_empty(),
        "Dropping texture handle on window close must queue texture free in egui"
    );
}

#[test]
fn test_multiwindow_independence() {
    let ctx = Context::default();
    init_egui_fonts(&ctx);

    let mut app = ImageJApp::new();

    let id1 = app.open_ramp_8bit("win1", 32, 32).unwrap();
    let id2 = app.open_ramp_8bit("win2", 48, 48).unwrap();

    // Frame 1: Render both windows
    let _ = ctx.run(RawInput::default(), |ctx| {
        CentralPanel::default().show(ctx, |ui| {
            app.render_image_canvas_content(ui, id1);
            app.render_image_canvas_content(ui, id2);
        });
    });
    assert_eq!(
        app.texture_upload_count(),
        2,
        "Both windows must upload on frame 1"
    );
    assert!(app.is_texture_cached(id1));
    assert!(app.is_texture_cached(id2));

    // Frame 2: Unchanged - neither window uploads
    let out2 = ctx.run(RawInput::default(), |ctx| {
        CentralPanel::default().show(ctx, |ui| {
            app.render_image_canvas_content(ui, id1);
            app.render_image_canvas_content(ui, id2);
        });
    });
    assert!(
        out2.textures_delta.set.is_empty(),
        "Neither window must upload on unchanged frame"
    );
    assert_eq!(app.texture_upload_count(), 2);
    assert_eq!(app.texture_cache_hit_count(), 2);

    // Frame 3: Mutate only window 1
    app.set_active_window(id1).unwrap();
    app.session.dispatch(Command::Invert).unwrap();

    let out3 = ctx.run(RawInput::default(), |ctx| {
        CentralPanel::default().show(ctx, |ui| {
            app.render_image_canvas_content(ui, id1);
            app.render_image_canvas_content(ui, id2);
        });
    });
    assert_eq!(
        out3.textures_delta.set.len(),
        1,
        "Only window 1 must upload after mutation"
    );
    assert_eq!(
        app.texture_upload_count(),
        3,
        "Upload count should increment by 1 for window 1"
    );
    assert_eq!(
        app.texture_cache_hit_count(),
        3,
        "Window 2 must hit the cache"
    );

    // Frame 4: Close window 1, window 2 must remain cached and untouched
    app.close_window(id1);
    assert!(!app.is_texture_cached(id1));
    assert!(app.is_texture_cached(id2));

    let out4 = ctx.run(RawInput::default(), |ctx| {
        CentralPanel::default().show(ctx, |ui| {
            app.render_image_canvas_content(ui, id2);
        });
    });
    assert!(
        out4.textures_delta.set.is_empty(),
        "Window 2 must remain cached with no upload"
    );
    assert_eq!(app.texture_upload_count(), 3);
    assert_eq!(app.texture_cache_hit_count(), 4);
    assert!(
        !out4.textures_delta.free.is_empty(),
        "Window 1 texture must be freed"
    );
}

#[test]
fn test_session_mutable_access_invalidates_cache_preventing_stale_visual() {
    let ctx = Context::default();
    init_egui_fonts(&ctx);

    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("mutable_access_test", 32, 32).unwrap();

    // Warm cache
    let _ = render_canvas(&ctx, &mut app, id);
    let _ = render_canvas(&ctx, &mut app, id);
    assert_eq!(app.texture_upload_count(), 1);

    // Mutate document via Session mutable access (get_image_mut)
    {
        let doc = app.session.get_image_mut(id).unwrap();
        doc.invert();
    }

    // Must invalidate cache and upload new texture
    let out = render_canvas(&ctx, &mut app, id);
    assert_eq!(
        out.textures_delta.set.len(),
        1,
        "Session mutable access must bump revision and trigger upload"
    );
    assert_eq!(
        app.texture_upload_count(),
        2,
        "Upload count must increment after Session mutable access"
    );

    // Next frame is cached
    let out_cached = render_canvas(&ctx, &mut app, id);
    assert!(out_cached.textures_delta.set.is_empty());
    assert_eq!(app.texture_upload_count(), 2);
    assert_eq!(app.texture_cache_hit_count(), 2);
}

#[test]
fn test_active_doc_mut_invalidates_cache() {
    let ctx = Context::default();
    init_egui_fonts(&ctx);

    let mut app = ImageJApp::new();
    let id = app.open_ramp_8bit("active_doc_mut_test", 32, 32).unwrap();

    // Warm cache
    let _ = render_canvas(&ctx, &mut app, id);
    assert_eq!(app.texture_upload_count(), 1);

    // Mutate via active_doc_mut
    {
        let doc = app.session.active_doc_mut().unwrap();
        doc.invert();
    }

    // Must regenerate
    let out = render_canvas(&ctx, &mut app, id);
    assert_eq!(
        out.textures_delta.set.len(),
        1,
        "active_doc_mut must bump revision and regenerate texture"
    );
    assert_eq!(app.texture_upload_count(), 2);
}

fn repo_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("locate repo root")
        .to_path_buf()
}

fn fixture_jar_path() -> std::path::PathBuf {
    repo_root().join("tools/plugin-host/fixtures/fixtures.jar")
}

#[test]
fn test_sync_plugin_filter_regenerates_texture() {
    let bridge = match ij_bridge::plugin::PluginBridge::locate_from_repo_root(repo_root()) {
        Ok(b) => b,
        Err(_) => return, // Skip if Java environment is not available
    };
    let ctx = Context::default();
    init_egui_fonts(&ctx);

    let mut app = ImageJApp::new().with_plugin_bridge(bridge);
    let id = app.open_ramp_8bit("sync_plugin_test", 32, 24).unwrap();

    // Warm cache
    let _ = render_canvas(&ctx, &mut app, id);
    let _ = render_canvas(&ctx, &mut app, id);
    assert_eq!(app.texture_upload_count(), 1);

    // Run plugin filter sync
    let outcome = app
        .dispatch_plugin_filter_sync(
            "fixtures.RectInvertFilter".to_string(),
            vec![fixture_jar_path()],
            String::new(),
        )
        .unwrap();
    assert_eq!(outcome.class_name, "fixtures.RectInvertFilter");

    // Next frame must regenerate texture
    let out = render_canvas(&ctx, &mut app, id);
    assert_eq!(
        out.textures_delta.set.len(),
        1,
        "Sync plugin filter must trigger texture upload"
    );
    assert_eq!(app.texture_upload_count(), 2);

    // Next frame is cached
    let out_cached = render_canvas(&ctx, &mut app, id);
    assert!(out_cached.textures_delta.set.is_empty());
    assert_eq!(app.texture_upload_count(), 2);
    assert_eq!(app.texture_cache_hit_count(), 2);
}

#[test]
fn test_async_plugin_filter_regenerates_texture() {
    let bridge = match ij_bridge::plugin::PluginBridge::locate_from_repo_root(repo_root()) {
        Ok(b) => b,
        Err(_) => return, // Skip if Java environment is not available
    };
    let ctx = Context::default();
    init_egui_fonts(&ctx);

    let mut app = ImageJApp::new().with_plugin_bridge(bridge);
    let id = app.open_ramp_8bit("async_plugin_test", 32, 24).unwrap();

    // Warm cache
    let _ = render_canvas(&ctx, &mut app, id);
    let _ = render_canvas(&ctx, &mut app, id);
    assert_eq!(app.texture_upload_count(), 1);

    // Start async plugin filter task
    app.start_plugin_filter_task(
        "fixtures.RectInvertFilter".to_string(),
        vec![fixture_jar_path()],
        String::new(),
    )
    .unwrap();

    // Wait and poll until completion
    let start = std::time::Instant::now();
    let mut finished = false;
    while start.elapsed() < std::time::Duration::from_secs(15) {
        if let Some(res) = app.poll_plugin_filter() {
            let outcome = res.expect("async plugin should succeed");
            assert_eq!(outcome.class_name, "fixtures.RectInvertFilter");
            finished = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(finished, "async task timed out");

    // Next frame must regenerate texture with the plugin output
    let out = render_canvas(&ctx, &mut app, id);
    assert_eq!(
        out.textures_delta.set.len(),
        1,
        "Async plugin completion must trigger texture upload"
    );
    assert_eq!(app.texture_upload_count(), 2);

    // Next frame is cached
    let out_cached = render_canvas(&ctx, &mut app, id);
    assert!(out_cached.textures_delta.set.is_empty());
    assert_eq!(app.texture_upload_count(), 2);
    assert_eq!(app.texture_cache_hit_count(), 2);
}
