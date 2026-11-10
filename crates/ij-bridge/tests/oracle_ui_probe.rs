use ij_bridge::JavaOracleBridge;
use ij_core::ImageDocument;
use std::path::PathBuf;

#[test]
fn ui_probe_exposes_reference_status_subtitle_and_size() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("repository root")
        .to_path_buf();
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root).expect("oracle bridge");
    let image = ImageDocument::new_ramp_8bit("ramp", 16, 16).expect("ramp image");
    let output = bridge
        .run_macro_on_image(
            &image,
            "print(call(\"tools.oracle.UiProbe.status\", \"3\", \"4\"));\n\
             print(call(\"tools.oracle.UiProbe.subtitle\"));\n\
             print(call(\"tools.oracle.UiProbe.imageSize\"));",
        )
        .expect("UI probe macro should run");

    let lines: Vec<_> = output.stdout.lines().collect();
    assert_eq!(lines.len(), 3, "unexpected oracle output: {}", output.stdout);
    assert!(lines[0].starts_with(" x=3, y=4, value="), "{}", lines[0]);
    assert!(lines[1].starts_with("16x16 pixels; 8-bit; "), "{}", lines[1]);
    assert_eq!(lines[2], "0K");
}
