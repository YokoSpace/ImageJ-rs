//! Headless UI coverage for `Image > Stacks > Images to Stack` and
//! `Image > Stacks > Stack to Images`.
//!
//! Java references: original_imagej/ij/plugin/ImagesToStack.java
//! (convertImagesToStack 36-116, convert 118-253) and
//! original_imagej/ij/plugin/StackEditor.java (convertStackToImages 240-310).
//! The pixel/metadata parity with the real oracle lives in
//! `crates/ij-session/tests/oracle_parity.rs`; these tests cover the app layer:
//! menu placement, enablement, dialog defaults and persistence, OK/Cancel,
//! recorder output, active-image bookkeeping and the >30-slice confirmation.

use ij_app::action::{AppAction, MenuKind, MenuNode, MENU_TREE};
use ij_app::app::ImageJApp;
use ij_core::{ImageDocument, Lut256, PixelPlane};
use ij_session::ImageId;

fn menu_contains(nodes: &[MenuNode], path: &[&str]) -> bool {
    let Some((head, tail)) = path.split_first() else {
        return true;
    };
    nodes.iter().any(|node| {
        node.label == *head
            && if tail.is_empty() {
                true
            } else if let MenuKind::Submenu(children) = node.kind {
                menu_contains(children, tail)
            } else {
                false
            }
    })
}

fn stacks_menu_labels() -> Vec<&'static str> {
    fn find<'a>(nodes: &'a [MenuNode], label: &str) -> Option<&'a [MenuNode]> {
        for node in nodes {
            if node.label == label {
                if let MenuKind::Submenu(children) = node.kind {
                    return Some(children);
                }
            }
            if let MenuKind::Submenu(children) = node.kind {
                if let Some(found) = find(children, label) {
                    return Some(found);
                }
            }
        }
        None
    }
    find(MENU_TREE, "Stacks")
        .expect("Image > Stacks submenu")
        .iter()
        .map(|node| node.label)
        .collect()
}

fn gray8(title: &str, width: u32, height: u32, first: u8) -> ImageDocument {
    let mut pixels = vec![255u8; (width * height) as usize];
    pixels[0] = first;
    ImageDocument::new(title, width, height, PixelPlane::Gray8(pixels)).unwrap()
}

fn gray8_stack(title: &str, width: u32, height: u32, slices: usize) -> ImageDocument {
    ImageDocument::new_stack(
        title,
        width,
        height,
        (1..=slices as u8)
            .map(|s| PixelPlane::Gray8(vec![s; (width * height) as usize]))
            .collect(),
    )
    .unwrap()
}

fn app_with(docs: Vec<ImageDocument>) -> (ImageJApp, Vec<ImageId>) {
    let mut app = ImageJApp::new();
    let ids = docs
        .into_iter()
        .map(|doc| app.session.insert_doc(doc))
        .collect();
    (app, ids)
}

fn render_images_to_stack(app: &mut ImageJApp) {
    let ctx = egui::Context::default();
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| app.render_images_to_stack_content(ui));
    });
}

#[test]
fn stack_convert_menu_entries_follow_java_order() {
    assert!(menu_contains(
        MENU_TREE,
        &["Image", "Stacks", "Images to Stack"]
    ));
    assert!(menu_contains(
        MENU_TREE,
        &["Image", "Stacks", "Stack to Images"]
    ));
    // ImageJ's Image>Stacks order is Invert Stack, Images to Stack, Stack to
    // Images, Z Project... (ij/gui/Menus.java Image>Stacks).
    let labels = stacks_menu_labels();
    let invert = labels.iter().position(|l| *l == "Invert Stack").unwrap();
    let to_stack = labels.iter().position(|l| *l == "Images to Stack").unwrap();
    let to_images = labels.iter().position(|l| *l == "Stack to Images").unwrap();
    let zproject = labels.iter().position(|l| *l == "Z Project...").unwrap();
    assert!(invert < to_stack && to_stack < to_images && to_images < zproject);
}

