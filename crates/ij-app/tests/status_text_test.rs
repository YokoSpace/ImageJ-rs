use ij_app::pixel_status;
use ij_bridge::JavaOracleBridge;
use ij_core::{Calibration, ImageDocument, Lut256, PixelPlane};
use std::path::PathBuf;

#[test]
fn out_of_bounds_returns_none() {
    let doc = ImageDocument::new_ramp_8bit("ramp", 16, 16).unwrap();
    assert_eq!(pixel_status(&doc, -1, 0), None);
    assert_eq!(pixel_status(&doc, 0, -1), None);
    assert_eq!(pixel_status(&doc, 16, 0), None);
    assert_eq!(pixel_status(&doc, 0, 16), None);
}

#[test]
fn uncalibrated_five_formats() {
    // 1. Gray8
    let gray8 = ImageDocument::new_ramp_8bit("ramp8", 16, 16).unwrap();
    assert_eq!(
        pixel_status(&gray8, 3, 4),
        Some(" x=3, y=4, value=48".to_string())
    );

    // 2. Gray16
    let gray16 = ImageDocument::new_ramp_16bit("ramp16", 16, 16).unwrap();
    assert_eq!(
        pixel_status(&gray16, 3, 4),
        Some(" x=3, y=4, value=12288".to_string())
    );

    // 3. Color256
    let mut lut = Lut256::default();
    lut.r[48] = 10;
    lut.g[48] = 20;
    lut.b[48] = 30;
    let mut pixels = vec![0u8; 256];
    pixels[4 * 16 + 3] = 48;
    let c256 = ImageDocument::new(
        "c256",
        16,
        16,
        PixelPlane::Color256 {
            pixels,
            lut: Box::new(lut),
        },
    )
    .unwrap();
    assert_eq!(
        pixel_status(&c256, 3, 4),
        Some(" x=3, y=4, index=48, value=10,20,30".to_string())
    );

    // 4. Gray32
    let mut fpixels = vec![0.0f32; 256];
    fpixels[0] = 1.0;
    fpixels[1 * 16 + 1] = 0.5;
    fpixels[2 * 16 + 2] = 1e-5;
    let gray32 = ImageDocument::new("g32", 16, 16, PixelPlane::Gray32(fpixels)).unwrap();
    assert_eq!(
        pixel_status(&gray32, 0, 0),
        Some(" x=0, y=0, value=1.0".to_string())
    );
    assert_eq!(
        pixel_status(&gray32, 1, 1),
        Some(" x=1, y=1, value=0.5000".to_string())
    );
    assert_eq!(
        pixel_status(&gray32, 2, 2),
        Some(" x=2, y=2, value=1.0000E-5".to_string())
    );

    // 5. ColorRgb
    let mut rgb_pixels = vec![0u32; 256];
    // Red: (255, 0, 0)
    rgb_pixels[1 * 16 + 1] = 0xff00_0000 | (255 << 16);
    // (18, 52, 86) -> #123456
    rgb_pixels[2 * 16 + 2] = 0xff00_0000 | (18 << 16) | (52 << 8) | 86;
    let rgb = ImageDocument::new("rgb", 16, 16, PixelPlane::ColorRgb(rgb_pixels)).unwrap();
    assert_eq!(
        pixel_status(&rgb, 1, 1),
        Some(" x=1, y=1, value=255,000,000 (red)".to_string())
    );
    assert_eq!(
        pixel_status(&rgb, 2, 2),
        Some(" x=2, y=2, value=018,052,086 (#123456)".to_string())
    );
}

#[test]
fn gray8_calibrated_spatial() {
    let mut doc = ImageDocument::new_ramp_8bit("calibrated", 16, 16).unwrap();
    doc.calibration = Calibration {
        pixel_width: 0.5,
        pixel_height: 0.5,
        pixel_depth: 1.0,
        unit: "mm".to_string(),
        invert_y: false,
    };
    // x=3 -> 1.50 (3), y=4 -> 2 (4)
    assert_eq!(
        pixel_status(&doc, 3, 4),
        Some(" x=1.50 (3), y=2 (4), value=48".to_string())
    );
}

#[test]
fn three_slice_stack_z_coordinates() {
    let plane1 = PixelPlane::Gray8(vec![10u8; 256]);
    let mut plane2_pixels = vec![0u8; 256];
    plane2_pixels[4 * 16 + 3] = 48;
    let plane2 = PixelPlane::Gray8(plane2_pixels);
    let plane3 = PixelPlane::Gray8(vec![30u8; 256]);
    let mut doc = ImageDocument::new_stack("stack", 16, 16, vec![plane1, plane2, plane3]).unwrap();
    doc.set_slice(2).unwrap();

    // Uncalibrated z
    assert_eq!(
        pixel_status(&doc, 3, 4),
        Some(" x=3, y=4, z=1, value=48".to_string())
    );

    // Calibrated z
    doc.calibration = Calibration {
        pixel_width: 0.5,
        pixel_height: 0.5,
        pixel_depth: 1.5,
        unit: "mm".to_string(),
        invert_y: false,
    };
    assert_eq!(
        pixel_status(&doc, 3, 4),
        Some(" x=1.50 (3), y=2 (4), z=1.50 (1), value=48".to_string())
    );
}

#[test]
fn oracle_ui_probe_parity_checks() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("repo root")
        .to_path_buf();
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root).expect("bridge");

    // 1. 8-bit ramp
    let doc8 = ImageDocument::new_ramp_8bit("ramp8", 16, 16).unwrap();
    let rust_status = pixel_status(&doc8, 3, 4).unwrap();
    let out = bridge
        .run_macro_on_image(
            &doc8,
            "print(call(\"tools.oracle.UiProbe.status\", \"3\", \"4\"));",
        )
        .unwrap();
    assert_eq!(rust_status, out.stdout.trim_end());

    // 2. 16-bit ramp
    let doc16 = ImageDocument::new_ramp_16bit("ramp16", 16, 16).unwrap();
    let rust_status = pixel_status(&doc16, 3, 4).unwrap();
    let out = bridge
        .run_macro_on_image(
            &doc16,
            "print(call(\"tools.oracle.UiProbe.status\", \"3\", \"4\"));",
        )
        .unwrap();
    assert_eq!(rust_status, out.stdout.trim_end());

    // 3. 32-bit ramp
    let doc32 = ImageDocument::new_ramp_32bit("ramp32", 16, 16).unwrap();
    let rust_status = pixel_status(&doc32, 3, 4).unwrap();
    let out = bridge
        .run_macro_on_image(
            &doc32,
            "print(call(\"tools.oracle.UiProbe.status\", \"3\", \"4\"));",
        )
        .unwrap();
    assert_eq!(rust_status, out.stdout.trim_end());

    // 4. RGB
    let mut rgb_pixels = vec![0u32; 256];
    rgb_pixels[4 * 16 + 3] = 0xff00_0000 | (255 << 16); // red
    let doc_rgb = ImageDocument::new("rgb", 16, 16, PixelPlane::ColorRgb(rgb_pixels)).unwrap();
    let rust_status = pixel_status(&doc_rgb, 3, 4).unwrap();
    let out = bridge
        .run_macro_on_image(
            &doc_rgb,
            "print(call(\"tools.oracle.UiProbe.status\", \"3\", \"4\"));",
        )
        .unwrap();
    assert_eq!(rust_status, out.stdout.trim_end());
}