#[test]
fn images_to_stack_requires_two_2d_images() {
    let ctx = egui::Context::default();
    // The menu item stays enabled like Java's plugin item; the count check
    // happens inside convertImagesToStack (ImagesToStack.java 55-77).
    let mut app = ImageJApp::new();
    assert!(app.action_enabled(AppAction::ImagesToStack).is_err());
    // The Image menu needs an active image, so the zero-window branch is only
    // reachable through the command itself (ImagesToStack.java 55-58).
    app.open_images_to_stack_dialog();
    assert_eq!(
        app.dialogs.last_error.as_deref(),
        Some("No images are open.")
    );

    let (mut app, _) = app_with(vec![gray8("a", 2, 2, 1)]);
    assert!(app.action_enabled(AppAction::ImagesToStack).is_ok());
    app.perform(AppAction::ImagesToStack, &ctx);
    assert!(!app.dialogs.images_to_stack.open);
    assert_eq!(
        app.dialogs.last_error.as_deref(),
        Some("At least two open 2D images required.")
    );

    // A stack does not count as a 2D image (ImagesToStack.java 60-66).
    let (mut app, _) = app_with(vec![gray8("a", 2, 2, 1), gray8_stack("stk", 2, 2, 3)]);
    app.perform(AppAction::ImagesToStack, &ctx);
    assert!(!app.dialogs.images_to_stack.open);
    assert_eq!(
        app.dialogs.last_error.as_deref(),
        Some("At least two open 2D images required.")
    );

    let (mut app, _) = app_with(vec![gray8("a", 2, 2, 1), gray8("b", 2, 2, 2)]);
    app.perform(AppAction::ImagesToStack, &ctx);
    assert!(app.dialogs.images_to_stack.open);
}

#[test]
fn images_to_stack_dialog_defaults() {
    let ctx = egui::Context::default();
    let (mut app, _) = app_with(vec![gray8("a", 2, 2, 1), gray8("b", 2, 2, 2)]);
    app.perform(AppAction::ImagesToStack, &ctx);
    let dialog = app.dialogs.images_to_stack.clone();
    assert!(dialog.open);
    // ImagesToStack.java 22-28, 34, 87-95.
    assert!(!dialog.sizes_differ);
    assert_eq!(dialog.count, 2);
    assert_eq!((dialog.min_width, dialog.min_height), (2, 2));
    assert_eq!((dialog.max_width, dialog.max_height), (2, 2));
    assert_eq!(dialog.method_index, 0);
    assert_eq!(dialog.name, "Stack");
    assert_eq!(dialog.title_filter, "");
    assert_eq!(dialog.fill, "");
    assert!(!dialog.bicubic);
    assert!(dialog.titles_as_labels);
    assert!(!dialog.keep);
    render_images_to_stack(&mut app);
}

#[test]
fn images_to_stack_dialog_reports_differing_sizes() {
    let ctx = egui::Context::default();
    let (mut app, _) = app_with(vec![gray8("a", 2, 2, 1), gray8("b", 4, 3, 2)]);
    app.perform(AppAction::ImagesToStack, &ctx);
    let dialog = app.dialogs.images_to_stack.clone();
    assert!(dialog.sizes_differ);
    assert_eq!(dialog.count, 2);
    assert_eq!((dialog.min_width, dialog.min_height), (2, 2));
    assert_eq!((dialog.max_width, dialog.max_height), (4, 3));
    // The Method combo and Bicubic checkbox only exist when the sizes differ
    // (ImagesToStack.java 89-94); rendering must not panic in either shape.
    render_images_to_stack(&mut app);
}

#[test]
fn images_to_stack_ok_creates_stack_and_closes_sources() {
    let ctx = egui::Context::default();
    let (mut app, ids) = app_with(vec![gray8("a", 2, 2, 1), gray8("b", 2, 2, 2)]);
    for id in &ids {
        app.image_views
            .insert(*id, ij_app::image_view::ImageView::new(2, 2));
    }
    app.perform(AppAction::ImagesToStack, &ctx);
    let id = app.apply_images_to_stack_dialog(&ctx).unwrap().unwrap();
    assert!(!app.dialogs.images_to_stack.open);
    assert!(app.dialogs.last_error.is_none());

    let doc = app.session.get_image(id).unwrap();
    assert_eq!(doc.title, "Stack");
    assert_eq!(doc.stack_size(), 2);
    assert_eq!(doc.slice_labels, vec![Some("a".into()), Some("b".into())]);

    // The sources are closed (ImagesToStack.java 208-211) and their view state
    // is pruned; the new stack gets a view and becomes active.
    assert_eq!(app.session.image_ids(), vec![id]);
    assert_eq!(app.image_views.len(), 1);
    assert!(app.image_views.contains_key(&id));
    assert_eq!(app.image_views[&id].width, 2);
    assert_eq!(app.active_image_id(), Some(id));
    assert_eq!(
        app.session.macro_recorder.last().map(String::as_str),
        Some("run(\"Images to Stack\", \"use\");")
    );
}

#[test]
fn images_to_stack_keep_sources_preserves_windows() {
    let ctx = egui::Context::default();
    let (mut app, ids) = app_with(vec![gray8("a", 2, 2, 1), gray8("b", 2, 2, 2)]);
    app.perform(AppAction::ImagesToStack, &ctx);
    app.dialogs.images_to_stack.keep = true;
    let id = app.apply_images_to_stack_dialog(&ctx).unwrap().unwrap();
    assert_eq!(app.session.image_ids().len(), 3);
    assert!(ids
        .iter()
        .all(|source| app.session.get_image(*source).is_some()));
    assert_eq!(app.active_image_id(), Some(id));
    assert_eq!(
        app.session.macro_recorder.last().map(String::as_str),
        Some("run(\"Images to Stack\", \"use keep\");")
    );
}

#[test]
fn images_to_stack_cancel_creates_nothing() {
    let ctx = egui::Context::default();
    let (mut app, _) = app_with(vec![gray8("a", 2, 2, 1), gray8("b", 2, 2, 2)]);
    app.perform(AppAction::ImagesToStack, &ctx);
    app.cancel_images_to_stack_dialog();
    assert!(!app.dialogs.images_to_stack.open);
    assert_eq!(app.session.image_ids().len(), 2);
    assert!(app.session.macro_recorder.is_empty());
}

#[test]
fn images_to_stack_dialog_persists_statics_but_resets_text_fields() {
    let ctx = egui::Context::default();
    let (mut app, _) = app_with(vec![gray8("a", 2, 2, 1), gray8("b", 4, 3, 2)]);
    app.perform(AppAction::ImagesToStack, &ctx);
    app.dialogs.images_to_stack.method_index = 2;
    app.dialogs.images_to_stack.bicubic = true;
    app.dialogs.images_to_stack.keep = true;
    app.dialogs.images_to_stack.titles_as_labels = false;
    app.dialogs.images_to_stack.name = "Merged".into();
    app.dialogs.images_to_stack.title_filter = "a".into();
    app.dialogs.images_to_stack.fill = "red".into();
    let id = app.apply_images_to_stack_dialog(&ctx).unwrap().unwrap();
    assert_eq!(app.session.get_image(id).unwrap().title, "Merged");
    assert_eq!(
        app.session.macro_recorder.last().map(String::as_str),
        Some("run(\"Images to Stack\", \"method=[Scale (smallest)] name=Merged title=a fill=red bicubic keep\");")
    );

    // staticMethod/staticBicubic/staticKeep/staticTitlesAsLabels survive
    // (ImagesToStack.java 22-25); name/filter/fill do not (34, 78, 84-86).
    app.perform(AppAction::ImagesToStack, &ctx);
    let dialog = app.dialogs.images_to_stack.clone();
    assert_eq!(dialog.method_index, 2);
    assert!(dialog.bicubic);
    assert!(dialog.keep);
    assert!(!dialog.titles_as_labels);
    assert_eq!(dialog.name, "Stack");
    assert_eq!(dialog.title_filter, "");
    assert_eq!(dialog.fill, "");
}

#[test]
fn images_to_stack_title_filter_and_duplicate_titles() {
    let ctx = egui::Context::default();
    let (mut app, _) = app_with(vec![
        gray8("keep", 2, 2, 1),
        gray8("skip", 2, 2, 2),
        gray8("keeper", 2, 2, 3),
    ]);
    app.perform(AppAction::ImagesToStack, &ctx);
    app.dialogs.images_to_stack.title_filter = "keep".into();
    app.dialogs.images_to_stack.keep = true;
    let id = app.apply_images_to_stack_dialog(&ctx).unwrap().unwrap();
    let doc = app.session.get_image(id).unwrap();
    assert_eq!(doc.stack_size(), 2);
    assert_eq!(
        doc.slice_labels,
        vec![Some("keep".into()), Some("keeper".into())]
    );
    // The filtered-out image stays open (ImagesToStack.java 208-211 only
    // closes the included ones).
    assert_eq!(app.session.image_ids().len(), 4);

    // A second run with the same name keeps the duplicate title: ImagesToStack
    // never calls WindowManager.checkForDuplicateName, and the real 1.54u
    // oracle prints `[Stack, a, b, Stack]` for an existing `Stack` window plus
    // a new stack (ImagesToStack.java 240-252).
    app.perform(AppAction::ImagesToStack, &ctx);
    app.dialogs.images_to_stack.keep = true;
    let second = app.apply_images_to_stack_dialog(&ctx).unwrap().unwrap();
    assert_eq!(app.session.get_image(second).unwrap().title, "Stack");
    assert_ne!(second, id, "the second run opens a distinct window");
    assert_eq!(app.active_image_id(), Some(second));
}

#[test]
fn images_to_stack_error_preserves_sources() {
    let ctx = egui::Context::default();
    let (mut app, ids) = app_with(vec![gray8("a", 2, 2, 1), gray8("b", 2, 2, 2)]);
    app.perform(AppAction::ImagesToStack, &ctx);
    // No title matches the filter: ImagesToStack.java 108-113 errors out.
    app.dialogs.images_to_stack.title_filter = "zzz".into();
    assert!(app.apply_images_to_stack_dialog(&ctx).is_err());
    assert!(app.dialogs.last_error.is_some());
    assert_eq!(app.session.image_ids(), ids, "sources stay open on error");
    assert!(app.session.macro_recorder.is_empty());
}

#[test]
fn images_to_stack_mixed_types_and_palette() {
    let ctx = egui::Context::default();
    let mut lut = Lut256::default();
    lut.r = (0..=255).map(|i| (255 - i) as u8).collect();
    lut.g = lut.r.clone();
    lut.b = lut.r.clone();
    let color = ImageDocument::new(
        "color",
        2,
        2,
        PixelPlane::Color256 {
            pixels: vec![0, 1, 2, 3],
            lut: Box::new(lut),
        },
    )
    .unwrap();
    let (mut app, _) = app_with(vec![color, gray8("b", 2, 2, 7)]);
    app.perform(AppAction::ImagesToStack, &ctx);
    let id = app.apply_images_to_stack_dialog(&ctx).unwrap().unwrap();
    let doc = app.session.get_image(id).unwrap();
    // An inverted LUT that is not shared by every image is dropped, so the
    // byte slices become plain 8-bit (ImagesToStack.java 216-232).
    assert!(matches!(doc.plane(), PixelPlane::Gray8(_)));
    assert_eq!(doc.stack_size(), 2);
}

#[test]
fn stack_to_images_requires_a_stack() {
    let ctx = egui::Context::default();
    let app = ImageJApp::new();
    assert_eq!(
        app.action_enabled(AppAction::StackToImages).unwrap_err(),
        "Requires an active image"
    );

    // The item stays enabled for a single-slice image; StackEditor.java 246-249
    // reports IJ.error("\"Convert Stack to Images\" requires a stack\n"+imp).
    let (mut app, _) = app_with(vec![gray8("one", 2, 2, 1)]);
    assert!(app.action_enabled(AppAction::StackToImages).is_ok());
    app.perform(AppAction::StackToImages, &ctx);
    // Deliberate text simplification: this app shows the title, whereas Java
    // appends ImagePlus.toString() with its internal ID/type/dimensions.
    assert_eq!(
        app.dialogs.last_error.as_deref(),
        Some("\"Convert Stack to Images\" requires a stack\none")
    );
    assert_eq!(app.session.image_ids().len(), 1);
    assert!(app.session.macro_recorder.is_empty());

    let (mut app, _) = app_with(vec![gray8_stack("stk", 2, 2, 3)]);
    assert!(app.action_enabled(AppAction::StackToImages).is_ok());
    app.perform(AppAction::StackToImages, &ctx);
    assert!(app.dialogs.last_error.is_none());
    assert_eq!(app.session.image_ids().len(), 3);
}

#[test]
fn stack_to_images_creates_windows_and_closes_source() {
    let ctx = egui::Context::default();
    let (mut app, ids) = app_with(vec![gray8_stack("stk", 2, 2, 3)]);
    let source = ids[0];
    app.image_views
        .insert(source, ij_app::image_view::ImageView::new(2, 2));
    app.perform(AppAction::StackToImages, &ctx);
    assert!(app.dialogs.last_error.is_none());

    let open = app.session.image_ids();
    assert_eq!(open.len(), 3);
    assert!(
        app.session.get_image(source).is_none(),
        "source stack closed"
    );
    let titles: Vec<String> = open
        .iter()
        .map(|id| app.session.get_image(*id).unwrap().title.clone())
        .collect();
    // StackEditor.getTitle (StackEditor.java 312-315).
    assert_eq!(titles, vec!["stk-0001", "stk-0002", "stk-0003"]);
    for id in &open {
        assert_eq!(app.session.get_image(*id).unwrap().stack_size(), 1);
        assert!(app.image_views.contains_key(id), "each window gets a view");
    }
    assert!(!app.image_views.contains_key(&source), "source view pruned");
    // The last window shown ends up active (StackEditor.java 288-290).
    assert_eq!(app.active_image_id(), open.last().copied());
    assert_eq!(
        app.session.macro_recorder.last().map(String::as_str),
        Some("run(\"Stack to Images\");")
    );
}

#[test]
fn stack_to_images_over_30_slices_asks_for_confirmation() {
    let ctx = egui::Context::default();
    let (mut app, ids) = app_with(vec![gray8_stack("big", 2, 2, 31)]);
    app.perform(AppAction::StackToImages, &ctx);
    // StackEditor.java 254-262: nothing happens until the prompt is answered.
    assert_eq!(app.dialogs.stack_to_images_pending, Some(ids[0]));
    assert_eq!(app.session.image_ids(), ids);

    app.resolve_stack_to_images_confirmation(false, &ctx);
    assert!(app.dialogs.stack_to_images_pending.is_none());
    assert_eq!(app.session.image_ids(), ids, "Cancel keeps the stack");

    app.perform(AppAction::StackToImages, &ctx);
    app.resolve_stack_to_images_confirmation(true, &ctx);
    assert_eq!(app.session.image_ids().len(), 31);
    assert!(app.session.get_image(ids[0]).is_none());
}

#[test]
fn stack_to_images_rejects_hyperstack_without_flattening() {
    let ctx = egui::Context::default();
    let hyper = ImageDocument::new_hyperstack(
        "hyper",
        2,
        2,
        vec![
            PixelPlane::Gray8(vec![1; 4]),
            PixelPlane::Gray8(vec![2; 4]),
            PixelPlane::Gray8(vec![3; 4]),
            PixelPlane::Gray8(vec![4; 4]),
        ],
        2,
        2,
        1,
    )
    .unwrap();
    let (mut app, ids) = app_with(vec![hyper]);
    app.perform(AppAction::StackToImages, &ctx);
    // Composite/hyperstack conversion needs P4.4d2; the command must fail
    // explicitly instead of silently flattening C/Z/T.
    assert!(app.dialogs.last_error.is_some());
    assert_eq!(app.session.image_ids(), ids);
    assert_eq!(app.session.get_image(ids[0]).unwrap().stack_size(), 4);
    assert!(app.session.macro_recorder.is_empty());
}

#[test]
fn stack_to_images_preserves_calibration_and_display_range() {
    let ctx = egui::Context::default();
    let mut stack = gray8_stack("cal", 2, 2, 2);
    stack.calibration.pixel_width = 0.5;
    stack.calibration.pixel_height = 0.25;
    stack.calibration.unit = "um".into();
    stack.display_range.min = 10.0;
    stack.display_range.max = 30.0;
    let (mut app, _) = app_with(vec![stack]);
    app.perform(AppAction::StackToImages, &ctx);
    for id in app.session.image_ids() {
        let doc = app.session.get_image(id).unwrap();
        assert_eq!(doc.calibration.pixel_width, 0.5);
        assert_eq!(doc.calibration.pixel_height, 0.25);
        assert_eq!(doc.calibration.unit, "um");
        // ImagePlus.getStack() calls stack.update(ip), so each output keeps the
        // source's current processor range (ImagePlus.java 1770-1797).
        assert_eq!((doc.display_range.min, doc.display_range.max), (10.0, 30.0));
    }
}

#[test]
fn images_to_stack_dialog_reports_largest_by_area_not_widest() {
    let ctx = egui::Context::default();
    // 4x3 (area 12) and 5x2 (area 10): the widest image is 5 wide and the
    // tallest is 3 high, but the largest by area is 4x3
    // (ImagesToStack.java 276-290).
    let (mut app, _) = app_with(vec![gray8("a", 4, 3, 1), gray8("b", 5, 2, 2)]);
    app.perform(AppAction::ImagesToStack, &ctx);
    let dialog = app.dialogs.images_to_stack.clone();
    assert!(dialog.sizes_differ);
    assert_eq!((dialog.min_width, dialog.min_height), (5, 2));
    assert_eq!(
        (dialog.max_width, dialog.max_height),
        (4, 3),
        "largest is the largest by area, not the widest and tallest"
    );
    assert_eq!(
        ij_app::app::images_to_stack_size_message(&dialog),
        "The 2 images differ in size (smallest=5x2,\nlargest=4x3). They will be converted\nto a stack using the specified method."
    );
    render_images_to_stack(&mut app);

    // The copy target is still the union of the widest and tallest image, so
    // the message must not be "fixed" by shrinking the output.
    app.dialogs.images_to_stack.keep = true;
    let id = app.apply_images_to_stack_dialog(&ctx).unwrap().unwrap();
    let doc = app.session.get_image(id).unwrap();
    assert_eq!((doc.width, doc.height), (5, 3));
}

#[test]
fn images_to_stack_equal_size_run_resets_static_method_and_bicubic() {
    let ctx = egui::Context::default();
    let (mut app, _) = app_with(vec![gray8("a", 2, 2, 1), gray8("b", 4, 3, 2)]);
    app.perform(AppAction::ImagesToStack, &ctx);
    app.dialogs.images_to_stack.method_index = 3;
    app.dialogs.images_to_stack.bicubic = true;
    app.dialogs.images_to_stack.keep = true;
    app.apply_images_to_stack_dialog(&ctx).unwrap().unwrap();
    assert_eq!(app.dialogs.images_to_stack_method, 3);
    assert!(app.dialogs.images_to_stack_bicubic);
    assert!(app
        .session
        .macro_recorder
        .last()
        .unwrap()
        .contains("method=[Scale (largest)]"));

    // An equal-size run never reads the Method choice, so `method` keeps its
    // instance default COPY_CENTER and `staticMethod = method` resets it
    // (ImagesToStack.java 21, 107, 124-129).
    for id in app.session.image_ids() {
        app.session.close_image(id);
    }
    app.session.insert_doc(gray8("c", 2, 2, 3));
    app.session.insert_doc(gray8("d", 2, 2, 4));
    app.perform(AppAction::ImagesToStack, &ctx);
    assert!(app.dialogs.images_to_stack.open);
    assert!(!app.dialogs.images_to_stack.sizes_differ);
    app.dialogs.images_to_stack.keep = true;
    app.apply_images_to_stack_dialog(&ctx).unwrap().unwrap();
    assert_eq!(
        app.dialogs.images_to_stack_method, 0,
        "an equal-size run stores COPY_CENTER back into staticMethod"
    );
    assert_eq!(
        app.session.macro_recorder.last().map(String::as_str),
        Some("run(\"Images to Stack\", \"use keep\");"),
        "no Method choice exists when the sizes match"
    );

    assert!(!app.dialogs.images_to_stack_bicubic,
        "the hidden Bicubic field keeps its instance default false (ImagesToStack.java 22, 113, 126)");

    // The next differing-size run therefore starts from Copy (center).
    for id in app.session.image_ids() {
        app.session.close_image(id);
    }
    app.session.insert_doc(gray8("e", 2, 2, 5));
    app.session.insert_doc(gray8("f", 4, 3, 6));
    app.perform(AppAction::ImagesToStack, &ctx);
    assert!(app.dialogs.images_to_stack.sizes_differ);
    assert_eq!(app.dialogs.images_to_stack.method_index, 0);
    assert!(!app.dialogs.images_to_stack.bicubic);
}

#[test]
fn images_to_stack_closes_only_merged_2d_windows_and_prunes_their_views() {
    let ctx = egui::Context::default();
    // The stack window sits between the two 2D inputs. Java keeps it and closes
    // exactly the merged 2D images (ImagesToStack.java 60-66, 208-211); the
    // real 1.54u oracle prints `[stk, Stack]` for this layout.
    let (mut app, ids) = app_with(vec![
        gray8("a", 2, 2, 1),
        gray8_stack("stk", 2, 2, 3),
        gray8("b", 2, 2, 2),
    ]);
    let (a_id, stack_id, b_id) = (ids[0], ids[1], ids[2]);
    for id in &ids {
        app.image_views
            .insert(*id, ij_app::image_view::ImageView::new(2, 2));
    }
    app.perform(AppAction::ImagesToStack, &ctx);
    let new_id = app.apply_images_to_stack_dialog(&ctx).unwrap().unwrap();

    assert_eq!(
        app.session.image_ids(),
        vec![stack_id, new_id],
        "the stack window survives; both merged 2D windows close"
    );
    assert!(app.session.get_image(a_id).is_none());
    assert!(app.session.get_image(b_id).is_none());
    assert_eq!(app.session.get_image(stack_id).unwrap().stack_size(), 3);
    assert_eq!(app.session.get_image(new_id).unwrap().stack_size(), 2);
    assert_eq!(app.active_image_id(), Some(new_id));

    // View state follows the session: the closed sources are pruned, the
    // surviving stack keeps its view and the new stack gets one.
    assert_eq!(app.image_views.len(), 2);
    assert!(app.image_views.contains_key(&stack_id));
    assert!(app.image_views.contains_key(&new_id));
    assert!(!app.image_views.contains_key(&a_id));
    assert!(!app.image_views.contains_key(&b_id));
}

#[test]
fn images_to_stack_keep_sources_keeps_every_view() {
    let ctx = egui::Context::default();
    let (mut app, ids) = app_with(vec![
        gray8_stack("stk", 2, 2, 3),
        gray8("a", 2, 2, 1),
        gray8("b", 2, 2, 2),
    ]);
    for id in &ids {
        app.image_views
            .insert(*id, ij_app::image_view::ImageView::new(2, 2));
    }
    app.perform(AppAction::ImagesToStack, &ctx);
    app.dialogs.images_to_stack.keep = true;
    let new_id = app.apply_images_to_stack_dialog(&ctx).unwrap().unwrap();
    assert_eq!(app.session.image_ids().len(), 4);
    assert_eq!(app.image_views.len(), 4);
    for id in ids.iter().chain(std::iter::once(&new_id)) {
        assert!(app.image_views.contains_key(id));
    }
    assert_eq!(app.active_image_id(), Some(new_id));
}

#[test]
fn images_to_stack_multiple_stacks_error_includes_concatenate_hint() {
    // ImagesToStack.java 69-74 appends the hint only when stackCount > 1.
    let ctx = egui::Context::default();
    for include_image in [false, true] {
        let mut docs = vec![gray8_stack("s1", 2, 2, 2), gray8_stack("s2", 2, 2, 3)];
        if include_image {
            docs.push(gray8("a", 2, 2, 1));
        }
        let (mut app, _) = app_with(docs);
        let ids = app.session.image_ids();
        app.perform(AppAction::ImagesToStack, &ctx);
        assert_eq!(app.dialogs.last_error.as_deref(), Some(
            "At least two open 2D images required.\n \nUse the Image>Stacks>Tools>Concatenate\ncommand to combine stacks."
        ));
        assert!(!app.dialogs.images_to_stack.open);
        assert_eq!(app.session.image_ids(), ids);
        assert!(app.session.macro_recorder.is_empty());
    }
}
