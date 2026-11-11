use ij_bridge::JavaOracleBridge;
use ij_core::{
    default_duplicate_title, display_histogram, format_ij_num, CoreError, DisplayHistogram,
    DisplayRange, ImageDocument, LineRoi, Lut256, MeasurementConfig, MeasurementFlags,
    MeasurementSummary, PixelPlane, PolygonRoi, RectRoi, ResultsTable, Roi, StackPosition,
    ThresholdLevels, Wand, WandMode,
};
use ij_io::{load_image, load_tiff, save_jpeg, save_png, save_tiff};
use ij_session::{Command, CommandOutcome, Session};
use std::collections::BTreeMap;
use std::path::PathBuf;
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .to_path_buf()
}

fn oracle_wand_points(doc: &ImageDocument, prelude: &str, x: i32, y: i32) -> Vec<(i32, i32)> {
    oracle_wand_points_with_options(doc, prelude, x, y, 0.0, "Legacy")
}

fn oracle_wand_points_with_options(
    doc: &ImageDocument,
    prelude: &str,
    x: i32,
    y: i32,
    tolerance: f64,
    mode: &str,
) -> Vec<(i32, i32)> {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let macro_text = format!(
        "{prelude} doWand({x},{y},{tolerance},\"{mode}\"); getSelectionCoordinates(xs,ys); \
         print(\"WAND_COUNT:\"+xs.length); for(i=0;i<xs.length;i++) print(xs[i]+\",\"+ys[i]); run(\"Select None\");"
    );
    let output = bridge.run_macro_on_image(doc, &macro_text).unwrap();
    let mut lines = output.stdout.lines();
    let count = lines.next().unwrap().strip_prefix("WAND_COUNT:").unwrap().parse::<usize>().unwrap();
    let points = lines.map(|line| {
        let (x, y) = line.split_once(',').unwrap();
        (x.parse::<i32>().unwrap(), y.parse::<i32>().unwrap())
    }).collect::<Vec<_>>();
    assert_eq!(points.len(), count);
    points
}

#[test]
fn parity_wand_explicit_connectivity_and_tolerance_match_java_oracle() {
    let mut pixels = vec![0u8; 49];
    for index in 1..=5 { pixels[index * 7 + index] = 7; }
    let doc = ImageDocument::new("wand-modes", 7, 7, PixelPlane::Gray8(pixels)).unwrap();
    for (mode, java_mode) in [
        (WandMode::FourConnected, "4-connected"),
        (WandMode::EightConnected, "8-connected"),
    ] {
        let oracle = oracle_wand_points_with_options(&doc, "", 3, 3, 0.0, java_mode);
        assert_eq!(Wand::auto_outline(&doc, 3, 3, 0.0, mode), oracle);
    }

    let mut pixels = vec![0u8; 25];
    for y in 1..=3 { for x in 1..=3 { pixels[y * 5 + x] = 7; } }
    pixels[2 * 5 + 2] = 8;
    let doc = ImageDocument::new("wand-tolerance", 5, 5, PixelPlane::Gray8(pixels)).unwrap();
    let oracle = oracle_wand_points_with_options(&doc, "", 2, 2, 1.0, "Legacy");
    assert_eq!(Wand::auto_outline(&doc, 2, 2, 1.0, WandMode::Legacy), oracle);
}

#[test]
fn parity_legacy_wand_rect_outline_matches_java_oracle() {
    let mut pixels = vec![0u8; 25];
    for y in 1..=3 { for x in 1..=3 { pixels[y * 5 + x] = 7; } }
    let doc = ImageDocument::new("wand-rect", 5, 5, PixelPlane::Gray8(pixels)).unwrap();
    let oracle = oracle_wand_points(&doc, "", 2, 2);
    assert_eq!(Wand::auto_outline(&doc, 2, 2, 0.0, WandMode::Legacy), oracle);
}

#[test]
fn parity_legacy_wand_diagonal_line_matches_java_oracle() {
    let mut pixels = vec![0u8; 49];
    for index in 1..=5 { pixels[index * 7 + index] = 7; }
    let doc = ImageDocument::new("wand-line", 7, 7, PixelPlane::Gray8(pixels)).unwrap();
    let oracle = oracle_wand_points(&doc, "", 3, 3);
    assert_eq!(Wand::auto_outline(&doc, 3, 3, 0.0, WandMode::Legacy), oracle);
}

#[test]
fn parity_legacy_wand_respects_active_threshold_like_java_oracle() {
    let mut pixels = vec![0u8; 25];
    for y in 1..=3 { for x in 1..=3 { pixels[y * 5 + x] = 7; } }
    pixels[2 * 5 + 2] = 8;
    let mut doc = ImageDocument::new("wand-threshold", 5, 5, PixelPlane::Gray8(pixels)).unwrap();
    doc.threshold = Some(ThresholdLevels { lower: 7.0, upper: 8.0 });
    let oracle = oracle_wand_points(&doc, "setThreshold(7,8);", 2, 2);
    assert_eq!(Wand::auto_outline(&doc, 2, 2, 0.0, WandMode::Legacy), oracle);
}

/// Parses the first two non-empty lines of ImageJ's batch-mode Results table
/// (header row followed by row 1) into column-name/value pairs.
fn oracle_columns(stdout: &str) -> BTreeMap<String, String> {
    let mut lines = stdout.lines().filter(|l| !l.trim().is_empty());
    let header = lines.next().expect("oracle header");
    let row = lines.next().expect("oracle data row");
    let names: Vec<&str> = header.split_whitespace().collect();
    let values: Vec<&str> = row.split_whitespace().collect();
    assert_eq!(
        values.len(),
        names.len() + 1,
        "unexpected oracle table shape:\n{stdout}"
    );
    names
        .into_iter()
        .zip(values[1..].iter())
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect()
}

/// Reads the `Array.print` line that the oracle emits after the `HIST` marker.
fn oracle_histogram(stdout: &str) -> Vec<u64> {
    let mut lines = stdout.lines();
    while let Some(line) = lines.next() {
        if line.trim() == "HIST" {
            let data = lines.next().expect("histogram data line");
            return data
                .split(',')
                .map(|token| token.trim().parse::<u64>().expect("histogram count"))
                .collect();
        }
    }
    panic!("oracle output had no HIST marker:\n{stdout}");
}

fn assert_measurement_columns(stats: &MeasurementSummary, columns: &BTreeMap<String, String>) {
    let expected = [
        ("Area", stats.area),
        ("Mean", stats.mean),
        ("StdDev", stats.std_dev),
        ("Mode", stats.mode),
        ("Min", stats.min),
        ("Max", stats.max),
        ("X", stats.x_centroid),
        ("Y", stats.y_centroid),
        ("XM", stats.x_center_of_mass),
        ("YM", stats.y_center_of_mass),
        ("Median", stats.median),
        ("Skew", stats.skewness),
        ("Kurt", stats.kurtosis),
    ];
    for (name, value) in expected {
        let oracle = columns
            .get(name)
            .unwrap_or_else(|| panic!("oracle did not report column {name}"));
        assert_eq!(
            &format_ij_num(value),
            oracle,
            "column {name} differs (full oracle row: {columns:?})"
        );
    }
}

const MOMENT_MEASUREMENTS: &str =
    "area mean standard modal min max median skewness kurtosis centroid center";

#[test]
fn parity_8bit_ramp_measurement_table_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let oracle = bridge
        .eval_macro(
            "newImage('test', '8-bit ramp', 16, 16, 1); makeRectangle(2, 3, 8, 6); run('Measure');",
        )
        .unwrap();

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

    assert_eq!(session.results.to_tsv_basic().trim(), oracle.stdout.trim());
}

#[test]
fn parity_tiff_exchange_and_invert_matches_java_oracle_pixels() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = ImageDocument::new_ramp_8bit("invert_test.tif", 16, 16).unwrap();
    doc.set_rect_roi(4, 4, 8, 8).unwrap();

    let oracle_res = bridge
        .run_macro_on_image(&doc, "makeRectangle(4, 4, 8, 8); run(\"Invert\");")
        .unwrap();
    let oracle_doc = oracle_res
        .output_image
        .expect("oracle must return saved TIFF");

    doc.invert();
    assert_eq!(doc.plane(), oracle_doc.plane());
}

#[test]
fn parity_file_revert_restores_saved_tiff_like_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let original = ImageDocument::new_ramp_8bit("revert.tif", 16, 16).unwrap();
    let oracle = bridge
        .run_macro_on_image(
            &original,
            "makeRectangle(2,3,5,6); run(\"Invert\"); run(\"Revert\");",
        )
        .unwrap()
        .output_image
        .expect("oracle must return the reverted TIFF");

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("revert.tif");
    save_tiff(&path, &original).unwrap();
    let mut session = Session::new();
    let id = match session
        .dispatch(Command::OpenTiff { path: path.clone() })
        .unwrap()
    {
        CommandOutcome::ImageOpened(id) => id,
        other => panic!("expected ImageOpened, got {other:?}"),
    };
    session
        .dispatch(Command::MakeRectRoi {
            x: 2,
            y: 3,
            width: 5,
            height: 6,
        })
        .unwrap();
    session.dispatch(Command::Invert).unwrap();
    assert!(session.is_dirty(id));
    session.dispatch(Command::Revert).unwrap();

    assert_eq!(session.get_image(id).unwrap().slices, oracle.slices);
    assert_eq!(session.get_image(id).unwrap().roi, oracle.roi);
    assert!(!session.is_dirty(id));
    assert_eq!(
        session.macro_recorder.last().map(String::as_str),
        Some("run(\"Revert\");")
    );
}

#[test]
fn parity_16bit_ramp_measurement_table_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let oracle = bridge
        .eval_macro(
            "newImage('test', '16-bit ramp', 16, 16, 1); makeRectangle(2, 3, 8, 6); run('Measure');",
        )
        .unwrap();

    let mut doc = ImageDocument::new_ramp_16bit("test", 16, 16).unwrap();
    doc.set_rect_roi(2, 3, 8, 6).unwrap();
    let mut table = ResultsTable::new();
    table.push(doc.measure().unwrap());

    assert_eq!(table.to_tsv_basic().trim(), oracle.stdout.trim());
}

#[test]
fn parity_16bit_ramp_invert_matches_java_oracle_pixels() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = ImageDocument::new_ramp_16bit("invert16.tif", 16, 16).unwrap();
    // ShortProcessor.invert() resets min/max to the image range before processing
    // (original_imagej/ij/process/ShortProcessor.java lines 591-596), so a display
    // range differing from the image range must not change the output pixels.
    doc.display_range = DisplayRange {
        min: 100.0,
        max: 200.0,
    };
    doc.set_rect_roi(4, 4, 8, 8).unwrap();

    let oracle_res = bridge
        .run_macro_on_image(
            &doc,
            "setMinAndMax(100, 200); makeRectangle(4, 4, 8, 8); run(\"Invert\");",
        )
        .unwrap();
    let oracle_doc = oracle_res
        .output_image
        .expect("oracle must return saved TIFF");

    doc.invert();
    assert_eq!(doc.plane(), oracle_doc.plane());
}

#[test]
fn parity_32bit_ramp_measurement_table_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let oracle = bridge
        .eval_macro(
            "newImage('test', '32-bit ramp', 16, 16, 1); makeRectangle(2, 3, 8, 6); run('Measure');",
        )
        .unwrap();

    let mut doc = ImageDocument::new_ramp_32bit("test", 16, 16).unwrap();
    doc.set_rect_roi(2, 3, 8, 6).unwrap();
    let mut table = ResultsTable::new();
    table.push(doc.measure().unwrap());

    assert_eq!(table.to_tsv_basic().trim(), oracle.stdout.trim());
}

#[test]
fn parity_32bit_ramp_invert_matches_java_oracle_pixels() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = ImageDocument::new_ramp_32bit("invert32.tif", 16, 16).unwrap();
    // The TIFF bridge drops the display range, so set the same 0-1 range ImageJ
    // uses for a new 32-bit ramp (NewImage.java line 354) in both runtimes.
    doc.set_rect_roi(4, 4, 8, 8).unwrap();

    let oracle_res = bridge
        .run_macro_on_image(
            &doc,
            "setMinAndMax(0, 1); makeRectangle(4, 4, 8, 8); run(\"Invert\");",
        )
        .unwrap();
    let oracle_doc = oracle_res
        .output_image
        .expect("oracle must return saved TIFF");

    doc.invert();
    assert_eq!(doc.plane(), oracle_doc.plane());
}

#[test]
fn parity_8bit_statistics_and_histogram_match_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let oracle = bridge
        .eval_macro(&format!(
            "newImage('test', '8-bit ramp', 16, 16, 1); \
             run(\"Set Measurements...\", \"{MOMENT_MEASUREMENTS}\"); \
             makeRectangle(2, 3, 10, 8); run('Measure'); \
             getHistogram(values, counts, 256); print(\"HIST\"); Array.print(counts);"
        ))
        .unwrap();

    let mut doc = ImageDocument::new_ramp_8bit("test", 16, 16).unwrap();
    doc.set_rect_roi(2, 3, 10, 8).unwrap();
    let stats = doc.measure().unwrap();

    assert_measurement_columns(&stats, &oracle_columns(&oracle.stdout));
    assert_eq!(stats.histogram, oracle_histogram(&oracle.stdout));
    assert_eq!(stats.histogram.len(), 256);
}

#[test]
fn parity_8bit_duplicate_values_statistics_match_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let oracle = bridge
        .eval_macro(&format!(
            "newImage('test', '8-bit', 6, 4, 1); \
             for (y=0; y<4; y++) {{ for (x=0; x<6; x++) {{ setPixel(x, y, (x*7+y*13)%17); }} }} \
             run(\"Set Measurements...\", \"{MOMENT_MEASUREMENTS}\"); run('Measure'); \
             getHistogram(values, counts, 256); print(\"HIST\"); Array.print(counts);"
        ))
        .unwrap();

    let width = 6u32;
    let height = 4u32;
    let mut pixels = vec![0u8; (width * height) as usize];
    for y in 0..height {
        for x in 0..width {
            pixels[(y * width + x) as usize] = ((x * 7 + y * 13) % 17) as u8;
        }
    }
    let doc = ImageDocument::new("test", width, height, PixelPlane::Gray8(pixels)).unwrap();
    let stats = doc.measure().unwrap();

    assert_measurement_columns(&stats, &oracle_columns(&oracle.stdout));
    assert_eq!(stats.histogram, oracle_histogram(&oracle.stdout));
}

#[test]
fn parity_8bit_zero_valued_moments_match_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let oracle = bridge
        .eval_macro(&format!(
            "newImage('test', '8-bit', 4, 4, 1); \
             for (y=0; y<4; y++) {{ for (x=0; x<4; x++) {{ setPixel(x, y, 0); }} }} \
             run(\"Set Measurements...\", \"{MOMENT_MEASUREMENTS}\"); run('Measure'); \
             getHistogram(values, counts, 256); print(\"HIST\"); Array.print(counts);"
        ))
        .unwrap();

    let doc = ImageDocument::new("test", 4, 4, PixelPlane::Gray8(vec![0u8; 16])).unwrap();
    let stats = doc.measure().unwrap();

    assert_measurement_columns(&stats, &oracle_columns(&oracle.stdout));
    assert_eq!(stats.histogram, oracle_histogram(&oracle.stdout));
}

#[test]
fn parity_16bit_statistics_and_histogram_match_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let oracle = bridge
        .eval_macro(&format!(
            "newImage('test', '16-bit ramp', 16, 16, 1); \
             run(\"Set Measurements...\", \"{MOMENT_MEASUREMENTS}\"); \
             makeRectangle(2, 3, 8, 6); run('Measure'); \
             getHistogram(values, counts, 65536); print(\"HIST\"); Array.print(counts);"
        ))
        .unwrap();

    let mut doc = ImageDocument::new_ramp_16bit("test", 16, 16).unwrap();
    doc.set_rect_roi(2, 3, 8, 6).unwrap();
    let stats = doc.measure().unwrap();

    assert_measurement_columns(&stats, &oracle_columns(&oracle.stdout));
    assert_eq!(stats.histogram, oracle_histogram(&oracle.stdout));
    assert_eq!(stats.histogram.len(), 65536);
}

#[test]
fn parity_16bit_duplicate_values_statistics_match_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let oracle = bridge
        .eval_macro(&format!(
            "newImage('test', '16-bit', 8, 6, 1); \
             for (y=0; y<6; y++) {{ for (x=0; x<8; x++) {{ setPixel(x, y, (x*97+y*211)%1000); }} }} \
             run(\"Set Measurements...\", \"{MOMENT_MEASUREMENTS}\"); run('Measure'); \
             getHistogram(values, counts, 65536); print(\"HIST\"); Array.print(counts);"
        ))
        .unwrap();

    let width = 8u32;
    let height = 6u32;
    let mut pixels = vec![0u16; (width * height) as usize];
    for y in 0..height {
        for x in 0..width {
            pixels[(y * width + x) as usize] = ((x * 97 + y * 211) % 1000) as u16;
        }
    }
    let doc = ImageDocument::new("test", width, height, PixelPlane::Gray16(pixels)).unwrap();
    let stats = doc.measure().unwrap();

    assert_measurement_columns(&stats, &oracle_columns(&oracle.stdout));
    assert_eq!(stats.histogram, oracle_histogram(&oracle.stdout));
}

#[test]
fn parity_16bit_zero_valued_moments_match_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let oracle = bridge
        .eval_macro(&format!(
            "newImage('test', '16-bit', 4, 4, 1); \
             run(\"Set Measurements...\", \"{MOMENT_MEASUREMENTS}\"); run('Measure'); \
             getHistogram(values, counts, 65536); print(\"HIST\"); Array.print(counts);"
        ))
        .unwrap();

    let doc = ImageDocument::new("test", 4, 4, PixelPlane::Gray16(vec![0u16; 16])).unwrap();
    let stats = doc.measure().unwrap();

    assert_measurement_columns(&stats, &oracle_columns(&oracle.stdout));
    assert_eq!(stats.histogram, oracle_histogram(&oracle.stdout));
}

#[test]
fn parity_32bit_statistics_and_histogram_match_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let oracle = bridge
        .eval_macro(&format!(
            "newImage('test', '32-bit ramp', 16, 16, 1); \
             run(\"Set Measurements...\", \"{MOMENT_MEASUREMENTS}\"); \
             makeRectangle(2, 3, 8, 6); run('Measure'); \
             getHistogram(values, counts, 256); print(\"HIST\"); Array.print(counts);"
        ))
        .unwrap();

    let mut doc = ImageDocument::new_ramp_32bit("test", 16, 16).unwrap();
    doc.set_rect_roi(2, 3, 8, 6).unwrap();
    let stats = doc.measure().unwrap();

    assert_measurement_columns(&stats, &oracle_columns(&oracle.stdout));
    assert_eq!(stats.histogram, oracle_histogram(&oracle.stdout));
}

#[test]
fn parity_32bit_zero_valued_moments_match_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let doc = ImageDocument::new("zero32.tif", 4, 4, PixelPlane::Gray32(vec![0.0; 16])).unwrap();
    let oracle = bridge
        .run_macro_on_image(
            &doc,
            &format!(
                "run(\"Set Measurements...\", \"{MOMENT_MEASUREMENTS}\"); run(\"Measure\"); \
                 getHistogram(values, counts, 256); print(\"HIST\"); Array.print(counts);"
            ),
        )
        .unwrap();
    let stats = doc.measure().unwrap();

    assert_measurement_columns(&stats, &oracle_columns(&oracle.stdout));
    assert_eq!(stats.histogram, oracle_histogram(&oracle.stdout));
}

#[test]
fn parity_32bit_non_finite_pixels_match_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let values = [
        f32::NAN,
        f32::INFINITY,
        f32::NEG_INFINITY,
        -2.5,
        0.0,
        1.0,
        2.0,
        3.0,
        4.0,
        5.5,
        -1.5,
        7.25,
    ];
    let doc = ImageDocument::new(
        "special_float.tif",
        4,
        3,
        PixelPlane::Gray32(values.to_vec()),
    )
    .unwrap();

    let oracle = bridge
        .run_macro_on_image(
            &doc,
            &format!(
                "run(\"Set Measurements...\", \"{MOMENT_MEASUREMENTS}\"); run(\"Measure\"); \
                 getHistogram(values, counts, 256); print(\"HIST\"); Array.print(counts);"
            ),
        )
        .unwrap();
    let stats = doc.measure().unwrap();

    // FloatStatistics excludes NaN and both infinities from every statistic,
    // including the centroid, so Area is 9 of the 12 pixels.
    assert_eq!(stats.pixel_count, 9);
    assert_measurement_columns(&stats, &oracle_columns(&oracle.stdout));
    assert_eq!(stats.histogram, oracle_histogram(&oracle.stdout));
}

#[test]
fn parity_32bit_all_non_finite_pixels_match_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    // FloatStatistics applies the same threshold comparisons to NaN and to both
    // infinities, so all three are excluded and leave an empty pixel set.
    for (name, value) in [
        ("nan", f32::NAN),
        ("pos_inf", f32::INFINITY),
        ("neg_inf", f32::NEG_INFINITY),
    ] {
        let doc = ImageDocument::new(
            format!("{name}.tif"),
            2,
            2,
            PixelPlane::Gray32(vec![value; 4]),
        )
        .unwrap();
        let oracle = bridge
            .run_macro_on_image(
                &doc,
                &format!(
                    "run(\"Set Measurements...\", \"{MOMENT_MEASUREMENTS}\"); run(\"Measure\"); \
                     getHistogram(values, counts, 256); print(\"HIST\"); Array.print(counts);"
                ),
            )
            .unwrap();
        let stats = doc.measure().unwrap();
        assert_eq!(stats.pixel_count, 0, "{name}");
        assert_measurement_columns(&stats, &oracle_columns(&oracle.stdout));
        assert_eq!(stats.histogram, oracle_histogram(&oracle.stdout), "{name}");
    }
}

#[test]
fn parity_8bit_fractional_results_table_formatting() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let oracle = bridge
        .eval_macro(
            "newImage('test', '8-bit', 4, 2, 1); \
             setPixel(0, 0, 1); setPixel(1, 0, 2); setPixel(2, 0, 3); setPixel(3, 0, 4); \
             setPixel(0, 1, 5); setPixel(1, 1, 6); setPixel(2, 1, 7); setPixel(3, 1, 8); \
             run('Measure');",
        )
        .unwrap();

    let doc = ImageDocument::new(
        "test",
        4,
        2,
        PixelPlane::Gray8(vec![1, 2, 3, 4, 5, 6, 7, 8]),
    )
    .unwrap();
    let mut table = ResultsTable::new();
    table.push(doc.measure().unwrap());

    // ResultsTable prints the fractional mean as 4.500, not 4.5.
    assert_eq!(table.to_tsv_basic().trim(), oracle.stdout.trim());
}

#[test]
fn parity_rgb_statistics_and_histogram_match_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let width = 4u32;
    let height = 3u32;
    let mut pixels = Vec::new();
    for y in 0..height {
        for x in 0..width {
            let r = x * 60 + 10;
            let g = y * 70 + 20;
            let b = (x * 30 + y * 40) % 256;
            pixels.push(0xff00_0000 | (r << 16) | (g << 8) | b);
        }
    }
    let doc =
        ImageDocument::new("rgb_test.tif", width, height, PixelPlane::ColorRgb(pixels)).unwrap();

    let oracle = bridge
        .run_macro_on_image(
            &doc,
            &format!(
                "run(\"Set Measurements...\", \"{MOMENT_MEASUREMENTS}\"); run(\"Measure\"); \
                 getHistogram(values, counts, 256); print(\"HIST\"); Array.print(counts);"
            ),
        )
        .unwrap();
    let stats = doc.measure().unwrap();

    assert_measurement_columns(&stats, &oracle_columns(&oracle.stdout));
    assert_eq!(stats.histogram, oracle_histogram(&oracle.stdout));
}

#[test]
fn parity_rgb_oval_roi_statistics_match_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let width = 6u32;
    let height = 5u32;
    let mut pixels = Vec::new();
    for y in 0..height {
        for x in 0..width {
            let r = x * 40 + 10;
            let g = y * 50 + 20;
            let b = (x * 30 + y * 40) % 256;
            pixels.push(0xff00_0000 | (r << 16) | (g << 8) | b);
        }
    }
    let mut doc =
        ImageDocument::new("rgb_oval.tif", width, height, PixelPlane::ColorRgb(pixels)).unwrap();
    doc.set_oval_roi(1, 1, 4, 3).unwrap();

    let oracle = bridge
        .run_macro_on_image(
            &doc,
            &format!(
                "makeOval(1, 1, 4, 3); \
                 run(\"Set Measurements...\", \"{MOMENT_MEASUREMENTS}\"); run(\"Measure\"); \
                 getHistogram(values, counts, 256); print(\"HIST\"); Array.print(counts);"
            ),
        )
        .unwrap();
    let stats = doc.measure().unwrap();

    assert_measurement_columns(&stats, &oracle_columns(&oracle.stdout));
    assert_eq!(stats.histogram, oracle_histogram(&oracle.stdout));
}

const AREA_FRACTION_MEASUREMENTS: &str = "area mean area_fraction";

#[test]
fn parity_perimeter_for_area_rois_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let cases = [
        ("no ROI", "", None, 1.0, 1.0),
        ("rect", "makeRectangle(2,3,8,6);", Some("rect"), 1.0, 1.0),
        ("oval", "makeOval(2,3,8,6);", Some("oval"), 1.0, 1.0),
        ("polygon", "makePolygon(2,2,10,3,8,10,3,9);", Some("polygon"), 1.0, 1.0),
        ("traced polygon", "makePolygon(2,2,10,2,10,8,2,8);", Some("traced"), 1.0, 1.0),
        ("calibrated rect", "makeRectangle(2,3,8,6);", Some("rect"), 0.5, 0.25),
        ("calibrated no ROI", "", None, 0.5, 0.25),
    ];
    for (name, roi_macro, roi_kind, pixel_width, pixel_height) in cases {
        let cal_macro = if pixel_width == 1.0 {
            String::new()
        } else {
            format!("setVoxelSize({pixel_width},{pixel_height},1,'mm');")
        };
        let oracle = bridge
            .eval_macro(&format!(
                "newImage('perimeter', '8-bit ramp', 16, 16, 1); {cal_macro} {roi_macro} \
                 run('Set Measurements...', 'area perimeter decimal=3'); run('Measure');"
            ))
            .unwrap();
        let mut doc = ImageDocument::new_ramp_8bit("perimeter", 16, 16).unwrap();
        doc.calibration.pixel_width = pixel_width;
        doc.calibration.pixel_height = pixel_height;
        match roi_kind {
            Some("rect") => doc.set_rect_roi(2, 3, 8, 6).unwrap(),
            Some("oval") => doc.set_oval_roi(2, 3, 8, 6).unwrap(),
            Some("polygon") => doc.set_polygon_roi(&[(2, 2), (10, 3), (8, 10), (3, 9)]).unwrap(),
            Some("traced") => doc.set_polygon_roi(&[(2, 2), (10, 2), (10, 8), (2, 8)]).unwrap(),
            _ => {}
        }
        let actual = format_ij_num(doc.measure().unwrap().perimeter);
        let expected = &oracle_columns(&oracle.stdout)["Perim."];
        assert_eq!(&actual, expected, "{name}");
    }
}

#[test]
fn parity_perimeter_for_line_roi_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let oracle = bridge
        .eval_macro(
            "newImage('line-perimeter','8-bit ramp',16,16,1); \
             setVoxelSize(0.5,0.25,1,'mm'); makeLine(2,3,10,9); \
             run('Set Measurements...', 'area mean perimeter decimal=3'); run('Measure');",
        )
        .unwrap();
    let mut doc = ImageDocument::new_ramp_8bit("line-perimeter", 16, 16).unwrap();
    doc.calibration.pixel_width = 0.5;
    doc.calibration.pixel_height = 0.25;
    doc.set_line_roi(2, 3, 10, 9);
    let actual = format_ij_num(doc.measure().unwrap().perimeter);
    assert_eq!(actual, oracle_columns(&oracle.stdout)["Perim."]);
}

#[test]
fn measurement_config_defaults_match_imagej_startup_state() {
    // Measurements.java lines 4-10 and Analyzer.java line 42: ImageJ starts
    // with Area, Mean and Min/Max enabled at three decimal places.
    let config = MeasurementConfig::default();
    assert_eq!(
        config.flags,
        MeasurementFlags::AREA | MeasurementFlags::MEAN | MeasurementFlags::MIN_MAX
    );
    assert_eq!(config.precision, 3);
    assert_eq!(MeasurementFlags::AREA, 1);
    assert_eq!(MeasurementFlags::MEAN, 2);
    assert_eq!(MeasurementFlags::STD_DEV, 4);
    assert_eq!(MeasurementFlags::MODE, 8);
    assert_eq!(MeasurementFlags::MIN_MAX, 16);
    assert_eq!(MeasurementFlags::CENTROID, 32);
    assert_eq!(MeasurementFlags::CENTER_OF_MASS, 64);
    assert_eq!(MeasurementFlags::PERIMETER, 128);
    assert_eq!(MeasurementFlags::LIMIT, 256);
    assert_eq!(MeasurementFlags::RECT, 512);
    assert_eq!(MeasurementFlags::ELLIPSE, 2048);
    assert_eq!(MeasurementFlags::SHAPE_DESCRIPTORS, 8192);
    assert_eq!(MeasurementFlags::INTEGRATED_DENSITY, 0x8000);
    assert_eq!(MeasurementFlags::MEDIAN, 0x10000);
    assert_eq!(MeasurementFlags::SKEWNESS, 0x20000);
    assert_eq!(MeasurementFlags::KURTOSIS, 0x40000);
    assert_eq!(MeasurementFlags::AREA_FRACTION, 0x80000);
    assert_eq!(MeasurementFlags::STACK_POSITION, 0x100000);
}

#[test]
fn parity_8bit_area_fraction_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();

    // Without a threshold, %Area is the fraction of non-zero pixels
    // (ImageStatistics.java 281-284).
    let oracle = bridge
        .eval_macro(&format!(
            "newImage('test', '8-bit ramp', 16, 16, 1); \
             run(\"Set Measurements...\", \"{AREA_FRACTION_MEASUREMENTS}\"); run('Measure');"
        ))
        .unwrap();
    let doc = ImageDocument::new_ramp_8bit("test", 16, 16).unwrap();
    let stats = doc.measure().unwrap();
    assert_eq!(
        format_ij_num(stats.area_fraction),
        oracle_columns(&oracle.stdout)["%Area"]
    );

    // An active threshold changes %Area even when Limit to threshold is off,
    // because calculateAreaFraction reads the processor threshold directly
    // (ImageStatistics.java 285-291).
    let oracle = bridge
        .eval_macro(&format!(
            "newImage('test', '8-bit ramp', 16, 16, 1); makeRectangle(2, 3, 10, 8); \
             setThreshold(100, 200); \
             run(\"Set Measurements...\", \"{AREA_FRACTION_MEASUREMENTS}\"); run('Measure');"
        ))
        .unwrap();
    let mut doc = ImageDocument::new_ramp_8bit("test", 16, 16).unwrap();
    doc.set_rect_roi(2, 3, 10, 8).unwrap();
    doc.threshold = Some(ThresholdLevels {
        lower: 100.0,
        upper: 200.0,
    });
    let stats = doc.measure().unwrap();
    assert_eq!(
        format_ij_num(stats.area_fraction),
        oracle_columns(&oracle.stdout)["%Area"]
    );
}

#[test]
fn parity_16bit_area_fraction_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();

    let oracle = bridge
        .eval_macro(&format!(
            "newImage('test', '16-bit ramp', 16, 16, 1); makeRectangle(2, 3, 10, 8); \
             run(\"Set Measurements...\", \"{AREA_FRACTION_MEASUREMENTS}\"); run('Measure');"
        ))
        .unwrap();
    let mut doc = ImageDocument::new_ramp_16bit("test", 16, 16).unwrap();
    doc.set_rect_roi(2, 3, 10, 8).unwrap();
    let stats = doc.measure().unwrap();
    assert_eq!(
        format_ij_num(stats.area_fraction),
        oracle_columns(&oracle.stdout)["%Area"]
    );

    let oracle = bridge
        .eval_macro(&format!(
            "newImage('test', '16-bit ramp', 16, 16, 1); setThreshold(20000, 45000); \
             run(\"Set Measurements...\", \"{AREA_FRACTION_MEASUREMENTS}\"); run('Measure');"
        ))
        .unwrap();
    let mut doc = ImageDocument::new_ramp_16bit("test", 16, 16).unwrap();
    doc.threshold = Some(ThresholdLevels {
        lower: 20000.0,
        upper: 45000.0,
    });
    let stats = doc.measure().unwrap();
    assert_eq!(
        format_ij_num(stats.area_fraction),
        oracle_columns(&oracle.stdout)["%Area"]
    );
}

#[test]
fn parity_32bit_area_fraction_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();

    // FloatStatistics counts non-zero samples when no threshold is active
    // (FloatStatistics.java 217-218).
    let oracle = bridge
        .eval_macro(&format!(
            "newImage('test', '32-bit ramp', 16, 16, 1); makeRectangle(2, 3, 10, 8); \
             run(\"Set Measurements...\", \"{AREA_FRACTION_MEASUREMENTS}\"); run('Measure');"
        ))
        .unwrap();
    let mut doc = ImageDocument::new_ramp_32bit("test", 16, 16).unwrap();
    doc.set_rect_roi(2, 3, 10, 8).unwrap();
    let stats = doc.measure().unwrap();
    assert_eq!(
        format_ij_num(stats.area_fraction),
        oracle_columns(&oracle.stdout)["%Area"]
    );

    // FloatStatistics compares the float pixel against float thresholds
    // (FloatStatistics.java 203-204, 219-220).
    let oracle = bridge
        .eval_macro(&format!(
            "newImage('test', '32-bit ramp', 16, 16, 1); setThreshold(0.25, 0.75); \
             run(\"Set Measurements...\", \"{AREA_FRACTION_MEASUREMENTS}\"); run('Measure');"
        ))
        .unwrap();
    let mut doc = ImageDocument::new_ramp_32bit("test", 16, 16).unwrap();
    doc.threshold = Some(ThresholdLevels {
        lower: 0.25,
        upper: 0.75,
    });
    let stats = doc.measure().unwrap();
    assert_eq!(
        format_ij_num(stats.area_fraction),
        oracle_columns(&oracle.stdout)["%Area"]
    );
}

#[test]
fn parity_rgb_area_fraction_is_zero_like_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let oracle = bridge
        .eval_macro(&format!(
            "newImage('test', 'RGB ramp', 16, 16, 1); makeRectangle(2, 3, 10, 8); \
             run(\"Set Measurements...\", \"{AREA_FRACTION_MEASUREMENTS}\"); run('Measure');"
        ))
        .unwrap();
    let mut doc =
        ImageDocument::new("test", 16, 16, PixelPlane::ColorRgb(rgb_ramp(16, 16))).unwrap();
    doc.set_rect_roi(2, 3, 10, 8).unwrap();
    // ColorStatistics never calls calculateAreaFraction, so %Area stays at
    // the ImageStatistics field default of 0.
    let stats = doc.measure().unwrap();
    assert_eq!(stats.area_fraction, 0.0);
    assert_eq!(
        format_ij_num(stats.area_fraction),
        oracle_columns(&oracle.stdout)["%Area"]
    );
}

/// `Analyze > Set Measurements...` flags exercised by the LIMIT tests.
/// Analyzer.measure always requests AREA+MEAN+STD_DEV+MODE+MIN_MAX+MEDIAN
/// (Analyzer.java 512), so those columns must all follow the threshold.
const LIMIT_MEASUREMENTS: &str = "area mean standard min centroid center integrated median \
                                  skewness kurtosis limit decimal=3";

fn limit_measurement_config() -> MeasurementConfig {
    MeasurementConfig {
        flags: MeasurementFlags::AREA
            | MeasurementFlags::MEAN
            | MeasurementFlags::STD_DEV
            | MeasurementFlags::MIN_MAX
            | MeasurementFlags::CENTROID
            | MeasurementFlags::CENTER_OF_MASS
            | MeasurementFlags::INTEGRATED_DENSITY
            | MeasurementFlags::MEDIAN
            | MeasurementFlags::SKEWNESS
            | MeasurementFlags::KURTOSIS
            | MeasurementFlags::LIMIT,
        precision: 3,
    }
}

/// Every column the LIMIT statistics can change, including the two integrated
/// densities (Analyzer.java 652-655).
fn assert_limit_measurement_columns(
    stats: &MeasurementSummary,
    columns: &BTreeMap<String, String>,
) {
    let expected = [
        ("Area", stats.area),
        ("Mean", stats.mean),
        ("StdDev", stats.std_dev),
        ("Min", stats.min),
        ("Max", stats.max),
        ("X", stats.x_centroid),
        ("Y", stats.y_centroid),
        ("XM", stats.x_center_of_mass),
        ("YM", stats.y_center_of_mass),
        ("IntDen", stats.int_den),
        ("Median", stats.median),
        ("Skew", stats.skewness),
        ("Kurt", stats.kurtosis),
        ("RawIntDen", stats.raw_int_den),
    ];
    for (name, value) in expected {
        let oracle = columns
            .get(name)
            .unwrap_or_else(|| panic!("oracle did not report column {name}"));
        assert_eq!(
            &format_ij_num(value),
            oracle,
            "column {name} differs (full oracle row: {columns:?})"
        );
    }
}

#[test]
fn parity_8bit_limit_to_threshold_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let oracle = bridge
        .eval_macro(&format!(
            "newImage('test', '8-bit ramp', 16, 16, 1); makeRectangle(2, 3, 10, 8); \
             setThreshold(100, 200); \
             run(\"Set Measurements...\", \"{LIMIT_MEASUREMENTS}\"); run('Measure');"
        ))
        .unwrap();

    let mut doc = ImageDocument::new_ramp_8bit("test", 16, 16).unwrap();
    doc.set_rect_roi(2, 3, 10, 8).unwrap();
    doc.threshold = Some(ThresholdLevels {
        lower: 100.0,
        upper: 200.0,
    });
    let stats = doc.measure_with_config(limit_measurement_config()).unwrap();

    // ByteStatistics restricts the histogram statistics, centroid and moments
    // to the truncated threshold bins (ByteStatistics.java 21-51).
    assert_limit_measurement_columns(&stats, &oracle_columns(&oracle.stdout));
}

#[test]
fn parity_16bit_limit_to_threshold_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let oracle = bridge
        .eval_macro(&format!(
            "newImage('test', '16-bit ramp', 16, 16, 1); makeRectangle(2, 3, 10, 8); \
             setThreshold(20000, 45000); \
             run(\"Set Measurements...\", \"{LIMIT_MEASUREMENTS}\"); run('Measure');"
        ))
        .unwrap();

    let mut doc = ImageDocument::new_ramp_16bit("test", 16, 16).unwrap();
    doc.set_rect_roi(2, 3, 10, 8).unwrap();
    doc.threshold = Some(ThresholdLevels {
        lower: 20000.0,
        upper: 45000.0,
    });
    let stats = doc.measure_with_config(limit_measurement_config()).unwrap();

    assert_limit_measurement_columns(&stats, &oracle_columns(&oracle.stdout));
}

#[test]
fn parity_32bit_limit_to_threshold_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let oracle = bridge
        .eval_macro(&format!(
            "newImage('test', '32-bit ramp', 16, 16, 1); makeRectangle(2, 3, 10, 8); \
             setThreshold(0.25, 0.75); \
             run(\"Set Measurements...\", \"{LIMIT_MEASUREMENTS}\"); run('Measure');"
        ))
        .unwrap();

    let mut doc = ImageDocument::new_ramp_32bit("test", 16, 16).unwrap();
    doc.set_rect_roi(2, 3, 10, 8).unwrap();
    doc.threshold = Some(ThresholdLevels {
        lower: 0.25,
        upper: 0.75,
    });
    let stats = doc.measure_with_config(limit_measurement_config()).unwrap();

    assert_limit_measurement_columns(&stats, &oracle_columns(&oracle.stdout));
}

#[test]
fn parity_limit_flag_without_threshold_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();

    let oracle = bridge
        .eval_macro(&format!(
            "newImage('test', '8-bit ramp', 16, 16, 1); makeRectangle(2, 3, 10, 8); \
             run(\"Set Measurements...\", \"{LIMIT_MEASUREMENTS}\"); run('Measure');"
        ))
        .unwrap();
    let mut doc = ImageDocument::new_ramp_8bit("test", 16, 16).unwrap();
    doc.set_rect_roi(2, 3, 10, 8).unwrap();
    let stats = doc.measure_with_config(limit_measurement_config()).unwrap();
    // ByteStatistics.java 24-26: with no threshold the LIMIT flag leaves the
    // numeric statistics unchanged and reports 0..255 in MinThr/MaxThr.
    assert_eq!(stats.threshold_range, Some(ThresholdLevels { lower: 0.0, upper: 255.0 }));
    let mut ordinary = doc.measure().unwrap();
    ordinary.threshold_range = stats.threshold_range;
    assert_eq!(stats, ordinary);
    assert_limit_measurement_columns(&stats, &oracle_columns(&oracle.stdout));

    let oracle = bridge
        .eval_macro(&format!(
            "newImage('test', '16-bit ramp', 16, 16, 1); makeRectangle(2, 3, 10, 8); \
             run(\"Set Measurements...\", \"{LIMIT_MEASUREMENTS}\"); run('Measure');"
        ))
        .unwrap();
    let mut doc = ImageDocument::new_ramp_16bit("test", 16, 16).unwrap();
    doc.set_rect_roi(2, 3, 10, 8).unwrap();
    let stats = doc.measure_with_config(limit_measurement_config()).unwrap();
    assert_eq!(stats.threshold_range, Some(ThresholdLevels { lower: 0.0, upper: 65535.0 }));
    let mut ordinary = doc.measure().unwrap();
    ordinary.threshold_range = stats.threshold_range;
    assert_eq!(stats, ordinary);
    assert_limit_measurement_columns(&stats, &oracle_columns(&oracle.stdout));

    let oracle = bridge
        .eval_macro(&format!(
            "newImage('test', '32-bit ramp', 16, 16, 1); makeRectangle(2, 3, 10, 8); \
             run(\"Set Measurements...\", \"{LIMIT_MEASUREMENTS}\"); run('Measure');"
        ))
        .unwrap();
    let mut doc = ImageDocument::new_ramp_32bit("test", 16, 16).unwrap();
    doc.set_rect_roi(2, 3, 10, 8).unwrap();
    let stats = doc.measure_with_config(limit_measurement_config()).unwrap();
    assert_eq!(stats.threshold_range, Some(ThresholdLevels {
        lower: -f32::MAX as f64, upper: f32::MAX as f64,
    }));
    let mut ordinary = doc.measure().unwrap();
    ordinary.threshold_range = stats.threshold_range;
    assert_eq!(stats, ordinary);
    assert_limit_measurement_columns(&stats, &oracle_columns(&oracle.stdout));
}

#[test]
fn parity_limit_preserves_full_roi_area_fraction_like_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    // calculateAreaFraction always reads the processor threshold and the full
    // ROI histogram, independently of LIMIT (ImageStatistics.java 276-293).
    let oracle = bridge
        .eval_macro(
            "newImage('test', '8-bit ramp', 16, 16, 1); makeRectangle(2, 3, 10, 8); \
             setThreshold(100, 200); \
             run(\"Set Measurements...\", \"area mean area_fraction limit decimal=3\"); \
             run('Measure');",
        )
        .unwrap();

    let mut doc = ImageDocument::new_ramp_8bit("test", 16, 16).unwrap();
    doc.set_rect_roi(2, 3, 10, 8).unwrap();
    doc.threshold = Some(ThresholdLevels {
        lower: 100.0,
        upper: 200.0,
    });
    let config = MeasurementConfig {
        flags: MeasurementFlags::AREA
            | MeasurementFlags::MEAN
            | MeasurementFlags::AREA_FRACTION
            | MeasurementFlags::LIMIT,
        precision: 3,
    };
    let stats = doc.measure_with_config(config).unwrap();
    let columns = oracle_columns(&oracle.stdout);
    assert_eq!(
        format_ij_num(stats.area_fraction),
        columns["%Area"],
        "full oracle row: {columns:?}"
    );
}

#[test]
fn parity_line_roi_limit_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let oracle = bridge
        .eval_macro(
            "newImage('line-limit','8-bit ramp',16,16,1); makeLine(2,3,12,9); \
             setThreshold(60,180); \
             run('Set Measurements...', 'area mean standard min median limit decimal=3'); \
             run('Measure');",
        )
        .unwrap();
    let mut doc = ImageDocument::new_ramp_8bit("line-limit", 16, 16).unwrap();
    doc.set_line_roi(2, 3, 12, 9);
    doc.threshold = Some(ThresholdLevels { lower: 60.0, upper: 180.0 });
    let stats = doc.measure_with_config(limit_measurement_config()).unwrap();
    let columns = oracle_columns(&oracle.stdout);
    for (name, value) in [
        ("Area", stats.area),
        ("Mean", stats.mean),
        ("StdDev", stats.std_dev),
        ("Min", stats.min),
        ("Max", stats.max),
        ("Median", stats.median),
    ] {
        assert_eq!(format_ij_num(value), columns[name], "{name}");
    }
}

#[test]
fn parity_line_roi_limit_without_threshold_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let oracle = bridge
        .eval_macro(
            "newImage('line-limit','8-bit ramp',16,16,1); makeLine(2,3,12,9); \
             run('Set Measurements...', 'area mean standard min median limit decimal=3'); \
             run('Measure');",
        )
        .unwrap();
    let mut doc = ImageDocument::new_ramp_8bit("line-limit", 16, 16).unwrap();
    doc.set_line_roi(2, 3, 12, 9);
    let stats = doc.measure_with_config(limit_measurement_config()).unwrap();
    let columns = oracle_columns(&oracle.stdout);
    for (name, value) in [
        ("Area", stats.area),
        ("Mean", stats.mean),
        ("StdDev", stats.std_dev),
        ("Min", stats.min),
        ("Max", stats.max),
        ("Median", stats.median),
    ] {
        assert_eq!(format_ij_num(value), columns[name], "{name}");
    }
}

#[test]
fn parity_16bit_line_roi_limit_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let oracle = bridge
        .eval_macro(
            "newImage('line-limit','16-bit ramp',16,16,1); makeLine(2,3,12,9); \
             setThreshold(10000,50000); \
             run('Set Measurements...', 'area mean standard min median limit decimal=3'); \
             run('Measure');",
        )
        .unwrap();
    let mut doc = ImageDocument::new_ramp_16bit("line-limit", 16, 16).unwrap();
    doc.set_line_roi(2, 3, 12, 9);
    doc.threshold = Some(ThresholdLevels { lower: 10000.0, upper: 50000.0 });
    let stats = doc.measure_with_config(limit_measurement_config()).unwrap();
    let columns = oracle_columns(&oracle.stdout);
    for (name, value) in [
        ("Area", stats.area),
        ("Mean", stats.mean),
        ("StdDev", stats.std_dev),
        ("Min", stats.min),
        ("Max", stats.max),
        ("Median", stats.median),
    ] {
        assert_eq!(format_ij_num(value), columns[name], "{name}");
    }
}

#[test]
fn parity_shape_descriptors_and_fitted_ellipse_match_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    for (name, selection) in [
        ("whole", ""),
        ("rectangle", "makeRectangle(2,3,10,8);"),
        ("oval", "makeOval(2,3,10,8);"),
        ("polygon", "makePolygon(2,2,12,2,8,5,12,10,2,10);"),
    ] {
        let oracle = bridge
            .eval_macro(&format!(
                "newImage('shape','8-bit ramp',16,16,1); {selection} \
                 run('Set Measurements...', 'area perimeter centroid fit shape decimal=3'); run('Measure');"
            ))
            .unwrap();
        let mut doc = ImageDocument::new_ramp_8bit("shape", 16, 16).unwrap();
        match name {
            "rectangle" => doc.set_rect_roi(2, 3, 10, 8).unwrap(),
            "oval" => doc.set_oval_roi(2, 3, 10, 8).unwrap(),
            "polygon" => doc
                .set_polygon_roi(&[(2, 2), (12, 2), (8, 5), (12, 10), (2, 10)])
                .unwrap(),
            _ => {}
        }
        let config = MeasurementConfig {
            flags: MeasurementFlags::AREA
                | MeasurementFlags::PERIMETER
                | MeasurementFlags::CENTROID
                | MeasurementFlags::ELLIPSE
                | MeasurementFlags::SHAPE_DESCRIPTORS,
            precision: 3,
        };
        let stats = doc.measure_with_config(config).unwrap();
        let columns = oracle_columns(&oracle.stdout);
        for (heading, value) in [
            ("Area", stats.area),
            ("X", stats.x_centroid),
            ("Y", stats.y_centroid),
            ("Perim.", stats.perimeter),
            ("Major", stats.major),
            ("Minor", stats.minor),
            ("Angle", stats.ellipse_angle),
            ("Circ.", stats.circularity),
            ("AR", stats.aspect_ratio),
            ("Round", stats.roundness),
            ("Solidity", stats.solidity),
        ] {
            let formatted = if matches!(heading, "Circ." | "AR" | "Round" | "Solidity") {
                format!("{value:.3}")
            } else {
                format_ij_num(value)
            };
            assert_eq!(formatted, columns[heading], "{name}: {heading}");
        }
    }
}

#[test]
fn parity_fitted_ellipse_with_threshold_and_anisotropic_pixels_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let oracle = bridge
        .eval_macro(
            "newImage('shape','8-bit ramp',16,16,1); makeRectangle(2,3,10,8); \
             setThreshold(100,200); \
             run('Set Measurements...', 'area fit shape limit decimal=3'); run('Measure');",
        )
        .unwrap();
    let mut doc = ImageDocument::new_ramp_8bit("shape", 16, 16).unwrap();
    doc.set_rect_roi(2, 3, 10, 8).unwrap();
    doc.threshold = Some(ThresholdLevels { lower: 100.0, upper: 200.0 });
    let config = MeasurementConfig {
        flags: MeasurementFlags::AREA | MeasurementFlags::ELLIPSE
            | MeasurementFlags::SHAPE_DESCRIPTORS | MeasurementFlags::LIMIT,
        precision: 3,
    };
    let stats = doc.measure_with_config(config).unwrap();
    let columns = oracle_columns(&oracle.stdout);
    for (heading, value) in [
        ("Major", stats.major), ("Minor", stats.minor),
        ("Angle", stats.ellipse_angle),
        ("Solidity", stats.solidity),
    ] {
        let formatted = if heading == "Solidity" { format!("{value:.3}") } else { format_ij_num(value) };
        assert_eq!(formatted, columns[heading], "{heading}");
    }

    let oracle = bridge
        .eval_macro(
            "newImage('shape','8-bit ramp',16,16,1); setVoxelSize(0.5,0.25,1,'mm'); \
             makeRectangle(2,3,10,8); \
             run('Set Measurements...', 'area fit decimal=3'); run('Measure');",
        )
        .unwrap();
    let mut doc = ImageDocument::new_ramp_8bit("shape", 16, 16).unwrap();
    doc.calibration.pixel_width = 0.5;
    doc.calibration.pixel_height = 0.25;
    doc.set_rect_roi(2, 3, 10, 8).unwrap();
    let stats = doc.measure_with_config(MeasurementConfig {
        flags: MeasurementFlags::AREA | MeasurementFlags::ELLIPSE,
        precision: 3,
    }).unwrap();
    let columns = oracle_columns(&oracle.stdout);
    for (heading, value) in [("Major", stats.major), ("Minor", stats.minor), ("Angle", stats.ellipse_angle)] {
        assert_eq!(format_ij_num(value), columns[heading], "{heading}");
    }
}

fn ramp8_plane(width: u32, height: u32) -> PixelPlane {
    ImageDocument::new_ramp_8bit("plane", width, height)
        .unwrap()
        .slices[0]
        .clone()
}

/// NewImage.java lines 193-201: the RGB ramp repeats one gray column.
fn rgb_ramp(width: u32, height: u32) -> Vec<u32> {
    let mut pixels = vec![0u32; (width * height) as usize];
    for x in 0..width {
        let value = ((x as f64 * 256.0) / width as f64) as u8;
        let pixel = 0xff00_0000 | ((value as u32) << 16) | ((value as u32) << 8) | value as u32;
        for y in 0..height {
            pixels[(y * width + x) as usize] = pixel;
        }
    }
    pixels
}

fn assert_stack_columns(stats: &MeasurementSummary, columns: &BTreeMap<String, String>) {
    let actual = [
        ("Ch", stats.stack_position.channel),
        ("Slice", stats.stack_position.slice),
        ("Frame", stats.stack_position.frame),
    ];
    for (name, value) in actual {
        assert_eq!(
            value.map(|v| v.to_string()),
            columns.get(name).cloned(),
            "column {name} differs (full oracle row: {columns:?})"
        );
    }
}

#[test]
fn parity_plain_stack_slice_column_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let oracle = bridge
        .eval_macro(
            "newImage('test', '8-bit ramp', 8, 8, 5); setSlice(3); \
             run(\"Set Measurements...\", \"area mean stack\"); run('Measure');",
        )
        .unwrap();
    let plane = ramp8_plane(8, 8);
    let mut doc = ImageDocument::new_stack("test", 8, 8, vec![plane; 5]).unwrap();
    doc.set_slice(3).unwrap();
    let stats = doc.measure().unwrap();

    assert_eq!(stats.stack_position.slice, Some(3));
    assert_stack_columns(&stats, &oracle_columns(&oracle.stdout));
}

#[test]
fn parity_hyperstack_channel_slice_frame_columns_match_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let oracle = bridge
        .eval_macro(
            "newImage('test', '8-bit ramp', 8, 8, 12); Stack.setDimensions(2, 3, 2); \
             Stack.setPosition(2, 2, 2); \
             run(\"Set Measurements...\", \"area mean stack\"); run('Measure');",
        )
        .unwrap();
    let plane = ramp8_plane(8, 8);
    let mut doc = ImageDocument::new_stack("test", 8, 8, vec![plane; 12]).unwrap();
    doc.set_dimensions(2, 3, 2).unwrap();
    doc.set_position(2, 2, 2);
    let stats = doc.measure().unwrap();

    assert_eq!(
        stats.stack_position,
        StackPosition {
            channel: Some(2),
            slice: Some(2),
            frame: Some(2),
        }
    );
    assert_stack_columns(&stats, &oracle_columns(&oracle.stdout));
}

#[test]
fn parity_hyperstack_omits_unit_dimension_columns_like_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    // A 2x3x1 hyperstack writes Ch and Slice but no Frame column
    // (Analyzer.java 664-678).
    let oracle = bridge
        .eval_macro(
            "newImage('test', '8-bit ramp', 8, 8, 6); Stack.setDimensions(2, 3, 1); \
             Stack.setPosition(1, 3, 1); \
             run(\"Set Measurements...\", \"area mean stack\"); run('Measure');",
        )
        .unwrap();
    let plane = ramp8_plane(8, 8);
    let mut doc = ImageDocument::new_stack("test", 8, 8, vec![plane; 6]).unwrap();
    doc.set_dimensions(2, 3, 1).unwrap();
    doc.set_position(1, 3, 1);
    let stats = doc.measure().unwrap();

    assert_eq!(
        stats.stack_position,
        StackPosition {
            channel: Some(1),
            slice: Some(3),
            frame: None,
        }
    );
    assert_stack_columns(&stats, &oracle_columns(&oracle.stdout));
}

#[test]
fn parity_channel_stack_without_hyperstack_reports_slice_like_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    // Four channels in one slice is not a hyperstack (ImagePlus.java
    // 1294-1297), so Analyzer.java 679-683 reports the raw slice index.
    let oracle = bridge
        .eval_macro(
            "newImage('test', '8-bit ramp', 8, 8, 4); Stack.setDimensions(4, 1, 1); \
             Stack.setPosition(3, 1, 1); \
             run(\"Set Measurements...\", \"area mean stack\"); run('Measure');",
        )
        .unwrap();
    let plane = ramp8_plane(8, 8);
    let mut doc = ImageDocument::new_stack("test", 8, 8, vec![plane; 4]).unwrap();
    doc.set_dimensions(4, 1, 1).unwrap();
    doc.set_position(3, 1, 1);
    let stats = doc.measure().unwrap();

    assert_eq!(
        stats.stack_position,
        StackPosition {
            channel: None,
            slice: Some(3),
            frame: None,
        }
    );
    assert_stack_columns(&stats, &oracle_columns(&oracle.stdout));
}

#[test]
fn parity_outside_roi_invert_preserves_pixels() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = ImageDocument::new_ramp_8bit("outside", 16, 16).unwrap();
    doc.set_rect_roi(100, 100, 4, 4).unwrap();
    let oracle = bridge
        .run_macro_on_image(&doc, "makeRectangle(100,100,4,4); run(\"Invert\");")
        .unwrap();
    doc.invert();
    assert_eq!(doc.plane(), oracle.output_image.unwrap().plane());
}

#[test]
fn parity_inverted_y_centroid() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let oracle = bridge.eval_macro("newImage('t','8-bit ramp',16,16,1); run('Set Measurements...', 'centroid invert decimal=3'); run('Measure');").unwrap();
    let mut doc = ImageDocument::new_ramp_8bit("t", 16, 16).unwrap();
    doc.calibration.invert_y = true;
    assert_eq!(
        format_ij_num(doc.measure().unwrap().y_centroid),
        oracle_columns(&oracle.stdout)["Y"]
    );
}

#[test]
fn parity_oval_roi_statistics_match_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let oracle = bridge
        .eval_macro(&format!(
            "newImage('test', '8-bit ramp', 16, 16, 1); \
             makeOval(2, 3, 8, 6); \
             run(\"Set Measurements...\", \"{MOMENT_MEASUREMENTS}\"); run('Measure'); \
             getHistogram(values, counts, 256); print(\"HIST\"); Array.print(counts);"
        ))
        .unwrap();

    let mut session = Session::new();
    session
        .dispatch(Command::NewRamp8Bit {
            title: "test".into(),
            width: 16,
            height: 16,
        })
        .unwrap();
    session
        .dispatch(Command::MakeOvalRoi {
            x: 2,
            y: 3,
            width: 8,
            height: 6,
        })
        .unwrap();
    let stats = match session.dispatch(Command::Measure).unwrap() {
        ij_session::CommandOutcome::Measured(stats) => stats,
        other => panic!("expected Measured outcome, got {other:?}"),
    };
    assert_eq!(
        session.macro_recorder.get(1).map(String::as_str),
        Some("makeOval(2, 3, 8, 6);")
    );

    // OvalRoi.getMask() fills 36 of the 48 bounding-box pixels.
    assert_eq!(stats.pixel_count, 36);
    assert_eq!(stats.area, 36.0);
    assert_measurement_columns(&stats, &oracle_columns(&oracle.stdout));
    assert_eq!(stats.histogram, oracle_histogram(&oracle.stdout));
}

#[test]
fn parity_oval_roi_off_image_boundaries_match_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let oracle = bridge
        .eval_macro(&format!(
            "newImage('test', '8-bit ramp', 16, 16, 1); \
             makeOval(-3, -2, 8, 7); \
             run(\"Set Measurements...\", \"{MOMENT_MEASUREMENTS}\"); run('Measure'); \
             getHistogram(values, counts, 256); print(\"HIST\"); Array.print(counts);"
        ))
        .unwrap();

    let mut doc = ImageDocument::new_ramp_8bit("test", 16, 16).unwrap();
    doc.set_oval_roi(-3, -2, 8, 7).unwrap();
    let stats = doc.measure().unwrap();

    // The mask is cropped to the image like ImageProcessor.setRoi(Rectangle).
    assert_eq!(stats.pixel_count, 22);
    assert_measurement_columns(&stats, &oracle_columns(&oracle.stdout));
    assert_eq!(stats.histogram, oracle_histogram(&oracle.stdout));
}

#[test]
fn parity_oval_roi_invert_matches_java_oracle_pixels() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = ImageDocument::new_ramp_8bit("oval_invert.tif", 16, 16).unwrap();
    doc.set_oval_roi(2, 3, 8, 6).unwrap();

    let oracle_res = bridge
        .run_macro_on_image(&doc, "makeOval(2, 3, 8, 6); run(\"Invert\");")
        .unwrap();
    let oracle_doc = oracle_res
        .output_image
        .expect("oracle must return saved TIFF");

    doc.invert();
    assert_eq!(doc.plane(), oracle_doc.plane());
}

#[test]
fn parity_16bit_oval_roi_invert_matches_java_oracle_pixels() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = ImageDocument::new_ramp_16bit("oval_invert16.tif", 16, 16).unwrap();
    doc.set_oval_roi(1, 1, 11, 13).unwrap();

    let oracle_res = bridge
        .run_macro_on_image(&doc, "makeOval(1, 1, 11, 13); run(\"Invert\");")
        .unwrap();
    let oracle_doc = oracle_res
        .output_image
        .expect("oracle must return saved TIFF");

    doc.invert();
    assert_eq!(doc.plane(), oracle_doc.plane());
}

#[test]
fn parity_32bit_oval_roi_statistics_match_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let oracle = bridge
        .eval_macro(&format!(
            "newImage('test', '32-bit ramp', 16, 16, 1); \
             makeOval(2, 3, 8, 6); \
             run(\"Set Measurements...\", \"{MOMENT_MEASUREMENTS}\"); run('Measure'); \
             getHistogram(values, counts, 256); print(\"HIST\"); Array.print(counts);"
        ))
        .unwrap();

    let mut doc = ImageDocument::new_ramp_32bit("test", 16, 16).unwrap();
    doc.set_oval_roi(2, 3, 8, 6).unwrap();
    let stats = doc.measure().unwrap();

    assert_eq!(stats.pixel_count, 36);
    assert_measurement_columns(&stats, &oracle_columns(&oracle.stdout));
    assert_eq!(stats.histogram, oracle_histogram(&oracle.stdout));
}

#[test]
fn parity_polygon_roi_statistics_match_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let oracle = bridge
        .eval_macro(&format!(
            "newImage('test', '8-bit ramp', 16, 16, 1); \
             makePolygon(2, 2, 12, 3, 9, 11, 3, 8); \
             run(\"Set Measurements...\", \"{MOMENT_MEASUREMENTS}\"); run('Measure'); \
             getHistogram(values, counts, 256); print(\"HIST\"); Array.print(counts);"
        ))
        .unwrap();

    let mut doc = ImageDocument::new_ramp_8bit("test", 16, 16).unwrap();
    doc.set_polygon_roi(&[(2, 2), (12, 3), (9, 11), (3, 8)])
        .unwrap();
    let stats = doc.measure().unwrap();

    // PolygonFiller fills 58 pixels of the 10x9 integer bounds.
    assert_eq!(stats.pixel_count, 58);
    assert_eq!(stats.area, 58.0);
    assert_measurement_columns(&stats, &oracle_columns(&oracle.stdout));
    assert_eq!(stats.histogram, oracle_histogram(&oracle.stdout));
}

#[test]
fn parity_polygon_roi_horizontal_edge_statistics_match_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let oracle = bridge
        .eval_macro(&format!(
            "newImage('test', '8-bit ramp', 16, 16, 1); \
             makePolygon(2, 2, 12, 2, 2, 10); \
             run(\"Set Measurements...\", \"{MOMENT_MEASUREMENTS}\"); run('Measure'); \
             getHistogram(values, counts, 256); print(\"HIST\"); Array.print(counts);"
        ))
        .unwrap();

    let mut doc = ImageDocument::new_ramp_8bit("test", 16, 16).unwrap();
    doc.set_polygon_roi(&[(2, 2), (12, 2), (2, 10)]).unwrap();
    let stats = doc.measure().unwrap();

    assert_measurement_columns(&stats, &oracle_columns(&oracle.stdout));
    assert_eq!(stats.histogram, oracle_histogram(&oracle.stdout));
}

#[test]
fn parity_polygon_roi_off_image_boundaries_match_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let oracle = bridge
        .eval_macro(&format!(
            "newImage('test', '8-bit ramp', 16, 16, 1); \
             makePolygon(-2, -1, 8, 2, 5, 9, 1, 6); \
             run(\"Set Measurements...\", \"{MOMENT_MEASUREMENTS}\"); run('Measure'); \
             getHistogram(values, counts, 256); print(\"HIST\"); Array.print(counts);"
        ))
        .unwrap();

    let mut doc = ImageDocument::new_ramp_8bit("test", 16, 16).unwrap();
    doc.set_polygon_roi(&[(-2, -1), (8, 2), (5, 9), (1, 6)])
        .unwrap();
    let stats = doc.measure().unwrap();

    assert_eq!(stats.pixel_count, 45);
    assert_measurement_columns(&stats, &oracle_columns(&oracle.stdout));
    assert_eq!(stats.histogram, oracle_histogram(&oracle.stdout));
}

#[test]
fn parity_polygon_roi_invert_matches_java_oracle_pixels() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = ImageDocument::new_ramp_8bit("polygon_invert.tif", 16, 16).unwrap();
    doc.set_polygon_roi(&[(2, 2), (12, 3), (9, 11), (3, 8)])
        .unwrap();

    let oracle_res = bridge
        .run_macro_on_image(
            &doc,
            "makePolygon(2, 2, 12, 3, 9, 11, 3, 8); run(\"Invert\");",
        )
        .unwrap();
    let oracle_doc = oracle_res
        .output_image
        .expect("oracle must return saved TIFF");

    doc.invert();
    assert_eq!(doc.plane(), oracle_doc.plane());
}

#[test]
fn parity_line_roi_invert_matches_java_oracle_pixels() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = ImageDocument::new_ramp_8bit("line_invert.tif", 16, 16).unwrap();
    doc.set_line_roi(2, 3, 10, 7);

    // Line ROIs are not areas, so Invert resets the ROI and affects every pixel.
    let oracle_res = bridge
        .run_macro_on_image(&doc, "makeLine(2, 3, 10, 7); run(\"Invert\");")
        .unwrap();
    let oracle_doc = oracle_res
        .output_image
        .expect("oracle must return saved TIFF");

    doc.invert();
    assert_eq!(doc.plane(), oracle_doc.plane());
}

fn line_measurement_tsv(doc: &ImageDocument) -> String {
    let mut table = ResultsTable::new();
    table.push(doc.measure().unwrap());
    table.to_tsv_basic()
}

#[test]
fn parity_straight_line_roi_measurement_matches_java_oracle_per_type() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let indexed = PixelPlane::Color256 {
        pixels: (0..256).map(|i| (i % 256) as u8).collect(),
        lut: Box::new(Lut256::default()),
    };
    let rgb = PixelPlane::ColorRgb(
        (0..256)
            .map(|i| 0xff00_0000 | ((i % 256) << 16) | (((i * 3) % 256) << 8) | ((i * 7) % 256))
            .collect(),
    );
    let docs = [
        (
            "8-bit",
            ImageDocument::new_ramp_8bit("line8.tif", 16, 16).unwrap(),
        ),
        (
            "16-bit",
            ImageDocument::new_ramp_16bit("line16.tif", 16, 16).unwrap(),
        ),
        (
            "32-bit",
            ImageDocument::new_ramp_32bit("line32.tif", 16, 16).unwrap(),
        ),
        (
            "8-bit color",
            ImageDocument::new("line_indexed.tif", 16, 16, indexed).unwrap(),
        ),
        (
            "RGB",
            ImageDocument::new("line_rgb.tif", 16, 16, rgb).unwrap(),
        ),
    ];

    for (name, mut doc) in docs {
        doc.set_line_roi(2, 3, 10, 7);
        let oracle = bridge
            .run_macro_on_image(&doc, "makeLine(2, 3, 10, 7); run(\"Measure\");")
            .unwrap();
        assert_eq!(
            line_measurement_tsv(&doc).trim(),
            oracle.stdout.trim(),
            "{name}: straight line measurement differs"
        );
    }
}

#[test]
fn parity_line_roi_interpolation_samples_and_off_image_endpoints() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let cases = [
        ("interpolated", (2, 3, 10, 7)),
        ("endpoint outside", (2, 3, 20, 7)),
        ("wholly outside", (100, 100, 110, 110)),
        ("zero length", (5, 5, 5, 5)),
        ("single pixel axis", (2, 5, 10, 5)),
    ];

    for (name, (x1, y1, x2, y2)) in cases {
        let mut doc = ImageDocument::new_ramp_8bit("line_samples.tif", 16, 16).unwrap();
        doc.set_line_roi(x1, y1, x2, y2);
        let oracle = bridge
            .run_macro_on_image(
                &doc,
                &format!("makeLine({x1}, {y1}, {x2}, {y2}); run(\"Measure\");"),
            )
            .unwrap();
        assert_eq!(
            line_measurement_tsv(&doc).trim(),
            oracle.stdout.trim(),
            "{name}: line profile sampling differs"
        );
    }
}

#[test]
fn parity_reversed_line_roi_reports_opposite_signed_angle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut forward = ImageDocument::new_ramp_8bit("line_forward.tif", 16, 16).unwrap();
    forward.set_line_roi(2, 3, 10, 7);
    let mut reversed = ImageDocument::new_ramp_8bit("line_reversed.tif", 16, 16).unwrap();
    reversed.set_line_roi(10, 7, 2, 3);

    for doc in [&forward, &reversed] {
        let (x1, y1, x2, y2) = match doc.roi.as_ref().unwrap() {
            Roi::Line(line) => (line.x1, line.y1, line.x2, line.y2),
            other => panic!("expected line ROI, got {other:?}"),
        };
        let oracle = bridge
            .run_macro_on_image(
                doc,
                &format!("makeLine({x1}, {y1}, {x2}, {y2}); run(\"Measure\");"),
            )
            .unwrap();
        assert_eq!(
            line_measurement_tsv(doc).trim(),
            oracle.stdout.trim(),
            "reversed line direction differs"
        );
    }

    let forward_stats = forward.measure().unwrap();
    let reversed_stats = reversed.measure().unwrap();
    assert_eq!(forward_stats.area, reversed_stats.area);
    assert_eq!(forward_stats.mean, reversed_stats.mean);
    assert_eq!(
        forward_stats.line.unwrap().length,
        reversed_stats.line.unwrap().length
    );
    assert!(forward_stats.line.unwrap().angle < 0.0);
    assert!(reversed_stats.line.unwrap().angle > 0.0);
}

#[test]
fn parity_calibrated_line_roi_length_angle_and_area() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = ImageDocument::new_ramp_8bit("line_calibrated.tif", 16, 16).unwrap();
    doc.calibration.pixel_width = 0.5;
    doc.calibration.pixel_height = 0.25;
    doc.calibration.unit = "um".to_string();
    doc.set_line_roi(2, 3, 10, 7);

    let oracle = bridge
        .run_macro_on_image(&doc, "makeLine(2, 3, 10, 7); run(\"Measure\");")
        .unwrap();
    assert_eq!(
        line_measurement_tsv(&doc).trim(),
        oracle.stdout.trim(),
        "calibrated line measurement differs"
    );
}

#[test]
fn parity_line_roi_measures_the_current_stack_slice() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let ramp = ImageDocument::new_ramp_8bit("slice2.tif", 16, 16).unwrap();
    let slices = vec![PixelPlane::Gray8(vec![0u8; 256]), ramp.plane().clone()];
    let mut doc = ImageDocument::new_stack("line_stack.tif", 16, 16, slices).unwrap();
    doc.set_slice(2).unwrap();
    doc.set_line_roi(2, 3, 10, 7);

    let oracle = bridge
        .run_macro_on_image(
            &doc,
            "setSlice(2); makeLine(2, 3, 10, 7); run(\"Measure\");",
        )
        .unwrap();
    assert_eq!(
        line_measurement_tsv(&doc).trim(),
        oracle.stdout.trim(),
        "line measurement must sample the current stack slice"
    );
    let stats = doc.measure().unwrap();
    assert_eq!(stats.pixel_count, 10);
    assert_eq!(stats.min, 32.0);
    assert_eq!(stats.max, 160.0);
}

#[test]
fn session_line_measurement_matches_java_oracle_results_table() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let doc = ImageDocument::new_ramp_8bit("session_line.tif", 16, 16).unwrap();
    let oracle = bridge
        .run_macro_on_image(&doc, "makeLine(2, 3, 10, 7); run(\"Measure\");")
        .unwrap();

    let mut session = Session::new();
    session
        .dispatch(Command::NewRamp8Bit {
            title: "session_line.tif".into(),
            width: 16,
            height: 16,
        })
        .unwrap();
    session
        .dispatch(Command::MakeLineRoi {
            x1: 2,
            y1: 3,
            x2: 10,
            y2: 7,
        })
        .unwrap();
    let stats = measured(&mut session);

    assert!(stats.line.is_some());
    assert_eq!(session.results.to_tsv_basic().trim(), oracle.stdout.trim());
}

#[test]
fn line_and_area_rows_share_imagej_results_table_columns() {
    let mut area_doc = ImageDocument::new_ramp_8bit("mixed_area.tif", 16, 16).unwrap();
    area_doc.set_rect_roi(2, 3, 8, 6).unwrap();
    let mut line_doc = ImageDocument::new_ramp_8bit("mixed_line.tif", 16, 16).unwrap();
    line_doc.set_line_roi(2, 3, 10, 7);

    let mut table = ResultsTable::new();
    table.push(area_doc.measure().unwrap());
    table.push(line_doc.measure().unwrap());

    // "Angle" has a predefined ResultsTable index (17) while "Length" is a
    // dynamically allocated column, so Angle precedes Length. The second row
    // makes the Mean/Angle/Length columns fractional, which switches every
    // row in those columns to three decimals (ResultsTable.java 214-217);
    // area rows leave the line-only cells unset, i.e. zero.
    assert_eq!(
        table.to_tsv_basic(),
        " \tArea\tMean\tMin\tMax\tAngle\tLength\n\
         1\t48\t88.000\t32\t144\t0.000\t0.000\n\
         2\t10\t96.000\t32\t160\t-26.565\t8.944\n"
    );
}

#[test]
fn area_only_results_table_keeps_the_existing_four_column_format() {
    let mut doc = ImageDocument::new_ramp_8bit("area_only.tif", 16, 16).unwrap();
    doc.set_rect_roi(2, 3, 8, 6).unwrap();
    let mut table = ResultsTable::new();
    table.push(doc.measure().unwrap());
    assert_eq!(
        table.to_tsv_basic(),
        " \tArea\tMean\tMin\tMax\n1\t48\t88\t32\t144\n"
    );
}

fn measured(session: &mut Session) -> MeasurementSummary {
    match session.dispatch(Command::Measure).unwrap() {
        ij_session::CommandOutcome::Measured(stats) => stats,
        other => panic!("expected Measured outcome, got {other:?}"),
    }
}

#[test]
fn parity_fully_outside_roi_measurement_matches_java_oracle_per_type() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let rgb = PixelPlane::ColorRgb(
        (0..256)
            .map(|i| 0xff00_0000 | ((i % 256) << 16) | ((i % 128) << 8) | (i % 64))
            .collect(),
    );
    let docs = [
        (
            "8-bit",
            ImageDocument::new_ramp_8bit("outside8.tif", 16, 16).unwrap(),
        ),
        (
            "16-bit",
            ImageDocument::new_ramp_16bit("outside16.tif", 16, 16).unwrap(),
        ),
        (
            "32-bit",
            ImageDocument::new_ramp_32bit("outside32.tif", 16, 16).unwrap(),
        ),
        (
            "RGB",
            ImageDocument::new("outside_rgb.tif", 16, 16, rgb).unwrap(),
        ),
    ];

    for (name, mut doc) in docs {
        doc.set_rect_roi(100, 100, 4, 4).unwrap();
        let oracle = bridge
            .run_macro_on_image(
                &doc,
                &format!(
                    "makeRectangle(100, 100, 4, 4); \
                     run(\"Set Measurements...\", \"{MOMENT_MEASUREMENTS}\"); run(\"Measure\");"
                ),
            )
            .unwrap();
        let stats = doc.measure().unwrap();
        assert_eq!(stats.pixel_count, 0, "{name}");
        assert_eq!(stats.area, 0.0, "{name}");
        assert_measurement_columns(&stats, &oracle_columns(&oracle.stdout));
    }
}

#[test]
fn parity_16bit_empty_mask_uses_get_histogram2_min_max() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let points = [(1, 1), (2, 2), (3, 3)];
    let mut doc = ImageDocument::new_ramp_16bit("empty_mask16.tif", 16, 16).unwrap();
    doc.set_polygon_roi(&points).unwrap();

    let oracle = bridge
        .run_macro_on_image(
            &doc,
            &format!(
                "makePolygon(1, 1, 2, 2, 3, 3); \
                 run(\"Set Measurements...\", \"{MOMENT_MEASUREMENTS}\"); run(\"Measure\");"
            ),
        )
        .unwrap();
    let stats = doc.measure().unwrap();

    // getHistogram2 scans the 2x2 bounding box without the mask, so the
    // all-false mask yields min = max(bboxMax, 255) = 8192 and max = 0.
    assert_eq!(stats.pixel_count, 0);
    assert_eq!(stats.min, 8192.0);
    assert_eq!(stats.max, 0.0);
    assert_measurement_columns(&stats, &oracle_columns(&oracle.stdout));
}

#[test]
fn parity_zero_height_rectangle_and_oval_creation_match_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();

    for (name, macro_call, command) in [
        (
            "rectangle",
            "makeRectangle(5, 5, 5, 0);",
            Command::MakeRectRoi {
                x: 5,
                y: 5,
                width: 5,
                height: 0,
            },
        ),
        (
            "oval",
            "makeOval(5, 5, 5, 0);",
            Command::MakeOvalRoi {
                x: 5,
                y: 5,
                width: 5,
                height: 0,
            },
        ),
    ] {
        let oracle = bridge
            .eval_macro(&format!(
                "newImage('t', '8-bit ramp', 16, 16, 1); {macro_call} \
                 run(\"Set Measurements...\", \"{MOMENT_MEASUREMENTS}\"); run('Measure');"
            ))
            .unwrap();

        let mut session = Session::new();
        session
            .dispatch(Command::NewRamp8Bit {
                title: "t".into(),
                width: 16,
                height: 16,
            })
            .unwrap();
        session.dispatch(command).unwrap();

        // Roi clamps a zero height to one pixel (Roi.java lines 151-152).
        assert_eq!(
            session.active_doc().unwrap().roi.as_ref().unwrap().bounds(),
            (5, 5, 5, 1),
            "{name}"
        );
        let stats = measured(&mut session);
        assert_eq!(stats.pixel_count, 5, "{name}");
        assert_measurement_columns(&stats, &oracle_columns(&oracle.stdout));
    }
}

#[test]
fn session_degenerate_roi_creation_clears_stale_roi_and_records_macro() {
    let mut session = Session::new();
    session
        .dispatch(Command::NewRamp8Bit {
            title: "ramp".into(),
            width: 16,
            height: 16,
        })
        .unwrap();
    session
        .dispatch(Command::MakeRectRoi {
            x: 2,
            y: 2,
            width: 4,
            height: 4,
        })
        .unwrap();
    assert!(session.active_doc().unwrap().roi.is_some());

    // width <= 0 or height < 0 deletes the previous selection
    // (IJ.java 1298-1308, 1320-1335).
    for (command, expected_macro) in [
        (
            Command::MakeRectRoi {
                x: 5,
                y: 5,
                width: 0,
                height: 0,
            },
            "makeRectangle(5, 5, 0, 0);",
        ),
        (
            Command::MakeOvalRoi {
                x: 5,
                y: 5,
                width: 5,
                height: -1,
            },
            "makeOval(5, 5, 5, -1);",
        ),
        (
            Command::MakePolygonRoi {
                points: vec![(2, 2), (2, 2), (2, 2)],
            },
            "makePolygon(2, 2, 2, 2, 2, 2);",
        ),
    ] {
        session
            .dispatch(Command::MakeRectRoi {
                x: 2,
                y: 2,
                width: 4,
                height: 4,
            })
            .unwrap();
        assert!(session.active_doc().unwrap().roi.is_some());
        session.dispatch(command).unwrap();
        assert!(
            session.active_doc().unwrap().roi.is_none(),
            "stale ROI survived {expected_macro}"
        );
        assert_eq!(
            session.macro_recorder.last().map(String::as_str),
            Some(expected_macro)
        );
        // The deleted selection measures the whole image.
        let stats = measured(&mut session);
        assert_eq!(stats.area, 256.0, "{expected_macro}");
    }
}

#[test]
fn roi_mask_returns_none_for_line_and_rectangle_even_outside_image() {
    let mut doc = ImageDocument::new_ramp_8bit("mask_contract", 16, 16).unwrap();
    doc.set_line_roi(100, 100, 104, 104);
    assert_eq!(doc.roi_mask().unwrap(), None);
    doc.set_rect_roi(100, 100, 4, 4).unwrap();
    assert_eq!(doc.roi_mask().unwrap(), None);
}

#[test]
fn parity_8bit_empty_oval_mask_measurement() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let oracle = bridge.eval_macro("newImage('t','8-bit ramp',16,16,1); makeOval(15,15,4,4); run('Set Measurements...', 'area mean min'); run('Measure');").unwrap();
    let mut doc = ImageDocument::new_ramp_8bit("t", 16, 16).unwrap();
    doc.set_oval_roi(15, 15, 4, 4).unwrap();
    let stats = doc.measure().unwrap();
    let cols = oracle_columns(&oracle.stdout);
    assert_eq!(format_ij_num(stats.min), cols["Min"]);
    assert_eq!(format_ij_num(stats.max), cols["Max"]);
}

#[test]
fn parity_flat_polygon_remains_empty_selection() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let oracle = bridge.eval_macro("newImage('t','8-bit ramp',16,16,1); makePolygon(2,2,12,2,12,2); run('Set Measurements...', 'area mean min'); run('Measure');").unwrap();
    let mut doc = ImageDocument::new_ramp_8bit("t", 16, 16).unwrap();
    doc.set_polygon_roi(&[(2, 2), (12, 2), (12, 2)]).unwrap();
    let stats = doc.measure().unwrap();
    assert_eq!(
        format_ij_num(stats.area),
        oracle_columns(&oracle.stdout)["Area"]
    );
}

// ---------------------------------------------------------------------------
// Unit 4: multi-slice ImageStack and C/Z/T hyperstack parity.
// ---------------------------------------------------------------------------

fn temp_tiff(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ij_rs_unit4_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

fn oracle_line<'a>(stdout: &'a str, prefix: &str) -> &'a str {
    stdout
        .lines()
        .find(|line| line.starts_with(prefix))
        .unwrap_or_else(|| panic!("missing {prefix} in oracle output:\n{stdout}"))
}

/// Compiles and runs a small Java program against the unmodified ImageJ
/// 1.54u jar. Macro `setPosition`/`getPosition` and macro `setSlice` on a
/// hyperstack hang in the headless `-batch` oracle, so direct `ImagePlus` API
/// calls are the only way to observe those semantics.
fn run_java_harness(class_name: &str, source: &str) -> String {
    let root = repo_root();
    let jar = root.join("tools/oracle/ij-oracle.jar");
    assert!(
        jar.exists(),
        "missing ImageJ oracle jar at {}",
        jar.display()
    );
    let dir = std::env::temp_dir().join(format!(
        "ij_rs_harness_{}_{}",
        std::process::id(),
        class_name
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let source_path = dir.join(format!("{class_name}.java"));
    std::fs::write(&source_path, source).unwrap();

    let javac = std::process::Command::new("javac")
        .arg("-cp")
        .arg(&jar)
        .arg(&source_path)
        .output()
        .expect("javac must be available for the Java harness");
    assert!(
        javac.status.success(),
        "javac failed for {class_name}:\n{}",
        String::from_utf8_lossy(&javac.stderr)
    );

    let run = std::process::Command::new("java")
        .arg("-Dapple.awt.UIElement=true")
        .arg("-cp")
        .arg(format!("{}:{}", dir.display(), jar.display()))
        .arg(class_name)
        .output()
        .expect("java must be available for the Java harness");
    assert!(
        run.status.success(),
        "java harness {class_name} failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

/// Builds a stack whose slice s (zero based) has the same pixels ImageJ gets
/// from setPixel(x, y, (x*17+y*29+s*61)%251).
fn distinct_stack_8bit(width: u32, height: u32, depth: u32) -> ImageDocument {
    let mut slices = Vec::new();
    for s in 0..depth {
        let mut pixels = vec![0u8; (width as usize) * (height as usize)];
        for y in 0..height {
            for x in 0..width {
                pixels[(y * width + x) as usize] = ((x * 17 + y * 29 + s * 61) % 251) as u8;
            }
        }
        slices.push(PixelPlane::Gray8(pixels));
    }
    ImageDocument::new_stack("stack.tif", width, height, slices).unwrap()
}

#[test]
fn parity_multipage_stack_dimensions_and_positions_match_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let path = temp_tiff("dims3.tif");
    let mut doc = distinct_stack_8bit(4, 3, 3);
    save_tiff(&path, &doc).unwrap();

    let oracle = bridge
        .eval_macro(&format!(
            "open(\"{p}\"); getDimensions(w,h,c,z,t); \
             print(\"DIMS:\"+w+\",\"+h+\",\"+c+\",\"+z+\",\"+t); \
             for (i=1;i<=3;i++) {{ setSlice(i); n=getSliceNumber(); \
             print(\"POS:\"+i+\",\"+n); }}",
            p = path.display()
        ))
        .unwrap();

    // A multi-IFD TIFF opens in ImageJ as a (1, N, 1) Z-stack, not a frame stack.
    assert_eq!(doc.get_dimensions(), [4, 3, 1, 3, 1]);
    assert_eq!(oracle_line(&oracle.stdout, "DIMS:"), "DIMS:4,3,1,3,1");
    for i in 1..=3u32 {
        doc.set_slice(i as i32).unwrap();
        assert_eq!(doc.get_slice_number(), i);
        assert_eq!(doc.get_position(), (1, i, 1));
        assert_eq!(
            oracle_line(&oracle.stdout, &format!("POS:{i},")),
            format!("POS:{i},{i}").as_str()
        );
    }
}

#[test]
fn parity_hyperstack_tiff_dimensions_match_java_oracle() {
    // ImageJ 1.54u's setPosition()/getPosition() macro functions hang in this
    // headless UIElement oracle, so C/Z/T -> stack-index ordering is covered by
    // the ij-core unit test. This differential test proves ImageJ reads the
    // C/Z/T dimensions we write in the TIFF ImageDescription.
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let path = temp_tiff("hyper12.tif");
    let mut doc = distinct_stack_8bit(3, 2, 12);
    doc.set_dimensions(2, 3, 2).unwrap();
    save_tiff(&path, &doc).unwrap();

    let oracle = bridge
        .eval_macro(&format!(
            "open(\"{p}\"); getDimensions(w,h,c,z,t); \
             print(\"DIMS:\"+w+\",\"+h+\",\"+c+\",\"+z+\",\"+t); \
             print(\"SLICE:\"+getSliceNumber());",
            p = path.display()
        ))
        .unwrap();

    assert_eq!(doc.get_dimensions(), [3, 2, 2, 3, 2]);
    assert_eq!(oracle_line(&oracle.stdout, "DIMS:"), "DIMS:3,2,2,3,2");
    assert_eq!(oracle_line(&oracle.stdout, "SLICE:"), "SLICE:1");

    // ImagePlus.getStackIndex: channel varies fastest, then Z, then T.
    for t in 1..=2u32 {
        for z in 1..=3u32 {
            for c in 1..=2u32 {
                doc.set_position(c as i32, z as i32, t as i32);
                let index = (t - 1) * 2 * 3 + (z - 1) * 2 + c;
                assert_eq!(doc.get_slice_number(), index);
                assert_eq!(doc.get_position(), (c, z, t));
            }
        }
    }
}

#[test]
fn java_resaved_hyperstack_roundtrip_preserves_slices_and_dimensions() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let in_path = temp_tiff("hyper_in.tif");
    let out_path = temp_tiff("hyper_out.tif");
    let mut doc = distinct_stack_8bit(3, 2, 12);
    doc.set_dimensions(2, 3, 2).unwrap();
    save_tiff(&in_path, &doc).unwrap();

    // ImageJ re-saves a hyperstack as a big-endian multi-IFD TIFF.
    bridge
        .eval_macro(&format!(
            "open(\"{in_p}\"); saveAs(\"Tiff\", \"{out_p}\");",
            in_p = in_path.display(),
            out_p = out_path.display()
        ))
        .unwrap();

    let loaded = load_tiff(&out_path).unwrap();
    assert_eq!(loaded.slices, doc.slices);
    assert_eq!(loaded.get_dimensions(), [3, 2, 2, 3, 2]);
}

#[test]
fn parity_stack_measurement_at_each_slice_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let path = temp_tiff("measure_stack.tif");
    let mut doc = distinct_stack_8bit(5, 4, 3);
    doc.set_rect_roi(1, 1, 3, 2).unwrap();
    save_tiff(&path, &doc).unwrap();

    for slice in 1..=3i32 {
        let oracle = bridge
            .eval_macro(&format!(
                "open(\"{p}\"); \
                 run(\"Set Measurements...\", \"{MOMENT_MEASUREMENTS}\"); \
                 makeRectangle(1,1,3,2); setSlice({slice}); run(\"Measure\");",
                p = path.display()
            ))
            .unwrap();

        doc.set_slice(slice).unwrap();
        let stats = doc.measure().unwrap();
        assert_measurement_columns(&stats, &oracle_columns(&oracle.stdout));
    }
}

#[test]
fn parity_stack_invert_current_slice_matches_java_oracle_pixels() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = distinct_stack_8bit(5, 4, 3);

    // "slice" is the ImageJ option that avoids the Process Stack? dialog.
    let oracle_res = bridge
        .run_macro_on_image(&doc, "setSlice(2); run(\"Invert\", \"slice\");")
        .unwrap();
    let oracle_doc = oracle_res
        .output_image
        .expect("oracle must return saved TIFF");

    doc.set_slice(2).unwrap();
    doc.invert();
    assert_eq!(doc.slices, oracle_doc.slices);
    assert_eq!(doc.get_dimensions(), oracle_doc.get_dimensions());
}

#[test]
fn parity_stack_invert_all_slices_matches_java_oracle_pixels() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = distinct_stack_8bit(5, 4, 3);

    let oracle_res = bridge
        .run_macro_on_image(&doc, "run(\"Invert\", \"stack\");")
        .unwrap();
    let oracle_doc = oracle_res
        .output_image
        .expect("oracle must return saved TIFF");

    doc.invert_stack();
    assert_eq!(doc.slices, oracle_doc.slices);
    assert_eq!(doc.get_dimensions(), oracle_doc.get_dimensions());
}

#[test]
fn java_saved_multipage_stack_loads_in_rust_with_all_slices() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let path = temp_tiff("java_stack.tif");
    let expected = distinct_stack_8bit(5, 4, 3);

    // TiffEncoder writes a multi-IFD stack with an ImageJ ImageDescription.
    bridge
        .eval_macro(&format!(
            "newImage('t','8-bit',5,4,3); \
             for (s=1;s<=3;s++) {{ setSlice(s); \
             for (y=0;y<4;y++) {{ for (x=0;x<5;x++) {{ \
             setPixel(x,y,(x*17+y*29+(s-1)*61)%251); }} }} }} \
             saveAs(\"Tiff\",\"{p}\");",
            p = path.display()
        ))
        .unwrap();

    let loaded = load_tiff(&path).unwrap();
    assert_eq!(loaded.slices, expected.slices);
    assert_eq!(loaded.get_dimensions(), [5, 4, 1, 3, 1]);
}

#[test]
fn session_open_multipage_stack_measure_set_slice_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let path = temp_tiff("session_stack.tif");
    let doc = distinct_stack_8bit(5, 4, 3);
    save_tiff(&path, &doc).unwrap();

    let oracle = bridge
        .eval_macro(&format!(
            "open(\"{p}\"); \
             run(\"Set Measurements...\", \"{MOMENT_MEASUREMENTS}\"); \
             makeRectangle(1,1,3,2); setSlice(3); run(\"Measure\");",
            p = path.display()
        ))
        .unwrap();

    let mut session = Session::new();
    session.dispatch(Command::OpenTiff { path }).unwrap();
    session.dispatch(Command::SetSlice { index: 3 }).unwrap();
    session
        .dispatch(Command::MakeRectRoi {
            x: 1,
            y: 1,
            width: 3,
            height: 2,
        })
        .unwrap();
    let stats = measured(&mut session);

    assert_measurement_columns(&stats, &oracle_columns(&oracle.stdout));
    assert!(
        session
            .macro_recorder
            .iter()
            .any(|line| line == "setSlice(3);"),
        "expected setSlice macro, got {:?}",
        session.macro_recorder
    );
    assert_eq!(session.active_doc().unwrap().get_slice_number(), 3);
}

#[test]
fn session_invert_stack_matches_java_oracle_pixels() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let doc = distinct_stack_8bit(5, 4, 3);
    let path = temp_tiff("session_invert_stack.tif");
    save_tiff(&path, &doc).unwrap();

    let oracle_res = bridge
        .run_macro_on_image(&doc, "run(\"Invert\", \"stack\");")
        .unwrap();
    let oracle_doc = oracle_res
        .output_image
        .expect("oracle must return saved TIFF");

    let mut session = Session::new();
    session.dispatch(Command::OpenTiff { path }).unwrap();
    session.dispatch(Command::InvertStack).unwrap();

    let result = session.active_doc().unwrap();
    assert_eq!(result.slices, oracle_doc.slices);
    assert_eq!(result.get_dimensions(), oracle_doc.get_dimensions());
}

// ---------------------------------------------------------------------------
// Unit 4 review findings: ImagePlus.setSlice vs macro setSlice, setDimensions
// position state, stack Invert recording, and Undo lifetime.
// ---------------------------------------------------------------------------

/// `ImagePlus.setSlice` only updates the hyperstack position when one C/Z/T
/// dimension equals the stack size (ImagePlus.java 1823-1829). Macro
/// `setSlice` instead calls `ImagePlus.setPosition(n)` for any hyperstack
/// (Functions.java 3544-3560). The headless batch macro engine hangs on
/// `setPosition`, so this pins both paths with a direct Java harness.
#[test]
fn parity_hyperstack_set_slice_method_vs_macro_matches_java_harness() {
    let oracle = run_java_harness(
        "HyperSetSliceHarness",
        r#"
import ij.*;
import ij.process.*;

public class HyperSetSliceHarness {
    static void print(ImagePlus imp, String label) {
        System.out.println(label + "=" + imp.getCurrentSlice() + ","
            + imp.getChannel() + "," + imp.getSlice() + "," + imp.getFrame());
    }

    public static void main(String[] args) {
        ImageStack stack = new ImageStack(3, 2);
        for (int i = 0; i < 12; i++)
            stack.addSlice("", new ByteProcessor(3, 2));
        ImagePlus imp = new ImagePlus("t", stack);
        imp.setDimensions(2, 3, 2);
        imp.setPosition(1, 2, 1);
        imp.setSlice(9);
        print(imp, "METHOD");
        imp.setPosition(9);
        print(imp, "MACRO");
    }
}
"#,
    );

    let mut doc = distinct_stack_8bit(3, 2, 12);
    doc.set_dimensions(2, 3, 2).unwrap();
    doc.set_position(1, 2, 1);
    doc.set_slice(9).unwrap();
    assert_eq!(oracle_line(&oracle, "METHOD="), "METHOD=9,1,2,1");
    assert_eq!(doc.get_slice_number(), 9);
    assert_eq!(doc.get_position(), (1, 2, 1));

    doc.set_slice_macro(9).unwrap();
    assert_eq!(oracle_line(&oracle, "MACRO="), "MACRO=9,1,2,2");
    assert_eq!(doc.get_slice_number(), 9);
    assert_eq!(doc.get_position(), (1, 2, 2));
}

/// ImagePlus.setDimensions falls back to (1, stackSize, 1) when C*Z*T does
/// not match the stack (ImagePlus.java 1266-1278) and, without a StackWindow,
/// never touches currentSlice or position (the `updateWin` branch at lines
/// 1281-1288 needs `isDisplayedHyperStack()`).
#[test]
fn parity_set_dimensions_preserves_position_and_current_slice_like_java_harness() {
    let oracle = run_java_harness(
        "SetDimensionsHarness",
        r#"
import ij.*;
import ij.process.*;
import java.util.Arrays;

public class SetDimensionsHarness {
    static void print(ImagePlus imp, String label) {
        System.out.println(label + "=" + Arrays.toString(imp.getDimensions()) + "|"
            + imp.getCurrentSlice() + "|" + imp.getChannel() + ","
            + imp.getSlice() + "," + imp.getFrame());
    }

    public static void main(String[] args) {
        ImageStack stack = new ImageStack(4, 3);
        for (int i = 0; i < 12; i++)
            stack.addSlice("", new ByteProcessor(4, 3));
        ImagePlus imp = new ImagePlus("t", stack);
        // getDimensions() runs verifyDimensions(), which is what happens when
        // ImageJ opens a stack and creates its window.
        imp.getDimensions();
        imp.setSlice(9);
        print(imp, "BEFORE");
        imp.setDimensions(2, 3, 2);
        print(imp, "VALID");
        imp.setDimensions(2, 2, 2);
        print(imp, "MISMATCH");
        imp.setDimensions(0, 0, 0);
        print(imp, "ZERO");
        imp.setDimensions(4, 3, 1);
        print(imp, "FOUR");
    }
}
"#,
    );

    let mut doc = distinct_stack_8bit(4, 3, 12);
    doc.set_slice(9).unwrap();
    assert_eq!(
        oracle_line(&oracle, "BEFORE="),
        "BEFORE=[4, 3, 1, 12, 1]|9|1,9,1"
    );

    doc.set_dimensions(2, 3, 2).unwrap();
    assert_eq!(
        oracle_line(&oracle, "VALID="),
        "VALID=[4, 3, 2, 3, 2]|9|1,9,1"
    );
    assert_eq!(doc.get_dimensions(), [4, 3, 2, 3, 2]);
    assert_eq!(doc.get_slice_number(), 9);
    assert_eq!(doc.get_position(), (1, 9, 1));

    doc.set_dimensions(2, 2, 2).unwrap();
    assert_eq!(
        oracle_line(&oracle, "MISMATCH="),
        "MISMATCH=[4, 3, 1, 12, 1]|9|1,9,1"
    );
    assert_eq!(doc.get_dimensions(), [4, 3, 1, 12, 1]);
    assert_eq!(doc.get_slice_number(), 9);
    assert_eq!(doc.get_position(), (1, 9, 1));

    doc.set_dimensions(0, 0, 0).unwrap();
    assert_eq!(
        oracle_line(&oracle, "ZERO="),
        "ZERO=[4, 3, 1, 12, 1]|9|1,9,1"
    );
    assert_eq!(doc.get_dimensions(), [4, 3, 1, 12, 1]);
    assert_eq!(doc.get_slice_number(), 9);
    assert_eq!(doc.get_position(), (1, 9, 1));

    doc.set_dimensions(4, 3, 1).unwrap();
    assert_eq!(
        oracle_line(&oracle, "FOUR="),
        "FOUR=[4, 3, 4, 3, 1]|9|1,9,1"
    );
    assert_eq!(doc.get_dimensions(), [4, 3, 4, 3, 1]);
    assert_eq!(doc.get_slice_number(), 9);
    assert_eq!(doc.get_position(), (1, 9, 1));
}

/// PlugInFilterRunner calls `Undo.reset()` when processing a complete stack
/// (PlugInFilterRunner.java 118-120), so Undo after `run("Invert", "stack")`
/// leaves every slice inverted.
#[test]
fn parity_stack_invert_clears_undo_like_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = distinct_stack_8bit(5, 4, 3);

    let oracle_res = bridge
        .run_macro_on_image(&doc, "run(\"Invert\", \"stack\"); run(\"Undo\");")
        .unwrap();
    let oracle_doc = oracle_res
        .output_image
        .expect("oracle must return saved TIFF");
    assert_eq!(oracle_doc.slices, invert_all_slices(&doc));

    doc.invert_stack();
    assert!(
        !doc.undo(),
        "stack processing must not leave an Undo snapshot"
    );
    assert_eq!(doc.slices, oracle_doc.slices);
}

/// Inverting one slice installs a processor snapshot, but scrolling to
/// another slice discards it (ImagePlus.setSlice calls
/// `ip.setSnapshotPixels(null)` at ImagePlus.java 1997-1999). Undo after the
/// scroll therefore does nothing.
#[test]
fn parity_slice_invert_undo_after_slice_change_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = distinct_stack_8bit(5, 4, 3);

    let oracle_res = bridge
        .run_macro_on_image(
            &doc,
            "setSlice(2); run(\"Invert\", \"slice\"); setSlice(3); run(\"Undo\");",
        )
        .unwrap();
    let oracle_doc = oracle_res
        .output_image
        .expect("oracle must return saved TIFF");

    doc.set_slice(2).unwrap();
    doc.invert();
    doc.set_slice(3).unwrap();
    assert!(
        !doc.undo(),
        "changing stack slices must discard the filter snapshot"
    );
    assert_eq!(doc.slices, oracle_doc.slices);
}

/// A one-plane image is not "processing a complete stack": the stack option
/// takes the single-image branch, so Undo restores it.
#[test]
fn parity_single_slice_invert_stack_undo_restores_like_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = ImageDocument::new_ramp_8bit("single.tif", 8, 8).unwrap();
    let original = doc.slices.clone();

    let oracle_res = bridge
        .run_macro_on_image(&doc, "run(\"Invert\", \"stack\"); run(\"Undo\");")
        .unwrap();
    let oracle_doc = oracle_res
        .output_image
        .expect("oracle must return saved TIFF");

    doc.invert_stack();
    assert!(doc.undo(), "single-image stack option keeps Undo");
    assert_eq!(doc.slices, oracle_doc.slices);
    assert_eq!(doc.slices, original);
}

// ---------------------------------------------------------------------------
// Unit 4 review: TIFF metadata (display range, spatial calibration, units)
//
// FileSaver.getDescriptionString (original_imagej/ij/io/FileSaver.java
// 714-808) and TiffEncoder.writeIFD (TiffEncoder.java 280-327) encode
// metadata in the ImageDescription and in XResolution/YResolution/
// ResolutionUnit. FileOpener.setCalibration (FileOpener.java 390-483) and
// TiffDecoder (TiffDecoder.java 471-490) read it back. These tests pin the
// behaviour in both directions.
// ---------------------------------------------------------------------------

#[test]
fn rust_saved_8bit_display_range_and_scale_read_by_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = ImageDocument::new_ramp_8bit("meta8.tif", 8, 8).unwrap();
    doc.display_range = DisplayRange {
        min: 10.0,
        max: 200.0,
    };
    doc.calibration.pixel_width = 0.5;
    doc.calibration.pixel_height = 0.25;
    doc.calibration.unit = "um".to_string();
    let path = temp_tiff("rust_meta_8bit.tif");
    save_tiff(&path, &doc).unwrap();

    let oracle = bridge
        .eval_macro(&format!(
            "open(\"{p}\"); getMinAndMax(lo,hi); getPixelSize(u,w,h); \
             print(\"META:\"+lo+\",\"+hi+\",\"+u+\",\"+w+\",\"+h);",
            p = path.display()
        ))
        .unwrap();

    // Calibration.setUnit sanitizes both "um" and "\u00b5m" to "\u00b5m"
    // (Calibration.java 128-137), so Java reports the canonical micron unit.
    assert_eq!(
        oracle_line(&oracle.stdout, "META:"),
        "META:10,200,\u{00b5}m,0.5,0.25"
    );
}

#[test]
fn rust_saved_16bit_and_32bit_display_ranges_read_by_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();

    let mut doc16 = ImageDocument::new_ramp_16bit("meta16.tif", 8, 8).unwrap();
    doc16.display_range = DisplayRange {
        min: 100.0,
        max: 60000.0,
    };
    let path16 = temp_tiff("rust_meta_16bit.tif");
    save_tiff(&path16, &doc16).unwrap();

    let mut doc32 = ImageDocument::new_ramp_32bit("meta32.tif", 8, 8).unwrap();
    doc32.display_range = DisplayRange {
        min: -1.5,
        max: 2.5,
    };
    let path32 = temp_tiff("rust_meta_32bit.tif");
    save_tiff(&path32, &doc32).unwrap();

    let oracle = bridge
        .eval_macro(&format!(
            "open(\"{p16}\"); getMinAndMax(a,b); print(\"R16:\"+a+\",\"+b); \
             open(\"{p32}\"); getMinAndMax(c,d); print(\"R32:\"+c+\",\"+d);",
            p16 = path16.display(),
            p32 = path32.display()
        ))
        .unwrap();

    assert_eq!(oracle_line(&oracle.stdout, "R16:"), "R16:100,60000");
    assert_eq!(oracle_line(&oracle.stdout, "R32:"), "R32:-1.5,2.5");
}

#[test]
fn rust_saved_infinite_float_display_range_survives_java_open() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = ImageDocument::new_ramp_32bit("infinite_range.tif", 8, 8).unwrap();
    doc.display_range = DisplayRange {
        min: 1.0,
        max: f64::INFINITY,
    };
    let path = temp_tiff("rust_infinite_range.tif");
    save_tiff(&path, &doc).unwrap();

    let oracle = bridge
        .eval_macro(&format!(
            "open(\"{}\"); getMinAndMax(lo,hi); print(\"R32:\"+lo+\",\"+hi);",
            path.display()
        ))
        .unwrap();
    assert_eq!(oracle_line(&oracle.stdout, "R32:"), "R32:1,Infinity");
}

#[test]
fn rust_saved_stack_voxel_size_read_by_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = distinct_stack_8bit(4, 3, 3);
    doc.calibration.pixel_width = 0.5;
    doc.calibration.pixel_height = 0.4;
    doc.calibration.pixel_depth = 2.0;
    doc.calibration.unit = "mm".to_string();
    let path = temp_tiff("rust_meta_stack.tif");
    save_tiff(&path, &doc).unwrap();

    let oracle = bridge
        .eval_macro(&format!(
            "open(\"{p}\"); getVoxelSize(w,h,d,u); \
             print(\"VOX:\"+u+\",\"+w+\",\"+h+\",\"+d);",
            p = path.display()
        ))
        .unwrap();

    // FileSaver only writes spacing for stacks (FileSaver.java 763-772), and
    // FileOpener reads it through decodeDescriptionString (605-650).
    assert_eq!(oracle_line(&oracle.stdout, "VOX:"), "VOX:mm,0.5,0.4,2");
}

/// TiffEncoder.writeIFD maps inch to ResolutionUnit 2 and cm to 3
/// (TiffEncoder.java 302-312); FileSaver's `unit=` line must agree so both
/// the tag-only and description readers recover the same unit.
#[test]
fn rust_saved_inch_and_cm_units_read_by_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();

    for (unit, expected) in [("inch", "inches"), ("cm", "cm")] {
        let mut doc = ImageDocument::new_ramp_8bit("unit.tif", 8, 8).unwrap();
        doc.calibration.pixel_width = 0.5;
        doc.calibration.pixel_height = 0.5;
        doc.calibration.unit = unit.to_string();
        let path = temp_tiff(&format!("rust_meta_unit_{unit}.tif"));
        save_tiff(&path, &doc).unwrap();

        let oracle = bridge
            .eval_macro(&format!(
                "open(\"{p}\"); getVoxelSize(w,h,d,u); \
                 print(\"UNIT:\"+u+\",\"+w+\",\"+h);",
                p = path.display()
            ))
            .unwrap();

        assert_eq!(
            oracle_line(&oracle.stdout, "UNIT:"),
            format!("UNIT:{expected},0.5,0.5")
        );
    }
}

#[test]
fn java_saved_display_range_scale_unit_and_inverty_load_in_rust() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let path = temp_tiff("java_meta_8bit.tif");

    bridge
        .eval_macro(&format!(
            "newImage('t','8-bit ramp',8,8,1); setMinAndMax(10,200); \
             setVoxelSize(0.5,0.25,1,'um'); setOption('inverty',true); \
             saveAs('Tiff','{p}');",
            p = path.display()
        ))
        .unwrap();

    let doc = load_tiff(&path).unwrap();
    assert_eq!(
        doc.display_range,
        DisplayRange {
            min: 10.0,
            max: 200.0
        }
    );
    assert_eq!(doc.calibration.pixel_width, 0.5);
    assert_eq!(doc.calibration.pixel_height, 0.25);
    assert_eq!(doc.calibration.pixel_depth, 1.0);
    assert_eq!(doc.calibration.unit, "\u{00b5}m");
    assert!(doc.calibration.invert_y);
}

#[test]
fn java_saved_stack_voxel_size_loads_in_rust() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let path = temp_tiff("java_meta_stack.tif");

    bridge
        .eval_macro(&format!(
            "newImage('t','8-bit',4,3,3); setVoxelSize(0.5,0.4,2,'mm'); \
             saveAs('Tiff','{p}');",
            p = path.display()
        ))
        .unwrap();

    let doc = load_tiff(&path).unwrap();
    assert_eq!(doc.calibration.pixel_width, 0.5);
    assert_eq!(doc.calibration.pixel_height, 0.4);
    assert_eq!(doc.calibration.pixel_depth, 2.0);
    assert_eq!(doc.calibration.unit, "mm");
}

/// FileSaver writes non-ASCII units as Java Properties `\uXXXX` escapes
/// (FileSaver.appendEscapedLine, FileSaver.java 810-823); the Rust reader
/// must decode them so a Java resave does not turn a calibrated image into a
/// raw "pixel" image.
#[test]
fn java_resaved_rust_metadata_roundtrips_through_rust_reader() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let in_path = temp_tiff("meta_roundtrip_in.tif");
    let out_path = temp_tiff("meta_roundtrip_out.tif");

    let mut doc = ImageDocument::new_ramp_8bit("meta_roundtrip.tif", 8, 8).unwrap();
    doc.display_range = DisplayRange {
        min: 12.0,
        max: 210.0,
    };
    doc.calibration.pixel_width = 0.5;
    doc.calibration.pixel_height = 0.25;
    doc.calibration.unit = "um".to_string();
    save_tiff(&in_path, &doc).unwrap();

    bridge
        .eval_macro(&format!(
            "open(\"{in_p}\"); saveAs(\"Tiff\",\"{out_p}\");",
            in_p = in_path.display(),
            out_p = out_path.display()
        ))
        .unwrap();

    let loaded = load_tiff(&out_path).unwrap();
    assert_eq!(loaded.display_range, doc.display_range);
    assert_eq!(loaded.calibration.pixel_width, 0.5);
    assert_eq!(loaded.calibration.pixel_height, 0.25);
    assert_eq!(loaded.calibration.unit, "\u{00b5}m");
}

/// Recorder records the Process Stack? answer as a `slice` or `stack`
/// option (IJ.java 1287-1291), and `run("Invert")` is only complete for a
/// single image.
#[test]
fn session_stack_invert_records_slice_or_stack_options_like_recorder() {
    let doc = distinct_stack_8bit(5, 4, 3);
    let path = temp_tiff("session_invert_recording.tif");
    save_tiff(&path, &doc).unwrap();

    let mut session = Session::new();
    session.dispatch(Command::OpenTiff { path }).unwrap();
    session.dispatch(Command::SetSlice { index: 2 }).unwrap();
    session.dispatch(Command::Invert).unwrap();
    assert_eq!(
        session.macro_recorder.last().map(String::as_str),
        Some("run(\"Invert\", \"slice\");")
    );

    session.dispatch(Command::Undo).unwrap();
    assert_eq!(session.active_doc().unwrap().slices, doc.slices);

    session.dispatch(Command::InvertStack).unwrap();
    assert_eq!(
        session.macro_recorder.last().map(String::as_str),
        Some("run(\"Invert\", \"stack\");")
    );
    let inverted = invert_all_slices(&doc);
    // PlugInFilterRunner resets Undo for complete-stack processing.
    session.dispatch(Command::Undo).unwrap();
    assert_eq!(session.active_doc().unwrap().slices, inverted);
}

#[test]
fn session_single_image_invert_records_optionless_run() {
    let mut session = Session::new();
    session
        .dispatch(Command::NewRamp8Bit {
            title: "single".into(),
            width: 8,
            height: 8,
        })
        .unwrap();
    session.dispatch(Command::Invert).unwrap();
    assert_eq!(
        session.macro_recorder.last().map(String::as_str),
        Some("run(\"Invert\");")
    );
    session.dispatch(Command::InvertStack).unwrap();
    assert_eq!(
        session.macro_recorder.last().map(String::as_str),
        Some("run(\"Invert\");")
    );
}

/// Session SetSlice mirrors the macro function, which calls setPosition for
/// hyperstacks (Functions.java 3550-3556).
#[test]
fn session_hyperstack_set_slice_uses_macro_set_position_semantics() {
    let mut doc = distinct_stack_8bit(3, 2, 12);
    doc.set_dimensions(2, 3, 2).unwrap();
    let path = temp_tiff("session_hyper_set_slice.tif");
    save_tiff(&path, &doc).unwrap();

    let mut session = Session::new();
    session.dispatch(Command::OpenTiff { path }).unwrap();
    session.dispatch(Command::SetSlice { index: 9 }).unwrap();

    let active = session.active_doc().unwrap();
    assert_eq!(active.get_slice_number(), 9);
    assert_eq!(active.get_position(), (1, 2, 2));
    assert_eq!(
        session.macro_recorder.last().map(String::as_str),
        Some("setSlice(9);")
    );
}

fn invert_all_slices(doc: &ImageDocument) -> Vec<PixelPlane> {
    let display_range = doc.display_range;
    let mut inverted = doc.clone();
    for index in 0..inverted.slices.len() {
        let plane = &mut inverted.slices[index];
        match plane {
            PixelPlane::Gray8(v) => v.iter_mut().for_each(|px| *px = 255 - *px),
            PixelPlane::Gray16(v) => {
                let min = *v.iter().min().unwrap() as i32;
                let max = *v.iter().max().unwrap() as i32;
                v.iter_mut()
                    .for_each(|px| *px = (max - (*px as i32 - min)) as u16);
            }
            PixelPlane::Gray32(v) => {
                let min = display_range.min as f32;
                let max = display_range.max as f32;
                v.iter_mut().for_each(|px| *px = max - (*px - min));
            }
            PixelPlane::Color256 { pixels, .. } => pixels.iter_mut().for_each(|px| *px = 255 - *px),
            PixelPlane::ColorRgb(v) => v
                .iter_mut()
                .for_each(|px| *px = 0xff00_0000 | (!*px & 0x00ff_ffff)),
        }
    }
    inverted.slices
}

// ---------------------------------------------------------------------------
// Unit 4 review: TIFF reader robustness against ImageJ 1.54u behaviour.
//
// TiffDecoder.saveImageDescription only uses `images=N` when the file is
// uncompressed (TiffDecoder.java 205-216), OpenIFD accepts compression 0 and 1
// (TiffDecoder.java 500-515), and FileOpener.setCalibration applies
// XResolution/YResolution only when a unit is defined (FileOpener.java
// 390-414). FileOpener calls setStackDisplayRange when the loaded range is
// degenerate (FileOpener.java 300-332). These fixtures exercise each case in
// both the Java oracle and the Rust reader.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum FixtureValue {
    Literal(u32),
    Blob(usize),
}

/// Builds a little-endian TIFF with one IFD. Pixel `data` starts at offset 8;
/// each blob stores a TIFF value that does not fit in the four-byte entry field
/// (ImageDescription, XResolution/YResolution, ImageJ metadata tags).
fn build_tiff_fixture(
    data: &[u8],
    blobs: &[Vec<u8>],
    entries: &[(u16, u16, u32, FixtureValue)],
) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"II");
    out.extend_from_slice(&42u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(data);
    if out.len() % 2 == 1 {
        out.push(0);
    }
    let ifd_offset = out.len() as u32;
    out[4..8].copy_from_slice(&ifd_offset.to_le_bytes());
    let blob_start = out.len() + 2 + entries.len() * 12 + 4;
    let mut blob_offsets = Vec::with_capacity(blobs.len());
    let mut cursor = blob_start;
    for blob in blobs {
        if cursor % 2 == 1 {
            cursor += 1;
        }
        blob_offsets.push(cursor as u32);
        cursor += blob.len();
    }
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    for &(tag, field_type, count, value) in entries {
        out.extend_from_slice(&tag.to_le_bytes());
        out.extend_from_slice(&field_type.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        let value = match value {
            FixtureValue::Literal(value) => value,
            FixtureValue::Blob(index) => blob_offsets[index],
        };
        if field_type == 3 && count == 1 {
            out.extend_from_slice(&(value as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
        } else {
            out.extend_from_slice(&value.to_le_bytes());
        }
    }
    out.extend_from_slice(&0u32.to_le_bytes());
    for (index, blob) in blobs.iter().enumerate() {
        while out.len() < blob_offsets[index] as usize {
            out.push(0);
        }
        out.extend_from_slice(blob);
    }
    out
}

fn base_fixture_entries(
    width: u32,
    height: u32,
    bits: u16,
    samples_per_pixel: u16,
    sample_format: u16,
    photometric: u16,
    compression: u16,
    strip_bytes: u32,
) -> Vec<(u16, u16, u32, FixtureValue)> {
    vec![
        (256, 4, 1, FixtureValue::Literal(width)),
        (257, 4, 1, FixtureValue::Literal(height)),
        (258, 3, 1, FixtureValue::Literal(u32::from(bits))),
        (259, 3, 1, FixtureValue::Literal(u32::from(compression))),
        (262, 3, 1, FixtureValue::Literal(u32::from(photometric))),
        (273, 4, 1, FixtureValue::Literal(8)),
        (
            277,
            3,
            1,
            FixtureValue::Literal(u32::from(samples_per_pixel)),
        ),
        (278, 4, 1, FixtureValue::Literal(height)),
        (279, 4, 1, FixtureValue::Literal(strip_bytes)),
        (339, 3, 1, FixtureValue::Literal(u32::from(sample_format))),
    ]
}

// P3.9a: independent JDK TIFF encoders supply compressed strips. Expected
// pixels always come from ImageJ 1.54u opening the assembled TIFF itself.
fn compressed_tiff_oracle() -> JavaOracleBridge {
    let root = std::env::var_os("IJ_TIFF_ORACLE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(repo_root);
    JavaOracleBridge::locate_from_repo_root(root).unwrap()
}

fn encode_tiff_fixture_strips(dir: &std::path::Path) {
    let source = r#"
import java.awt.image.*;
import java.io.*;
import java.nio.file.*;
import java.util.*;
import javax.imageio.*;
import javax.imageio.stream.*;
import javax.imageio.plugins.tiff.*;
public class CompressedTiffEncoders {
    public static void main(String[] args) throws Exception {
        Path dir = Paths.get("__DIR__");
        try (var paths = Files.list(dir)) {
            for (Path path : paths.filter(p -> p.toString().endsWith(".raw")).toList()) {
                byte[] raw = Files.readAllBytes(path);
                String type = path.getFileName().toString().split("_")[0];
                BufferedImage image = new BufferedImage(raw.length, 1, BufferedImage.TYPE_BYTE_GRAY);
                System.arraycopy(raw, 0, ((DataBufferByte)image.getRaster().getDataBuffer()).getData(), 0, raw.length);
                ImageWriter writer = ImageIO.getImageWritersByFormatName("TIFF").next();
                ImageWriteParam param = writer.getDefaultWriteParam();
                param.setCompressionMode(ImageWriteParam.MODE_EXPLICIT);
                param.setCompressionType(type);
                ByteArrayOutputStream bytes = new ByteArrayOutputStream();
                try (ImageOutputStream out = new MemoryCacheImageOutputStream(bytes)) {
                    writer.setOutput(out);
                    writer.write(null, new IIOImage(image, null, null), param);
                } finally { writer.dispose(); }
                byte[] tiff = bytes.toByteArray();
                ImageReader reader = ImageIO.getImageReadersByFormatName("TIFF").next();
                try (ImageInputStream in = new MemoryCacheImageInputStream(new ByteArrayInputStream(tiff))) {
                    reader.setInput(in);
                    TIFFDirectory tags = TIFFDirectory.createFromMetadata(reader.getImageMetadata(0));
                    TIFFField offsets = tags.getTIFFField(273), counts = tags.getTIFFField(279);
                    if (offsets.getCount()!=1) throw new AssertionError("expected one strip");
                    TIFFField predictor = tags.getTIFFField(317);
                    if (predictor!=null && predictor.getAsInt(0)!=1) throw new AssertionError("unexpected predictor");
                    int start = (int)offsets.getAsLong(0), length = (int)counts.getAsLong(0);
                    Files.write(Paths.get(path.toString()+".encoded"), Arrays.copyOfRange(tiff, start, start+length));
                } finally { reader.dispose(); }
            }
        }
    }
}
"#.replace("__DIR__", &dir.to_string_lossy());
    let suffix: String = dir
        .file_name()
        .unwrap()
        .to_string_lossy()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    let class_name = format!("CompressedTiffEncoders{suffix}");
    run_java_harness(
        &class_name,
        &source.replace("CompressedTiffEncoders", &class_name),
    );
}

struct CompressedTiffFixture {
    width: u32,
    height: u32,
    bits: u16,
    samples: u16,
    compression: u16,
    predictor: u16,
    rows: u32,
    le: bool,
    strips: Vec<Vec<u8>>,
    description: Option<String>,
}

fn compressed_tiff_fixture(fixture: &CompressedTiffFixture) -> Vec<u8> {
    compressed_tiff_fixture_at(fixture, 0)
}

fn compressed_tiff_fixture_at(f: &CompressedTiffFixture, base: u32) -> Vec<u8> {
    let word = |v: u32| {
        if f.le {
            v.to_le_bytes()
        } else {
            v.to_be_bytes()
        }
    };
    let mut out = if f.le { b"II".to_vec() } else { b"MM".to_vec() };
    out.extend_from_slice(&if f.le {
        42u16.to_le_bytes()
    } else {
        42u16.to_be_bytes()
    });
    out.extend_from_slice(&[0; 4]);
    let mut offsets = Vec::new();
    for strip in &f.strips {
        // Gaps ensure strip offsets, rather than physical contiguity, are used.
        out.extend_from_slice(&[0x5a; 6]);
        offsets.push(base + out.len() as u32);
        out.extend_from_slice(strip);
    }
    let mut entries = vec![
        (256, 4, 1, f.width),
        (257, 4, 1, f.height),
        (258, 3, 1, u32::from(f.bits)),
        (259, 3, 1, u32::from(f.compression)),
        (262, 3, 1, if f.samples == 3 { 2 } else { 1 }),
        (273, 4, f.strips.len() as u32, offsets[0]),
        (277, 3, 1, u32::from(f.samples)),
        (278, 4, 1, f.rows),
        (279, 4, f.strips.len() as u32, f.strips[0].len() as u32),
        (317, 3, 1, u32::from(f.predictor)),
        (339, 3, 1, 1),
    ];
    if f.strips.len() > 1 {
        entries[5].3 = base + out.len() as u32;
        for offset in offsets {
            out.extend_from_slice(&word(offset));
        }
        entries[8].3 = base + out.len() as u32;
        for strip in &f.strips {
            out.extend_from_slice(&word(strip.len() as u32));
        }
    }
    if let Some(description) = &f.description {
        entries.push((
            270,
            2,
            description.len() as u32 + 1,
            base + out.len() as u32,
        ));
        out.extend_from_slice(description.as_bytes());
        out.push(0);
    }
    if out.len() % 2 == 1 {
        out.push(0);
    }
    let ifd_offset = base + out.len() as u32;
    out[4..8].copy_from_slice(&word(ifd_offset));
    if f.le {
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    } else {
        out.extend_from_slice(&(entries.len() as u16).to_be_bytes());
    }
    entries.sort_by_key(|e| e.0);
    for (tag, field, count, value) in entries {
        if f.le {
            push_tiff_entry(&mut out, tag, field, count, value);
        } else {
            push_big_endian_tiff_entry(&mut out, tag, field, count, value);
        }
    }
    out.extend_from_slice(&[0; 4]);
    out
}

fn assert_compressed_tiffs_match_oracle(paths: &[PathBuf]) {
    let mut macro_text = String::new();
    for (index, path) in paths.iter().enumerate() {
        macro_text.push_str(&format!(
            "open(\"{}\"); print(\"TIFF_{index}:\"+getWidth()+\",\"+getHeight()+\",\"+bitDepth()+\",\"+nSlices); \
             for(s=1;s<=nSlices;s++) {{ setSlice(s); for(y=0;y<getHeight();y++) for(x=0;x<getWidth();x++) print(\"PX:\"+d2s(getPixel(x,y),0)); }} close();\n",
            path.display()
        ));
    }
    let oracle = compressed_tiff_oracle().eval_macro(&macro_text).unwrap();
    let mut lines = oracle.stdout.lines();
    for (index, path) in paths.iter().enumerate() {
        let mut session = Session::new();
        session
            .dispatch(Command::OpenTiff { path: path.clone() })
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let doc = session.active_doc().unwrap();
        let bits = match doc.slices[0] {
            PixelPlane::Gray16(_) => 16,
            PixelPlane::ColorRgb(_) => 24,
            _ => 8,
        };
        assert_eq!(
            lines.next().unwrap(),
            format!(
                "TIFF_{index}:{},{},{bits},{}",
                doc.width,
                doc.height,
                doc.slices.len()
            ),
            "{}",
            path.display()
        );
        for plane in &doc.slices {
            let values: Vec<u32> = match plane {
                PixelPlane::Gray8(v) => v.iter().map(|&v| u32::from(v)).collect(),
                PixelPlane::Gray16(v) => v.iter().map(|&v| u32::from(v)).collect(),
                PixelPlane::ColorRgb(v) => v.iter().map(|&v| v & 0xffffff).collect(),
                _ => panic!("unexpected fixture pixel type"),
            };
            for (pixel, value) in values.into_iter().enumerate() {
                let java: i64 = lines
                    .next()
                    .unwrap()
                    .strip_prefix("PX:")
                    .unwrap()
                    .parse()
                    .unwrap();
                let java = if bits == 24 {
                    java as u32 & 0xffffff
                } else {
                    java as u32
                };
                assert_eq!(value, java, "{} pixel {pixel}", path.display());
            }
        }
    }
    assert!(lines.next().is_none());
}

#[test]
fn parity_compressed_tiff_codecs_predictor_endianness_and_strips() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixtures = Vec::new();
    for (compression, codec) in [
        (32773, "PackBits"),
        (8, "ZLib"),
        (32946, "Deflate"),
        (5, "LZW"),
    ] {
        for (bits, samples) in [(8, 1), (16, 1), (8, 3)] {
            for le in [true, false] {
                for predictor in [1, 2] {
                    for (width, rows) in [(7usize, 2), (7, 5), (1, 2)] {
                        let height = 5usize;
                        let stride = width * samples * (bits / 8);
                        let mut raw = Vec::new();
                        for y in 0..height {
                            for x in 0..width {
                                for c in 0..samples {
                                    let value = (x * 193
                                        + y * 71
                                        + c * 103
                                        + if x % 3 == 0 { 65500 } else { 0 })
                                        as u16;
                                    if bits == 16 {
                                        raw.extend_from_slice(&if le {
                                            value.to_le_bytes()
                                        } else {
                                            value.to_be_bytes()
                                        });
                                    } else {
                                        raw.push(value as u8);
                                    }
                                }
                            }
                        }
                        if predictor == 2 && compression != 32773 {
                            for row in raw.chunks_exact_mut(stride) {
                                if bits == 16 {
                                    for x in (1..width).rev() {
                                        let read = |i| {
                                            if le {
                                                u16::from_le_bytes([row[i], row[i + 1]])
                                            } else {
                                                u16::from_be_bytes([row[i], row[i + 1]])
                                            }
                                        };
                                        let value = read(x * 2).wrapping_sub(read((x - 1) * 2));
                                        row[x * 2..x * 2 + 2].copy_from_slice(&if le {
                                            value.to_le_bytes()
                                        } else {
                                            value.to_be_bytes()
                                        });
                                    }
                                } else {
                                    for i in (samples..stride).rev() {
                                        row[i] = row[i].wrapping_sub(row[i - samples]);
                                    }
                                }
                            }
                        }
                        let id = fixtures.len();
                        let mut strips = Vec::new();
                        for (s, strip) in raw.chunks(stride * rows).enumerate() {
                            let path = dir.path().join(format!("{codec}_{id}_{s}.raw"));
                            std::fs::write(&path, strip).unwrap();
                            strips.push(path);
                        }
                        fixtures.push((format!("compressed_{id}_{codec}_{bits}_{samples}_{le}_{predictor}_{rows}.tif"),
                            CompressedTiffFixture { width: width as u32, height: height as u32, bits: bits as u16, samples: samples as u16,
                                compression, predictor, rows: rows as u32, le, strips: Vec::new(), description: None }, strips));
                    }
                }
            }
        }
    }
    encode_tiff_fixture_strips(dir.path());
    let mut paths = Vec::new();
    for (name, mut fixture, strips) in fixtures {
        fixture.strips = strips
            .iter()
            .map(|p| std::fs::read(format!("{}.encoded", p.display())).unwrap())
            .collect();
        paths.push(write_fixture(&name, &compressed_tiff_fixture(&fixture)));
    }
    assert_compressed_tiffs_match_oracle(&paths);
}

#[test]
fn parity_compressed_tiff_high_expansion_and_lzw_dictionary() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixtures = Vec::new();
    for codec in ["PackBits", "ZLib", "Deflate", "LZW"] {
        for repeated in [true, false] {
            let raw: Vec<u8> = (0..32768u32)
                .map(|i| {
                    if repeated {
                        71
                    } else {
                        ((i.wrapping_mul(1664525)
                            .wrapping_add(i.wrapping_mul(i).wrapping_mul(17)))
                            >> 16) as u8
                    }
                })
                .collect();
            let path = dir.path().join(format!("{codec}_{repeated}.raw"));
            std::fs::write(&path, &raw).unwrap();
            fixtures.push((codec, repeated, path));
        }
    }
    encode_tiff_fixture_strips(dir.path());
    let mut paths = Vec::new();
    for (codec, repeated, path) in fixtures {
        let compression = match codec {
            "PackBits" => 32773,
            "ZLib" => 8,
            "Deflate" => 32946,
            _ => 5,
        };
        let strip = std::fs::read(format!("{}.encoded", path.display())).unwrap();
        if repeated {
            assert!(strip.len() < 32768 / 8);
        }
        paths.push(write_fixture(
            &format!("compressed_expansion_{codec}_{repeated}.tif"),
            &compressed_tiff_fixture(&CompressedTiffFixture {
                width: 256,
                height: 128,
                bits: 8,
                samples: 1,
                compression,
                predictor: 1,
                rows: 128,
                le: true,
                strips: vec![strip],
                description: None,
            }),
        ));
    }
    assert_compressed_tiffs_match_oracle(&paths);
}

#[test]
fn parity_compressed_tiff_stacks_and_imagej_description() {
    let dir = tempfile::tempdir().unwrap();
    let mut fixtures = Vec::new();
    for (compression, codec) in [
        (32773, "PackBits"),
        (8, "ZLib"),
        (32946, "Deflate"),
        (5, "LZW"),
    ] {
        for (bits, samples) in [(8, 1), (16, 1), (8, 3)] {
            let mut inputs = Vec::new();
            for slice in 0..2 {
                let raw: Vec<u8> = (0..7 * 5 * samples * (bits / 8))
                    .map(|i| ((i * 17 + slice * 31) % 256) as u8)
                    .collect();
                let path = dir
                    .path()
                    .join(format!("{codec}_{bits}_{samples}_{slice}.raw"));
                std::fs::write(&path, &raw).unwrap();
                inputs.push(path);
            }
            fixtures.push((compression, codec, bits, samples, inputs));
        }
    }
    encode_tiff_fixture_strips(dir.path());
    let mut paths = Vec::new();
    for (compression, codec, bits, samples, inputs) in fixtures {
        let mut f = CompressedTiffFixture {
            width: 7,
            height: 5,
            bits,
            samples,
            compression,
            predictor: 1,
            rows: 5,
            le: true,
            strips: vec![std::fs::read(format!("{}.encoded", inputs[0].display())).unwrap()],
            // TiffDecoder.saveImageDescription ignores images= for compression.
            description: Some("ImageJ=1.54u\nimages=99\nunit=mm\nmin=3\nmax=200\n".into()),
        };
        let mut bytes = compressed_tiff_fixture(&f);
        paths.push(write_fixture(
            &format!("compressed_desc_{codec}_{bits}_{samples}.tif"),
            &bytes,
        ));
        f.strips = vec![std::fs::read(format!("{}.encoded", inputs[1].display())).unwrap()];
        f.description = None;
        let second = compressed_tiff_fixture_at(&f, bytes.len() as u32);
        let next_position = bytes.len() - 4;
        bytes[next_position..].copy_from_slice(&second[4..8]);
        bytes.extend_from_slice(&second);
        paths.push(write_fixture(
            &format!("compressed_stack_{codec}_{bits}_{samples}.tif"),
            &bytes,
        ));
    }
    assert_compressed_tiffs_match_oracle(&paths);
}

#[test]
fn parity_compressed_tiff_packbits_controls_and_lzw_clear_codes() {
    let mut packbits = vec![128, 127]; // -128 is a no-op; 127 copies 128 literals.
    packbits.extend(0..128);
    packbits.extend_from_slice(&[129, 91, 0, 7]); // -127 repeats 128 bytes; 0 copies one.
    let mut paths = vec![write_fixture(
        "compressed_packbits_controls.tif",
        &compressed_tiff_fixture(&CompressedTiffFixture {
            width: 257,
            height: 1,
            bits: 8,
            samples: 1,
            compression: 32773,
            predictor: 1,
            rows: 1,
            le: true,
            strips: vec![packbits],
            description: None,
        }),
    )];
    for (name, width, codes) in [
        ("clear", 4, vec![256u16, 65, 66, 256, 67, 68, 257]),
        ("next_symbol", 3, vec![256u16, 65, 258, 257]),
        ("no_eoi", 3, vec![256u16, 65, 258]),
    ] {
        let mut encoded = vec![0; (codes.len() * 9).div_ceil(8)];
        for (i, code) in codes.iter().enumerate() {
            for bit in 0..9 {
                encoded[(i * 9 + bit) / 8] |=
                    (((code >> (8 - bit)) & 1) as u8) << (7 - (i * 9 + bit) % 8);
            }
        }
        paths.push(write_fixture(
            &format!("compressed_lzw_{name}.tif"),
            &compressed_tiff_fixture(&CompressedTiffFixture {
                width,
                height: 1,
                bits: 8,
                samples: 1,
                compression: 5,
                predictor: 1,
                rows: 1,
                le: true,
                strips: vec![encoded],
                description: None,
            }),
        ));
    }
    assert_compressed_tiffs_match_oracle(&paths);
}

fn description_blob(description: &str) -> Vec<u8> {
    let mut bytes = description.as_bytes().to_vec();
    bytes.push(0);
    bytes
}

fn write_fixture(name: &str, bytes: &[u8]) -> PathBuf {
    let path = temp_tiff(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

fn push_tiff_entry(out: &mut Vec<u8>, tag: u16, field_type: u16, count: u32, value: u32) {
    out.extend_from_slice(&tag.to_le_bytes());
    out.extend_from_slice(&field_type.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    if field_type == 3 && count == 1 {
        out.extend_from_slice(&(value as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
    } else {
        out.extend_from_slice(&value.to_le_bytes());
    }
}

/// Builds the 768-entry ColorMap payload for `seed`. Channel 0 is
/// `i + seed`, channel 1 is `255 - i` and channel 2 is `seed`, so two seeds
/// always differ at index 1 while every channel sum stays non-zero.
fn fixture_color_map(seed: u8) -> Vec<u8> {
    let mut map = Vec::with_capacity(768 * 2);
    for channel in 0..3 {
        for i in 0..256u32 {
            let value = match channel {
                0 => (i as u8).wrapping_add(seed),
                1 => (255 - i) as u8,
                _ => seed,
            };
            // TIFF palette entries are 16-bit; ImageJ keeps the high byte.
            map.extend_from_slice(&(u16::from(value) * 257).to_le_bytes());
        }
    }
    map
}

/// Palette whose signed-byte channel sum is zero (red entries 1 and 255), so
/// TiffDecoder leaves the 8-bit FileInfo at GRAY8 while still installing the
/// palette as the processor's display LUT.
fn net_zero_color_map() -> Vec<u8> {
    let mut map = vec![0u8; 768 * 2];
    map[0..2].copy_from_slice(&(u16::from(1u8) * 257).to_le_bytes());
    map[2..4].copy_from_slice(&(u16::from(255u8) * 257).to_le_bytes());
    map
}

/// Builds a little-endian single-IFD grayscale TIFF that carries a 768-entry
/// ColorMap tag, so Java's getColorMap runs with PhotometricInterpretation 0/1.
#[allow(clippy::too_many_arguments)]
fn build_gray_photometric_color_map_tiff(
    width: u32,
    height: u32,
    bits: u16,
    sample_format: u16,
    photometric: u16,
    data: &[u8],
    color_map: &[u8],
    description: Option<&str>,
) -> Vec<u8> {
    let mut blobs = vec![color_map.to_vec()];
    let description_index = description.map(|text| {
        blobs.push(description_blob(text));
        blobs.len() - 1
    });
    let mut entries = base_fixture_entries(
        width,
        height,
        bits,
        1,
        sample_format,
        photometric,
        1,
        data.len() as u32,
    );
    if let Some(index) = description_index {
        entries.push((270, 2, blobs[index].len() as u32, FixtureValue::Blob(index)));
    }
    entries.push((320, 3, 768, FixtureValue::Blob(0)));
    build_tiff_fixture(data, &blobs, &entries)
}

/// Builds a little-endian palette TIFF with one IFD per slice. Pixel data is
/// contiguous from offset 8 and each IFD carries its own ColorMap tag, so
/// ImageJ's `allSameSizeAndType` treats the file as one stack.
fn build_multi_ifd_color256_tiff(width: u32, height: u32, palette_seeds: &[u8]) -> Vec<u8> {
    let plane_bytes = (width * height) as usize;
    assert!(!palette_seeds.is_empty());
    let mut out = vec![0u8; 8];
    out[..2].copy_from_slice(b"II");
    out[2..4].copy_from_slice(&42u16.to_le_bytes());

    let mut strip_offsets = Vec::with_capacity(palette_seeds.len());
    for seed in palette_seeds {
        strip_offsets.push(out.len() as u32);
        for i in 0..plane_bytes {
            out.push((i as u8).wrapping_mul(7).wrapping_add(*seed));
        }
    }
    if out.len() % 2 == 1 {
        out.push(0);
    }

    let n = palette_seeds.len();
    let num_entries = 11usize;
    let ifd_start = out.len();
    let ifd_size = 2 + num_entries * 12 + 4;
    let mut map_offsets = Vec::with_capacity(n);
    let mut cursor = ifd_start + ifd_size * n;
    for _ in 0..n {
        if cursor % 2 == 1 {
            cursor += 1;
        }
        map_offsets.push(cursor);
        cursor += 768 * 2;
    }
    out[4..8].copy_from_slice(&(ifd_start as u32).to_le_bytes());

    for (index, (offset, map_offset)) in strip_offsets.iter().zip(&map_offsets).enumerate() {
        let next_ifd = if index + 1 < n {
            ifd_start + (index + 1) * ifd_size
        } else {
            0
        };
        out.extend_from_slice(&(num_entries as u16).to_le_bytes());
        push_tiff_entry(&mut out, 256, 4, 1, width);
        push_tiff_entry(&mut out, 257, 4, 1, height);
        push_tiff_entry(&mut out, 258, 3, 1, 8);
        push_tiff_entry(&mut out, 259, 3, 1, 1);
        push_tiff_entry(&mut out, 262, 3, 1, 3);
        push_tiff_entry(&mut out, 273, 4, 1, *offset);
        push_tiff_entry(&mut out, 277, 3, 1, 1);
        push_tiff_entry(&mut out, 278, 4, 1, height);
        push_tiff_entry(&mut out, 279, 4, 1, plane_bytes as u32);
        push_tiff_entry(&mut out, 320, 3, 768, *map_offset as u32);
        push_tiff_entry(&mut out, 339, 3, 1, 1);
        out.extend_from_slice(&(next_ifd as u32).to_le_bytes());
    }
    assert_eq!(out.len(), ifd_start + ifd_size * n);

    for (seed, map_offset) in palette_seeds.iter().zip(&map_offsets) {
        while out.len() < *map_offset {
            out.push(0);
        }
        out.extend_from_slice(&fixture_color_map(*seed));
    }
    out
}

/// Builds a single-IFD 8-bit grayscale TIFF whose X/YResolution rationals give
/// `pixels_per_cm` pixels per centimetre and whose ResolutionUnit is `cm`.
fn build_cm_tiff(
    width: u32,
    height: u32,
    pixels_per_cm: u32,
    description: Option<&str>,
) -> Vec<u8> {
    let data: Vec<u8> = (0..(width * height) as u8).collect();
    let mut blobs: Vec<Vec<u8>> = Vec::new();
    let description_index = description.map(|text| {
        blobs.push(description_blob(text));
        blobs.len() - 1
    });
    let mut rational = Vec::with_capacity(8);
    rational.extend_from_slice(&pixels_per_cm.to_le_bytes());
    rational.extend_from_slice(&1u32.to_le_bytes());
    let x_index = blobs.len();
    blobs.push(rational.clone());
    let y_index = blobs.len();
    blobs.push(rational);

    let mut entries: Vec<(u16, u16, u32, FixtureValue)> = vec![
        (256, 4, 1, FixtureValue::Literal(width)),
        (257, 4, 1, FixtureValue::Literal(height)),
        (258, 3, 1, FixtureValue::Literal(8)),
        (259, 3, 1, FixtureValue::Literal(1)),
        (262, 3, 1, FixtureValue::Literal(1)),
    ];
    if let Some(index) = description_index {
        entries.push((270, 2, blobs[index].len() as u32, FixtureValue::Blob(index)));
    }
    entries.extend_from_slice(&[
        (273, 4, 1, FixtureValue::Literal(8)),
        (277, 3, 1, FixtureValue::Literal(1)),
        (278, 4, 1, FixtureValue::Literal(height)),
        (279, 4, 1, FixtureValue::Literal(data.len() as u32)),
        (282, 5, 1, FixtureValue::Blob(x_index)),
        (283, 5, 1, FixtureValue::Blob(y_index)),
        (296, 3, 1, FixtureValue::Literal(3)),
        (339, 3, 1, FixtureValue::Literal(1)),
    ]);
    build_tiff_fixture(&data, &blobs, &entries)
}

fn assert_close(actual: f64, expected: f64, what: &str) {
    let tolerance = 1e-9 * expected.abs().max(1.0);
    assert!(
        (actual - expected).abs() <= tolerance,
        "{what}: Rust {actual} differs from Java {expected}"
    );
}

#[test]
fn rust_reads_single_ifd_images3_contiguous_planes_like_java() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let data = [1u8, 2, 3, 4, 5, 6];
    let blob = description_blob("ImageJ=1.54u\nimages=3\n");
    let mut entries = base_fixture_entries(2, 1, 8, 1, 1, 1, 1, data.len() as u32);
    entries.push((270, 2, blob.len() as u32, FixtureValue::Blob(0)));
    let path = write_fixture(
        "single_ifd_images3.tif",
        &build_tiff_fixture(&data, &[blob], &entries),
    );

    let oracle = bridge
        .eval_macro(&format!(
            "open(\"{p}\"); getDimensions(w,h,c,z,t); \
             print(\"DIMS:\"+w+\",\"+h+\",\"+c+\",\"+z+\",\"+t); \
             for (i=1;i<=3;i++) {{ setSlice(i); \
             print(\"PX:\"+i+\",\"+getPixel(0,0)+\",\"+getPixel(1,0)); }}",
            p = path.display()
        ))
        .unwrap();

    // A single IFD whose ImageDescription says images=3 stores three
    // contiguous planes after the first strip offset; ImageJ reads all three.
    let doc = load_tiff(&path).unwrap();
    assert_eq!(oracle_line(&oracle.stdout, "DIMS:"), "DIMS:2,1,1,3,1");
    assert_eq!(doc.get_dimensions(), [2, 1, 1, 3, 1]);
    assert_eq!(
        doc.slices,
        vec![
            PixelPlane::Gray8(vec![1, 2]),
            PixelPlane::Gray8(vec![3, 4]),
            PixelPlane::Gray8(vec![5, 6]),
        ]
    );
    for (index, expected) in [(1, "PX:1,1,2"), (2, "PX:2,3,4"), (3, "PX:3,5,6")] {
        assert_eq!(
            oracle_line(&oracle.stdout, &format!("PX:{index},")),
            expected
        );
    }
}

#[test]
fn resolution_tags_without_a_unit_do_not_calibrate_like_java() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let data = [1u8, 2, 3, 4];
    let x_rational = [2u32.to_le_bytes(), 1u32.to_le_bytes()].concat();
    let y_rational = [4u32.to_le_bytes(), 1u32.to_le_bytes()].concat();
    let mut entries = base_fixture_entries(2, 2, 8, 1, 1, 1, 1, data.len() as u32);
    entries.push((282, 5, 1, FixtureValue::Blob(0)));
    entries.push((283, 5, 1, FixtureValue::Blob(1)));
    let path = write_fixture(
        "xres_without_unit.tif",
        &build_tiff_fixture(&data, &[x_rational.clone(), y_rational.clone()], &entries),
    );

    let oracle = bridge
        .eval_macro(&format!(
            "open(\"{p}\"); getVoxelSize(w,h,d,u); \
             print(\"VOX:\"+u+\",\"+w+\",\"+h+\",\"+d);",
            p = path.display()
        ))
        .unwrap();
    assert_eq!(oracle_line(&oracle.stdout, "VOX:"), "VOX:pixels,1,1,1");

    let doc = load_tiff(&path).unwrap();
    assert_eq!(doc.calibration.unit, "pixel");
    assert_eq!(doc.calibration.pixel_width, 1.0);
    assert_eq!(doc.calibration.pixel_height, 1.0);

    // ResolutionUnit=1 does define a unit (" "), so the same resolutions are
    // applied and the unit stays the TIFF "no absolute unit" marker.
    let mut with_unit = entries.clone();
    with_unit.push((296, 3, 1, FixtureValue::Literal(1)));
    let unit_path = write_fixture(
        "xres_unit_none.tif",
        &build_tiff_fixture(&data, &[x_rational, y_rational], &with_unit),
    );
    let oracle_unit = bridge
        .eval_macro(&format!(
            "open(\"{p}\"); getVoxelSize(w,h,d,u); \
             print(\"VOX:\"+u+\",\"+w+\",\"+h+\",\"+d);",
            p = unit_path.display()
        ))
        .unwrap();
    assert_eq!(oracle_line(&oracle_unit.stdout, "VOX:"), "VOX: ,0.5,0.25,1");
    let doc = load_tiff(&unit_path).unwrap();
    assert_eq!(doc.calibration.unit, " ");
    assert_eq!(doc.calibration.pixel_width, 0.5);
    assert_eq!(doc.calibration.pixel_height, 0.25);
}

#[test]
fn multiframe_equal_min_max_uses_the_stack_range_like_java() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();

    let data8 = [1u8, 2, 3, 4];
    let blob8 = description_blob("ImageJ=1.54u\nimages=2\nmin=5.0\nmax=5.0\n");
    let mut entries8 = base_fixture_entries(2, 1, 8, 1, 1, 1, 1, data8.len() as u32);
    entries8.push((270, 2, blob8.len() as u32, FixtureValue::Blob(0)));
    let path8 = write_fixture(
        "equal_range_8bit.tif",
        &build_tiff_fixture(&data8, &[blob8], &entries8),
    );

    let data16 = [
        10u16.to_le_bytes(),
        20u16.to_le_bytes(),
        100u16.to_le_bytes(),
        200u16.to_le_bytes(),
    ]
    .concat();
    let blob16 = description_blob("ImageJ=1.54u\nimages=2\nmin=5.0\nmax=5.0\n");
    let mut entries16 = base_fixture_entries(2, 1, 16, 1, 1, 1, 1, data16.len() as u32);
    entries16.push((270, 2, blob16.len() as u32, FixtureValue::Blob(0)));
    let path16 = write_fixture(
        "equal_range_16bit.tif",
        &build_tiff_fixture(&data16, &[blob16], &entries16),
    );

    let oracle = bridge
        .eval_macro(&format!(
            "open(\"{p8}\"); getMinAndMax(a,b); print(\"R8:\"+a+\",\"+b); \
             open(\"{p16}\"); getMinAndMax(c,d); print(\"R16:\"+c+\",\"+d);",
            p8 = path8.display(),
            p16 = path16.display()
        ))
        .unwrap();
    assert_eq!(oracle_line(&oracle.stdout, "R8:"), "R8:0,255");
    assert_eq!(oracle_line(&oracle.stdout, "R16:"), "R16:10,200");

    // FileOpener's equal-min/max check runs on stacks and resets every slice,
    // so an 8-bit frame stack falls back to 0..255 and a 16-bit stack spans
    // the data in all slices.
    let doc8 = load_tiff(&path8).unwrap();
    assert_eq!(doc8.get_dimensions(), [2, 1, 1, 2, 1]);
    assert_eq!(
        doc8.display_range,
        DisplayRange {
            min: 0.0,
            max: 255.0
        }
    );
    let doc16 = load_tiff(&path16).unwrap();
    assert_eq!(doc16.get_dimensions(), [2, 1, 1, 2, 1]);
    assert_eq!(
        doc16.display_range,
        DisplayRange {
            min: 10.0,
            max: 200.0
        }
    );
}

#[test]
fn compression_zero_is_read_as_uncompressed_like_java() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let data = [7u8, 8, 9, 10];
    let entries = base_fixture_entries(2, 2, 8, 1, 1, 1, 0, data.len() as u32);
    let path = write_fixture(
        "compression_zero.tif",
        &build_tiff_fixture(&data, &[], &entries),
    );

    let oracle = bridge
        .eval_macro(&format!(
            "open(\"{p}\"); getDimensions(w,h,c,z,t); getPixel(1,1); \
             print(\"DIMS:\"+w+\",\"+h+\",\"+c+\",\"+z+\",\"+t);",
            p = path.display()
        ))
        .unwrap();
    assert_eq!(oracle_line(&oracle.stdout, "DIMS:"), "DIMS:2,2,1,1,1");

    // TiffDecoder treats compression 0 exactly like 1 (uncompressed).
    let doc = load_tiff(&path).unwrap();
    assert_eq!(doc.slices, vec![PixelPlane::Gray8(data.to_vec())]);
}

#[test]
fn multi_ifd_color256_stack_with_different_palettes_uses_ifd1_lut_or_is_rejected() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let path = write_fixture(
        "multi_ifd_palette.tif",
        &build_multi_ifd_color256_tiff(2, 2, &[0, 9]),
    );

    // FileOpener.openStack creates one ImageStack with the ColorModel built
    // from the first FileInfo, so every slice is displayed through IFD1's LUT
    // even though IFD2 carries a different ColorMap (Opener.java 824-825,
    // FileOpener.java 246-247).
    let oracle = bridge
        .eval_macro(&format!(
            "open(\"{p}\"); getDimensions(w,h,c,z,t); \
             getLut(r,g,b); print(\"LUT1:\"+r[1]+\",\"+g[1]+\",\"+b[1]); \
             setSlice(2); getLut(r,g,b); \
             print(\"LUT2:\"+r[1]+\",\"+g[1]+\",\"+b[1]); \
             print(\"DIMS:\"+w+\",\"+h+\",\"+c+\",\"+z+\",\"+t);",
            p = path.display()
        ))
        .unwrap();
    assert_eq!(oracle_line(&oracle.stdout, "DIMS:"), "DIMS:2,2,1,2,1");
    assert_eq!(oracle_line(&oracle.stdout, "LUT1:"), "LUT1:1,254,0");
    assert_eq!(
        oracle_line(&oracle.stdout, "LUT2:"),
        "LUT2:1,254,0",
        "ImageJ applies IFD1's LUT to slice 2"
    );

    // The Rust model keeps each slice's LUT, and save_tiff rejects a stack
    // with mixed palettes, so loading must fail explicitly instead of
    // accepting pixels that ImageJ would display through slice 1's palette.
    let err = load_tiff(&path).unwrap_err();
    assert!(
        err.to_string().contains("palette"),
        "expected an explicit palette error, got: {err}"
    );
}

#[test]
fn eight_bit_grayscale_color_map_loads_as_color256_with_lut_like_java() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let data = [1u8, 8, 15, 22];
    let palette = fixture_color_map(3);
    // getColorMap runs for any ColorMap tag, not just PhotometricInterpretation
    // 3 (TiffDecoder.java 536), and promotes GRAY8 to COLOR8 whenever the
    // signed byte sum is non-zero (TiffDecoder.java 181-182).
    let black_is_zero = write_fixture(
        "gray8_black_is_zero_color_map.tif",
        &build_gray_photometric_color_map_tiff(2, 2, 8, 1, 1, &data, &palette, None),
    );
    let white_is_zero = write_fixture(
        "gray8_white_is_zero_color_map.tif",
        &build_gray_photometric_color_map_tiff(2, 2, 8, 1, 0, &data, &palette, None),
    );
    // A palette whose signed sum is zero keeps the ImageJ type at GRAY8 but is
    // still installed by createColorModel (FileOpener.java 509-514).
    let zero_sum = write_fixture(
        "gray8_zero_sum_color_map.tif",
        &build_gray_photometric_color_map_tiff(2, 2, 8, 1, 1, &data, &net_zero_color_map(), None),
    );

    let oracle = bridge
        .eval_macro(&format!(
            "open(\"{a}\"); getLut(r,g,b); \
             print(\"BLACK:\"+bitDepth()+\",\"+r[0]+\",\"+r[1]+\",\"+g[1]+\",\"+b[1]+\",\"+getPixel(1,1)); \
             close(); \
             open(\"{b}\"); getLut(r,g,b); \
             print(\"WHITE:\"+bitDepth()+\",\"+r[0]+\",\"+r[1]+\",\"+g[1]+\",\"+b[1]+\",\"+getPixel(1,1)); \
             close(); \
             open(\"{c}\"); getLut(r,g,b); \
             print(\"ZERO:\"+bitDepth()+\",\"+r[0]+\",\"+r[1]+\",\"+g[1]+\",\"+b[1]+\",\"+getPixel(1,1));",
            a = black_is_zero.display(),
            b = white_is_zero.display(),
            c = zero_sum.display()
        ))
        .unwrap();
    assert_eq!(
        oracle_line(&oracle.stdout, "BLACK:"),
        "BLACK:8,3,4,254,3,22"
    );
    assert_eq!(
        oracle_line(&oracle.stdout, "WHITE:"),
        "WHITE:8,3,4,254,3,22"
    );
    assert_eq!(oracle_line(&oracle.stdout, "ZERO:"), "ZERO:8,1,255,0,0,22");

    for path in [&black_is_zero, &white_is_zero] {
        let doc = load_tiff(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let mut lut = Lut256::default();
        for i in 0..256 {
            lut.r[i] = (i as u8).wrapping_add(3);
            lut.g[i] = (255 - i) as u8;
            lut.b[i] = 3;
        }
        assert_eq!(
            doc.slices,
            vec![PixelPlane::Color256 {
                pixels: data.to_vec(),
                lut: Box::new(lut),
            }],
            "{}",
            path.display()
        );
    }

    // Rust cannot represent "GRAY8 type with a custom LUT", so the preserved
    // palette necessarily becomes COLOR_256; the display LUT is exact.
    let doc = load_tiff(&zero_sum).unwrap();
    let PixelPlane::Color256 { pixels, lut } = doc.plane() else {
        panic!(
            "zero-sum ColorMap must load as Color256, got {:?}",
            doc.plane()
        );
    };
    assert_eq!(pixels, &data);
    assert_eq!((lut.r[0], lut.r[1], lut.g[1], lut.b[1]), (1, 255, 0, 0));
}

#[test]
fn gray16_color_map_is_ignored_only_without_description_like_java() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let data: Vec<u8> = [5u16, 1239, 2473, 3707]
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect();
    let palette = fixture_color_map(3);
    let no_description = write_fixture(
        "gray16_color_map_no_description.tif",
        &build_gray_photometric_color_map_tiff(2, 2, 16, 1, 1, &data, &palette, None),
    );
    // getString returns null for very short descriptions, so FileInfo.description
    // stays null and the LUT is discarded (TiffDecoder.java 850-851).
    let short_description = write_fixture(
        "gray16_color_map_short_description.tif",
        &build_gray_photometric_color_map_tiff(2, 2, 16, 1, 1, &data, &palette, Some("abcdef")),
    );
    let long_description = write_fixture(
        "gray16_color_map_description.tif",
        &build_gray_photometric_color_map_tiff(
            2,
            2,
            16,
            1,
            1,
            &data,
            &palette,
            Some("ImageJ=1.54u\n"),
        ),
    );

    let oracle = bridge
        .eval_macro(&format!(
            "open(\"{a}\"); getLut(r,g,b); print(\"NONE:\"+r[0]+\",\"+r[1]+\",\"+g[1]+\",\"+b[1]); close(); \
             open(\"{b}\"); getLut(r,g,b); print(\"SHORT:\"+r[0]+\",\"+r[1]+\",\"+g[1]+\",\"+b[1]); close(); \
             open(\"{c}\"); getLut(r,g,b); print(\"LONG:\"+r[0]+\",\"+r[1]+\",\"+g[1]+\",\"+b[1]);",
            a = no_description.display(),
            b = short_description.display(),
            c = long_description.display()
        ))
        .unwrap();
    assert_eq!(oracle_line(&oracle.stdout, "NONE:"), "NONE:0,1,1,1");
    assert_eq!(oracle_line(&oracle.stdout, "SHORT:"), "SHORT:0,1,1,1");
    assert_eq!(oracle_line(&oracle.stdout, "LONG:"), "LONG:3,4,254,3");

    let expected = vec![PixelPlane::Gray16(vec![5, 1239, 2473, 3707])];
    for path in [&no_description, &short_description] {
        let doc = load_tiff(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert_eq!(doc.slices, expected, "{}", path.display());
    }

    // With a description ImageJ keeps the palette and applies it to the
    // 8-bit projection of the 16-bit data; Gray16 has no LUT field.
    let err = load_tiff(&long_description).unwrap_err();
    assert!(
        err.to_string().contains("16-bit") && err.to_string().contains("LUT"),
        "expected an explicit 16-bit display LUT error, got: {err}"
    );
}

#[test]
fn gray32_color_map_is_rejected_because_java_applies_the_lut() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let data: Vec<u8> = [0.5f32, 1.5, 2.5, 3.5]
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect();
    let palette = fixture_color_map(3);
    let no_description = write_fixture(
        "gray32_color_map_no_description.tif",
        &build_gray_photometric_color_map_tiff(2, 2, 32, 3, 1, &data, &palette, None),
    );
    let with_description = write_fixture(
        "gray32_color_map_description.tif",
        &build_gray_photometric_color_map_tiff(
            2,
            2,
            32,
            3,
            1,
            &data,
            &palette,
            Some("ImageJ=1.54u\n"),
        ),
    );

    // Unlike GRAY16_UNSIGNED, TiffDecoder keeps the LUT for 32-bit files
    // regardless of the ImageDescription (TiffDecoder.java 850-851).
    let oracle = bridge
        .eval_macro(&format!(
            "open(\"{a}\"); getLut(r,g,b); print(\"NONE:\"+r[0]+\",\"+r[1]+\",\"+g[1]+\",\"+b[1]); close(); \
             open(\"{b}\"); getLut(r,g,b); print(\"LONG:\"+r[0]+\",\"+r[1]+\",\"+g[1]+\",\"+b[1]);",
            a = no_description.display(),
            b = with_description.display()
        ))
        .unwrap();
    assert_eq!(oracle_line(&oracle.stdout, "NONE:"), "NONE:3,4,254,3");
    assert_eq!(oracle_line(&oracle.stdout, "LONG:"), "LONG:3,4,254,3");

    for path in [&no_description, &with_description] {
        let err = load_tiff(path).unwrap_err();
        assert!(
            err.to_string().contains("32-bit") && err.to_string().contains("LUT"),
            "expected an explicit 32-bit display LUT error, got: {err}"
        );
    }
}

#[test]
fn non_imagej_cm_pixel_size_below_one_micron_threshold_converts_to_microns() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    // 2000 pixels/cm means 0.0005 cm per pixel. Without an ImageJ description
    // FileOpener uses the 0.001 threshold and converts to 5 um.
    let path = write_fixture("cm_tiny_no_desc.tif", &build_cm_tiff(4, 4, 2000, None));

    let oracle = bridge
        .eval_macro(&format!(
            "open(\"{p}\"); getPixelSize(u,w,h); print(\"CAL:\"+u+\",\"+w+\",\"+h);",
            p = path.display()
        ))
        .unwrap();
    let line = oracle_line(&oracle.stdout, "CAL:");
    let mut fields = line["CAL:".len()..].split(',');
    let java_unit = fields.next().unwrap().to_string();
    let java_width: f64 = fields.next().unwrap().parse().unwrap();
    let java_height: f64 = fields.next().unwrap().parse().unwrap();
    assert_eq!(java_unit, "\u{00b5}m");
    assert_close(java_width, 5.0, "Java pixel width");

    let doc = load_tiff(&path).unwrap();
    assert_eq!(doc.calibration.unit, java_unit);
    assert_close(doc.calibration.pixel_width, java_width, "pixel width");
    assert_close(doc.calibration.pixel_height, java_height, "pixel height");
}

#[test]
fn imagej_description_cm_pixel_size_uses_the_smaller_micron_threshold() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let above = write_fixture(
        "cm_tiny_imagej_desc.tif",
        &build_cm_tiff(4, 4, 2000, Some("ImageJ=1.54u\nunit=cm\n")),
    );
    let below = write_fixture(
        "cm_nano_imagej_desc.tif",
        &build_cm_tiff(4, 4, 20000, Some("ImageJ=1.54u\nunit=cm\n")),
    );

    // An ImageJ description lowers the threshold to 0.0001: 0.0005 cm stays in
    // centimetres, while 0.00005 cm becomes 0.5 um (FileOpener.java 398-408).
    let oracle = bridge
        .eval_macro(&format!(
            "a=newArray(\"{above}\",\"{below}\"); \
             for(i=0;i<2;i++) {{ open(a[i]); getPixelSize(u,w,h); \
             print(\"CAL:\"+i+\":\"+u+\",\"+w+\",\"+h); }}",
            above = above.display(),
            below = below.display()
        ))
        .unwrap();
    let read = |prefix: &str| {
        let line = oracle_line(&oracle.stdout, prefix);
        let mut fields = line[prefix.len()..].split(',');
        (
            fields.next().unwrap().to_string(),
            fields.next().unwrap().parse::<f64>().unwrap(),
            fields.next().unwrap().parse::<f64>().unwrap(),
        )
    };
    let (above_unit, above_width, above_height) = read("CAL:0:");
    let (below_unit, below_width, below_height) = read("CAL:1:");
    assert_eq!(above_unit, "cm");
    assert_close(above_width, 0.0005, "Java above-threshold width");
    assert_eq!(below_unit, "\u{00b5}m");
    assert_close(below_width, 0.5, "Java below-threshold width");

    let above_doc = load_tiff(&above).unwrap();
    assert_eq!(above_doc.calibration.unit, above_unit);
    assert_close(
        above_doc.calibration.pixel_width,
        above_width,
        "above-threshold pixel width",
    );
    assert_close(
        above_doc.calibration.pixel_height,
        above_height,
        "above-threshold pixel height",
    );

    let below_doc = load_tiff(&below).unwrap();
    assert_eq!(below_doc.calibration.unit, below_unit);
    assert_close(
        below_doc.calibration.pixel_width,
        below_width,
        "below-threshold pixel width",
    );
    assert_close(
        below_doc.calibration.pixel_height,
        below_height,
        "below-threshold pixel height",
    );
}

#[test]
fn converted_cm_pixel_depth_matches_java_conversion() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let path = write_fixture(
        "cm_nano_depth.tif",
        &build_cm_tiff(4, 4, 20000, Some("ImageJ=1.54u\nunit=cm\nspacing=0.0002\n")),
    );

    // FileOpener multiplies fi.pixelDepth by 10000 whenever the cm pixel size
    // is converted and the depth is not exactly 1.0 (FileOpener.java 405-406).
    let oracle = bridge
        .eval_macro(&format!(
            "open(\"{p}\"); getPixelSize(u,w,h,d); print(\"CAL:\"+u+\",\"+w+\",\"+h+\",\"+d);",
            p = path.display()
        ))
        .unwrap();
    let line = oracle_line(&oracle.stdout, "CAL:");
    let mut fields = line["CAL:".len()..].split(',');
    let java_unit = fields.next().unwrap().to_string();
    let java_width: f64 = fields.next().unwrap().parse().unwrap();
    let java_height: f64 = fields.next().unwrap().parse().unwrap();
    let java_depth: f64 = fields.next().unwrap().parse().unwrap();
    assert_eq!(java_unit, "\u{00b5}m");
    assert_close(java_depth, 2.0, "Java pixel depth");

    let doc = load_tiff(&path).unwrap();
    assert_eq!(doc.calibration.unit, java_unit);
    assert_close(doc.calibration.pixel_width, java_width, "pixel width");
    assert_close(doc.calibration.pixel_height, java_height, "pixel height");
    assert_close(doc.calibration.pixel_depth, java_depth, "pixel depth");
}

#[test]
fn custom_lut_metadata_is_rejected_instead_of_dropped() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let data = [0u8, 1, 2, 3];

    // TiffEncoder.writeMetaData layout: byte counts [header size, data length],
    // then an "IJIJ" header with one "luts" entry (TiffEncoder.java 371-472).
    let mut counts = Vec::new();
    counts.extend_from_slice(&12u32.to_le_bytes());
    counts.extend_from_slice(&768u32.to_le_bytes());
    let mut metadata = Vec::new();
    metadata.extend_from_slice(&0x494a_494au32.to_le_bytes());
    metadata.extend_from_slice(&0x6c75_7473u32.to_le_bytes());
    metadata.extend_from_slice(&1u32.to_le_bytes());
    metadata.extend(std::iter::repeat(0u8).take(768));

    let mut entries = base_fixture_entries(2, 2, 8, 1, 1, 1, 1, data.len() as u32);
    entries.push((50838, 4, 2, FixtureValue::Blob(0)));
    entries.push((50839, 1, metadata.len() as u32, FixtureValue::Blob(1)));
    let path = write_fixture(
        "custom_lut_metadata.tif",
        &build_tiff_fixture(&data, &[counts, metadata], &entries),
    );

    // ImageJ accepts the file; the Rust model has no per-channel LUT field, so
    // it must refuse rather than silently turn a custom palette into gray.
    bridge
        .eval_macro(&format!("open(\"{p}\");", p = path.display()))
        .unwrap();
    let err = load_tiff(&path).unwrap_err();
    assert!(
        err.to_string().contains("LUT"),
        "expected an explicit LUT error, got: {err}"
    );
}

#[test]
fn unsupported_calibration_metadata_is_rejected_instead_of_dropped() {
    let data = [1u8, 2, 3, 4];
    let blob = description_blob(
        "ImageJ=1.54u\nunit=pixel\nxorigin=12.5\nfinterval=2\n\
         tunit=ms\nyunit=mm\nvunit=OD\ncf=0\nc0=1.0\nc1=0.0\ninfo=bench\n",
    );
    let mut entries = base_fixture_entries(2, 2, 8, 1, 1, 1, 1, data.len() as u32);
    entries.push((270, 2, blob.len() as u32, FixtureValue::Blob(0)));
    let path = write_fixture(
        "unsupported_calibration.tif",
        &build_tiff_fixture(&data, &[blob], &entries),
    );

    let err = load_tiff(&path).unwrap_err();
    assert!(
        err.to_string().contains("calibration"),
        "expected an explicit calibration metadata error, got: {err}"
    );
}

// ---------------------------------------------------------------------------
// Unit 5 (first increment): 3x3 Smooth for Gray8, Gray16 and Gray32.
//
// Filters.run calls ImageProcessor.smooth(), which is `filter(BLUR_MORE)` when
// the image width is greater than 1 (original_imagej/ij/process/
// ImageProcessor.java lines 1634-1636). Pixels are exchanged through TIFF, so
// the oracle starts from exactly the Rust pixel buffer.
// ---------------------------------------------------------------------------

fn smooth_pattern_8bit(width: u32, height: u32) -> ImageDocument {
    let mut pixels = vec![0u8; (width * height) as usize];
    for y in 0..height {
        for x in 0..width {
            pixels[(y * width + x) as usize] = ((x * 37 + y * 61) % 251) as u8;
        }
    }
    ImageDocument::new("smooth8.tif", width, height, PixelPlane::Gray8(pixels)).unwrap()
}

fn smooth_pattern_16bit(width: u32, height: u32) -> ImageDocument {
    let mut pixels = vec![0u16; (width * height) as usize];
    for y in 0..height {
        for x in 0..width {
            pixels[(y * width + x) as usize] = ((x * 997 + y * 2111) % 65521) as u16;
        }
    }
    ImageDocument::new("smooth16.tif", width, height, PixelPlane::Gray16(pixels)).unwrap()
}

/// Values are deliberately fractional so a 64-bit implementation of the
/// FloatProcessor sum produces different bits from ImageJ's float arithmetic.
fn smooth_pattern_32bit(width: u32, height: u32) -> ImageDocument {
    let mut pixels = Vec::with_capacity((width * height) as usize);
    for y in 0..height {
        for x in 0..width {
            let i = (y * width + x) as f32;
            pixels.push(i * 0.1 + 0.001 * x as f32 - 0.7);
        }
    }
    ImageDocument::new("smooth32.tif", width, height, PixelPlane::Gray32(pixels)).unwrap()
}

fn oracle_smooth(bridge: &JavaOracleBridge, doc: &ImageDocument, prelude: &str) -> ImageDocument {
    let out = bridge
        .run_macro_on_image(doc, &format!("{prelude} run(\"Smooth\");"))
        .unwrap();
    out.output_image.expect("oracle must return saved TIFF")
}

#[test]
fn parity_8bit_smooth_full_image_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let doc = smooth_pattern_8bit(6, 4);
    let oracle = oracle_smooth(&bridge, &doc, "");

    let mut rust = doc.clone();
    rust.smooth().unwrap();

    assert_ne!(doc.plane(), oracle.plane(), "oracle smooth was a no-op");
    assert_eq!(rust.plane(), oracle.plane());
}

#[test]
fn parity_8bit_smooth_rect_roi_leaks_last_column_and_row_like_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = smooth_pattern_8bit(6, 4);
    doc.set_rect_roi(1, 1, 4, 2).unwrap();
    let oracle = oracle_smooth(&bridge, &doc, "makeRectangle(1, 1, 4, 2);");

    let mut rust = doc.clone();
    rust.smooth().unwrap();

    // ByteProcessor.filter's xMax/yMax edge passes rewrite the image's last
    // column and row even though they lie outside the ROI rectangle
    // (ByteProcessor.java 678-681), and reset(mask) never restores them.
    assert_eq!(rust.plane(), oracle.plane());
}

#[test]
fn parity_8bit_smooth_interior_rect_roi_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = smooth_pattern_8bit(6, 4);
    doc.set_rect_roi(1, 0, 2, 2).unwrap();
    let oracle = oracle_smooth(&bridge, &doc, "makeRectangle(1, 0, 2, 2);");

    let mut rust = doc.clone();
    rust.smooth().unwrap();

    assert_ne!(doc.plane(), oracle.plane(), "oracle smooth was a no-op");
    assert_eq!(rust.plane(), oracle.plane());
}

#[test]
fn parity_8bit_smooth_oval_roi_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = smooth_pattern_8bit(6, 4);
    doc.set_oval_roi(1, 1, 4, 2).unwrap();
    let oracle = oracle_smooth(&bridge, &doc, "makeOval(1, 1, 4, 2);");

    let mut rust = doc.clone();
    rust.smooth().unwrap();

    assert_ne!(doc.plane(), oracle.plane(), "oracle smooth was a no-op");
    assert_eq!(rust.plane(), oracle.plane());
}

#[test]
fn parity_8bit_smooth_clipped_oval_roi_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = smooth_pattern_8bit(6, 4);
    doc.set_oval_roi(-2, -1, 5, 4).unwrap();
    let oracle = oracle_smooth(&bridge, &doc, "makeOval(-2, -1, 5, 4);");

    let mut rust = doc.clone();
    rust.smooth().unwrap();

    assert_eq!(rust.plane(), oracle.plane());
}

#[test]
fn parity_16bit_smooth_full_image_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let doc = smooth_pattern_16bit(6, 4);
    let oracle = oracle_smooth(&bridge, &doc, "");

    let mut rust = doc.clone();
    rust.smooth().unwrap();

    assert_ne!(doc.plane(), oracle.plane(), "oracle smooth was a no-op");
    assert_eq!(rust.plane(), oracle.plane());
}

#[test]
fn parity_16bit_smooth_clipped_rect_roi_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = smooth_pattern_16bit(6, 4);
    doc.set_rect_roi(-2, 1, 6, 5).unwrap();
    let oracle = oracle_smooth(&bridge, &doc, "makeRectangle(-2, 1, 6, 5);");

    let mut rust = doc.clone();
    rust.smooth().unwrap();

    assert_eq!(rust.plane(), oracle.plane());
}

#[test]
fn parity_16bit_smooth_oval_roi_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = smooth_pattern_16bit(6, 4);
    doc.set_oval_roi(1, 1, 4, 2).unwrap();
    let oracle = oracle_smooth(&bridge, &doc, "makeOval(1, 1, 4, 2);");

    let mut rust = doc.clone();
    rust.smooth().unwrap();

    // ShortProcessor.filter3x3 only rewrites the ROI rectangle, so unlike the
    // 8-bit path the last image column and row stay untouched.
    assert_eq!(rust.plane(), oracle.plane());
}

#[test]
fn parity_32bit_smooth_full_image_matches_java_oracle_f32() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let doc = smooth_pattern_32bit(6, 4);
    let oracle = oracle_smooth(&bridge, &doc, "");

    let mut rust = doc.clone();
    rust.smooth().unwrap();

    assert_ne!(doc.plane(), oracle.plane(), "oracle smooth was a no-op");
    assert_eq!(rust.plane(), oracle.plane());
}

#[test]
fn parity_32bit_smooth_oval_roi_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = smooth_pattern_32bit(6, 4);
    doc.set_oval_roi(1, 1, 4, 2).unwrap();
    let oracle = oracle_smooth(&bridge, &doc, "makeOval(1, 1, 4, 2);");

    let mut rust = doc.clone();
    rust.smooth().unwrap();

    assert_eq!(rust.plane(), oracle.plane());
}

#[test]
fn parity_smooth_width_one_is_a_noop_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let docs = [
        smooth_pattern_8bit(1, 4),
        smooth_pattern_16bit(1, 4),
        smooth_pattern_32bit(1, 4),
    ];

    for doc in docs {
        let oracle = oracle_smooth(&bridge, &doc, "");
        let mut rust = doc.clone();
        rust.smooth().unwrap();
        // ImageProcessor.smooth() returns without filtering when width<=1.
        assert_eq!(rust.plane(), oracle.plane());
        assert_eq!(rust.plane(), doc.plane());
    }
}

#[test]
fn parity_smooth_height_one_row_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let docs = [
        smooth_pattern_8bit(5, 1),
        smooth_pattern_16bit(5, 1),
        smooth_pattern_32bit(5, 1),
    ];

    for doc in docs {
        let oracle = oracle_smooth(&bridge, &doc, "");
        let mut rust = doc.clone();
        rust.smooth().unwrap();
        // A single row is smoothed with the row replicated above and below.
        assert_ne!(doc.plane(), oracle.plane(), "oracle smooth was a no-op");
        assert_eq!(rust.plane(), oracle.plane());
    }
}

#[test]
fn parity_smooth_outside_roi_preserves_pixels() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    for mut doc in [
        smooth_pattern_8bit(6, 4),
        smooth_pattern_16bit(6, 4),
        smooth_pattern_32bit(6, 4),
    ] {
        doc.set_rect_roi(100, 100, 4, 4).unwrap();
        let oracle = oracle_smooth(&bridge, &doc, "makeRectangle(100, 100, 4, 4);");

        let mut rust = doc.clone();
        rust.smooth().unwrap();
        assert_eq!(rust.plane(), oracle.plane());
        assert_eq!(rust.plane(), doc.plane());
    }
}

#[test]
fn parity_smooth_stack_slice_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = distinct_stack_8bit(6, 4, 3);
    doc.set_slice(2).unwrap();
    // "slice" answers "Process Stack?" with No, which is the only stack mode
    // this increment implements.
    let oracle = bridge
        .run_macro_on_image(&doc, "setSlice(2); run(\"Smooth\", \"slice\");")
        .unwrap()
        .output_image
        .expect("oracle must return saved TIFF")
        .slices;
    let original = doc.slices.clone();

    let mut rust = doc.clone();
    rust.smooth().unwrap();

    assert_eq!(rust.slices, oracle);
    assert_eq!(rust.slices[0], original[0]);
    assert_ne!(rust.slices[1], original[1]);
    assert_eq!(rust.slices[2], original[2]);
}

#[test]
fn session_smooth_records_macro_and_updates_pixels() {
    let mut session = Session::new();
    session
        .dispatch(Command::NewRamp8Bit {
            title: "smooth".into(),
            width: 6,
            height: 4,
        })
        .unwrap();
    let before = session.active_doc().unwrap().plane().clone();
    session.dispatch(Command::Smooth).unwrap();
    assert_ne!(session.active_doc().unwrap().plane(), &before);
    assert_eq!(
        session.macro_recorder.last().map(String::as_str),
        Some("run(\"Smooth\");")
    );

    // A stack answer to "Process Stack?" is recorded as the `slice` option.
    let path = temp_tiff("session_smooth_stack.tif");
    save_tiff(&path, &distinct_stack_8bit(6, 4, 3)).unwrap();
    let mut stack_session = Session::new();
    stack_session.dispatch(Command::OpenTiff { path }).unwrap();
    stack_session
        .dispatch(Command::SetSlice { index: 2 })
        .unwrap();
    stack_session.dispatch(Command::Smooth).unwrap();
    assert_eq!(
        stack_session.macro_recorder.last().map(String::as_str),
        Some("run(\"Smooth\", \"slice\");")
    );
}

#[test]
fn smooth_rejects_unsupported_pixel_types_instead_of_ignoring_them() {
    let indexed = ImageDocument::new(
        "indexed8.tif",
        4,
        4,
        PixelPlane::Color256 {
            pixels: vec![3u8; 16],
            lut: Box::new(ij_core::Lut256::default()),
        },
    )
    .unwrap();
    let error = indexed.clone().smooth().unwrap_err();
    assert!(
        error.to_string().contains("8-bit color") && error.to_string().contains("Smooth"),
        "expected an explicit unsupported-type error, got: {error}"
    );

    let rgb =
        ImageDocument::new("rgb.tif", 4, 4, PixelPlane::ColorRgb(vec![0xff80_4020; 16])).unwrap();
    let error = rgb.clone().smooth().unwrap_err();
    assert!(
        error.to_string().contains("RGB") && error.to_string().contains("Smooth"),
        "expected an explicit unsupported-type error, got: {error}"
    );

    let mut session = Session::new();
    session
        .dispatch(Command::NewRamp8Bit {
            title: "ok".into(),
            width: 4,
            height: 4,
        })
        .unwrap();
    assert!(session.dispatch(Command::Smooth).is_ok());
}

#[test]
fn parity_8bit_smooth_polygon_roi_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = smooth_pattern_8bit(6, 4);
    doc.set_polygon_roi(&[(1, 1), (4, 1), (4, 3), (1, 3)])
        .unwrap();
    let oracle = oracle_smooth(&bridge, &doc, "makePolygon(1, 1, 4, 1, 4, 3, 1, 3);");

    let mut rust = doc.clone();
    rust.smooth().unwrap();

    assert_ne!(doc.plane(), oracle.plane(), "oracle smooth was a no-op");
    assert_eq!(rust.plane(), oracle.plane());
}

#[test]
fn parity_smooth_line_roi_resets_to_the_whole_image_like_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = smooth_pattern_8bit(6, 4);
    doc.set_line_roi(1, 1, 4, 2);
    // Filters.setup marks a non-area selection as noRoi and Filters.run calls
    // ip.resetRoi(), so Smooth processes the entire image (Filters.java 32-33).
    let oracle = oracle_smooth(&bridge, &doc, "makeLine(1, 1, 4, 2);");
    let plain = smooth_pattern_8bit(6, 4);
    assert_eq!(
        oracle.plane(),
        oracle_smooth(&bridge, &plain, "").plane(),
        "line ROI should behave like no selection"
    );

    let mut rust = doc.clone();
    rust.smooth().unwrap();
    assert_eq!(rust.plane(), oracle.plane());
}

#[test]
fn parity_8bit_smooth_clipped_rect_roi_with_edge_leaks_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = smooth_pattern_8bit(6, 4);
    // Clipped to (1, 0, 5, 3): xMax lands on width-2 and yMax on height-2, so
    // the 8-bit edge passes also rewrite the last column and row.
    doc.set_rect_roi(1, -1, 6, 3).unwrap();
    let oracle = oracle_smooth(&bridge, &doc, "makeRectangle(1, -1, 6, 3);");

    let mut rust = doc.clone();
    rust.smooth().unwrap();

    assert_eq!(rust.plane(), oracle.plane());
}

#[test]
fn parity_smooth_oval_touching_image_edges() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = smooth_pattern_8bit(6, 4);
    doc.set_oval_roi(0, 0, 6, 4).unwrap();
    let oracle = oracle_smooth(&bridge, &doc, "makeOval(0, 0, 6, 4);");
    doc.smooth().unwrap();
    assert_eq!(doc.plane(), oracle.plane());
}

// ---------------------------------------------------------------------------
// Unit 5 (second increment): 3x3 Sharpen for Gray8, Gray16 and Gray32.
//
// Filters.run calls ImageProcessor.sharpen(), which is
// `convolve3x3({-1,-1,-1,-1,12,-1,-1,-1,-1})` when the image is wider than
// one pixel (ImageProcessor.java 1640-1646). ByteProcessor/ShortProcessor sum
// in int and round with `(sum+scale/2)/scale` where scale is the kernel sum
// (4), so Java's truncating division applies; FloatProcessor sums in float,
// multiplies by `1f/scale` and never clamps. Unlike `filter(BLUR_MORE)` the
// convolve paths have no extra edge passes, so only the ROI rectangle is
// written and `reset(mask)` restores the pixels outside an irregular mask.
// ---------------------------------------------------------------------------

fn sharpen_pattern_8bit(width: u32, height: u32) -> ImageDocument {
    let mut pixels = vec![0u8; (width * height) as usize];
    for y in 0..height {
        for x in 0..width {
            pixels[(y * width + x) as usize] = ((x * 83 + y * 149 + 7) % 256) as u8;
        }
    }
    ImageDocument::new("sharpen8.tif", width, height, PixelPlane::Gray8(pixels)).unwrap()
}

fn sharpen_pattern_16bit(width: u32, height: u32) -> ImageDocument {
    let mut pixels = vec![0u16; (width * height) as usize];
    for y in 0..height {
        for x in 0..width {
            pixels[(y * width + x) as usize] = ((x * 4099 + y * 8191 + 17) % 65536) as u16;
        }
    }
    ImageDocument::new("sharpen16.tif", width, height, PixelPlane::Gray16(pixels)).unwrap()
}

/// Fractional values whose 3x3 sums round differently in `f64`; the kernel
/// is linear, so only the exact `f32` arithmetic order is being pinned.
fn sharpen_pattern_32bit(width: u32, height: u32) -> ImageDocument {
    let mut pixels = Vec::with_capacity((width * height) as usize);
    for y in 0..height {
        for x in 0..width {
            let i = (y * width + x) as f32;
            pixels.push((i * 0.1234567 - 0.37) * 1.000001 + 0.0001 * x as f32);
        }
    }
    ImageDocument::new("sharpen32.tif", width, height, PixelPlane::Gray32(pixels)).unwrap()
}

fn oracle_sharpen(bridge: &JavaOracleBridge, doc: &ImageDocument, prelude: &str) -> ImageDocument {
    let out = bridge
        .run_macro_on_image(doc, &format!("{prelude} run(\"Sharpen\");"))
        .unwrap();
    out.output_image.expect("oracle must return saved TIFF")
}

#[test]
fn parity_8bit_sharpen_full_image_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let doc = sharpen_pattern_8bit(6, 4);
    let oracle = oracle_sharpen(&bridge, &doc, "");

    let mut rust = doc.clone();
    rust.sharpen().unwrap();

    assert_ne!(doc.plane(), oracle.plane(), "oracle sharpen was a no-op");
    assert_eq!(rust.plane(), oracle.plane());
}

/// The kernel sums to 4, so Java computes `(sum+2)/4` with truncating
/// division (ByteProcessor.java 548-554). This 4x2 pattern exercises a
/// positive remainder that truncates down (123), plus both clamps.
#[test]
fn parity_8bit_sharpen_rounding_and_saturation_match_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let doc = ImageDocument::new(
        "sharpen_round8.tif",
        4,
        2,
        PixelPlane::Gray8(vec![0, 255, 5, 100, 255, 0, 0, 200]),
    )
    .unwrap();
    let oracle = oracle_sharpen(&bridge, &doc, "");

    let mut rust = doc.clone();
    rust.sharpen().unwrap();

    assert_eq!(rust.plane(), oracle.plane());
    assert_eq!(
        rust.plane(),
        &PixelPlane::Gray8(vec![0, 255, 0, 123, 255, 0, 0, 255])
    );
}

/// 16-bit Sharpen saturates at 65535 and 0 after the same `(sum+2)/4`
/// rounding (ShortProcessor.java 725-731).
#[test]
fn parity_16bit_sharpen_rounding_and_saturation_match_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let doc = ImageDocument::new(
        "sharpen_round16.tif",
        4,
        2,
        PixelPlane::Gray16(vec![0, 65535, 5, 60000, 65535, 0, 0, 40000]),
    )
    .unwrap();
    let oracle = oracle_sharpen(&bridge, &doc, "");

    let mut rust = doc.clone();
    rust.sharpen().unwrap();

    assert_eq!(rust.plane(), oracle.plane());
    assert_eq!(
        rust.plane(),
        &PixelPlane::Gray16(vec![0, 65535, 0, 65535, 65535, 0, 0, 59999])
    );
}

#[test]
fn parity_32bit_sharpen_full_image_matches_java_oracle_f32() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let doc = sharpen_pattern_32bit(6, 4);
    let oracle = oracle_sharpen(&bridge, &doc, "");

    let mut rust = doc.clone();
    rust.sharpen().unwrap();

    assert_ne!(doc.plane(), oracle.plane(), "oracle sharpen was a no-op");
    assert_eq!(rust.plane(), oracle.plane());
}

#[test]
fn parity_8bit_sharpen_rect_roi_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = sharpen_pattern_8bit(6, 4);
    doc.set_rect_roi(1, 1, 3, 2).unwrap();
    let oracle = oracle_sharpen(&bridge, &doc, "makeRectangle(1, 1, 3, 2);");

    let mut rust = doc.clone();
    rust.sharpen().unwrap();

    assert_eq!(rust.plane(), oracle.plane());
    assert_ne!(doc.plane(), oracle.plane(), "oracle sharpen was a no-op");
}

/// Clipped rectangle whose right/bottom edge touches the image border. The
/// convolve path has no ByteProcessor.filter edge passes, so the pixels
/// outside the clipped rectangle must stay untouched.
#[test]
fn parity_8bit_sharpen_clipped_rect_roi_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = sharpen_pattern_8bit(6, 4);
    doc.set_rect_roi(-2, 1, 6, 5).unwrap();
    let oracle = oracle_sharpen(&bridge, &doc, "makeRectangle(-2, 1, 6, 5);");

    let mut rust = doc.clone();
    rust.sharpen().unwrap();

    assert_eq!(rust.plane(), oracle.plane());
}

/// Irregular ROI whose mask covers the whole image: `reset(mask)` keeps every
/// pixel, and sampling clamps at all four borders.
#[test]
fn parity_8bit_sharpen_oval_roi_touching_image_edges_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = sharpen_pattern_8bit(6, 4);
    doc.set_oval_roi(0, 0, 6, 4).unwrap();
    let oracle = oracle_sharpen(&bridge, &doc, "makeOval(0, 0, 6, 4);");

    let mut rust = doc.clone();
    rust.sharpen().unwrap();

    assert_eq!(rust.plane(), oracle.plane());
    assert_ne!(doc.plane(), oracle.plane(), "oracle sharpen was a no-op");
}

#[test]
fn parity_8bit_sharpen_clipped_oval_roi_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = sharpen_pattern_8bit(6, 4);
    doc.set_oval_roi(-2, -1, 5, 4).unwrap();
    let oracle = oracle_sharpen(&bridge, &doc, "makeOval(-2, -1, 5, 4);");

    let mut rust = doc.clone();
    rust.sharpen().unwrap();

    assert_eq!(rust.plane(), oracle.plane());
}

#[test]
fn parity_16bit_sharpen_oval_roi_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = sharpen_pattern_16bit(6, 4);
    doc.set_oval_roi(1, 1, 4, 2).unwrap();
    let oracle = oracle_sharpen(&bridge, &doc, "makeOval(1, 1, 4, 2);");

    let mut rust = doc.clone();
    rust.sharpen().unwrap();

    assert_eq!(rust.plane(), oracle.plane());
}

#[test]
fn parity_32bit_sharpen_oval_roi_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = sharpen_pattern_32bit(6, 4);
    doc.set_oval_roi(1, 1, 4, 2).unwrap();
    let oracle = oracle_sharpen(&bridge, &doc, "makeOval(1, 1, 4, 2);");

    let mut rust = doc.clone();
    rust.sharpen().unwrap();

    assert_eq!(rust.plane(), oracle.plane());
}

#[test]
fn parity_sharpen_width_one_is_a_noop_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let docs = [
        sharpen_pattern_8bit(1, 4),
        sharpen_pattern_16bit(1, 4),
        sharpen_pattern_32bit(1, 4),
    ];

    for doc in docs {
        let oracle = oracle_sharpen(&bridge, &doc, "");
        let mut rust = doc.clone();
        rust.sharpen().unwrap();
        // ImageProcessor.sharpen() returns without convolving when width<=1.
        assert_eq!(rust.plane(), oracle.plane());
        assert_eq!(rust.plane(), doc.plane());
    }
}

#[test]
fn parity_sharpen_one_row_and_two_pixel_images_match_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let docs = [
        sharpen_pattern_8bit(5, 1),
        sharpen_pattern_16bit(5, 1),
        sharpen_pattern_32bit(5, 1),
        sharpen_pattern_8bit(2, 2),
        sharpen_pattern_16bit(2, 2),
        sharpen_pattern_32bit(2, 2),
        sharpen_pattern_8bit(2, 1),
        sharpen_pattern_16bit(2, 1),
        sharpen_pattern_32bit(2, 1),
    ];

    for doc in docs {
        let oracle = oracle_sharpen(&bridge, &doc, "");
        let mut rust = doc.clone();
        rust.sharpen().unwrap();
        // One row/column is replicated by the clamped sampling.
        assert_eq!(rust.plane(), oracle.plane());
    }
}

#[test]
fn parity_8bit_sharpen_polygon_roi_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = sharpen_pattern_8bit(6, 4);
    doc.set_polygon_roi(&[(1, 1), (4, 1), (4, 3), (1, 3)])
        .unwrap();
    let oracle = oracle_sharpen(&bridge, &doc, "makePolygon(1, 1, 4, 1, 4, 3, 1, 3);");

    let mut rust = doc.clone();
    rust.sharpen().unwrap();

    assert_eq!(rust.plane(), oracle.plane());
}

#[test]
fn parity_sharpen_line_roi_resets_to_the_whole_image_like_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = sharpen_pattern_8bit(6, 4);
    doc.set_line_roi(1, 1, 4, 2);
    // Filters.setup flags a non-area selection as noRoi and Filters.run calls
    // ip.resetRoi(), so Sharpen processes the entire image (Filters.java 24,
    // 32-33).
    let oracle = oracle_sharpen(&bridge, &doc, "makeLine(1, 1, 4, 2);");
    let plain = sharpen_pattern_8bit(6, 4);
    assert_eq!(
        oracle.plane(),
        oracle_sharpen(&bridge, &plain, "").plane(),
        "line ROI should behave like no selection"
    );

    let mut rust = doc.clone();
    rust.sharpen().unwrap();
    assert_eq!(rust.plane(), oracle.plane());
}

#[test]
fn parity_sharpen_stack_slice_and_undo_match_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = distinct_stack_8bit(6, 4, 3);
    doc.set_slice(2).unwrap();
    let original = doc.slices.clone();

    let oracle = bridge
        .run_macro_on_image(&doc, "setSlice(2); run(\"Sharpen\", \"slice\");")
        .unwrap()
        .output_image
        .expect("oracle must return saved TIFF")
        .slices;
    let oracle_undo = bridge
        .run_macro_on_image(
            &doc,
            "setSlice(2); run(\"Sharpen\", \"slice\"); run(\"Undo\");",
        )
        .unwrap()
        .output_image
        .expect("oracle must return saved TIFF")
        .slices;

    let mut rust = doc.clone();
    rust.sharpen().unwrap();

    assert_eq!(rust.slices, oracle);
    assert_eq!(rust.slices[0], original[0]);
    assert_ne!(rust.slices[1], original[1]);
    assert_eq!(rust.slices[2], original[2]);

    assert!(rust.undo(), "slice Sharpen must leave an Undo snapshot");
    assert_eq!(rust.slices, oracle_undo);
    assert_eq!(rust.slices, original);
}

#[test]
fn sharpen_rejects_unsupported_pixel_types_instead_of_ignoring_them() {
    let indexed = ImageDocument::new(
        "indexed8.tif",
        4,
        4,
        PixelPlane::Color256 {
            pixels: vec![3u8; 16],
            lut: Box::new(ij_core::Lut256::default()),
        },
    )
    .unwrap();
    let error = indexed.clone().sharpen().unwrap_err();
    assert!(
        error.to_string().contains("8-bit color") && error.to_string().contains("Sharpen"),
        "expected an explicit unsupported-type error, got: {error}"
    );

    let rgb =
        ImageDocument::new("rgb.tif", 4, 4, PixelPlane::ColorRgb(vec![0xff80_4020; 16])).unwrap();
    let error = rgb.clone().sharpen().unwrap_err();
    assert!(
        error.to_string().contains("RGB") && error.to_string().contains("Sharpen"),
        "expected an explicit unsupported-type error, got: {error}"
    );

    // Rejected types must not consume or install an Undo snapshot.
    assert!(!rgb.clone().undo());
}

#[test]
fn session_sharpen_records_macro_and_updates_pixels() {
    let path = temp_tiff("session_sharpen.tif");
    let original = sharpen_pattern_8bit(6, 4);
    save_tiff(&path, &original).unwrap();

    let mut session = Session::new();
    session.dispatch(Command::OpenTiff { path }).unwrap();
    let mut expected = original.clone();
    expected.sharpen().unwrap();
    session.dispatch(Command::Sharpen).unwrap();
    assert_eq!(session.active_doc().unwrap().plane(), expected.plane());
    assert_ne!(expected.plane(), original.plane());
    assert_eq!(
        session.macro_recorder.last().map(String::as_str),
        Some("run(\"Sharpen\");")
    );

    // A stack answer to "Process Stack?" is recorded as the `slice` option.
    let path = temp_tiff("session_sharpen_stack.tif");
    save_tiff(&path, &distinct_stack_8bit(6, 4, 3)).unwrap();
    let mut stack_session = Session::new();
    stack_session.dispatch(Command::OpenTiff { path }).unwrap();
    stack_session
        .dispatch(Command::SetSlice { index: 2 })
        .unwrap();
    stack_session.dispatch(Command::Sharpen).unwrap();
    assert_eq!(
        stack_session.macro_recorder.last().map(String::as_str),
        Some("run(\"Sharpen\", \"slice\");")
    );
}

// ---------------------------------------------------------------------------
// Unit 5 (third increment): Find Edges for Gray8, Gray16 and Gray32.
//
// Filters.run calls ImageProcessor.findEdges(), which is `filter(FIND_EDGES)`
// when the image is wider than one pixel (ImageProcessor.java 1649-1653).
// ByteProcessor.filter computes the Sobel magnitude only for the interior
// `xMin..xMax`/`yMin..yMax` and then runs four extra `filterEdge` passes that
// clamp at the image border (ByteProcessor.java 568-681). ShortProcessor and
// FloatProcessor process the whole ROI rectangle with clamped sampling
// (ShortProcessor.java 648-730, FloatProcessor.java 575-649). The 8-bit and
// 16-bit sums are truncated after `Math.sqrt`; the float path keeps every
// accumulation in single precision before widening to `double` for the square
// root. Pixels are exchanged through TIFF, so the oracle starts from exactly
// the Rust pixel buffer.
// ---------------------------------------------------------------------------

fn find_edges_pattern_8bit(width: u32, height: u32) -> ImageDocument {
    let mut pixels = vec![0u8; (width * height) as usize];
    for y in 0..height {
        for x in 0..width {
            pixels[(y * width + x) as usize] = ((x * 53 + y * 97 + 11) % 256) as u8;
        }
    }
    ImageDocument::new("find_edges8.tif", width, height, PixelPlane::Gray8(pixels)).unwrap()
}

fn find_edges_pattern_16bit(width: u32, height: u32) -> ImageDocument {
    let mut pixels = vec![0u16; (width * height) as usize];
    for y in 0..height {
        for x in 0..width {
            pixels[(y * width + x) as usize] = ((x * 4001 + y * 7919 + 23) % 65536) as u16;
        }
    }
    ImageDocument::new(
        "find_edges16.tif",
        width,
        height,
        PixelPlane::Gray16(pixels),
    )
    .unwrap()
}

/// Fractional values; a 64-bit implementation of the float Sobel sum produces
/// different bits from ImageJ's single-precision arithmetic.
fn find_edges_pattern_32bit(width: u32, height: u32) -> ImageDocument {
    let mut pixels = Vec::with_capacity((width * height) as usize);
    for y in 0..height {
        for x in 0..width {
            let i = (y * width + x) as f32;
            pixels.push(i * 0.137 + 0.0003 * x as f32 - 0.41);
        }
    }
    ImageDocument::new(
        "find_edges32.tif",
        width,
        height,
        PixelPlane::Gray32(pixels),
    )
    .unwrap()
}

fn oracle_find_edges(
    bridge: &JavaOracleBridge,
    doc: &ImageDocument,
    prelude: &str,
) -> ImageDocument {
    let out = bridge
        .run_macro_on_image(doc, &format!("{prelude} run(\"Find Edges\");"))
        .unwrap();
    out.output_image.expect("oracle must return saved TIFF")
}

#[test]
fn parity_8bit_find_edges_full_image_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let doc = find_edges_pattern_8bit(6, 4);
    let oracle = oracle_find_edges(&bridge, &doc, "");

    let mut rust = doc.clone();
    rust.find_edges().unwrap();

    assert_ne!(doc.plane(), oracle.plane(), "oracle Find Edges was a no-op");
    assert_eq!(rust.plane(), oracle.plane());
}

#[test]
fn parity_16bit_find_edges_full_image_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let doc = find_edges_pattern_16bit(6, 4);
    let oracle = oracle_find_edges(&bridge, &doc, "");

    let mut rust = doc.clone();
    rust.find_edges().unwrap();

    assert_ne!(doc.plane(), oracle.plane(), "oracle Find Edges was a no-op");
    assert_eq!(rust.plane(), oracle.plane());
}

#[test]
fn parity_32bit_find_edges_full_image_matches_java_oracle_f32() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let doc = find_edges_pattern_32bit(6, 4);
    let oracle = oracle_find_edges(&bridge, &doc, "");

    let mut rust = doc.clone();
    rust.find_edges().unwrap();

    assert_ne!(doc.plane(), oracle.plane(), "oracle Find Edges was a no-op");
    assert_eq!(rust.plane(), oracle.plane());
}

/// A pattern whose centre pixel has `sum1 == sum2 == 3`, so
/// `sqrt(18) == 4.2426...` truncates to 4 (ByteProcessor.java 604-607). The
/// second image forces the 8-bit magnitude above 255 and must clamp
/// (ByteProcessor.java 608).
#[test]
fn parity_8bit_find_edges_sobel_truncation_and_saturation_match_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let truncating = ImageDocument::new(
        "find_edges_trunc8.tif",
        3,
        3,
        PixelPlane::Gray8(vec![1, 1, 0, 1, 0, 0, 0, 0, 0]),
    )
    .unwrap();
    let truncating_oracle = oracle_find_edges(&bridge, &truncating, "");
    let mut rust = truncating.clone();
    rust.find_edges().unwrap();
    assert_eq!(rust.plane(), truncating_oracle.plane());
    assert_eq!(
        rust.plane(),
        &PixelPlane::Gray8(vec![1, 4, 3, 4, 4, 1, 3, 1, 0])
    );

    let saturating = ImageDocument::new(
        "find_edges_sat8.tif",
        3,
        3,
        PixelPlane::Gray8(vec![0, 0, 0, 0, 0, 0, 255, 255, 255]),
    )
    .unwrap();
    let saturating_oracle = oracle_find_edges(&bridge, &saturating, "");
    let mut rust = saturating.clone();
    rust.find_edges().unwrap();
    assert_eq!(rust.plane(), saturating_oracle.plane());
    assert_eq!(
        rust.plane(),
        &PixelPlane::Gray8(vec![0, 0, 0, 255, 255, 255, 255, 255, 255])
    );
}

/// ShortProcessor keeps the square root in `double` and clamps at 65535
/// before the truncating `(short)` cast (ShortProcessor.java 709-714).
#[test]
fn parity_16bit_find_edges_truncation_and_saturation_match_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let truncating = ImageDocument::new(
        "find_edges_trunc16.tif",
        3,
        3,
        PixelPlane::Gray16(vec![1, 1, 0, 1, 0, 0, 0, 0, 0]),
    )
    .unwrap();
    let truncating_oracle = oracle_find_edges(&bridge, &truncating, "");
    let mut rust = truncating.clone();
    rust.find_edges().unwrap();
    assert_eq!(rust.plane(), truncating_oracle.plane());
    assert_eq!(
        rust.plane(),
        &PixelPlane::Gray16(vec![1, 4, 3, 4, 4, 1, 3, 1, 0])
    );

    let saturating = ImageDocument::new(
        "find_edges_sat16.tif",
        3,
        3,
        PixelPlane::Gray16(vec![0, 0, 0, 0, 0, 0, 65535, 65535, 65535]),
    )
    .unwrap();
    let saturating_oracle = oracle_find_edges(&bridge, &saturating, "");
    let mut rust = saturating.clone();
    rust.find_edges().unwrap();
    assert_eq!(rust.plane(), saturating_oracle.plane());
    assert_eq!(
        rust.plane(),
        &PixelPlane::Gray16(vec![0, 0, 0, 65535, 65535, 65535, 65535, 65535, 65535])
    );
}

/// FloatProcessor computes both Sobel sums and `sum1*sum1 + sum2*sum2` in
/// single precision, then takes a double square root and casts back to float
/// (FloatProcessor.java 639-644). The centre pixel is 2.5298223f, while a
/// float64 evaluation of the same inputs would give 2.5298221f.
#[test]
fn parity_32bit_find_edges_f32_sobel_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let doc = ImageDocument::new(
        "find_edges_f32.tif",
        3,
        3,
        PixelPlane::Gray32(vec![0.1f32, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9]),
    )
    .unwrap();
    let oracle = oracle_find_edges(&bridge, &doc, "");

    let mut rust = doc.clone();
    rust.find_edges().unwrap();
    assert_eq!(rust.plane(), oracle.plane());

    let center = match rust.plane() {
        PixelPlane::Gray32(pixels) => pixels[4],
        other => panic!("expected a float plane, got {other:?}"),
    };
    assert_eq!(center.to_bits(), 2.5298223f32.to_bits());
    assert_ne!(center.to_bits(), 2.5298221f32.to_bits());
}

#[test]
fn parity_find_edges_width_one_is_a_noop_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let docs = [
        find_edges_pattern_8bit(1, 4),
        find_edges_pattern_16bit(1, 4),
        find_edges_pattern_32bit(1, 4),
    ];

    for doc in docs {
        let oracle = oracle_find_edges(&bridge, &doc, "");
        let mut rust = doc.clone();
        rust.find_edges().unwrap();
        // ImageProcessor.findEdges() returns without filtering when width<=1.
        assert_eq!(rust.plane(), oracle.plane());
        assert_eq!(rust.plane(), doc.plane());
    }
}

#[test]
fn parity_find_edges_one_row_and_two_pixel_images_match_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let docs = [
        find_edges_pattern_8bit(5, 1),
        find_edges_pattern_16bit(5, 1),
        find_edges_pattern_32bit(5, 1),
        find_edges_pattern_8bit(2, 2),
        find_edges_pattern_16bit(2, 2),
        find_edges_pattern_32bit(2, 2),
        find_edges_pattern_8bit(2, 1),
        find_edges_pattern_16bit(2, 1),
        find_edges_pattern_32bit(2, 1),
    ];

    for doc in docs {
        let oracle = oracle_find_edges(&bridge, &doc, "");
        let mut rust = doc.clone();
        rust.find_edges().unwrap();
        // Rows/columns outside the 3x3 interior are filled by the clamped
        // filterEdge passes (8-bit) or by clamped sampling (16/32-bit).
        assert_eq!(rust.plane(), oracle.plane());
    }
}

#[test]
fn parity_8bit_find_edges_interior_rect_roi_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = find_edges_pattern_8bit(6, 4);
    doc.set_rect_roi(1, 0, 2, 2).unwrap();
    let oracle = oracle_find_edges(&bridge, &doc, "makeRectangle(1, 0, 2, 2);");

    let mut rust = doc.clone();
    rust.find_edges().unwrap();

    assert_ne!(doc.plane(), oracle.plane(), "oracle Find Edges was a no-op");
    assert_eq!(rust.plane(), oracle.plane());
}

/// A rectangle ending on `width-2`/`height-2` makes ByteProcessor.filter's
/// right/bottom edge passes rewrite the image's last column and row
/// (ByteProcessor.java 680-681), which lie outside the ROI and are never
/// restored because a rectangular ROI has no mask.
#[test]
fn parity_8bit_find_edges_rect_roi_leaks_last_column_and_row_like_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = find_edges_pattern_8bit(6, 4);
    doc.set_rect_roi(1, 1, 4, 2).unwrap();
    let oracle = oracle_find_edges(&bridge, &doc, "makeRectangle(1, 1, 4, 2);");

    let mut rust = doc.clone();
    rust.find_edges().unwrap();

    assert_eq!(rust.plane(), oracle.plane());
}

#[test]
fn parity_find_edges_clipped_rect_roi_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    for mut doc in [
        find_edges_pattern_8bit(6, 4),
        find_edges_pattern_16bit(6, 4),
        find_edges_pattern_32bit(6, 4),
    ] {
        doc.set_rect_roi(-2, 1, 6, 5).unwrap();
        let oracle = oracle_find_edges(&bridge, &doc, "makeRectangle(-2, 1, 6, 5);");
        let mut rust = doc.clone();
        rust.find_edges().unwrap();
        assert_eq!(rust.plane(), oracle.plane());
    }
}

#[test]
fn parity_8bit_find_edges_oval_roi_touching_image_edges_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = find_edges_pattern_8bit(6, 4);
    doc.set_oval_roi(0, 0, 6, 4).unwrap();
    let oracle = oracle_find_edges(&bridge, &doc, "makeOval(0, 0, 6, 4);");

    let mut rust = doc.clone();
    rust.find_edges().unwrap();

    assert_eq!(rust.plane(), oracle.plane());
    assert_ne!(doc.plane(), oracle.plane(), "oracle Find Edges was a no-op");
}

#[test]
fn parity_find_edges_clipped_oval_roi_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    for mut doc in [
        find_edges_pattern_8bit(6, 4),
        find_edges_pattern_16bit(6, 4),
        find_edges_pattern_32bit(6, 4),
    ] {
        doc.set_oval_roi(-2, -1, 5, 4).unwrap();
        let oracle = oracle_find_edges(&bridge, &doc, "makeOval(-2, -1, 5, 4);");
        let mut rust = doc.clone();
        rust.find_edges().unwrap();
        assert_eq!(rust.plane(), oracle.plane());
    }
}

#[test]
fn parity_8bit_find_edges_polygon_roi_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = find_edges_pattern_8bit(6, 4);
    doc.set_polygon_roi(&[(0, 0), (5, 1), (5, 3), (2, 3), (0, 2)])
        .unwrap();
    let oracle = oracle_find_edges(&bridge, &doc, "makePolygon(0, 0, 5, 1, 5, 3, 2, 3, 0, 2);");

    let mut rust = doc.clone();
    rust.find_edges().unwrap();

    assert_eq!(rust.plane(), oracle.plane());
    assert_ne!(doc.plane(), oracle.plane(), "oracle Find Edges was a no-op");
}

#[test]
fn parity_find_edges_outside_roi_preserves_pixels() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    for mut doc in [
        find_edges_pattern_8bit(6, 4),
        find_edges_pattern_16bit(6, 4),
        find_edges_pattern_32bit(6, 4),
    ] {
        doc.set_rect_roi(100, 100, 4, 4).unwrap();
        let oracle = oracle_find_edges(&bridge, &doc, "makeRectangle(100, 100, 4, 4);");
        let mut rust = doc.clone();
        rust.find_edges().unwrap();
        assert_eq!(rust.plane(), oracle.plane());
        assert_eq!(rust.plane(), doc.plane());
    }
}

#[test]
fn parity_find_edges_line_roi_resets_to_the_whole_image_like_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = find_edges_pattern_8bit(6, 4);
    doc.set_line_roi(1, 1, 4, 2);
    // Filters.setup marks a non-area selection as noRoi and Filters.run calls
    // ip.resetRoi(), so Find Edges processes the entire image (Filters.java
    // 24, 32-33).
    let oracle = oracle_find_edges(&bridge, &doc, "makeLine(1, 1, 4, 2);");
    let plain = find_edges_pattern_8bit(6, 4);
    assert_eq!(
        oracle.plane(),
        oracle_find_edges(&bridge, &plain, "").plane(),
        "line ROI should behave like no selection"
    );

    let mut rust = doc.clone();
    rust.find_edges().unwrap();
    assert_eq!(rust.plane(), oracle.plane());
}

#[test]
fn parity_find_edges_stack_slice_and_undo_match_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = distinct_stack_8bit(6, 4, 3);
    doc.set_slice(2).unwrap();
    let original = doc.slices.clone();

    let oracle = bridge
        .run_macro_on_image(&doc, "setSlice(2); run(\"Find Edges\", \"slice\");")
        .unwrap()
        .output_image
        .expect("oracle must return saved TIFF")
        .slices;
    let oracle_undo = bridge
        .run_macro_on_image(
            &doc,
            "setSlice(2); run(\"Find Edges\", \"slice\"); run(\"Undo\");",
        )
        .unwrap()
        .output_image
        .expect("oracle must return saved TIFF")
        .slices;

    let mut rust = doc.clone();
    rust.find_edges().unwrap();

    assert_eq!(rust.slices, oracle);
    assert_eq!(rust.slices[0], original[0]);
    assert_ne!(rust.slices[1], original[1]);
    assert_eq!(rust.slices[2], original[2]);

    assert!(rust.undo(), "slice Find Edges must leave an Undo snapshot");
    assert_eq!(rust.slices, oracle_undo);
    assert_eq!(rust.slices, original);
}

#[test]
fn find_edges_rejects_unsupported_pixel_types_instead_of_ignoring_them() {
    let indexed = ImageDocument::new(
        "indexed8.tif",
        4,
        4,
        PixelPlane::Color256 {
            pixels: vec![3u8; 16],
            lut: Box::new(ij_core::Lut256::default()),
        },
    )
    .unwrap();
    let error = indexed.clone().find_edges().unwrap_err();
    assert!(
        error.to_string().contains("8-bit color") && error.to_string().contains("Find Edges"),
        "expected an explicit unsupported-type error, got: {error}"
    );

    let rgb =
        ImageDocument::new("rgb.tif", 4, 4, PixelPlane::ColorRgb(vec![0xff80_4020; 16])).unwrap();
    let error = rgb.clone().find_edges().unwrap_err();
    assert!(
        error.to_string().contains("RGB") && error.to_string().contains("Find Edges"),
        "expected an explicit unsupported-type error, got: {error}"
    );

    // Rejected types must not consume or install an Undo snapshot.
    assert!(!rgb.clone().undo());
}

#[test]
fn session_find_edges_records_macro_and_updates_pixels() {
    let path = temp_tiff("session_find_edges.tif");
    let original = find_edges_pattern_8bit(6, 4);
    save_tiff(&path, &original).unwrap();

    let mut session = Session::new();
    session.dispatch(Command::OpenTiff { path }).unwrap();
    let mut expected = original.clone();
    expected.find_edges().unwrap();
    session.dispatch(Command::FindEdges).unwrap();
    assert_eq!(session.active_doc().unwrap().plane(), expected.plane());
    assert_ne!(expected.plane(), original.plane());
    assert_eq!(
        session.macro_recorder.last().map(String::as_str),
        Some("run(\"Find Edges\");")
    );

    // A stack answer to "Process Stack?" is recorded as the `slice` option.
    let path = temp_tiff("session_find_edges_stack.tif");
    save_tiff(&path, &distinct_stack_8bit(6, 4, 3)).unwrap();
    let mut stack_session = Session::new();
    stack_session.dispatch(Command::OpenTiff { path }).unwrap();
    stack_session
        .dispatch(Command::SetSlice { index: 2 })
        .unwrap();
    stack_session.dispatch(Command::FindEdges).unwrap();
    assert_eq!(
        stack_session.macro_recorder.last().map(String::as_str),
        Some("run(\"Find Edges\", \"slice\");")
    );

    let mut ok_session = Session::new();
    ok_session
        .dispatch(Command::NewRamp8Bit {
            title: "ok".into(),
            width: 4,
            height: 4,
        })
        .unwrap();
    assert!(ok_session.dispatch(Command::FindEdges).is_ok());
}

// ---------------------------------------------------------------------------
// Unit 5 (fourth increment): Gaussian Blur for Gray8, Gray16 and Gray32.
//
// The plugin converts the processor to float, blurs each direction with a
// normalized 1-D Gaussian kernel, and converts back. ByteProcessor and
// ShortProcessor add 0.5f and truncate on the way back; FloatProcessor blurs
// in place. For sigma > 8.5 each line is downscaled, convolved and upscaled
// (GaussianBlur.java 245-271). ROI bounds come from ImageProcessor.getRoi();
// SUPPORTS_MASKING restores pixels outside an irregular mask and
// resetOutOfRoi restores the extra x-direction lines above/below the ROI
// rectangle (GaussianBlur.java 190-192, PlugInFilterRunner.java 268-272).
// Pixels are exchanged through TIFF, so the oracle starts from exactly the
// Rust pixel buffer.
// ---------------------------------------------------------------------------

fn gaussian_pattern_8bit(width: u32, height: u32) -> ImageDocument {
    let mut pixels = vec![0u8; (width * height) as usize];
    for y in 0..height {
        for x in 0..width {
            pixels[(y * width + x) as usize] = ((x * 43 + y * 71 + 5) % 256) as u8;
        }
    }
    ImageDocument::new("gaussian8.tif", width, height, PixelPlane::Gray8(pixels)).unwrap()
}

fn gaussian_pattern_16bit(width: u32, height: u32) -> ImageDocument {
    let mut pixels = vec![0u16; (width * height) as usize];
    for y in 0..height {
        for x in 0..width {
            pixels[(y * width + x) as usize] = ((x * 3001 + y * 5171 + 13) % 65536) as u16;
        }
    }
    ImageDocument::new("gaussian16.tif", width, height, PixelPlane::Gray16(pixels)).unwrap()
}

/// Fractional values whose single-precision accumulation order differs from a
/// float64 evaluation.
fn gaussian_pattern_32bit(width: u32, height: u32) -> ImageDocument {
    let mut pixels = Vec::with_capacity((width * height) as usize);
    for y in 0..height {
        for x in 0..width {
            let i = (y * width + x) as f32;
            pixels.push(i * 0.173 + 0.0007 * x as f32 - 1.3);
        }
    }
    ImageDocument::new("gaussian32.tif", width, height, PixelPlane::Gray32(pixels)).unwrap()
}

fn oracle_gaussian_blur(
    bridge: &JavaOracleBridge,
    doc: &ImageDocument,
    sigma: f64,
    prelude: &str,
) -> ImageDocument {
    let out = bridge
        .run_macro_on_image(
            doc,
            &format!("{prelude} run(\"Gaussian Blur...\", \"sigma={sigma}\");"),
        )
        .unwrap();
    out.output_image.expect("oracle must return saved TIFF")
}

#[test]
fn parity_8bit_gaussian_blur_full_image_sigma1_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let doc = gaussian_pattern_8bit(8, 6);
    let oracle = oracle_gaussian_blur(&bridge, &doc, 1.0, "");

    let mut rust = doc.clone();
    rust.gaussian_blur(1.0).unwrap();

    assert_ne!(doc.plane(), oracle.plane(), "oracle blur was a no-op");
    assert_eq!(rust.plane(), oracle.plane());
}

#[test]
fn parity_16bit_gaussian_blur_full_image_sigma1_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let doc = gaussian_pattern_16bit(8, 6);
    let oracle = oracle_gaussian_blur(&bridge, &doc, 1.0, "");

    let mut rust = doc.clone();
    rust.gaussian_blur(1.0).unwrap();

    assert_ne!(doc.plane(), oracle.plane(), "oracle blur was a no-op");
    assert_eq!(rust.plane(), oracle.plane());
}

#[test]
fn parity_32bit_gaussian_blur_full_image_sigma1_matches_java_oracle_f32() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let doc = gaussian_pattern_32bit(8, 6);
    let oracle = oracle_gaussian_blur(&bridge, &doc, 1.0, "");

    let mut rust = doc.clone();
    rust.gaussian_blur(1.0).unwrap();

    assert_ne!(doc.plane(), oracle.plane(), "oracle blur was a no-op");
    assert_eq!(rust.plane(), oracle.plane());
}

#[test]
fn parity_gaussian_blur_sigma_zero_is_identity_for_gray_types() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    for doc in [
        gaussian_pattern_8bit(6, 4),
        gaussian_pattern_16bit(6, 4),
        gaussian_pattern_32bit(6, 4),
    ] {
        let oracle = oracle_gaussian_blur(&bridge, &doc, 0.0, "");
        let mut rust = doc.clone();
        rust.gaussian_blur(0.0).unwrap();
        // blurFloat skips both directions when sigma is not positive.
        assert_eq!(rust.plane(), oracle.plane());
        assert_eq!(rust.plane(), doc.plane());
    }
}

#[test]
fn parity_gaussian_blur_sigma_8_5_does_not_downscale() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    // doDownscaling is `sigma > 2*4 + 0.5`, so 8.5 must use the plain path.
    for doc in [
        gaussian_pattern_8bit(8, 6),
        gaussian_pattern_16bit(8, 6),
        gaussian_pattern_32bit(8, 6),
    ] {
        let oracle = oracle_gaussian_blur(&bridge, &doc, 8.5, "");
        let mut rust = doc.clone();
        rust.gaussian_blur(8.5).unwrap();
        assert_ne!(doc.plane(), oracle.plane(), "oracle blur was a no-op");
        assert_eq!(rust.plane(), oracle.plane());
    }
}

#[test]
fn parity_gaussian_blur_sigma_above_8_5_downscales_lines() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    for doc in [
        gaussian_pattern_8bit(9, 7),
        gaussian_pattern_16bit(9, 7),
        gaussian_pattern_32bit(9, 7),
    ] {
        let oracle = oracle_gaussian_blur(&bridge, &doc, 12.0, "");
        let mut rust = doc.clone();
        rust.gaussian_blur(12.0).unwrap();
        assert_ne!(doc.plane(), oracle.plane(), "oracle blur was a no-op");
        assert_eq!(rust.plane(), oracle.plane());
    }
}

#[test]
fn parity_gaussian_blur_huge_sigma_clamps_reduce_by_to_line_length() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    // floor(40/4)=10, but reduceBy is min(10, length); the x and y passes see
    // different line lengths and therefore different downscale factors.
    for doc in [
        gaussian_pattern_8bit(6, 4),
        gaussian_pattern_16bit(6, 4),
        gaussian_pattern_32bit(6, 4),
    ] {
        let oracle = oracle_gaussian_blur(&bridge, &doc, 40.0, "");
        let mut rust = doc.clone();
        rust.gaussian_blur(40.0).unwrap();
        assert_eq!(rust.plane(), oracle.plane());
    }
}

#[test]
fn parity_8bit_gaussian_blur_interior_rect_roi_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = gaussian_pattern_8bit(8, 6);
    doc.set_rect_roi(2, 1, 4, 3).unwrap();
    let oracle = oracle_gaussian_blur(&bridge, &doc, 2.0, "makeRectangle(2, 1, 4, 3);");

    let mut rust = doc.clone();
    rust.gaussian_blur(2.0).unwrap();

    assert_ne!(doc.plane(), oracle.plane(), "oracle blur was a no-op");
    assert_eq!(rust.plane(), oracle.plane());
    // The x pass may touch extra lines but resetOutOfRoi restores them.
    assert_eq!(
        match rust.plane() {
            PixelPlane::Gray8(p) => p[0],
            _ => unreachable!(),
        },
        match doc.plane() {
            PixelPlane::Gray8(p) => p[0],
            _ => unreachable!(),
        }
    );
}

#[test]
fn parity_16bit_gaussian_blur_clipped_rect_roi_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = gaussian_pattern_16bit(8, 6);
    doc.set_rect_roi(-2, 1, 8, 7).unwrap();
    let oracle = oracle_gaussian_blur(&bridge, &doc, 2.0, "makeRectangle(-2, 1, 8, 7);");

    let mut rust = doc.clone();
    rust.gaussian_blur(2.0).unwrap();

    assert_eq!(rust.plane(), oracle.plane());
}

#[test]
fn parity_8bit_gaussian_blur_oval_roi_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = gaussian_pattern_8bit(8, 6);
    doc.set_oval_roi(1, 1, 5, 4).unwrap();
    let oracle = oracle_gaussian_blur(&bridge, &doc, 2.0, "makeOval(1, 1, 5, 4);");

    let mut rust = doc.clone();
    rust.gaussian_blur(2.0).unwrap();

    assert_ne!(doc.plane(), oracle.plane(), "oracle blur was a no-op");
    assert_eq!(rust.plane(), oracle.plane());
}

#[test]
fn parity_32bit_gaussian_blur_clipped_oval_roi_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = gaussian_pattern_32bit(8, 6);
    doc.set_oval_roi(-2, -1, 6, 5).unwrap();
    let oracle = oracle_gaussian_blur(&bridge, &doc, 2.0, "makeOval(-2, -1, 6, 5);");

    let mut rust = doc.clone();
    rust.gaussian_blur(2.0).unwrap();

    assert_eq!(rust.plane(), oracle.plane());
}

#[test]
fn parity_8bit_gaussian_blur_polygon_roi_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = gaussian_pattern_8bit(8, 6);
    doc.set_polygon_roi(&[(1, 1), (6, 1), (6, 4), (1, 4)])
        .unwrap();
    let oracle = oracle_gaussian_blur(&bridge, &doc, 2.0, "makePolygon(1, 1, 6, 1, 6, 4, 1, 4);");

    let mut rust = doc.clone();
    rust.gaussian_blur(2.0).unwrap();

    assert_ne!(doc.plane(), oracle.plane(), "oracle blur was a no-op");
    assert_eq!(rust.plane(), oracle.plane());
}

/// sigma = 0.2 makes `ceil(5*sigma)` (1) smaller than the kernel radius (2),
/// so the y pass reads an unblurred line outside the x pass's extra lines.
#[test]
fn parity_8bit_gaussian_blur_small_sigma_roi_extra_line_limit_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = gaussian_pattern_8bit(8, 6);
    doc.set_rect_roi(2, 2, 4, 2).unwrap();
    let oracle = oracle_gaussian_blur(&bridge, &doc, 0.2, "makeRectangle(2, 2, 4, 2);");

    let mut rust = doc.clone();
    rust.gaussian_blur(0.2).unwrap();

    assert_eq!(rust.plane(), oracle.plane());
}

/// A ROI that lies entirely outside the image becomes a 0x0 processor ROI.
/// `blur1Direction` still processes `ceil(5*sigma)` lines and `convolveLine`'s
/// first loop runs to `min(kRadius, length)` instead of stopping at `writeTo`,
/// so the x pass blurs every row while the y pass has no lines. `resetOutOfRoi`
/// restores nothing because the ROI width is zero. This is a genuine ImageJ
/// 1.54u quirk, not a rounding difference.
#[test]
fn parity_gaussian_blur_outside_roi_still_blurs_x_direction_like_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    for mut doc in [
        gaussian_pattern_8bit(8, 6),
        gaussian_pattern_16bit(8, 6),
        gaussian_pattern_32bit(8, 6),
    ] {
        doc.set_rect_roi(100, 100, 4, 4).unwrap();
        let oracle = oracle_gaussian_blur(&bridge, &doc, 2.0, "makeRectangle(100, 100, 4, 4);");
        let mut rust = doc.clone();
        rust.gaussian_blur(2.0).unwrap();
        assert_eq!(rust.plane(), oracle.plane());
        assert_ne!(rust.plane(), doc.plane(), "oracle x-only blur was a no-op");
    }
}

#[test]
fn parity_gaussian_blur_stack_slice_and_undo_match_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = distinct_stack_8bit(7, 5, 3);
    doc.set_slice(2).unwrap();
    let original = doc.slices.clone();

    let oracle = bridge
        .run_macro_on_image(
            &doc,
            "setSlice(2); run(\"Gaussian Blur...\", \"sigma=1.5 slice\");",
        )
        .unwrap()
        .output_image
        .expect("oracle must return saved TIFF")
        .slices;
    let oracle_undo = bridge
        .run_macro_on_image(
            &doc,
            "setSlice(2); run(\"Gaussian Blur...\", \"sigma=1.5 slice\"); run(\"Undo\");",
        )
        .unwrap()
        .output_image
        .expect("oracle must return saved TIFF")
        .slices;

    let mut rust = doc.clone();
    rust.gaussian_blur(1.5).unwrap();

    assert_eq!(rust.slices, oracle);
    assert_eq!(rust.slices[0], original[0]);
    assert_ne!(rust.slices[1], original[1]);
    assert_eq!(rust.slices[2], original[2]);

    assert!(
        rust.undo(),
        "slice Gaussian Blur must leave an Undo snapshot"
    );
    assert_eq!(rust.slices, oracle_undo);
    assert_eq!(rust.slices, original);
}

#[test]
fn session_gaussian_blur_records_macro_and_updates_pixels() {
    let path = temp_tiff("session_gaussian.tif");
    let original = gaussian_pattern_8bit(8, 6);
    save_tiff(&path, &original).unwrap();

    let mut session = Session::new();
    session.dispatch(Command::OpenTiff { path }).unwrap();
    let mut expected = original.clone();
    expected.gaussian_blur(2.0).unwrap();
    session
        .dispatch(Command::GaussianBlur { sigma: 2.0 })
        .unwrap();
    assert_eq!(session.active_doc().unwrap().plane(), expected.plane());
    assert_ne!(expected.plane(), original.plane());
    assert_eq!(
        session.macro_recorder.last().map(String::as_str),
        Some("run(\"Gaussian Blur...\", \"sigma=2\");")
    );

    // A stack answer to "Process Stack?" is recorded as the `slice` option.
    let path = temp_tiff("session_gaussian_stack.tif");
    save_tiff(&path, &distinct_stack_8bit(7, 5, 3)).unwrap();
    let mut stack_session = Session::new();
    stack_session.dispatch(Command::OpenTiff { path }).unwrap();
    stack_session
        .dispatch(Command::SetSlice { index: 2 })
        .unwrap();
    stack_session
        .dispatch(Command::GaussianBlur { sigma: 8.5 })
        .unwrap();
    assert_eq!(
        stack_session.macro_recorder.last().map(String::as_str),
        Some("run(\"Gaussian Blur...\", \"sigma=8.5 slice\");")
    );
}

#[test]
fn gaussian_blur_rejects_unsupported_pixel_types_instead_of_ignoring_them() {
    let indexed = ImageDocument::new(
        "indexed8.tif",
        4,
        4,
        PixelPlane::Color256 {
            pixels: vec![3u8; 16],
            lut: Box::new(ij_core::Lut256::default()),
        },
    )
    .unwrap();
    let error = indexed.clone().gaussian_blur(1.0).unwrap_err();
    assert!(
        error.to_string().contains("8-bit color") && error.to_string().contains("Gaussian Blur"),
        "expected an explicit unsupported-type error, got: {error}"
    );

    let rgb =
        ImageDocument::new("rgb.tif", 4, 4, PixelPlane::ColorRgb(vec![0xff80_4020; 16])).unwrap();
    let error = rgb.clone().gaussian_blur(1.0).unwrap_err();
    assert!(
        error.to_string().contains("RGB") && error.to_string().contains("Gaussian Blur"),
        "expected an explicit unsupported-type error, got: {error}"
    );

    // Rejected types must not consume or install an Undo snapshot.
    assert!(!rgb.clone().undo());
}

#[test]
fn parity_8bit_gaussian_blur_downscaling_with_oval_roi_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = gaussian_pattern_8bit(12, 9);
    doc.set_oval_roi(2, 1, 7, 6).unwrap();
    let oracle = oracle_gaussian_blur(&bridge, &doc, 12.0, "makeOval(2, 1, 7, 6);");

    let mut rust = doc.clone();
    rust.gaussian_blur(12.0).unwrap();

    assert_ne!(doc.plane(), oracle.plane(), "oracle blur was a no-op");
    assert_eq!(rust.plane(), oracle.plane());
}

#[test]
fn parity_16bit_gaussian_blur_downscaling_with_interior_rect_roi_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = gaussian_pattern_16bit(12, 9);
    doc.set_rect_roi(3, 2, 6, 4).unwrap();
    let oracle = oracle_gaussian_blur(&bridge, &doc, 12.0, "makeRectangle(3, 2, 6, 4);");

    let mut rust = doc.clone();
    rust.gaussian_blur(12.0).unwrap();

    assert_eq!(rust.plane(), oracle.plane());
}

/// With downscaling the x pass writes only through `upscaleLine`, whose loop
/// is `writeFrom..writeTo`, so an entirely-outside ROI is a genuine no-op
/// (unlike the non-downscaled quirk).
#[test]
fn parity_gaussian_blur_outside_roi_with_downscaling_is_a_noop_like_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    for mut doc in [
        gaussian_pattern_8bit(9, 7),
        gaussian_pattern_16bit(9, 7),
        gaussian_pattern_32bit(9, 7),
    ] {
        doc.set_rect_roi(100, 100, 4, 4).unwrap();
        let oracle = oracle_gaussian_blur(&bridge, &doc, 12.0, "makeRectangle(100, 100, 4, 4);");
        let mut rust = doc.clone();
        rust.gaussian_blur(12.0).unwrap();
        assert_eq!(rust.plane(), oracle.plane());
        assert_eq!(rust.plane(), doc.plane());
    }
}

#[test]
fn parity_gaussian_blur_single_pixel_and_single_row_match_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    for doc in [
        gaussian_pattern_8bit(1, 1),
        gaussian_pattern_16bit(1, 1),
        gaussian_pattern_32bit(1, 1),
        gaussian_pattern_8bit(1, 5),
        gaussian_pattern_16bit(1, 5),
        gaussian_pattern_32bit(1, 5),
        gaussian_pattern_8bit(5, 1),
        gaussian_pattern_16bit(5, 1),
        gaussian_pattern_32bit(5, 1),
        gaussian_pattern_8bit(2, 2),
        gaussian_pattern_16bit(2, 2),
        gaussian_pattern_32bit(2, 2),
    ] {
        let oracle = oracle_gaussian_blur(&bridge, &doc, 1.0, "");
        let mut rust = doc.clone();
        rust.gaussian_blur(1.0).unwrap();
        assert_eq!(rust.plane(), oracle.plane());
    }
}

#[test]
fn gaussian_blur_rejects_extreme_sigma_instead_of_overflowing() {
    let mut doc = gaussian_pattern_8bit(4, 4);
    let error = doc.gaussian_blur(1.0e9).unwrap_err();
    assert!(
        error.to_string().contains("Gaussian Blur") && error.to_string().contains("sigma"),
        "expected an explicit sigma error, got: {error}"
    );
    assert!(
        !doc.undo(),
        "rejected sigma must not install an Undo snapshot"
    );
}

#[test]
fn parity_16bit_gaussian_blur_oval_roi_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = gaussian_pattern_16bit(8, 6);
    doc.set_oval_roi(1, 1, 5, 4).unwrap();
    let oracle = oracle_gaussian_blur(&bridge, &doc, 1.5, "makeOval(1, 1, 5, 4);");

    let mut rust = doc.clone();
    rust.gaussian_blur(1.5).unwrap();

    assert_ne!(doc.plane(), oracle.plane(), "oracle blur was a no-op");
    assert_eq!(rust.plane(), oracle.plane());
}

#[test]
fn parity_32bit_gaussian_blur_interior_rect_roi_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = gaussian_pattern_32bit(8, 6);
    doc.set_rect_roi(2, 1, 4, 3).unwrap();
    let oracle = oracle_gaussian_blur(&bridge, &doc, 3.0, "makeRectangle(2, 1, 4, 3);");

    let mut rust = doc.clone();
    rust.gaussian_blur(3.0).unwrap();

    assert_eq!(rust.plane(), oracle.plane());
}

// ---------------------------------------------------------------------------
// Unit 5 (fifth increment): AutoThresholder.Otsu and AutoThresholder.IsoData
// for 8-bit grayscale images.
//
// ImageProcessor.setAutoThreshold(String) (original_imagej/ij/process/
// ImageProcessor.java lines 594-675) builds a 256-bin ROI histogram, calls
// AutoThresholder.getThreshold (AutoThresholder.java lines 43-88) and stores
// (minThreshold, maxThreshold) with ByteProcessor's clamping
// (ByteProcessor.java lines 480-486). Pixels are never modified, so the oracle
// output TIFF must be byte-identical to the input.
// ---------------------------------------------------------------------------

fn pattern_8bit(title: &str, width: u32, height: u32, values: &[u8]) -> ImageDocument {
    assert_eq!(values.len(), (width * height) as usize);
    ImageDocument::new(title, width, height, PixelPlane::Gray8(values.to_vec())).unwrap()
}

fn uniform_8bit(title: &str, width: u32, height: u32, value: u8) -> ImageDocument {
    pattern_8bit(
        title,
        width,
        height,
        &vec![value; (width * height) as usize],
    )
}

fn oracle_auto_threshold(
    bridge: &JavaOracleBridge,
    doc: &ImageDocument,
    prelude: &str,
    method_and_options: &str,
) -> (f64, f64) {
    let out = bridge
        .run_macro_on_image(
            doc,
            &format!(
                "{prelude} setAutoThreshold(\"{method_and_options}\"); getThreshold(l1,l2); \
                 print(\"THRESH \"+d2s(l1,6)+\" \"+d2s(l2,6));"
            ),
        )
        .unwrap();
    let line = out
        .stdout
        .lines()
        .find(|line| line.trim_start().starts_with("THRESH "))
        .unwrap_or_else(|| panic!("oracle printed no THRESH line:\n{}", out.stdout));
    let mut tokens = line.trim().split_whitespace();
    tokens.next();
    (
        tokens.next().expect("lower threshold").parse().unwrap(),
        tokens.next().expect("upper threshold").parse().unwrap(),
    )
}

fn assert_auto_threshold_matches_oracle(
    bridge: &JavaOracleBridge,
    doc: &ImageDocument,
    prelude: &str,
    method_and_options: &str,
    method: ij_core::AutoThresholdMethod,
    dark_background: bool,
) {
    let oracle = oracle_auto_threshold(bridge, doc, prelude, method_and_options);
    let mut rust = doc.clone();
    let levels = rust.set_auto_threshold(method, dark_background).unwrap();
    assert_eq!(
        (levels.lower, levels.upper),
        oracle,
        "threshold differs for {method_and_options:?} (prelude {prelude:?})"
    );
    assert_eq!(
        rust.plane(),
        doc.plane(),
        "thresholding must not touch pixels"
    );
}

#[test]
fn parity_8bit_otsu_ramp_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let doc = ImageDocument::new_ramp_8bit("ramp.tif", 16, 16).unwrap();
    assert_auto_threshold_matches_oracle(
        &bridge,
        &doc,
        "",
        "Otsu",
        ij_core::AutoThresholdMethod::Otsu,
        false,
    );
}

#[test]
fn parity_8bit_isodata_ramp_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let doc = ImageDocument::new_ramp_8bit("ramp.tif", 16, 16).unwrap();
    assert_auto_threshold_matches_oracle(
        &bridge,
        &doc,
        "",
        "IsoData",
        ij_core::AutoThresholdMethod::IsoData,
        false,
    );
}

#[test]
fn parity_8bit_otsu_dark_background_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let doc = ImageDocument::new_ramp_8bit("ramp.tif", 16, 16).unwrap();
    assert_auto_threshold_matches_oracle(
        &bridge,
        &doc,
        "",
        "Otsu dark",
        ij_core::AutoThresholdMethod::Otsu,
        true,
    );
}

#[test]
fn parity_8bit_isodata_dark_background_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let doc = ImageDocument::new_ramp_8bit("ramp.tif", 16, 16).unwrap();
    assert_auto_threshold_matches_oracle(
        &bridge,
        &doc,
        "",
        "IsoData dark",
        ij_core::AutoThresholdMethod::IsoData,
        true,
    );
}

#[test]
fn parity_8bit_otsu_equal_count_three_bin_tie_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    // Three equal bins make the between-class variance exactly equal at
    // thresholds 10 and 20; Otsu's strict `max_bcv < bcv` keeps the first.
    let doc = pattern_8bit("tie.tif", 3, 1, &[10, 20, 30]);
    assert_auto_threshold_matches_oracle(
        &bridge,
        &doc,
        "",
        "Otsu",
        ij_core::AutoThresholdMethod::Otsu,
        false,
    );
    assert_eq!(doc.plane(), &PixelPlane::Gray8(vec![10, 20, 30]));
}

#[test]
fn parity_8bit_isodata_three_bin_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let doc = pattern_8bit("three.tif", 3, 1, &[10, 20, 30]);
    assert_auto_threshold_matches_oracle(
        &bridge,
        &doc,
        "",
        "IsoData",
        ij_core::AutoThresholdMethod::IsoData,
        false,
    );
}

#[test]
fn parity_8bit_bilevel_single_value_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let doc = uniform_8bit("uniform.tif", 4, 4, 100);
    // bilevel returns firstNonZero-1 = 99 (AutoThresholder.java 181-184).
    assert_auto_threshold_matches_oracle(
        &bridge,
        &doc,
        "",
        "Otsu",
        ij_core::AutoThresholdMethod::Otsu,
        false,
    );
    assert_auto_threshold_matches_oracle(
        &bridge,
        &doc,
        "",
        "Otsu dark",
        ij_core::AutoThresholdMethod::Otsu,
        true,
    );
    assert_auto_threshold_matches_oracle(
        &bridge,
        &doc,
        "",
        "IsoData",
        ij_core::AutoThresholdMethod::IsoData,
        false,
    );
}

#[test]
fn parity_8bit_bilevel_two_values_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut values = vec![10u8; 16];
    values[0] = 20;
    let doc = pattern_8bit("two.tif", 4, 4, &values);
    assert_auto_threshold_matches_oracle(
        &bridge,
        &doc,
        "",
        "Otsu",
        ij_core::AutoThresholdMethod::Otsu,
        false,
    );
    assert_auto_threshold_matches_oracle(
        &bridge,
        &doc,
        "",
        "Otsu dark",
        ij_core::AutoThresholdMethod::Otsu,
        true,
    );
}

#[test]
fn parity_8bit_otsu_all_zero_histogram_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    // All-zero pixels are a one-bin histogram (bin 0), so bilevel fails and
    // Otsu returns Integer.MIN_VALUE, stored verbatim by ByteProcessor.
    let doc = uniform_8bit("zero.tif", 4, 4, 0);
    let oracle = oracle_auto_threshold(&bridge, &doc, "", "Otsu");
    assert_eq!(oracle, (0.0, -2147483648.0));
    assert_auto_threshold_matches_oracle(
        &bridge,
        &doc,
        "",
        "Otsu",
        ij_core::AutoThresholdMethod::Otsu,
        false,
    );
    // Dark background computes MIN_VALUE+1, then ByteProcessor clamps to 0.
    assert_auto_threshold_matches_oracle(
        &bridge,
        &doc,
        "",
        "Otsu dark",
        ij_core::AutoThresholdMethod::Otsu,
        true,
    );
}

#[test]
fn parity_8bit_isodata_all_zero_histogram_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let doc = uniform_8bit("zero.tif", 4, 4, 0);
    // IsoData never finds a fixed point and returns -1, which getThreshold
    // maps to 0; dark background then starts at 1.
    assert_auto_threshold_matches_oracle(
        &bridge,
        &doc,
        "",
        "IsoData",
        ij_core::AutoThresholdMethod::IsoData,
        false,
    );
    assert_auto_threshold_matches_oracle(
        &bridge,
        &doc,
        "",
        "IsoData dark",
        ij_core::AutoThresholdMethod::IsoData,
        true,
    );
}

#[test]
fn parity_8bit_otsu_broad_histogram_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let doc = smooth_pattern_8bit(8, 6);
    assert_auto_threshold_matches_oracle(
        &bridge,
        &doc,
        "",
        "Otsu",
        ij_core::AutoThresholdMethod::Otsu,
        false,
    );
}

#[test]
fn parity_8bit_isodata_broad_histogram_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let doc = smooth_pattern_8bit(8, 6);
    assert_auto_threshold_matches_oracle(
        &bridge,
        &doc,
        "",
        "IsoData",
        ij_core::AutoThresholdMethod::IsoData,
        false,
    );
}

#[test]
fn parity_8bit_auto_threshold_rect_roi_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = ImageDocument::new_ramp_8bit("ramp.tif", 16, 16).unwrap();
    doc.set_rect_roi(2, 3, 8, 6).unwrap();
    assert_auto_threshold_matches_oracle(
        &bridge,
        &doc,
        "makeRectangle(2, 3, 8, 6);",
        "Otsu",
        ij_core::AutoThresholdMethod::Otsu,
        false,
    );
}

#[test]
fn parity_8bit_auto_threshold_clipped_rect_roi_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = ImageDocument::new_ramp_8bit("ramp.tif", 16, 16).unwrap();
    doc.set_rect_roi(-4, -4, 24, 6).unwrap();
    assert_auto_threshold_matches_oracle(
        &bridge,
        &doc,
        "makeRectangle(-4, -4, 24, 6);",
        "Otsu",
        ij_core::AutoThresholdMethod::Otsu,
        false,
    );
}

#[test]
fn parity_8bit_auto_threshold_outside_roi_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = ImageDocument::new_ramp_8bit("ramp.tif", 16, 16).unwrap();
    doc.set_rect_roi(-20, -20, 5, 5).unwrap();
    // A fully clipped ROI is 0x0, so the histogram is all zeros.
    assert_auto_threshold_matches_oracle(
        &bridge,
        &doc,
        "makeRectangle(-20, -20, 5, 5);",
        "Otsu",
        ij_core::AutoThresholdMethod::Otsu,
        false,
    );
}

#[test]
fn parity_8bit_auto_threshold_oval_roi_mask_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = smooth_pattern_8bit(8, 6);
    doc.set_oval_roi(1, 1, 5, 4).unwrap();
    assert_auto_threshold_matches_oracle(
        &bridge,
        &doc,
        "makeOval(1, 1, 5, 4);",
        "Otsu",
        ij_core::AutoThresholdMethod::Otsu,
        false,
    );
    assert_auto_threshold_matches_oracle(
        &bridge,
        &doc,
        "makeOval(1, 1, 5, 4);",
        "IsoData",
        ij_core::AutoThresholdMethod::IsoData,
        false,
    );
}

#[test]
fn parity_8bit_auto_threshold_polygon_roi_mask_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = smooth_pattern_8bit(8, 6);
    doc.set_polygon_roi(&[(1, 1), (6, 2), (5, 5), (2, 6)])
        .unwrap();
    assert_auto_threshold_matches_oracle(
        &bridge,
        &doc,
        "makePolygon(1, 1, 6, 2, 5, 5, 2, 6);",
        "Otsu",
        ij_core::AutoThresholdMethod::Otsu,
        false,
    );
}

#[test]
fn parity_8bit_auto_threshold_line_roi_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = ImageDocument::new_ramp_8bit("ramp.tif", 16, 16).unwrap();
    doc.set_line_roi(1, 2, 9, 7);
    // Line selections have no mask; Roi.setIntBounds makes the processed
    // rectangle 9x6, not 8x5.
    assert_auto_threshold_matches_oracle(
        &bridge,
        &doc,
        "makeLine(1, 2, 9, 7);",
        "Otsu",
        ij_core::AutoThresholdMethod::Otsu,
        false,
    );
}

#[test]
fn parity_8bit_auto_threshold_vertical_line_roi_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = ImageDocument::new_ramp_8bit("ramp.tif", 16, 16).unwrap();
    doc.set_line_roi(5, 1, 5, 9);
    assert_auto_threshold_matches_oracle(
        &bridge,
        &doc,
        "makeLine(5, 1, 5, 9);",
        "Otsu",
        ij_core::AutoThresholdMethod::Otsu,
        false,
    );
}

#[test]
fn parity_8bit_auto_threshold_leaves_pixels_untouched_in_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let doc = smooth_pattern_8bit(8, 6);
    let out = bridge
        .run_macro_on_image(&doc, "setAutoThreshold(\"Otsu\");")
        .unwrap();
    let oracle_doc = out.output_image.expect("oracle must return saved TIFF");
    assert_eq!(oracle_doc.plane(), doc.plane());

    let mut rust = doc.clone();
    rust.set_auto_threshold(ij_core::AutoThresholdMethod::Otsu, false)
        .unwrap();
    assert_eq!(rust.plane(), doc.plane());
}

#[test]
fn session_auto_threshold_records_macro_and_sets_state() {
    let mut session = Session::new();
    session
        .dispatch(Command::NewRamp8Bit {
            title: "ramp".into(),
            width: 16,
            height: 16,
        })
        .unwrap();

    let outcome = session
        .dispatch(Command::AutoThreshold {
            method: "Otsu".into(),
            dark_background: false,
            histogram_16: false,
        })
        .unwrap();
    match outcome {
        ij_session::CommandOutcome::Thresholded(levels) => {
            assert_eq!((levels.lower, levels.upper), (0.0, 112.0));
        }
        other => panic!("expected Thresholded outcome, got {other:?}"),
    }
    assert_eq!(
        session.macro_recorder.last().map(String::as_str),
        Some("setAutoThreshold(\"Otsu\");")
    );

    session
        .dispatch(Command::AutoThreshold {
            method: "IsoData".into(),
            dark_background: true,
            histogram_16: false,
        })
        .unwrap();
    assert_eq!(
        session.macro_recorder.last().map(String::as_str),
        Some("setAutoThreshold(\"IsoData dark\");")
    );
    assert_eq!(
        session.active_doc().unwrap().threshold,
        Some(ThresholdLevels {
            lower: 121.0,
            upper: 255.0
        })
    );
}

#[test]
fn auto_threshold_rejects_unimplemented_methods() {
    let mut session = Session::new();
    session
        .dispatch(Command::NewRamp8Bit {
            title: "ramp".into(),
            width: 16,
            height: 16,
        })
        .unwrap();
    for method in [
        "Huang",
        "Default",
        "IJ_IsoData",
        "Yen",
        "otsu",
        "",
        "Otsu dark",
        "Otsu 16",
    ] {
        let error = session
            .dispatch(Command::AutoThreshold {
                method: method.into(),
                dark_background: false,
                histogram_16: false,
            })
            .unwrap_err();
        let message = error.to_string();
        assert!(
            message.contains("not implemented") && message.contains("IsoData, Otsu"),
            "unexpected error for {method:?}: {message}"
        );
    }
}

#[test]
fn auto_threshold_rejects_unsupported_pixel_types() {
    let cases: [(ImageDocument, &str); 3] = [
        (
            ImageDocument::new_ramp_32bit("f.tif", 4, 4).unwrap(),
            "32-bit float",
        ),
        (
            ImageDocument::new(
                "c.tif",
                4,
                4,
                PixelPlane::Color256 {
                    pixels: vec![1u8; 16],
                    lut: Box::default(),
                },
            )
            .unwrap(),
            "8-bit color",
        ),
        (
            ImageDocument::new("rgb.tif", 4, 4, PixelPlane::ColorRgb(vec![0u32; 16])).unwrap(),
            "RGB",
        ),
    ];
    for (index, (doc, expected)) in cases.into_iter().enumerate() {
        let path = temp_tiff(&format!("auto_threshold_unsupported_{index}.tif"));
        save_tiff(&path, &doc).unwrap();
        let mut session = Session::new();
        session.dispatch(Command::OpenTiff { path }).unwrap();
        let error = session
            .dispatch(Command::AutoThreshold {
                method: "Otsu".into(),
                dark_background: false,
                histogram_16: false,
            })
            .unwrap_err();
        assert!(
            error.to_string().contains(expected),
            "unexpected error for {expected}: {error}"
        );
    }
}

#[test]
fn auto_threshold_rejects_stacks_instead_of_silently_using_one_slice() {
    let stack = ImageDocument::new_stack(
        "stack.tif",
        4,
        4,
        vec![
            PixelPlane::Gray8(vec![0u8; 16]),
            PixelPlane::Gray8(vec![1u8; 16]),
        ],
    )
    .unwrap();
    let path = temp_tiff("auto_threshold_stack.tif");
    save_tiff(&path, &stack).unwrap();
    let mut session = Session::new();
    session.dispatch(Command::OpenTiff { path }).unwrap();
    for histogram_16 in [false, true] {
        let error = session
            .dispatch(Command::AutoThreshold {
                method: "Otsu".into(),
                dark_background: false,
                histogram_16,
            })
            .unwrap_err();
        assert!(error.to_string().contains("stack"), "{error}");
    }
}

// ---------------------------------------------------------------------------
// Unit 5 (seventh increment): 16-bit auto thresholding, default and 16-bit
// histogram modes.
//
// ImageProcessor.setAutoThreshold(Method, boolean, int) handles 16-bit images
// in two different ways (original_imagej/ij/process/ImageProcessor.java lines
// 629-675):
//
// * Default mode (method string without "16"): resetMinAndMax() rescales the
//   processor to the full-image min/max, convertToByte(true) maps pixels with
//   256/(max-min+1) (TypeConverter.java lines 49-69), the 256-bin ROI
//   histogram is thresholded, and scaleAndSetThreshold() maps the 0-255
//   levels back onto the min/max range.
// * 16-bit histogram mode (method string containing "16"): ShortStatistics
//   exposes getHistogram2()/getHistogram() as histogram16 (ShortStatistics.java
//   lines 35-40); AutoThresholder trims the histogram to [minbin,maxbin] and
//   adds minbin back (AutoThresholder.java lines 49-62 and 85). A histogram
//   longer than 256 bins also makes tmax 65535 instead of 255.
//
// Neither mode writes pixels; only minThreshold/maxThreshold (and, in default
// mode, the display range reset) change.
// ---------------------------------------------------------------------------

fn pattern_16bit(title: &str, width: u32, height: u32, values: &[u16]) -> ImageDocument {
    assert_eq!(values.len(), (width * height) as usize);
    ImageDocument::new(title, width, height, PixelPlane::Gray16(values.to_vec())).unwrap()
}

fn uniform_16bit(title: &str, width: u32, height: u32, value: u16) -> ImageDocument {
    pattern_16bit(
        title,
        width,
        height,
        &vec![value; (width * height) as usize],
    )
}

fn oracle_auto_threshold_16(
    bridge: &JavaOracleBridge,
    doc: &ImageDocument,
    prelude: &str,
    method_and_options: &str,
) -> (f64, f64, ImageDocument) {
    let out = bridge
        .run_macro_on_image(
            doc,
            &format!(
                // Batch mode's Toolbar foreground is white, and
                // ShortProcessor.setColor(white) calls setMinAndMax(0,255)
                // when the display range has collapsed to 0..0, which resets
                // the threshold as a side effect (ShortProcessor.java lines
                // 987-1000). A macro foreground value keeps the subsequent
                // getProcessor() from touching the display range.
                "setColor(0); {prelude} setAutoThreshold(\"{method_and_options}\"); \
                 getThreshold(l1,l2); print(\"THRESH \"+d2s(l1,6)+\" \"+d2s(l2,6));"
            ),
        )
        .unwrap();
    let line = out
        .stdout
        .lines()
        .find(|line| line.trim_start().starts_with("THRESH "))
        .unwrap_or_else(|| panic!("oracle printed no THRESH line:\n{}", out.stdout));
    let mut tokens = line.trim().split_whitespace();
    tokens.next();
    (
        tokens.next().expect("lower threshold").parse().unwrap(),
        tokens.next().expect("upper threshold").parse().unwrap(),
        out.output_image.expect("oracle must return saved TIFF"),
    )
}

fn assert_auto_threshold_16_matches_oracle(
    bridge: &JavaOracleBridge,
    doc: &ImageDocument,
    prelude: &str,
    method_and_options: &str,
    method: ij_core::AutoThresholdMethod,
    dark_background: bool,
    histogram_16: bool,
) {
    let (oracle_lower, oracle_upper, oracle_doc) =
        oracle_auto_threshold_16(bridge, doc, prelude, method_and_options);
    let mut rust = doc.clone();
    let levels = rust
        .set_auto_threshold_with_options(method, dark_background, histogram_16)
        .unwrap();
    assert_eq!(
        (levels.lower, levels.upper),
        (oracle_lower, oracle_upper),
        "threshold differs for {method_and_options:?} (prelude {prelude:?}, histogram_16={histogram_16})"
    );
    assert_eq!(
        rust.display_range, oracle_doc.display_range,
        "display-range side effect differs for {method_and_options:?}"
    );
    assert_eq!(
        rust.plane(),
        doc.plane(),
        "thresholding must not touch pixels"
    );
    assert_eq!(rust.threshold, Some(levels));
}

#[test]
fn parity_16bit_default_ramp_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let doc = ImageDocument::new_ramp_16bit("ramp16.tif", 16, 16).unwrap();
    for (options, method, dark) in [
        ("Otsu", ij_core::AutoThresholdMethod::Otsu, false),
        ("IsoData", ij_core::AutoThresholdMethod::IsoData, false),
        ("Otsu dark", ij_core::AutoThresholdMethod::Otsu, true),
        ("IsoData dark", ij_core::AutoThresholdMethod::IsoData, true),
    ] {
        assert_auto_threshold_16_matches_oracle(&bridge, &doc, "", options, method, dark, false);
    }
}

#[test]
fn parity_16bit_hist16_ramp_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let doc = ImageDocument::new_ramp_16bit("ramp16.tif", 16, 16).unwrap();
    for (options, method, dark) in [
        ("Otsu 16-bit", ij_core::AutoThresholdMethod::Otsu, false),
        (
            "IsoData 16-bit",
            ij_core::AutoThresholdMethod::IsoData,
            false,
        ),
        ("Otsu dark 16-bit", ij_core::AutoThresholdMethod::Otsu, true),
        (
            "IsoData dark 16-bit",
            ij_core::AutoThresholdMethod::IsoData,
            true,
        ),
    ] {
        assert_auto_threshold_16_matches_oracle(&bridge, &doc, "", options, method, dark, true);
    }
}

#[test]
fn parity_16bit_three_bin_odd_range_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let doc = pattern_16bit("three16.tif", 3, 1, &[10, 20, 30]);
    for (options, method, dark, hist16) in [
        ("Otsu", ij_core::AutoThresholdMethod::Otsu, false, false),
        (
            "IsoData",
            ij_core::AutoThresholdMethod::IsoData,
            false,
            false,
        ),
        (
            "Otsu 16-bit",
            ij_core::AutoThresholdMethod::Otsu,
            false,
            true,
        ),
        (
            "IsoData 16-bit",
            ij_core::AutoThresholdMethod::IsoData,
            false,
            true,
        ),
        (
            "Otsu dark 16-bit",
            ij_core::AutoThresholdMethod::Otsu,
            true,
            true,
        ),
    ] {
        assert_auto_threshold_16_matches_oracle(&bridge, &doc, "", options, method, dark, hist16);
    }
}

#[test]
fn parity_16bit_bilevel_single_value_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let uniform = uniform_16bit("uniform16.tif", 4, 4, 100);
    for (options, method, dark, hist16) in [
        ("Otsu", ij_core::AutoThresholdMethod::Otsu, false, false),
        (
            "IsoData",
            ij_core::AutoThresholdMethod::IsoData,
            false,
            false,
        ),
        ("Otsu dark", ij_core::AutoThresholdMethod::Otsu, true, false),
        (
            "Otsu 16-bit",
            ij_core::AutoThresholdMethod::Otsu,
            false,
            true,
        ),
        (
            "Otsu dark 16-bit",
            ij_core::AutoThresholdMethod::Otsu,
            true,
            true,
        ),
    ] {
        assert_auto_threshold_16_matches_oracle(
            &bridge, &uniform, "", options, method, dark, hist16,
        );
    }
    // Above 255 the 16-bit histogram has more than 256 bins, so dark
    // background uses tmax 65535 instead of 255.
    let high = uniform_16bit("uniform_high16.tif", 4, 4, 1000);
    assert_auto_threshold_16_matches_oracle(
        &bridge,
        &high,
        "",
        "Otsu dark 16-bit",
        ij_core::AutoThresholdMethod::Otsu,
        true,
        true,
    );
}

#[test]
fn parity_16bit_bilevel_two_values_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut values = vec![10u16; 16];
    values[0] = 20;
    let doc = pattern_16bit("two16.tif", 4, 4, &values);
    for (options, method, dark, hist16) in [
        ("Otsu", ij_core::AutoThresholdMethod::Otsu, false, false),
        (
            "Otsu 16-bit",
            ij_core::AutoThresholdMethod::Otsu,
            false,
            true,
        ),
        (
            "Otsu dark 16-bit",
            ij_core::AutoThresholdMethod::Otsu,
            true,
            true,
        ),
    ] {
        assert_auto_threshold_16_matches_oracle(&bridge, &doc, "", options, method, dark, hist16);
    }
}

#[test]
fn parity_16bit_all_zero_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let doc = uniform_16bit("zero16.tif", 4, 4, 0);
    for (options, method, dark, hist16) in [
        ("Otsu", ij_core::AutoThresholdMethod::Otsu, false, false),
        (
            "IsoData",
            ij_core::AutoThresholdMethod::IsoData,
            false,
            false,
        ),
        ("Otsu dark", ij_core::AutoThresholdMethod::Otsu, true, false),
        (
            "Otsu 16-bit",
            ij_core::AutoThresholdMethod::Otsu,
            false,
            true,
        ),
        (
            "IsoData 16-bit",
            ij_core::AutoThresholdMethod::IsoData,
            false,
            true,
        ),
        (
            "Otsu dark 16-bit",
            ij_core::AutoThresholdMethod::Otsu,
            true,
            true,
        ),
    ] {
        assert_auto_threshold_16_matches_oracle(&bridge, &doc, "", options, method, dark, hist16);
    }
}

#[test]
fn parity_16bit_no_pixel_roi_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = ImageDocument::new_ramp_16bit("ramp16.tif", 16, 16).unwrap();
    doc.set_rect_roi(-20, -20, 5, 5).unwrap();
    for (options, method, dark, hist16) in [
        ("Otsu", ij_core::AutoThresholdMethod::Otsu, false, false),
        (
            "IsoData",
            ij_core::AutoThresholdMethod::IsoData,
            false,
            false,
        ),
        (
            "Otsu 16-bit",
            ij_core::AutoThresholdMethod::Otsu,
            false,
            true,
        ),
        (
            "IsoData 16-bit",
            ij_core::AutoThresholdMethod::IsoData,
            false,
            true,
        ),
    ] {
        assert_auto_threshold_16_matches_oracle(
            &bridge,
            &doc,
            "makeRectangle(-20, -20, 5, 5);",
            options,
            method,
            dark,
            hist16,
        );
    }
}

#[test]
fn parity_16bit_empty_mask_roi_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let points = [(1, 1), (2, 2), (3, 3)];
    let mut doc = ImageDocument::new_ramp_16bit("empty_mask16.tif", 16, 16).unwrap();
    doc.set_polygon_roi(&points).unwrap();

    // Default mode still thresholds the all-zero 256-bin histogram.
    for (options, method) in [
        ("Otsu", ij_core::AutoThresholdMethod::Otsu),
        ("IsoData", ij_core::AutoThresholdMethod::IsoData),
    ] {
        assert_auto_threshold_16_matches_oracle(
            &bridge,
            &doc,
            "makePolygon(1, 1, 2, 2, 3, 3);",
            options,
            method,
            false,
            false,
        );
    }

    // 16-bit mode: getHistogram2() is 8193 bins long because the mask's
    // bounding box contains value 8192, but every bin is zero. ImageJ's
    // AutoThresholder indexes histogram[-1]; IJ.setAutoThreshold catches the
    // resulting ArrayIndexOutOfBoundsException and leaves NO_THRESHOLD.
    let out = bridge
        .run_macro_on_image(
            &doc,
            "setColor(0); makePolygon(1, 1, 2, 2, 3, 3); \
             setAutoThreshold(\"Otsu 16-bit\"); getThreshold(l1,l2); \
             print(\"THRESH \"+l1+\" \"+l2);",
        )
        .unwrap();
    assert!(
        out.stdout.contains("out of bounds"),
        "expected ImageJ's ArrayIndexOutOfBoundsException log:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("THRESH -1 -1"),
        "ImageJ must leave NO_THRESHOLD after the exception:\n{}",
        out.stdout
    );

    let mut rust = doc.clone();
    let error = rust
        .set_auto_threshold_with_options(ij_core::AutoThresholdMethod::Otsu, false, true)
        .unwrap_err();
    assert!(matches!(
        error,
        ij_core::CoreError::EmptyThresholdHistogram { bins: 8193 }
    ));
    assert_eq!(
        rust.threshold, None,
        "a rejected threshold must not be stored"
    );
    assert_eq!(rust.plane(), doc.plane());
}

#[test]
fn parity_16bit_oval_roi_mask_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = smooth_pattern_16bit(8, 6);
    doc.set_oval_roi(1, 1, 5, 4).unwrap();
    for (options, method, dark, hist16) in [
        ("Otsu", ij_core::AutoThresholdMethod::Otsu, false, false),
        (
            "IsoData",
            ij_core::AutoThresholdMethod::IsoData,
            false,
            false,
        ),
        (
            "Otsu 16-bit",
            ij_core::AutoThresholdMethod::Otsu,
            false,
            true,
        ),
        (
            "IsoData 16-bit",
            ij_core::AutoThresholdMethod::IsoData,
            false,
            true,
        ),
        (
            "Otsu dark 16-bit",
            ij_core::AutoThresholdMethod::Otsu,
            true,
            true,
        ),
    ] {
        assert_auto_threshold_16_matches_oracle(
            &bridge,
            &doc,
            "makeOval(1, 1, 5, 4);",
            options,
            method,
            dark,
            hist16,
        );
    }
}

#[test]
fn parity_16bit_small_max_oval_mask_tmax_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    // The oval's bounding rectangle stays below 256, so getHistogram2() is
    // 256 bins long and 16-bit dark background keeps tmax 255 even though the
    // processor is 16-bit.
    let mut values = vec![0u16; 48];
    for y in 0..6u32 {
        for x in 0..8u32 {
            values[(y * 8 + x) as usize] = ((x * 37 + y * 61) % 251) as u16;
        }
    }
    let mut doc = pattern_16bit("small16.tif", 8, 6, &values);
    doc.set_oval_roi(1, 1, 5, 4).unwrap();
    for (options, method, dark, hist16) in [
        ("Otsu dark", ij_core::AutoThresholdMethod::Otsu, true, false),
        (
            "Otsu dark 16-bit",
            ij_core::AutoThresholdMethod::Otsu,
            true,
            true,
        ),
        (
            "IsoData dark 16-bit",
            ij_core::AutoThresholdMethod::IsoData,
            true,
            true,
        ),
    ] {
        assert_auto_threshold_16_matches_oracle(
            &bridge,
            &doc,
            "makeOval(1, 1, 5, 4);",
            options,
            method,
            dark,
            hist16,
        );
    }
}

#[test]
fn parity_16bit_hist16_large_roi_tmax_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    // A 250,000-pixel ROI makes ShortStatistics use the full 65536-bin
    // histogram, so tmax is 65535 even though every pixel is below 256.
    let large = uniform_16bit("large10.tif", 500, 500, 10);
    assert_auto_threshold_16_matches_oracle(
        &bridge,
        &large,
        "",
        "Otsu dark 16-bit",
        ij_core::AutoThresholdMethod::Otsu,
        true,
        true,
    );
    let small = uniform_16bit("small10.tif", 4, 4, 10);
    assert_auto_threshold_16_matches_oracle(
        &bridge,
        &small,
        "",
        "Otsu dark 16-bit",
        ij_core::AutoThresholdMethod::Otsu,
        true,
        true,
    );
}

#[test]
fn parity_16bit_high_bins_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    // Bins 300..500 exercise the minbin/maxbin trimming and minbin addition.
    let doc = pattern_16bit("high16.tif", 3, 1, &[300, 400, 500]);
    for (options, method, dark, hist16) in [
        ("Otsu", ij_core::AutoThresholdMethod::Otsu, false, false),
        (
            "IsoData",
            ij_core::AutoThresholdMethod::IsoData,
            false,
            false,
        ),
        (
            "Otsu 16-bit",
            ij_core::AutoThresholdMethod::Otsu,
            false,
            true,
        ),
        (
            "IsoData 16-bit",
            ij_core::AutoThresholdMethod::IsoData,
            false,
            true,
        ),
        (
            "Otsu dark 16-bit",
            ij_core::AutoThresholdMethod::Otsu,
            true,
            true,
        ),
    ] {
        assert_auto_threshold_16_matches_oracle(&bridge, &doc, "", options, method, dark, hist16);
    }
}

#[test]
fn parity_16bit_rect_roi_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = smooth_pattern_16bit(8, 6);
    doc.set_rect_roi(2, 2, 4, 3).unwrap();
    for (options, method, dark, hist16) in [
        ("Otsu", ij_core::AutoThresholdMethod::Otsu, false, false),
        (
            "Otsu 16-bit",
            ij_core::AutoThresholdMethod::Otsu,
            false,
            true,
        ),
    ] {
        assert_auto_threshold_16_matches_oracle(
            &bridge,
            &doc,
            "makeRectangle(2, 2, 4, 3);",
            options,
            method,
            dark,
            hist16,
        );
    }
}

#[test]
fn session_auto_threshold_16bit_default_records_macro_and_resets_display_range() {
    let mut session = Session::new();
    let id = session.insert_doc(ImageDocument::new_ramp_16bit("ramp16", 16, 16).unwrap());

    let outcome = session
        .dispatch(Command::AutoThreshold {
            method: "Otsu".into(),
            dark_background: false,
            histogram_16: false,
        })
        .unwrap();
    match outcome {
        ij_session::CommandOutcome::Thresholded(levels) => {
            assert_eq!((levels.lower, levels.upper), (0.0, 28672.0));
        }
        other => panic!("expected Thresholded outcome, got {other:?}"),
    }
    let doc = session.get_image(id).unwrap();
    // resetMinAndMax() makes the processor display range the image range
    // (ImageProcessor.java lines 642-647).
    assert_eq!(
        doc.display_range,
        DisplayRange {
            min: 0.0,
            max: 61440.0
        }
    );
    assert!(
        session.is_dirty(id),
        "the reset display range is persisted in the TIFF description"
    );
    assert_eq!(
        session.macro_recorder.last().map(String::as_str),
        Some("setAutoThreshold(\"Otsu\");")
    );
}

#[test]
fn session_auto_threshold_16bit_hist16_records_option_without_dirtying() {
    let mut session = Session::new();
    let id = session.insert_doc(ImageDocument::new_ramp_16bit("ramp16", 16, 16).unwrap());
    session.set_dirty(id, false);

    let outcome = session
        .dispatch(Command::AutoThreshold {
            method: "Otsu".into(),
            dark_background: false,
            histogram_16: true,
        })
        .unwrap();
    match outcome {
        ij_session::CommandOutcome::Thresholded(levels) => {
            assert_eq!((levels.lower, levels.upper), (0.0, 28672.0));
        }
        other => panic!("expected Thresholded outcome, got {other:?}"),
    }
    let doc = session.get_image(id).unwrap();
    assert_eq!(
        doc.display_range,
        DisplayRange {
            min: 0.0,
            max: 65535.0
        },
        "16-bit histogram mode does not call resetMinAndMax"
    );
    assert!(
        !session.is_dirty(id),
        "threshold state is not persisted in the TIFF description"
    );
    // ThresholdAdjuster.autoSetLevels appends " 16-bit" after the dark flag
    // (ThresholdAdjuster.java line 503).
    assert_eq!(
        session.macro_recorder.last().map(String::as_str),
        Some("setAutoThreshold(\"Otsu 16-bit\");")
    );

    session
        .dispatch(Command::AutoThreshold {
            method: "IsoData".into(),
            dark_background: true,
            histogram_16: true,
        })
        .unwrap();
    assert_eq!(
        session.macro_recorder.last().map(String::as_str),
        Some("setAutoThreshold(\"IsoData dark 16-bit\");")
    );
}

#[test]
fn auto_threshold_rejects_16bit_histogram_option_on_8bit_images() {
    let mut session = Session::new();
    session
        .dispatch(Command::NewRamp8Bit {
            title: "ramp".into(),
            width: 16,
            height: 16,
        })
        .unwrap();
    let error = session
        .dispatch(Command::AutoThreshold {
            method: "Otsu".into(),
            dark_background: false,
            histogram_16: true,
        })
        .unwrap_err();
    assert!(
        error.to_string().contains("16-bit histogram"),
        "unexpected error: {error}"
    );
}

// ---------------------------------------------------------------------------
// Unit 5 (sixth increment): RankFilters Median/Minimum/Maximum and Despeckle
// for 8-bit grayscale images.
//
// RankFilters.rank builds a circular kernel from `makeLineRadii`
// (original_imagej/ij/plugin/filter/RankFilters.java lines 866-890), filters
// the ROI rectangle through a padded cache (nearest-image-pixel padding at the
// image border, lines 599-643), rounds byte output with `+0.5f` and masks the
// result with `&0xff` (lines 648-664), and PlugInFilterRunner restores pixels
// outside an irregular ROI mask afterwards (lines 271-275). Despeckle is the
// median filter with radius 1 (RankFilters.java lines 117-119).
// ---------------------------------------------------------------------------

/// Values with many duplicates plus the 0/248 extremes, so median ties and
/// min/max ordering are exercised by exact bytes rather than by chance.
fn rank_pattern_8bit(width: u32, height: u32) -> ImageDocument {
    let mut pixels = vec![0u8; (width * height) as usize];
    for y in 0..height {
        for x in 0..width {
            let step = ((x * 5 + y * 3) % 32) as u8;
            pixels[(y * width + x) as usize] = step.wrapping_mul(8);
        }
    }
    ImageDocument::new("rank8.tif", width, height, PixelPlane::Gray8(pixels)).unwrap()
}

fn rank_impulse_8bit(width: u32, height: u32) -> ImageDocument {
    let mut pixels = vec![0u8; (width * height) as usize];
    let center = (height / 2) * width + width / 2;
    pixels[center as usize] = 255;
    ImageDocument::new(
        "rank_impulse8.tif",
        width,
        height,
        PixelPlane::Gray8(pixels),
    )
    .unwrap()
}

fn oracle_rank(
    bridge: &JavaOracleBridge,
    doc: &ImageDocument,
    prelude: &str,
    command: &str,
) -> ImageDocument {
    let out = bridge
        .run_macro_on_image(doc, &format!("{prelude} {command}"))
        .unwrap();
    out.output_image.expect("oracle must return saved TIFF")
}

#[test]
fn parity_8bit_median_full_image_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let doc = rank_pattern_8bit(9, 7);
    let oracle = oracle_rank(&bridge, &doc, "", "run(\"Median...\", \"radius=2\");");

    let mut rust = doc.clone();
    rust.rank_filter(ij_core::RankFilterKind::Median, 2.0)
        .unwrap();

    assert_ne!(doc.plane(), oracle.plane(), "oracle median was a no-op");
    assert_eq!(rust.plane(), oracle.plane());
}

#[test]
fn parity_8bit_minimum_full_image_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let doc = rank_pattern_8bit(9, 7);
    let oracle = oracle_rank(&bridge, &doc, "", "run(\"Minimum...\", \"radius=3\");");

    let mut rust = doc.clone();
    rust.rank_filter(ij_core::RankFilterKind::Minimum, 3.0)
        .unwrap();

    assert_ne!(doc.plane(), oracle.plane(), "oracle minimum was a no-op");
    assert_eq!(rust.plane(), oracle.plane());
}

#[test]
fn parity_8bit_maximum_full_image_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let doc = rank_pattern_8bit(9, 7);
    let oracle = oracle_rank(&bridge, &doc, "", "run(\"Maximum...\", \"radius=1\");");

    let mut rust = doc.clone();
    rust.rank_filter(ij_core::RankFilterKind::Maximum, 1.0)
        .unwrap();

    assert_ne!(doc.plane(), oracle.plane(), "oracle maximum was a no-op");
    assert_eq!(rust.plane(), oracle.plane());
}

/// `makeLineRadii` is the only source of the footprint: radius < 1.5 gives the
/// truncated `(int)(r*r)+1` kernel, 1.5..1.75 is remapped to 1.75, 2.5..2.85 to
/// 2.85 (RankFilters.java lines 866-871). A single bright pixel under Maximum
/// exposes the footprint byte-for-byte.
#[test]
fn parity_8bit_rank_radius_footprints_match_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let doc = rank_impulse_8bit(9, 9);
    for radius in [0.0, 0.5, 1.0, 1.4, 1.5, 1.75, 2.0, 2.85, 4.5] {
        let oracle = oracle_rank(
            &bridge,
            &doc,
            "",
            &format!("run(\"Maximum...\", \"radius={radius}\");"),
        );
        let mut rust = doc.clone();
        rust.rank_filter(ij_core::RankFilterKind::Maximum, radius)
            .unwrap();
        assert_eq!(
            rust.plane(),
            oracle.plane(),
            "maximum footprint differs for radius={radius}"
        );
    }
}

/// Radius 0 is not the identity: `(int)(0*0)+1 = 1` makes a 3x3 kernel whose
/// oblique lines are empty, i.e. the 5-pixel plus shape.
#[test]
fn parity_8bit_median_radius_zero_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let doc = rank_pattern_8bit(7, 7);
    let oracle = oracle_rank(&bridge, &doc, "", "run(\"Median...\", \"radius=0\");");

    let mut rust = doc.clone();
    rust.rank_filter(ij_core::RankFilterKind::Median, 0.0)
        .unwrap();

    assert_eq!(rust.plane(), oracle.plane());
}

/// A kernel larger than the image exercises the cache's edge replication in
/// both directions (RankFilters.java lines 599-643).
#[test]
fn parity_8bit_rank_kernel_larger_than_image_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let doc = rank_pattern_8bit(3, 2);
    let oracle = oracle_rank(&bridge, &doc, "", "run(\"Median...\", \"radius=5\");");

    let mut rust = doc.clone();
    rust.rank_filter(ij_core::RankFilterKind::Median, 5.0)
        .unwrap();

    assert_eq!(rust.plane(), oracle.plane());
}

#[test]
fn parity_8bit_rank_rect_roi_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = rank_pattern_8bit(8, 6);
    doc.set_rect_roi(2, 1, 4, 3).unwrap();
    let oracle = oracle_rank(
        &bridge,
        &doc,
        "makeRectangle(2, 1, 4, 3);",
        "run(\"Median...\", \"radius=2\");",
    );

    let mut rust = doc.clone();
    rust.rank_filter(ij_core::RankFilterKind::Median, 2.0)
        .unwrap();

    assert_ne!(doc.plane(), oracle.plane(), "oracle median was a no-op");
    assert_eq!(rust.plane(), oracle.plane());
}

#[test]
fn parity_8bit_rank_oval_roi_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = rank_pattern_8bit(8, 6);
    doc.set_oval_roi(1, 1, 6, 4).unwrap();
    let oracle = oracle_rank(
        &bridge,
        &doc,
        "makeOval(1, 1, 6, 4);",
        "run(\"Minimum...\", \"radius=2\");",
    );

    let mut rust = doc.clone();
    rust.rank_filter(ij_core::RankFilterKind::Minimum, 2.0)
        .unwrap();

    assert_ne!(doc.plane(), oracle.plane(), "oracle minimum was a no-op");
    assert_eq!(rust.plane(), oracle.plane());
}

#[test]
fn parity_8bit_rank_clipped_oval_roi_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = rank_pattern_8bit(8, 6);
    doc.set_oval_roi(-2, -1, 6, 4).unwrap();
    let oracle = oracle_rank(
        &bridge,
        &doc,
        "makeOval(-2, -1, 6, 4);",
        "run(\"Maximum...\", \"radius=3\");",
    );

    let mut rust = doc.clone();
    rust.rank_filter(ij_core::RankFilterKind::Maximum, 3.0)
        .unwrap();

    assert_eq!(rust.plane(), oracle.plane());
}

#[test]
fn parity_8bit_rank_polygon_roi_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = rank_pattern_8bit(8, 6);
    doc.set_polygon_roi(&[(1, 1), (6, 1), (6, 4), (1, 4)])
        .unwrap();
    let oracle = oracle_rank(
        &bridge,
        &doc,
        "makePolygon(1, 1, 6, 1, 6, 4, 1, 4);",
        "run(\"Median...\", \"radius=1\");",
    );

    let mut rust = doc.clone();
    rust.rank_filter(ij_core::RankFilterKind::Median, 1.0)
        .unwrap();

    assert_ne!(doc.plane(), oracle.plane(), "oracle median was a no-op");
    assert_eq!(rust.plane(), oracle.plane());
}

#[test]
fn parity_rank_line_roi_resets_to_whole_image_like_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = rank_pattern_8bit(8, 6);
    doc.set_line_roi(1, 1, 6, 4);
    // PlugInFilterRunner.prepareProcessor resets non-area selections
    // (lines 207-213), so the rank filter sees the whole image.
    let oracle = oracle_rank(
        &bridge,
        &doc,
        "makeLine(1, 1, 6, 4);",
        "run(\"Median...\", \"radius=2\");",
    );
    let plain = rank_pattern_8bit(8, 6);
    let oracle_plain = oracle_rank(&bridge, &plain, "", "run(\"Median...\", \"radius=2\");");
    assert_eq!(oracle.plane(), oracle_plain.plane());

    let mut rust = doc.clone();
    rust.rank_filter(ij_core::RankFilterKind::Median, 2.0)
        .unwrap();
    assert_eq!(rust.plane(), oracle.plane());
}

#[test]
fn parity_8bit_despeckle_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let doc = rank_pattern_8bit(9, 7);
    let oracle = oracle_rank(&bridge, &doc, "", "run(\"Despeckle\");");

    let mut rust = doc.clone();
    rust.despeckle().unwrap();

    assert_eq!(rust.plane(), oracle.plane());
}

#[test]
fn parity_8bit_rank_stack_slice_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = distinct_stack_8bit(6, 4, 3);
    doc.set_slice(2).unwrap();
    let oracle = bridge
        .run_macro_on_image(
            &doc,
            "setSlice(2); run(\"Minimum...\", \"radius=2 slice\");",
        )
        .unwrap()
        .output_image
        .expect("oracle must return saved TIFF")
        .slices;
    let original = doc.slices.clone();

    let mut rust = doc.clone();
    rust.rank_filter(ij_core::RankFilterKind::Minimum, 2.0)
        .unwrap();

    assert_eq!(rust.slices, oracle);
    assert_eq!(rust.slices[0], original[0]);
    assert_ne!(rust.slices[1], original[1]);
    assert_eq!(rust.slices[2], original[2]);
}

/// RankFilters does not carry KEEP_THRESHOLD, but `resetBinaryThreshold` only
/// clears a threshold whose `lutUpdateMode` is NO_LUT_UPDATE
/// (PlugInFilterRunner.java lines 122-123, ImageProcessor.java 835-838).
/// `setAutoThreshold` stores its levels with RED_LUT, so the levels survive.
#[test]
fn parity_8bit_rank_preserves_auto_threshold_state_like_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let doc = ImageDocument::new_ramp_8bit("ramp.tif", 16, 16).unwrap();
    let out = bridge
        .run_macro_on_image(
            &doc,
            "setAutoThreshold(\"Otsu\"); getThreshold(a,b); \
             run(\"Median...\", \"radius=2\"); getThreshold(c,d); \
             print(\"THRESH \"+d2s(a,6)+\" \"+d2s(b,6)+\" \"+d2s(c,6)+\" \"+d2s(d,6));",
        )
        .unwrap();
    let line = out
        .stdout
        .lines()
        .find(|line| line.trim_start().starts_with("THRESH "))
        .unwrap_or_else(|| panic!("oracle printed no THRESH line:\n{}", out.stdout));
    let values: Vec<f64> = line
        .trim()
        .split_whitespace()
        .skip(1)
        .map(|token| token.parse().unwrap())
        .collect();
    assert_eq!(values.len(), 4);
    let oracle_after = (values[2], values[3]);
    assert_eq!(
        (values[0], values[1]),
        oracle_after,
        "oracle itself changed the threshold levels"
    );

    let mut rust = doc.clone();
    let before = rust
        .set_auto_threshold(ij_core::AutoThresholdMethod::Otsu, false)
        .unwrap();
    rust.rank_filter(ij_core::RankFilterKind::Median, 2.0)
        .unwrap();

    assert_eq!((before.lower, before.upper), (values[0], values[1]));
    assert_eq!(
        rust.threshold,
        Some(ThresholdLevels {
            lower: oracle_after.0,
            upper: oracle_after.1
        }),
        "rank filtering must preserve RED_LUT auto-threshold levels"
    );
    let oracle_image = out.output_image.expect("oracle must return saved TIFF");
    assert_eq!(rust.plane(), oracle_image.plane());
}

/// Even a degenerate min==max auto-threshold pair is preserved, because
/// setAutoThreshold stores it with RED_LUT rather than NO_LUT_UPDATE.
/// Otsu with dark background on a uniform 255 image yields (255, 255).
#[test]
fn parity_8bit_rank_preserves_equal_threshold_levels_like_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let doc = uniform_8bit("uniform255.tif", 8, 8, 255);
    let out = bridge
        .run_macro_on_image(
            &doc,
            "setAutoThreshold(\"Otsu dark\"); getThreshold(a,b); \
             run(\"Minimum...\", \"radius=1\"); getThreshold(c,d); \
             print(\"THRESH \"+d2s(a,6)+\" \"+d2s(b,6)+\" \"+d2s(c,6)+\" \"+d2s(d,6));",
        )
        .unwrap();
    let line = out
        .stdout
        .lines()
        .find(|line| line.trim_start().starts_with("THRESH "))
        .unwrap_or_else(|| panic!("oracle printed no THRESH line:\n{}", out.stdout));
    let values: Vec<f64> = line
        .trim()
        .split_whitespace()
        .skip(1)
        .map(|token| token.parse().unwrap())
        .collect();
    assert_eq!(values.len(), 4);
    assert_eq!(
        values[0], values[1],
        "expected an equal-level oracle threshold"
    );
    assert_eq!(
        (values[2], values[3]),
        (values[0], values[1]),
        "oracle itself cleared the equal-level threshold"
    );

    let mut rust = doc.clone();
    let before = rust
        .set_auto_threshold(ij_core::AutoThresholdMethod::Otsu, true)
        .unwrap();
    assert_eq!((before.lower, before.upper), (values[0], values[1]));
    rust.rank_filter(ij_core::RankFilterKind::Minimum, 1.0)
        .unwrap();
    assert_eq!(
        rust.threshold,
        Some(ThresholdLevels {
            lower: values[2],
            upper: values[3]
        })
    );
}

#[test]
fn parity_8bit_rank_undo_restores_pixels_like_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let doc = rank_pattern_8bit(9, 7);
    let oracle = oracle_rank(
        &bridge,
        &doc,
        "",
        "run(\"Median...\", \"radius=2\"); run(\"Undo\");",
    );
    assert_eq!(
        oracle.plane(),
        doc.plane(),
        "oracle Undo must restore the pre-filter pixels"
    );

    let mut rust = doc.clone();
    rust.rank_filter(ij_core::RankFilterKind::Median, 2.0)
        .unwrap();
    assert!(rust.snapshot.is_some(), "rank filter must install Undo");
    assert!(rust.undo());
    assert_eq!(rust.plane(), oracle.plane());
    assert!(rust.snapshot.is_none());
}

#[test]
fn session_rank_filters_record_macro_and_update_pixels() {
    let source = rank_pattern_8bit(9, 7);
    let path = temp_tiff("session_rank_record.tif");
    save_tiff(&path, &source).unwrap();
    let mut session = Session::new();
    session.dispatch(Command::OpenTiff { path }).unwrap();
    let before = session.active_doc().unwrap().plane().clone();

    session.dispatch(Command::Median { radius: 2.0 }).unwrap();
    assert_ne!(session.active_doc().unwrap().plane(), &before);
    assert_eq!(
        session.macro_recorder.last().map(String::as_str),
        Some("run(\"Median...\", \"radius=2\");")
    );

    session.dispatch(Command::Minimum { radius: 1.5 }).unwrap();
    assert_eq!(
        session.macro_recorder.last().map(String::as_str),
        Some("run(\"Minimum...\", \"radius=1.5\");")
    );

    session.dispatch(Command::Maximum { radius: 0.0 }).unwrap();
    assert_eq!(
        session.macro_recorder.last().map(String::as_str),
        Some("run(\"Maximum...\", \"radius=0\");")
    );

    session.dispatch(Command::Despeckle).unwrap();
    assert_eq!(
        session.macro_recorder.last().map(String::as_str),
        Some("run(\"Despeckle\");")
    );

    // A stack answer to "Process Stack?" is recorded as the `slice` option.
    let path = temp_tiff("session_rank_stack.tif");
    save_tiff(&path, &distinct_stack_8bit(6, 4, 3)).unwrap();
    let mut stack_session = Session::new();
    stack_session.dispatch(Command::OpenTiff { path }).unwrap();
    stack_session
        .dispatch(Command::SetSlice { index: 2 })
        .unwrap();
    stack_session
        .dispatch(Command::Median { radius: 2.0 })
        .unwrap();
    assert_eq!(
        stack_session.macro_recorder.last().map(String::as_str),
        Some("run(\"Median...\", \"slice radius=2\");")
    );
    stack_session.dispatch(Command::Despeckle).unwrap();
    assert_eq!(
        stack_session.macro_recorder.last().map(String::as_str),
        Some("run(\"Despeckle\", \"slice\");")
    );
}

/// Feed the recorded transcript back to ImageJ 1.54u: the recorded macro must
/// reproduce the same pixels the Rust rank filter produced.
#[test]
fn rank_filter_recorded_macro_reproduces_oracle_output() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let source = rank_pattern_8bit(9, 7);

    let mut session = Session::new();
    let path = temp_tiff("rank_recorded_macro.tif");
    save_tiff(&path, &source).unwrap();
    session.dispatch(Command::OpenTiff { path }).unwrap();

    for command in [
        Command::Median { radius: 2.0 },
        Command::Minimum { radius: 1.5 },
        Command::Maximum { radius: 3.0 },
        Command::Despeckle,
    ] {
        let input = session.active_doc().unwrap().clone();
        session.dispatch(command).unwrap();
        let transcript = session.macro_recorder.last().unwrap().clone();
        let oracle = bridge
            .run_macro_on_image(&input, &transcript)
            .unwrap()
            .output_image
            .expect("oracle must return saved TIFF");
        assert_eq!(
            session.active_doc().unwrap().plane(),
            oracle.plane(),
            "recorded transcript differs from oracle output: {transcript}"
        );
    }
}

#[test]
fn rank_filters_reject_unsupported_pixel_types_instead_of_ignoring_them() {
    let cases = [
        (
            ImageDocument::new(
                "indexed8.tif",
                4,
                4,
                PixelPlane::Color256 {
                    pixels: vec![3u8; 16],
                    lut: Box::default(),
                },
            )
            .unwrap(),
            "8-bit color",
        ),
        (
            ImageDocument::new("rgb.tif", 4, 4, PixelPlane::ColorRgb(vec![0xff80_4020; 16]))
                .unwrap(),
            "RGB",
        ),
    ];
    for (index, (doc, expected)) in cases.into_iter().enumerate() {
        let before = doc.clone();
        for kind in [
            ij_core::RankFilterKind::Median,
            ij_core::RankFilterKind::Minimum,
            ij_core::RankFilterKind::Maximum,
        ] {
            let mut attempted = doc.clone();
            let error = attempted.rank_filter(kind, 2.0).unwrap_err();
            assert!(
                error.to_string().contains(expected) && error.to_string().contains(kind.as_str()),
                "unexpected error for {expected}/{kind:?}: {error}"
            );
            // Rejected filters must not consume or install Undo state.
            assert_eq!(attempted.snapshot, None);
            assert_eq!(attempted.plane(), before.plane());
        }
        let error = doc.clone().despeckle().unwrap_err();
        assert!(
            error.to_string().contains(expected) && error.to_string().contains("Despeckle"),
            "unexpected Despeckle error for {expected}: {error}"
        );
        assert_eq!(before.plane(), doc.plane(), "case {index} input mutated");
    }
}

#[test]
fn rank_filters_reject_unsupported_radii_instead_of_guessing() {
    let doc = rank_pattern_8bit(6, 4);
    for (kind, radius) in [
        (ij_core::RankFilterKind::Median, -1.0),
        (ij_core::RankFilterKind::Minimum, -0.5),
        (ij_core::RankFilterKind::Maximum, f64::NAN),
        (ij_core::RankFilterKind::Median, f64::INFINITY),
        // The median dialog caps radius at 100; min/max at 1000
        // (RankFilters.java line 188).
        (ij_core::RankFilterKind::Median, 100.5),
        (ij_core::RankFilterKind::Minimum, 1000.5),
        (ij_core::RankFilterKind::Maximum, 1000.5),
    ] {
        let mut attempted = doc.clone();
        let error = attempted.rank_filter(kind, radius).unwrap_err();
        assert!(
            error.to_string().contains("radius"),
            "expected an explicit radius error for {kind:?}/{radius}, got: {error}"
        );
        assert_eq!(attempted.snapshot, None);
        assert_eq!(attempted.plane(), doc.plane());
    }
    // The documented dialog limits are supported.
    for (kind, radius) in [
        (ij_core::RankFilterKind::Median, 100.0),
        (ij_core::RankFilterKind::Minimum, 1000.0),
        (ij_core::RankFilterKind::Maximum, 1000.0),
    ] {
        let mut allowed = ImageDocument::new_ramp_8bit("small.tif", 3, 3).unwrap();
        allowed.rank_filter(kind, radius).unwrap();
    }
}

#[test]
fn rank_filters_preserve_roi_display_range_and_calibration() {
    let mut doc = rank_pattern_8bit(8, 6);
    doc.set_rect_roi(2, 1, 4, 3).unwrap();
    doc.display_range = DisplayRange {
        min: 10.0,
        max: 200.0,
    };
    doc.calibration.unit = "mm".into();
    doc.calibration.pixel_width = 0.5;
    let roi = doc.roi.clone();
    let calibration = doc.calibration.clone();

    doc.rank_filter(ij_core::RankFilterKind::Median, 2.0)
        .unwrap();

    assert_eq!(doc.roi, roi);
    assert_eq!(doc.calibration, calibration);
    assert_eq!(
        doc.display_range,
        DisplayRange {
            min: 10.0,
            max: 200.0
        }
    );
}

#[test]
fn parity_8bit_rank_minimum_footprints_match_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut pixels = vec![255u8; 81];
    pixels[4 * 9 + 4] = 0;
    let doc =
        ImageDocument::new("rank_inverted_impulse.tif", 9, 9, PixelPlane::Gray8(pixels)).unwrap();
    for radius in [0.5, 1.5, 2.85] {
        let oracle = oracle_rank(
            &bridge,
            &doc,
            "",
            &format!("run(\"Minimum...\", \"radius={radius}\");"),
        );
        let mut rust = doc.clone();
        rust.rank_filter(ij_core::RankFilterKind::Minimum, radius)
            .unwrap();
        assert_eq!(
            rust.plane(),
            oracle.plane(),
            "minimum footprint differs for radius={radius}"
        );
    }
}

/// The cache pads x with the nearest inside column and y with the nearest
/// image row (RankFilters.java lines 599-643); 1-pixel-wide, 1-pixel-high and
/// 1x1 images therefore collapse every out-of-image sample onto that pixel.
#[test]
fn parity_8bit_rank_tiny_images_match_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    for (width, height) in [(1, 6), (6, 1), (1, 1), (2, 2)] {
        let doc = rank_pattern_8bit(width, height);
        let oracle = oracle_rank(&bridge, &doc, "", "run(\"Median...\", \"radius=2\");");
        let mut rust = doc.clone();
        rust.rank_filter(ij_core::RankFilterKind::Median, 2.0)
            .unwrap();
        assert_eq!(
            rust.plane(),
            oracle.plane(),
            "median differs for {width}x{height}"
        );
    }
}

#[test]
fn parity_8bit_rank_small_and_edge_rois_match_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();

    // ROI smaller than the kernel.
    let mut single = rank_pattern_8bit(9, 7);
    single.set_rect_roi(4, 3, 1, 1).unwrap();
    let oracle = oracle_rank(
        &bridge,
        &single,
        "makeRectangle(4, 3, 1, 1);",
        "run(\"Median...\", \"radius=4\");",
    );
    let mut rust = single.clone();
    rust.rank_filter(ij_core::RankFilterKind::Median, 4.0)
        .unwrap();
    assert_eq!(rust.plane(), oracle.plane());

    // ROI touching the top-left image corner.
    let mut corner = rank_pattern_8bit(9, 7);
    corner.set_rect_roi(0, 0, 3, 2).unwrap();
    let oracle = oracle_rank(
        &bridge,
        &corner,
        "makeRectangle(0, 0, 3, 2);",
        "run(\"Minimum...\", \"radius=3\");",
    );
    let mut rust = corner.clone();
    rust.rank_filter(ij_core::RankFilterKind::Minimum, 3.0)
        .unwrap();
    assert_eq!(rust.plane(), oracle.plane());

    // ROI touching the bottom-right image corner.
    let mut far = rank_pattern_8bit(9, 7);
    far.set_rect_roi(6, 4, 3, 3).unwrap();
    let oracle = oracle_rank(
        &bridge,
        &far,
        "makeRectangle(6, 4, 3, 3);",
        "run(\"Maximum...\", \"radius=2.5\");",
    );
    let mut rust = far.clone();
    rust.rank_filter(ij_core::RankFilterKind::Maximum, 2.5)
        .unwrap();
    assert_eq!(rust.plane(), oracle.plane());
}

/// A large radius makes `makeLineRadii`'s `nPoints` and every kernel row
/// dominate the result; the whole 9x7 image then shares one clamped window.
#[test]
fn parity_8bit_rank_large_radius_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let doc = rank_pattern_8bit(9, 7);
    let oracle = oracle_rank(&bridge, &doc, "", "run(\"Median...\", \"radius=20\");");
    let mut rust = doc.clone();
    rust.rank_filter(ij_core::RankFilterKind::Median, 20.0)
        .unwrap();
    assert_eq!(rust.plane(), oracle.plane());
}

/// Large-radius min/max: windows far wider than an image row must agree with
/// the incremental entering/leaving side-column scans
/// (RankFilters.java lines 546-560), not only with a full-window rescan.
#[test]
fn parity_8bit_rank_large_radius_minmax_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let doc = rank_pattern_8bit(17, 13);
    for (kind, radius) in [
        (ij_core::RankFilterKind::Minimum, 20.0),
        (ij_core::RankFilterKind::Maximum, 20.0),
        (ij_core::RankFilterKind::Minimum, 50.0),
        (ij_core::RankFilterKind::Maximum, 50.0),
    ] {
        let command = format!("run(\"{kind:?}...\", \"radius={radius}\");");
        let oracle = oracle_rank(&bridge, &doc, "", &command);
        let mut rust = doc.clone();
        rust.rank_filter(kind, radius).unwrap();
        assert_ne!(
            doc.plane(),
            oracle.plane(),
            "8-bit {kind:?} radius {radius} was a no-op"
        );
        assert_eq!(
            rust.plane(),
            oracle.plane(),
            "8-bit large-radius {kind:?} radius {radius}"
        );
    }
}

/// Images narrower/shorter than the kernel put the whole inside part into the
/// middle of the cache line, with padding on one or both sides
/// (RankFilters.java lines 619-643). The same holds for a one-pixel ROI.
#[test]
fn parity_8bit_rank_large_radius_tiny_images_and_roi_match_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    for (width, height) in [(1, 6), (6, 1), (1, 1)] {
        let doc = rank_pattern_8bit(width, height);
        let oracle = oracle_rank(&bridge, &doc, "", "run(\"Maximum...\", \"radius=20\");");
        let mut rust = doc.clone();
        rust.rank_filter(ij_core::RankFilterKind::Maximum, 20.0)
            .unwrap();
        assert_eq!(
            rust.plane(),
            oracle.plane(),
            "8-bit radius-20 maximum on {width}x{height}"
        );
    }

    let mut single = rank_pattern_8bit(9, 7);
    single.set_rect_roi(4, 3, 1, 1).unwrap();
    let oracle = oracle_rank(
        &bridge,
        &single,
        "makeRectangle(4, 3, 1, 1);",
        "run(\"Minimum...\", \"radius=20\");",
    );
    let mut rust = single.clone();
    rust.rank_filter(ij_core::RankFilterKind::Minimum, 20.0)
        .unwrap();
    assert_eq!(
        rust.plane(),
        oracle.plane(),
        "8-bit 1x1 ROI minimum radius 20"
    );
}

/// The Gray8 median must select the `kNPoints/2`-th order statistic
/// (RankFilters.java lines 729-752); radius 20 makes duplicates and ties
/// frequent enough that an off-by-one shows up in the exact bytes.
#[test]
fn parity_8bit_rank_median_radius_20_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let doc = rank_pattern_8bit(21, 15);
    let oracle = oracle_rank(&bridge, &doc, "", "run(\"Median...\", \"radius=20\");");
    let mut rust = doc.clone();
    rust.rank_filter(ij_core::RankFilterKind::Median, 20.0)
        .unwrap();
    assert_ne!(
        doc.plane(),
        oracle.plane(),
        "8-bit radius-20 median was a no-op"
    );
    assert_eq!(rust.plane(), oracle.plane(), "8-bit median radius 20");
}

/// Radius 100 is the documented median limit (RankFilters.java line 188).
/// The 5x4 image keeps the ~31k-sample kernel bounded while still covering
/// cache padding in both directions.
#[test]
fn parity_8bit_rank_median_radius_limit_100_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let doc = rank_pattern_8bit(5, 4);
    let oracle = oracle_rank(&bridge, &doc, "", "run(\"Median...\", \"radius=100\");");
    let mut rust = doc.clone();
    rust.rank_filter(ij_core::RankFilterKind::Median, 100.0)
        .unwrap();
    assert_eq!(rust.plane(), oracle.plane(), "8-bit median radius 100");
}

/// Large-radius ROI cases: a rectangle offset inside the image and an
/// irregular oval mask. RankFilters filters the whole ROI rectangle, so the
/// incremental state must keep advancing across masked-out pixels
/// (RankFilters.java lines 271-275, 546-560); only mask pixels keep the result.
#[test]
fn parity_8bit_rank_large_radius_roi_masks_match_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();

    let mut rect = rank_pattern_8bit(13, 9);
    rect.set_rect_roi(3, 2, 5, 4).unwrap();
    let oracle = oracle_rank(
        &bridge,
        &rect,
        "makeRectangle(3, 2, 5, 4);",
        "run(\"Minimum...\", \"radius=20\");",
    );
    let mut rust = rect.clone();
    rust.rank_filter(ij_core::RankFilterKind::Minimum, 20.0)
        .unwrap();
    assert_eq!(
        rust.plane(),
        oracle.plane(),
        "8-bit rect ROI minimum radius 20"
    );

    let mut oval = rank_pattern_8bit(13, 9);
    oval.set_oval_roi(2, 1, 9, 7).unwrap();
    let oracle = oracle_rank(
        &bridge,
        &oval,
        "makeOval(2, 1, 9, 7);",
        "run(\"Maximum...\", \"radius=20\");",
    );
    let mut rust = oval.clone();
    rust.rank_filter(ij_core::RankFilterKind::Maximum, 20.0)
        .unwrap();
    assert_eq!(
        rust.plane(),
        oracle.plane(),
        "8-bit oval ROI maximum radius 20"
    );
}

/// Radius 1000 is the documented upper limit for Minimum/Maximum
/// (RankFilters.java line 188). Its kernel has ~3.1 million samples, so the
/// test only stays practical because min/max advance incrementally instead of
/// rescanning the full window per pixel.
#[test]
fn parity_8bit_rank_radius_limit_1000_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let doc = rank_pattern_8bit(5, 4);
    for kind in [
        ij_core::RankFilterKind::Minimum,
        ij_core::RankFilterKind::Maximum,
    ] {
        let oracle = oracle_rank(
            &bridge,
            &doc,
            "",
            &format!("run(\"{kind:?}...\", \"radius=1000\");"),
        );
        let mut rust = doc.clone();
        rust.rank_filter(kind, 1000.0).unwrap();
        assert_eq!(rust.plane(), oracle.plane(), "8-bit {kind:?} radius 1000");
    }
}

/// Not part of the CI suite (`#[ignore]` deliberately, so the workspace tests
/// stay fast): prints wall-clock timings for the Gray8 rank filters. Run with
/// `cargo test --release --test oracle_parity -- --ignored --nocapture`.
///
/// The point is to show that a radius-50 window (~7800 samples), a radius-200
/// window (~125k samples) and the radius-1000 kernel limit (~3.1M samples) do
/// not cost a full-window scan (and, for the median, a sort) per pixel. No
/// timing assertion is made: the test only reports.
#[test]
#[ignore]
fn perf_probe_8bit_rank_filters() {
    use std::time::Instant;

    let doc = rank_pattern_8bit(128, 128);
    for (kind, radius) in [
        (ij_core::RankFilterKind::Median, 20.0),
        (ij_core::RankFilterKind::Minimum, 50.0),
        (ij_core::RankFilterKind::Maximum, 50.0),
    ] {
        let mut timed = doc.clone();
        let start = Instant::now();
        timed.rank_filter(kind, radius).unwrap();
        println!("{kind:?} radius {radius} on 128x128: {:?}", start.elapsed());
    }

    let doc = rank_pattern_8bit(32, 32);
    for kind in [
        ij_core::RankFilterKind::Minimum,
        ij_core::RankFilterKind::Maximum,
    ] {
        let mut timed = doc.clone();
        let start = Instant::now();
        timed.rank_filter(kind, 200.0).unwrap();
        println!("{kind:?} radius 200 on 32x32: {:?}", start.elapsed());
    }

    let doc = rank_pattern_8bit(8, 8);
    for kind in [
        ij_core::RankFilterKind::Minimum,
        ij_core::RankFilterKind::Maximum,
    ] {
        let mut timed = doc.clone();
        let start = Instant::now();
        timed.rank_filter(kind, 1000.0).unwrap();
        println!("{kind:?} radius 1000 on 8x8: {:?}", start.elapsed());
    }

    // Scaling probes: the same windows on more pixels. With a full-window scan
    // these grow with pixels * kernel samples; the cache engine keeps them
    // near pixels * kernel height (plus one full scan per image row).
    let doc = rank_pattern_8bit(256, 256);
    for kind in [
        ij_core::RankFilterKind::Minimum,
        ij_core::RankFilterKind::Maximum,
    ] {
        let mut timed = doc.clone();
        let start = Instant::now();
        timed.rank_filter(kind, 50.0).unwrap();
        println!("{kind:?} radius 50 on 256x256: {:?}", start.elapsed());
    }

    let doc = rank_pattern_8bit(16, 16);
    for kind in [
        ij_core::RankFilterKind::Minimum,
        ij_core::RankFilterKind::Maximum,
    ] {
        let mut timed = doc.clone();
        let start = Instant::now();
        timed.rank_filter(kind, 1000.0).unwrap();
        println!("{kind:?} radius 1000 on 16x16: {:?}", start.elapsed());
    }
}

// ---------------------------------------------------------------------------
// Unit 5 (rank filters, 16-bit and 32-bit float): Median/Minimum/Maximum and
// Despeckle for ShortProcessor and FloatProcessor.
//
// RankFilters converts short pixels with `&0xffff` (RankFilters.java line 629)
// and writes them back as `(short)(((int)(v+0.5f))&0xffff)` (line 656); float
// pixels are cached and written with raw float values (lines 631, 512-513).
// The float median is NaN-aware (lines 757-783) and min/max scan with strict
// comparisons starting at `-Float.MAX_VALUE` (lines 670-690), so NaN and the
// sign of zero must match bit-for-bit.
// ---------------------------------------------------------------------------

/// `PixelPlane`'s derived `PartialEq` treats NaN as unequal to itself and
/// collapses `-0.0`/`0.0`, so float parity is compared through the raw bits.
fn assert_planes_bitwise_equal(actual: &PixelPlane, expected: &PixelPlane, context: &str) {
    match (actual, expected) {
        (PixelPlane::Gray8(a), PixelPlane::Gray8(b)) => assert_eq!(a, b, "{context}"),
        (PixelPlane::Gray16(a), PixelPlane::Gray16(b)) => assert_eq!(a, b, "{context}"),
        (PixelPlane::Gray32(a), PixelPlane::Gray32(b)) => {
            assert_eq!(a.len(), b.len(), "{context}: length");
            for (index, (left, right)) in a.iter().zip(b.iter()).enumerate() {
                assert_eq!(
                    left.to_bits(),
                    right.to_bits(),
                    "{context}: pixel {index} differs ({left:?} vs {right:?})"
                );
            }
        }
        _ => panic!("{context}: pixel types differ"),
    }
}

/// Values with duplicates plus the 0/65535 extremes, so u16 min/max and median
/// ties are exercised rather than passing by chance.
fn rank_pattern_16bit(width: u32, height: u32) -> ImageDocument {
    let mut pixels = vec![0u16; (width * height) as usize];
    for y in 0..height {
        for x in 0..width {
            let base = ((x * 17 + y * 29) % 13) as u16;
            let value = match (x + y) % 5 {
                0 => 0,
                1 => 65_535,
                2 => base,
                3 => 65_535 - base,
                _ => base * 4099,
            };
            pixels[(y * width + x) as usize] = value;
        }
    }
    ImageDocument::new("rank16.tif", width, height, PixelPlane::Gray16(pixels)).unwrap()
}

fn rank_impulse_16bit(width: u32, height: u32) -> ImageDocument {
    let mut pixels = vec![0u16; (width * height) as usize];
    let center = (height / 2) * width + width / 2;
    pixels[center as usize] = 65_535;
    ImageDocument::new(
        "rank_impulse16.tif",
        width,
        height,
        PixelPlane::Gray16(pixels),
    )
    .unwrap()
}

/// NaN, both signed zeros and ordinary values, in a deterministic mix.
fn rank_pattern_32bit(width: u32, height: u32) -> ImageDocument {
    let mut pixels = vec![0f32; (width * height) as usize];
    for y in 0..height {
        for x in 0..width {
            let value = match (x * 7 + y * 5) % 16 {
                0 | 7 => f32::NAN,
                1 => -0.0,
                2 => 0.0,
                _ => (((x * 13 + y * 11) % 23) as f32) * 0.5 - 4.0,
            };
            pixels[(y * width + x) as usize] = value;
        }
    }
    ImageDocument::new("rank32.tif", width, height, PixelPlane::Gray32(pixels)).unwrap()
}

/// A 3x3 NaN block whose centre window contains only NaNs; the surrounding
/// values keep min/max order statistics non-trivial.
fn rank_nan_block_32bit() -> ImageDocument {
    let mut pixels = vec![0f32; 49];
    for y in 0..7u32 {
        for x in 0..7u32 {
            let value = if (2..5).contains(&x) && (2..5).contains(&y) {
                f32::NAN
            } else {
                (x as f32) - 3.0 + (y as f32) * 0.25
            };
            pixels[(y * 7 + x) as usize] = value;
        }
    }
    ImageDocument::new("rank_nan_block.tif", 7, 7, PixelPlane::Gray32(pixels)).unwrap()
}

/// Every pixel is non-positive, so Maximum is decided by zero values and the
/// sign of the selected zero is the observable. The mixed signs also expose
/// the incremental path's `newPointsMax >= max` replacement
/// (RankFilters.java lines 547-555) at radius >= 1.75.
fn rank_signed_zero_pattern_32bit(width: u32, height: u32) -> ImageDocument {
    let mut pixels = vec![0f32; (width * height) as usize];
    for y in 0..height {
        for x in 0..width {
            let value = match (x * 5 + y * 3 + x * y) % 11 {
                0 | 4 => -0.0,
                1 | 8 => 0.0,
                5 => -2.5,
                _ => -1.0 - (((x + 2 * y) % 7) as f32),
            };
            pixels[(y * width + x) as usize] = value;
        }
    }
    ImageDocument::new(
        "rank_signed_zero.tif",
        width,
        height,
        PixelPlane::Gray32(pixels),
    )
    .unwrap()
}

fn distinct_stack_gray16(width: u32, height: u32, depth: u32) -> ImageDocument {
    let mut slices = Vec::new();
    for s in 0..depth {
        let mut pixels = vec![0u16; (width * height) as usize];
        for y in 0..height {
            for x in 0..width {
                pixels[(y * width + x) as usize] =
                    ((x * 1013 + y * 2003 + s * 4099) % 65_536) as u16;
            }
        }
        slices.push(PixelPlane::Gray16(pixels));
    }
    ImageDocument::new_stack("stack16.tif", width, height, slices).unwrap()
}

fn distinct_stack_gray32(width: u32, height: u32, depth: u32) -> ImageDocument {
    let mut slices = Vec::new();
    for s in 0..depth {
        let mut pixels = vec![0f32; (width * height) as usize];
        for y in 0..height {
            for x in 0..width {
                let value = match (x * 7 + y * 5 + s * 3) % 16 {
                    0 => f32::NAN,
                    1 => -0.0,
                    2 => 0.0,
                    _ => (((x * 13 + y * 11 + s * 5) % 23) as f32) * 0.5 - 4.0,
                };
                pixels[(y * width + x) as usize] = value;
            }
        }
        slices.push(PixelPlane::Gray32(pixels));
    }
    ImageDocument::new_stack("stack32.tif", width, height, slices).unwrap()
}

#[test]
fn parity_16bit_rank_filters_full_image_match_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let doc = rank_pattern_16bit(9, 7);
    for (kind, command, radius) in [
        (
            ij_core::RankFilterKind::Median,
            "run(\"Median...\", \"radius=0\");",
            0.0,
        ),
        (
            ij_core::RankFilterKind::Median,
            "run(\"Median...\", \"radius=2\");",
            2.0,
        ),
        (
            ij_core::RankFilterKind::Minimum,
            "run(\"Minimum...\", \"radius=3\");",
            3.0,
        ),
        (
            ij_core::RankFilterKind::Maximum,
            "run(\"Maximum...\", \"radius=1\");",
            1.0,
        ),
    ] {
        let oracle = oracle_rank(&bridge, &doc, "", command);
        let mut rust = doc.clone();
        rust.rank_filter(kind, radius).unwrap();
        assert_ne!(doc.plane(), oracle.plane(), "{kind:?} was a no-op");
        assert_planes_bitwise_equal(
            rust.plane(),
            oracle.plane(),
            &format!("16-bit {kind:?} radius {radius}"),
        );
    }
}

#[test]
fn parity_16bit_rank_radius_footprints_match_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let doc = rank_impulse_16bit(9, 9);
    for radius in [0.0, 1.0, 1.5, 2.0, 4.5] {
        let oracle = oracle_rank(
            &bridge,
            &doc,
            "",
            &format!("run(\"Maximum...\", \"radius={radius}\");"),
        );
        let mut rust = doc.clone();
        rust.rank_filter(ij_core::RankFilterKind::Maximum, radius)
            .unwrap();
        assert_planes_bitwise_equal(
            rust.plane(),
            oracle.plane(),
            &format!("16-bit maximum footprint radius {radius}"),
        );
    }
}

#[test]
fn parity_16bit_rank_kernel_larger_than_image_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let doc = rank_pattern_16bit(3, 2);
    let oracle = oracle_rank(&bridge, &doc, "", "run(\"Median...\", \"radius=5\");");
    let mut rust = doc.clone();
    rust.rank_filter(ij_core::RankFilterKind::Median, 5.0)
        .unwrap();
    assert_planes_bitwise_equal(rust.plane(), oracle.plane(), "16-bit radius 5 on 3x2");
}

/// Radius 20/50 on Gray16 covers windows far larger than an image row, where
/// `RankFilters.filterLine` advances min/max incrementally from the entering
/// and leaving kernel columns (RankFilters.java lines 546-560). The image stays
/// at 17x13 so the suite remains fast while the window (~3200 samples for
/// radius 50) is much larger than the 221 pixels themselves.
#[test]
fn parity_16bit_rank_large_radius_minmax_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let doc = rank_pattern_16bit(17, 13);
    for (kind, radius) in [
        (ij_core::RankFilterKind::Minimum, 20.0),
        (ij_core::RankFilterKind::Maximum, 20.0),
        (ij_core::RankFilterKind::Minimum, 50.0),
        (ij_core::RankFilterKind::Maximum, 50.0),
    ] {
        let command = format!("run(\"{kind:?}...\", \"radius={radius}\");");
        let oracle = oracle_rank(&bridge, &doc, "", &command);
        let mut rust = doc.clone();
        rust.rank_filter(kind, radius).unwrap();
        assert_ne!(
            doc.plane(),
            oracle.plane(),
            "16-bit {kind:?} radius {radius} was a no-op"
        );
        assert_planes_bitwise_equal(
            rust.plane(),
            oracle.plane(),
            &format!("16-bit large-radius {kind:?} radius {radius}"),
        );
    }
}

/// Images narrower/shorter than the kernel put the whole inside part into the
/// middle of the cache line, with padding on one or both sides
/// (RankFilters.java lines 619-643). The same is true for a one-pixel ROI.
#[test]
fn parity_16bit_rank_large_radius_tiny_images_and_roi_match_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    for (width, height) in [(1, 6), (6, 1), (1, 1)] {
        let doc = rank_pattern_16bit(width, height);
        let oracle = oracle_rank(&bridge, &doc, "", "run(\"Maximum...\", \"radius=20\");");
        let mut rust = doc.clone();
        rust.rank_filter(ij_core::RankFilterKind::Maximum, 20.0)
            .unwrap();
        assert_planes_bitwise_equal(
            rust.plane(),
            oracle.plane(),
            &format!("16-bit radius-20 maximum on {width}x{height}"),
        );
    }

    let mut single = rank_pattern_16bit(9, 7);
    single.set_rect_roi(4, 3, 1, 1).unwrap();
    let oracle = oracle_rank(
        &bridge,
        &single,
        "makeRectangle(4, 3, 1, 1);",
        "run(\"Minimum...\", \"radius=20\");",
    );
    let mut rust = single.clone();
    rust.rank_filter(ij_core::RankFilterKind::Minimum, 20.0)
        .unwrap();
    assert_planes_bitwise_equal(
        rust.plane(),
        oracle.plane(),
        "16-bit 1x1 ROI minimum radius 20",
    );
}

/// The Gray16 median uses the `kNPoints/2`-th order statistic
/// (RankFilters.java lines 729-752). Radius 20 makes duplicates and ties
/// frequent, so an off-by-one in the selection or a sorted-index replacement
/// shows up in the exact uint16 values.
#[test]
fn parity_16bit_rank_median_radius_20_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let doc = rank_pattern_16bit(21, 15);
    let oracle = oracle_rank(&bridge, &doc, "", "run(\"Median...\", \"radius=20\");");
    let mut rust = doc.clone();
    rust.rank_filter(ij_core::RankFilterKind::Median, 20.0)
        .unwrap();
    assert_ne!(
        doc.plane(),
        oracle.plane(),
        "16-bit radius-20 median was a no-op"
    );
    assert_planes_bitwise_equal(
        rust.plane(),
        oracle.plane(),
        "16-bit median radius 20 on 21x15",
    );
}

/// Large-radius ROI cases: a clipped rectangle offset inside the image and an
/// irregular oval mask. RankFilters filters the whole ROI rectangle, so the
/// incremental min/max state must keep advancing across masked-out pixels
/// (RankFilters.java lines 271-275, 546-560); only mask pixels keep the result.
#[test]
fn parity_16bit_rank_large_radius_roi_masks_match_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();

    let mut rect = rank_pattern_16bit(13, 9);
    rect.set_rect_roi(3, 2, 5, 4).unwrap();
    let oracle = oracle_rank(
        &bridge,
        &rect,
        "makeRectangle(3, 2, 5, 4);",
        "run(\"Minimum...\", \"radius=20\");",
    );
    let mut rust = rect.clone();
    rust.rank_filter(ij_core::RankFilterKind::Minimum, 20.0)
        .unwrap();
    assert_planes_bitwise_equal(
        rust.plane(),
        oracle.plane(),
        "16-bit rect ROI minimum radius 20",
    );

    let mut oval = rank_pattern_16bit(13, 9);
    oval.set_oval_roi(2, 1, 9, 7).unwrap();
    let oracle = oracle_rank(
        &bridge,
        &oval,
        "makeOval(2, 1, 9, 7);",
        "run(\"Maximum...\", \"radius=20\");",
    );
    let mut rust = oval.clone();
    rust.rank_filter(ij_core::RankFilterKind::Maximum, 20.0)
        .unwrap();
    assert_planes_bitwise_equal(
        rust.plane(),
        oracle.plane(),
        "16-bit oval ROI maximum radius 20",
    );
}

/// Radius 1000 is the documented upper limit for Minimum/Maximum
/// (RankFilters.java line 188). Its kernel has ~3.1 million samples, so this
/// test is only practical because min/max do not rescan the full window per
/// pixel; it stays bounded by using a 5x4 image.
#[test]
fn parity_16bit_rank_radius_limit_1000_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let doc = rank_pattern_16bit(5, 4);
    for kind in [
        ij_core::RankFilterKind::Minimum,
        ij_core::RankFilterKind::Maximum,
    ] {
        let oracle = oracle_rank(
            &bridge,
            &doc,
            "",
            &format!("run(\"{kind:?}...\", \"radius=1000\");"),
        );
        let mut rust = doc.clone();
        rust.rank_filter(kind, 1000.0).unwrap();
        assert_planes_bitwise_equal(
            rust.plane(),
            oracle.plane(),
            &format!("16-bit {kind:?} radius 1000"),
        );
    }
}

/// Not part of the CI suite (`#[ignore]` deliberately, so the workspace tests
/// stay fast): prints wall-clock timings for the Gray16 rank filters on a
/// realistic moderate image. Run with
/// `cargo test --release --test oracle_parity -- --ignored --nocapture`.
///
/// The point is to show that a radius-50 window (~7800 samples) and a
/// radius-200 window (~125k samples) do not cost a full-window scan per pixel.
/// No timing assertion is made: the test only reports.
#[test]
#[ignore]
fn perf_probe_16bit_rank_filters() {
    use std::time::Instant;

    let doc = rank_pattern_16bit(256, 256);
    for (kind, radius) in [
        (ij_core::RankFilterKind::Median, 20.0),
        (ij_core::RankFilterKind::Minimum, 50.0),
        (ij_core::RankFilterKind::Maximum, 50.0),
    ] {
        let mut timed = doc.clone();
        let start = Instant::now();
        timed.rank_filter(kind, radius).unwrap();
        println!("{kind:?} radius {radius} on 256x256: {:?}", start.elapsed());
    }

    let doc = rank_pattern_16bit(64, 64);
    for kind in [
        ij_core::RankFilterKind::Minimum,
        ij_core::RankFilterKind::Maximum,
    ] {
        let mut timed = doc.clone();
        let start = Instant::now();
        timed.rank_filter(kind, 200.0).unwrap();
        println!("{kind:?} radius 200 on 64x64: {:?}", start.elapsed());
    }
}

#[test]
fn parity_16bit_rank_roi_masks_match_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();

    let mut rect = rank_pattern_16bit(8, 6);
    rect.set_rect_roi(2, 1, 4, 3).unwrap();
    let oracle = oracle_rank(
        &bridge,
        &rect,
        "makeRectangle(2, 1, 4, 3);",
        "run(\"Median...\", \"radius=2\");",
    );
    let mut rust = rect.clone();
    rust.rank_filter(ij_core::RankFilterKind::Median, 2.0)
        .unwrap();
    assert_ne!(rect.plane(), oracle.plane(), "rect median was a no-op");
    assert_planes_bitwise_equal(rust.plane(), oracle.plane(), "16-bit rect ROI median");

    let mut oval = rank_pattern_16bit(8, 6);
    oval.set_oval_roi(1, 1, 6, 4).unwrap();
    let oracle = oracle_rank(
        &bridge,
        &oval,
        "makeOval(1, 1, 6, 4);",
        "run(\"Minimum...\", \"radius=2\");",
    );
    let mut rust = oval.clone();
    rust.rank_filter(ij_core::RankFilterKind::Minimum, 2.0)
        .unwrap();
    assert_planes_bitwise_equal(rust.plane(), oracle.plane(), "16-bit oval ROI minimum");

    // A clipped oval keeps the mask's shape at the image border.
    let mut clipped = rank_pattern_16bit(8, 6);
    clipped.set_oval_roi(-2, -1, 6, 4).unwrap();
    let oracle = oracle_rank(
        &bridge,
        &clipped,
        "makeOval(-2, -1, 6, 4);",
        "run(\"Maximum...\", \"radius=3\");",
    );
    let mut rust = clipped.clone();
    rust.rank_filter(ij_core::RankFilterKind::Maximum, 3.0)
        .unwrap();
    assert_planes_bitwise_equal(
        rust.plane(),
        oracle.plane(),
        "16-bit clipped oval ROI maximum",
    );
}

#[test]
fn parity_16bit_despeckle_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let doc = rank_pattern_16bit(9, 7);
    let oracle = oracle_rank(&bridge, &doc, "", "run(\"Despeckle\");");
    let mut rust = doc.clone();
    rust.despeckle().unwrap();
    assert_ne!(doc.plane(), oracle.plane(), "16-bit despeckle was a no-op");
    assert_planes_bitwise_equal(rust.plane(), oracle.plane(), "16-bit despeckle");
}

#[test]
fn parity_16bit_rank_stack_slice_and_undo_match_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = distinct_stack_gray16(6, 4, 3);
    doc.set_slice(2).unwrap();
    let oracle = bridge
        .run_macro_on_image(&doc, "setSlice(2); run(\"Median...\", \"radius=2 slice\");")
        .unwrap()
        .output_image
        .expect("oracle must return saved TIFF");
    let original = doc.slices.clone();

    let mut rust = doc.clone();
    rust.rank_filter(ij_core::RankFilterKind::Median, 2.0)
        .unwrap();
    let oracle_slices = oracle.slices;
    assert_eq!(rust.slices.len(), oracle_slices.len());
    for (index, (left, right)) in rust.slices.iter().zip(oracle_slices.iter()).enumerate() {
        assert_planes_bitwise_equal(left, right, &format!("16-bit stack slice {index}"));
    }
    assert_planes_bitwise_equal(&rust.slices[0], &original[0], "16-bit slice 1 untouched");
    assert_planes_bitwise_equal(&rust.slices[2], &original[2], "16-bit slice 3 untouched");
    assert_ne!(rust.slices[1], original[1], "16-bit slice 2 unchanged");

    let undo_doc = rank_pattern_16bit(9, 7);
    let oracle_undo = oracle_rank(
        &bridge,
        &undo_doc,
        "",
        "run(\"Minimum...\", \"radius=2\"); run(\"Undo\");",
    );
    let mut rust = undo_doc.clone();
    rust.rank_filter(ij_core::RankFilterKind::Minimum, 2.0)
        .unwrap();
    assert!(rust.snapshot.is_some(), "rank filter must install Undo");
    assert!(rust.undo());
    assert_planes_bitwise_equal(rust.plane(), oracle_undo.plane(), "16-bit undo");
}

#[test]
fn parity_32bit_median_nan_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let doc = rank_pattern_32bit(7, 6);
    for radius in [0.0, 1.0, 1.75, 2.85] {
        let oracle = oracle_rank(
            &bridge,
            &doc,
            "",
            &format!("run(\"Median...\", \"radius={radius}\");"),
        );
        let mut rust = doc.clone();
        rust.rank_filter(ij_core::RankFilterKind::Median, radius)
            .unwrap();
        assert_planes_bitwise_equal(
            rust.plane(),
            oracle.plane(),
            &format!("32-bit median radius {radius}"),
        );
    }
    // The pattern must actually carry NaNs, else the NaN-aware path is untested.
    let output = oracle_rank(&bridge, &doc, "", "run(\"Median...\", \"radius=1\");");
    let PixelPlane::Gray32(pixels) = output.plane() else {
        panic!("oracle changed the pixel type");
    };
    assert!(
        pixels.iter().any(|value| value.is_nan()),
        "expected the NaN-aware median to leave NaNs in the output"
    );

    // A kernel larger than the image exercises the padded cache in both
    // directions (RankFilters.java lines 599-643).
    let tiny = rank_pattern_32bit(3, 2);
    let oracle = oracle_rank(&bridge, &tiny, "", "run(\"Median...\", \"radius=5\");");
    let mut rust = tiny.clone();
    rust.rank_filter(ij_core::RankFilterKind::Median, 5.0)
        .unwrap();
    assert_planes_bitwise_equal(rust.plane(), oracle.plane(), "32-bit radius 5 on 3x2");
}

#[test]
fn parity_32bit_minimum_maximum_nan_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();

    // The centre of the NaN block sees a window that is entirely NaN:
    // `getAreaMax` starts at `-Float.MAX_VALUE` and never updates, so minimum
    // writes `+Float.MAX_VALUE` and maximum writes `-Float.MAX_VALUE`
    // (RankFilters.java lines 670-690, 538-559).
    let nan_block = rank_nan_block_32bit();
    for (kind, radius) in [
        (ij_core::RankFilterKind::Minimum, 1.0),
        (ij_core::RankFilterKind::Maximum, 1.0),
        // Radius >= 2 uses the incremental min/max path, which must also stay
        // at ±Float.MAX_VALUE when a whole window is NaN.
        (ij_core::RankFilterKind::Minimum, 2.0),
        (ij_core::RankFilterKind::Maximum, 2.0),
    ] {
        let command = format!("run(\"{kind:?}...\", \"radius={radius}\");");
        let oracle = oracle_rank(&bridge, &nan_block, "", &command);
        let mut rust = nan_block.clone();
        rust.rank_filter(kind, radius).unwrap();
        assert_planes_bitwise_equal(
            rust.plane(),
            oracle.plane(),
            &format!("32-bit all-NaN window {kind:?} radius {radius}"),
        );
        let PixelPlane::Gray32(pixels) = oracle.plane() else {
            panic!("oracle changed the pixel type");
        };
        if radius == 1.0 {
            // The 3x3 NaN block only fills the radius-1 window; radius 2 pulls
            // the surrounding finite values into the centre window.
            let expected = if kind == ij_core::RankFilterKind::Minimum {
                f32::MAX
            } else {
                -f32::MAX
            };
            assert_eq!(
                pixels[3 * 7 + 3],
                expected,
                "oracle all-NaN window must saturate to ±Float.MAX_VALUE"
            );
        }
    }

    // NaN neighbours are skipped by the strict comparisons; radius 2 exercises
    // the incremental min/max path.
    let doc = rank_pattern_32bit(9, 7);
    for (kind, command) in [
        (
            ij_core::RankFilterKind::Minimum,
            "run(\"Minimum...\", \"radius=2\");",
        ),
        (
            ij_core::RankFilterKind::Maximum,
            "run(\"Maximum...\", \"radius=2.5\");",
        ),
    ] {
        let oracle = oracle_rank(&bridge, &doc, "", command);
        let mut rust = doc.clone();
        let radius = if kind == ij_core::RankFilterKind::Minimum {
            2.0
        } else {
            2.5
        };
        rust.rank_filter(kind, radius).unwrap();
        assert_planes_bitwise_equal(
            rust.plane(),
            oracle.plane(),
            &format!("32-bit NaN-aware {kind:?} radius {radius}"),
        );
    }
}

#[test]
fn parity_32bit_rank_signed_zero_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let max_doc = rank_signed_zero_pattern_32bit(8, 7);
    // Negating keeps every zero and makes all values non-negative, so Minimum
    // is decided by zeros just like Maximum is on the original pattern.
    let negated = match max_doc.plane() {
        PixelPlane::Gray32(pixels) => pixels.iter().map(|value| -value).collect(),
        _ => panic!("unexpected pixel type"),
    };
    let min_doc = ImageDocument::new(
        "rank_signed_zero_min.tif",
        8,
        7,
        PixelPlane::Gray32(negated),
    )
    .unwrap();

    for (kind, doc, radius) in [
        (ij_core::RankFilterKind::Maximum, &max_doc, 0.0),
        (ij_core::RankFilterKind::Maximum, &max_doc, 1.0),
        (ij_core::RankFilterKind::Maximum, &max_doc, 2.85),
        (ij_core::RankFilterKind::Minimum, &min_doc, 1.0),
        (ij_core::RankFilterKind::Minimum, &min_doc, 2.85),
    ] {
        let command = format!("run(\"{kind:?}...\", \"radius={radius}\");");
        let oracle = oracle_rank(&bridge, doc, "", &command);
        let mut rust = doc.clone();
        rust.rank_filter(kind, radius).unwrap();
        assert_planes_bitwise_equal(
            rust.plane(),
            oracle.plane(),
            &format!("32-bit signed-zero {kind:?} radius {radius}"),
        );
        let PixelPlane::Gray32(pixels) = oracle.plane() else {
            panic!("oracle changed the pixel type");
        };
        assert!(
            pixels.iter().any(|value| value.to_bits() == 0x8000_0000)
                && pixels.iter().any(|value| value.to_bits() == 0x0000_0000),
            "oracle {kind:?} radius {radius} output must contain both signed zeros"
        );
    }
}

#[test]
fn parity_32bit_rank_roi_masks_match_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();

    let mut rect = rank_pattern_32bit(8, 6);
    rect.set_rect_roi(2, 1, 4, 3).unwrap();
    let oracle = oracle_rank(
        &bridge,
        &rect,
        "makeRectangle(2, 1, 4, 3);",
        "run(\"Maximum...\", \"radius=2\");",
    );
    let mut rust = rect.clone();
    rust.rank_filter(ij_core::RankFilterKind::Maximum, 2.0)
        .unwrap();
    assert_planes_bitwise_equal(rust.plane(), oracle.plane(), "32-bit rect ROI maximum");

    let mut oval = rank_pattern_32bit(8, 6);
    oval.set_oval_roi(1, 1, 6, 4).unwrap();
    let oracle = oracle_rank(
        &bridge,
        &oval,
        "makeOval(1, 1, 6, 4);",
        "run(\"Median...\", \"radius=1\");",
    );
    let mut rust = oval.clone();
    rust.rank_filter(ij_core::RankFilterKind::Median, 1.0)
        .unwrap();
    assert_planes_bitwise_equal(rust.plane(), oracle.plane(), "32-bit oval ROI median");

    // Radius >= 1.75 keeps the incremental min/max state running across the
    // masked-out pixels of the oval; only the mask's own pixels keep the
    // filtered value.
    let mut oval_max = rank_pattern_32bit(8, 6);
    oval_max.set_oval_roi(1, 1, 6, 4).unwrap();
    let oracle = oracle_rank(
        &bridge,
        &oval_max,
        "makeOval(1, 1, 6, 4);",
        "run(\"Maximum...\", \"radius=2.85\");",
    );
    let mut rust = oval_max.clone();
    rust.rank_filter(ij_core::RankFilterKind::Maximum, 2.85)
        .unwrap();
    assert_planes_bitwise_equal(
        rust.plane(),
        oracle.plane(),
        "32-bit oval ROI maximum radius 2.85",
    );
}

#[test]
fn parity_32bit_despeckle_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let doc = rank_pattern_32bit(9, 7);
    let oracle = oracle_rank(&bridge, &doc, "", "run(\"Despeckle\");");
    let mut rust = doc.clone();
    rust.despeckle().unwrap();
    assert_planes_bitwise_equal(rust.plane(), oracle.plane(), "32-bit despeckle");
}

#[test]
fn parity_32bit_rank_stack_slice_and_undo_match_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = distinct_stack_gray32(6, 4, 3);
    doc.set_slice(2).unwrap();
    let oracle = bridge
        .run_macro_on_image(
            &doc,
            "setSlice(2); run(\"Minimum...\", \"radius=2 slice\");",
        )
        .unwrap()
        .output_image
        .expect("oracle must return saved TIFF");
    let original = doc.slices.clone();

    let mut rust = doc.clone();
    rust.rank_filter(ij_core::RankFilterKind::Minimum, 2.0)
        .unwrap();
    let oracle_slices = oracle.slices;
    assert_eq!(rust.slices.len(), oracle_slices.len());
    for (index, (left, right)) in rust.slices.iter().zip(oracle_slices.iter()).enumerate() {
        assert_planes_bitwise_equal(left, right, &format!("32-bit stack slice {index}"));
    }
    assert_planes_bitwise_equal(&rust.slices[0], &original[0], "32-bit slice 1 untouched");
    assert_planes_bitwise_equal(&rust.slices[2], &original[2], "32-bit slice 3 untouched");
    let unchanged = matches!(
        (&rust.slices[1], &original[1]),
        (PixelPlane::Gray32(a), PixelPlane::Gray32(b))
            if a.iter().zip(b.iter()).all(|(l, r)| l.to_bits() == r.to_bits())
    );
    assert!(!unchanged, "32-bit slice 2 unchanged");

    let undo_doc = rank_pattern_32bit(9, 7);
    let oracle_undo = oracle_rank(
        &bridge,
        &undo_doc,
        "",
        "run(\"Median...\", \"radius=2\"); run(\"Undo\");",
    );
    let mut rust = undo_doc.clone();
    rust.rank_filter(ij_core::RankFilterKind::Median, 2.0)
        .unwrap();
    assert!(rust.snapshot.is_some(), "rank filter must install Undo");
    assert!(rust.undo());
    assert_planes_bitwise_equal(rust.plane(), oracle_undo.plane(), "32-bit undo");
}

// ---------------------------------------------------------------------------
// TIFF ROI metadata (FileSaver/TiffEncoder `roi ` metadata block).
// ---------------------------------------------------------------------------

/// Java `Line.getBounds()` uses `Roi.setIntBounds` with the open-line
/// convention, which encloses the one-pixel draw area (`Line.java` 350-359,
/// `Roi.java` 538-550). The Rust core line bounds keep the raw endpoint span.
fn java_line_bounds(line: &LineRoi) -> (i32, i32, i32, i32) {
    let min_x = line.x1.min(line.x2);
    let min_y = line.y1.min(line.y2);
    let x = (min_x as f64 + 0.5).floor() as i32;
    let y = (min_y as f64 + 0.5).floor() as i32;
    let width = ((min_x as f64 + (line.x2 - line.x1).abs() as f64 + 1.5).floor() as i32) - x;
    let height = ((min_y as f64 + (line.y2 - line.y1).abs() as f64 + 1.5).floor() as i32) - y;
    (x, y, width, height)
}

#[test]
fn parity_java_saved_roi_metadata_loads_in_rust_exactly() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let rect = temp_tiff("java_roi_rect.tif");
    let oval = temp_tiff("java_roi_oval.tif");
    let polygon = temp_tiff("java_roi_polygon.tif");
    let line = temp_tiff("java_roi_line.tif");

    bridge
        .eval_macro(&format!(
            "newImage('rect','8-bit',16,16,1); makeRectangle(2,3,8,6); saveAs('Tiff','{rect}'); \
             newImage('oval','8-bit',16,16,1); makeOval(2,3,8,6); saveAs('Tiff','{oval}'); \
             newImage('polygon','8-bit',16,16,1); makePolygon(2,2,12,3,9,11,3,8); \
             saveAs('Tiff','{polygon}'); \
             newImage('line','8-bit',16,16,1); makeLine(2,3,10,7); saveAs('Tiff','{line}');",
            rect = rect.display(),
            oval = oval.display(),
            polygon = polygon.display(),
            line = line.display(),
        ))
        .unwrap();

    // FileSaver writes the ROI through RoiEncoder into the TIFF `roi `
    // metadata block. FileOpener decodes it back into the selection.
    assert_eq!(
        load_tiff(&rect).unwrap().roi,
        Some(Roi::Rect(RectRoi {
            x: 2,
            y: 3,
            width: 8,
            height: 6
        }))
    );
    assert_eq!(
        load_tiff(&oval).unwrap().roi,
        Some(Roi::Oval(RectRoi {
            x: 2,
            y: 3,
            width: 8,
            height: 6
        }))
    );
    assert_eq!(
        load_tiff(&polygon).unwrap().roi,
        Some(Roi::Polygon(PolygonRoi {
            xpoints: vec![2, 12, 9, 3],
            ypoints: vec![2, 3, 11, 8],
        }))
    );
    assert_eq!(
        load_tiff(&line).unwrap().roi,
        Some(Roi::Line(LineRoi {
            x1: 2,
            y1: 3,
            x2: 10,
            y2: 7
        }))
    );
}

#[test]
fn parity_java_saved_off_image_roi_metadata_loads_in_rust_exactly() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let rect = temp_tiff("java_roi_rect_off.tif");
    let oval = temp_tiff("java_roi_oval_off.tif");
    let polygon = temp_tiff("java_roi_polygon_off.tif");
    let line = temp_tiff("java_roi_line_off.tif");

    bridge
        .eval_macro(&format!(
            "newImage('rect','8-bit',16,16,1); makeRectangle(-7,-9,10,12); saveAs('Tiff','{rect}'); \
             newImage('oval','8-bit',16,16,1); makeOval(-3,-2,8,7); saveAs('Tiff','{oval}'); \
             newImage('polygon','8-bit',16,16,1); makePolygon(-2,-1,8,2,5,9,1,6); \
             saveAs('Tiff','{polygon}'); \
             newImage('line','8-bit',16,16,1); makeLine(-3,2,7,-4); saveAs('Tiff','{line}');",
            rect = rect.display(),
            oval = oval.display(),
            polygon = polygon.display(),
            line = line.display(),
        ))
        .unwrap();

    assert_eq!(
        load_tiff(&rect).unwrap().roi,
        Some(Roi::Rect(RectRoi {
            x: -7,
            y: -9,
            width: 10,
            height: 12
        }))
    );
    assert_eq!(
        load_tiff(&oval).unwrap().roi,
        Some(Roi::Oval(RectRoi {
            x: -3,
            y: -2,
            width: 8,
            height: 7
        }))
    );
    assert_eq!(
        load_tiff(&polygon).unwrap().roi,
        Some(Roi::Polygon(PolygonRoi {
            xpoints: vec![-2, 8, 5, 1],
            ypoints: vec![-1, 2, 9, 6],
        }))
    );
    assert_eq!(
        load_tiff(&line).unwrap().roi,
        Some(Roi::Line(LineRoi {
            x1: -3,
            y1: 2,
            x2: 7,
            y2: -4,
        }))
    );
}

#[test]
fn parity_rust_saved_roi_metadata_is_read_exactly_by_java() {
    let rect = temp_tiff("rust_roi_rect.tif");
    let oval = temp_tiff("rust_roi_oval.tif");
    let polygon = temp_tiff("rust_roi_polygon.tif");
    let line = temp_tiff("rust_roi_line.tif");
    let large_rect = temp_tiff("rust_roi_large_rect.tif");
    let large_polygon = temp_tiff("rust_roi_large_polygon.tif");

    let mut doc = ImageDocument::new_ramp_8bit("rect", 16, 16).unwrap();
    doc.set_rect_roi(2, 3, 8, 6).unwrap();
    save_tiff(&rect, &doc).unwrap();

    let mut doc = ImageDocument::new_ramp_8bit("oval", 16, 16).unwrap();
    doc.set_oval_roi(2, 3, 8, 6).unwrap();
    save_tiff(&oval, &doc).unwrap();

    let mut doc = ImageDocument::new_ramp_8bit("polygon", 16, 16).unwrap();
    doc.set_polygon_roi(&[(2, 2), (12, 3), (9, 11), (3, 8)])
        .unwrap();
    save_tiff(&polygon, &doc).unwrap();

    let mut doc = ImageDocument::new_ramp_8bit("line", 16, 16).unwrap();
    doc.set_line_roi(2, 3, 10, 7);
    save_tiff(&line, &doc).unwrap();

    // These bounds require the float (subpixel) ROI encoding because
    // RoiDecoder.getShort reads values below -5000 as unsigned.
    let mut doc = ImageDocument::new_ramp_8bit("large_rect", 16, 16).unwrap();
    doc.set_rect_roi(-6000, 40_000, 100, 80).unwrap();
    save_tiff(&large_rect, &doc).unwrap();

    let mut doc = ImageDocument::new_ramp_8bit("large_polygon", 16, 16).unwrap();
    doc.set_polygon_roi(&[(-6000, 0), (-5900, 50), (100, 20)])
        .unwrap();
    save_tiff(&large_polygon, &doc).unwrap();

    let source = format!(
        r#"
import ij.ImagePlus;
import ij.io.Opener;
import ij.gui.*;
import java.awt.Polygon;
import java.awt.Rectangle;
public class RustRoiTiffProbe {{
    static void dump(int index, String path) throws Exception {{
        ImagePlus imp = new Opener().openImage(path);
        Roi roi = imp.getRoi();
        System.out.print("CASE" + index + ":");
        if (roi == null) {{ System.out.println("null"); return; }}
        Rectangle b = roi.getBounds();
        System.out.print("type=" + roi.getType() + "|bounds=" + b.x + "," + b.y + "," + b.width + "," + b.height);
        if (roi instanceof PolygonRoi) {{
            Polygon p = roi.getPolygon();
            System.out.print("|points=");
            for (int i = 0; i < p.npoints; i++)
                System.out.print(p.xpoints[i] + "," + p.ypoints[i] + ";");
        }}
        if (roi instanceof ij.gui.Line) {{
            ij.gui.Line l = (ij.gui.Line) roi;
            System.out.print("|line=" + l.x1d + "," + l.y1d + "," + l.x2d + "," + l.y2d);
        }}
        System.out.println();
    }}
    public static void main(String[] args) throws Exception {{
        dump(0, "{rect}");
        dump(1, "{oval}");
        dump(2, "{polygon}");
        dump(3, "{line}");
        dump(4, "{large_rect}");
        dump(5, "{large_polygon}");
    }}
}}
"#,
        rect = rect.display(),
        oval = oval.display(),
        polygon = polygon.display(),
        line = line.display(),
        large_rect = large_rect.display(),
        large_polygon = large_polygon.display(),
    );
    let stdout = run_java_harness("RustRoiTiffProbe", &source);
    let cases: Vec<&str> = stdout
        .lines()
        .filter(|line| line.starts_with("CASE"))
        .collect();
    assert_eq!(cases.len(), 6, "unexpected Java output:\n{stdout}");

    assert_eq!(cases[0], "CASE0:type=0|bounds=2,3,8,6");
    assert_eq!(cases[1], "CASE1:type=1|bounds=2,3,8,6");
    assert_eq!(
        cases[2],
        "CASE2:type=2|bounds=2,2,10,9|points=2,2;12,3;9,11;3,8;"
    );
    let expected_line = java_line_bounds(&LineRoi {
        x1: 2,
        y1: 3,
        x2: 10,
        y2: 7,
    });
    assert_eq!(
        cases[3],
        format!(
            "CASE3:type=5|bounds={},{},{},{}|line=2.0,3.0,10.0,7.0",
            expected_line.0, expected_line.1, expected_line.2, expected_line.3
        )
        .as_str()
    );
    assert_eq!(cases[4], "CASE4:type=0|bounds=-6000,40000,100,80");
    assert_eq!(
        cases[5],
        "CASE5:type=2|bounds=-6000,0,6100,50|points=-6000,0;-5900,50;100,20;"
    );
}

/// Builds a version-229 ROI record. The TIFF ROI payload is always big-endian
/// (`RoiEncoder.putShort/putFloat`), independent of the TIFF byte order.
fn roi_payload_for_test(
    roi_type: u8,
    bounds: [i16; 4],
    options: u16,
    coordinates: &[(i16, i16)],
    float_rect: Option<[f32; 4]>,
) -> Vec<u8> {
    let n = coordinates.len() as u16;
    let length = if n == 0 {
        128
    } else {
        64 + coordinates.len() * 4
    };
    let mut record = vec![0u8; length];
    record[..4].copy_from_slice(b"Iout");
    record[4..6].copy_from_slice(&229u16.to_be_bytes());
    record[6] = roi_type;
    record[8..10].copy_from_slice(&bounds[0].to_be_bytes());
    record[10..12].copy_from_slice(&bounds[1].to_be_bytes());
    record[12..14].copy_from_slice(&bounds[2].to_be_bytes());
    record[14..16].copy_from_slice(&bounds[3].to_be_bytes());
    record[16..18].copy_from_slice(&n.to_be_bytes());
    if let Some(values) = float_rect {
        for (index, value) in values.iter().enumerate() {
            let offset = 18 + index * 4;
            record[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
        }
    }
    record[50..52].copy_from_slice(&options.to_be_bytes());
    for (index, &(x, y)) in coordinates.iter().enumerate() {
        let x_offset = 64 + index * 2;
        let y_offset = 64 + coordinates.len() * 2 + index * 2;
        record[x_offset..x_offset + 2].copy_from_slice(&x.to_be_bytes());
        record[y_offset..y_offset + 2].copy_from_slice(&y.to_be_bytes());
    }
    record
}

/// Builds a version-229 polygon ROI record, optionally with the float
/// (subpixel) coordinate arrays and an explicit Header2 offset. The integer
/// relative coordinates are always written, mirroring RoiEncoder.java
/// 271-286, even when ImageJ decodes the float array.
fn roi_polygon_payload_for_test(
    bounds: [i32; 4],
    options: u16,
    coordinates: &[(i16, i16)],
    float_coordinates: Option<&[(f32, f32)]>,
    header2_offset: usize,
) -> Vec<u8> {
    assert!(float_coordinates.is_none_or(|points| points.len() == coordinates.len()));
    let coordinate_end =
        ROI_POLYGON_HEADER + coordinates.len() * 4 + float_coordinates.map_or(0, |p| p.len() * 8);
    let length = coordinate_end
        .max(header2_offset.saturating_add(ROI_POLYGON_HEADER2))
        .max(ROI_POLYGON_HEADER);
    let mut record = vec![0u8; length];
    record[..4].copy_from_slice(b"Iout");
    record[4..6].copy_from_slice(&229u16.to_be_bytes());
    record[6] = 0; // Roi.POLYGON
    for (index, value) in bounds.iter().enumerate() {
        let offset = 8 + index * 2;
        record[offset..offset + 2].copy_from_slice(&(*value as u16).to_be_bytes());
    }
    record[16..18].copy_from_slice(&(coordinates.len() as u16).to_be_bytes());
    record[50..52].copy_from_slice(&options.to_be_bytes());
    record[60..64].copy_from_slice(&(header2_offset as i32).to_be_bytes());
    for (index, &(x, y)) in coordinates.iter().enumerate() {
        let x_offset = ROI_POLYGON_HEADER + index * 2;
        let y_offset = ROI_POLYGON_HEADER + coordinates.len() * 2 + index * 2;
        record[x_offset..x_offset + 2].copy_from_slice(&x.to_be_bytes());
        record[y_offset..y_offset + 2].copy_from_slice(&y.to_be_bytes());
    }
    if let Some(float_coordinates) = float_coordinates {
        let x_base = ROI_POLYGON_HEADER + coordinates.len() * 4;
        let y_base = x_base + coordinates.len() * 4;
        for (index, &(x, y)) in float_coordinates.iter().enumerate() {
            let x_offset = x_base + index * 4;
            let y_offset = y_base + index * 4;
            record[x_offset..x_offset + 4].copy_from_slice(&x.to_be_bytes());
            record[y_offset..y_offset + 4].copy_from_slice(&y.to_be_bytes());
        }
    }
    record
}

const ROI_POLYGON_HEADER: usize = 64;
const ROI_POLYGON_HEADER2: usize = 64;

fn roi_metadata_blobs(roi: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let mut counts = Vec::new();
    counts.extend_from_slice(&12u32.to_le_bytes());
    counts.extend_from_slice(&(roi.len() as u32).to_le_bytes());
    let mut metadata = Vec::new();
    metadata.extend_from_slice(&0x494a_494au32.to_le_bytes());
    metadata.extend_from_slice(&0x726f_6920u32.to_le_bytes());
    metadata.extend_from_slice(&1u32.to_le_bytes());
    metadata.extend_from_slice(roi);
    (counts, metadata)
}

fn tiff_with_roi_metadata(name: &str, roi: &[u8]) -> PathBuf {
    let data = [0u8; 16];
    let (counts, metadata) = roi_metadata_blobs(roi);
    let mut entries = base_fixture_entries(4, 4, 8, 1, 1, 1, 1, data.len() as u32);
    entries.push((50838, 4, 2, FixtureValue::Blob(0)));
    entries.push((50839, 1, metadata.len() as u32, FixtureValue::Blob(1)));
    write_fixture(
        name,
        &build_tiff_fixture(&data, &[counts, metadata], &entries),
    )
}

#[test]
fn parity_unsupported_roi_metadata_is_rejected_by_rust_not_java() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();

    // Fractional subpixel rectangle: ImageJ keeps the selection; the integer
    // Rust ROI model cannot represent it without changing the mask.
    let fractional = tiff_with_roi_metadata(
        "roi_fractional_subpixel.tif",
        &roi_payload_for_test(1, [3, 2, 9, 10], 128, &[], Some([2.5, 3.0, 8.0, 6.0])),
    );
    let oracle = bridge
        .eval_macro(&format!(
            "open(\"{p}\"); print(\"TYPE:\"+selectionType());",
            p = fractional.display()
        ))
        .unwrap();
    assert_eq!(oracle_line(&oracle.stdout, "TYPE:"), "TYPE:0");
    let error = load_tiff(&fractional).unwrap_err();
    assert!(
        error.to_string().contains("ROI") && error.to_string().contains("subpixel"),
        "expected an explicit subpixel ROI error, got: {error}"
    );

    // Freehand ROI (type 7): ImageJ opens it as Roi.FREEROI.
    let freehand = tiff_with_roi_metadata(
        "roi_freehand_type.tif",
        &roi_payload_for_test(7, [5, 6, 12, 13], 0, &[(0, 0), (5, 0), (2, 6)], None),
    );
    let oracle = bridge
        .eval_macro(&format!(
            "open(\"{p}\"); print(\"TYPE:\"+selectionType());",
            p = freehand.display()
        ))
        .unwrap();
    assert_eq!(oracle_line(&oracle.stdout, "TYPE:"), "TYPE:3");
    let error = load_tiff(&freehand).unwrap_err();
    assert!(
        error.to_string().contains("ROI") && error.to_string().contains("type"),
        "expected an explicit unsupported ROI type error, got: {error}"
    );
}

#[test]
fn session_open_rejects_unsupported_roi_metadata_clearly() {
    let path = tiff_with_roi_metadata(
        "roi_fractional_subpixel_session.tif",
        &roi_payload_for_test(1, [3, 2, 9, 10], 128, &[], Some([2.5, 3.0, 8.0, 6.0])),
    );
    let mut session = Session::new();
    let error = session
        .dispatch(Command::OpenTiff { path })
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("ROI") && error.contains("subpixel"),
        "expected the session to surface an explicit ROI error, got: {error}"
    );
    assert!(
        session.active_doc().is_err(),
        "a rejected open must not install a partial image"
    );
}

#[test]
fn parity_degenerate_polygon_roi_is_rejected_instead_of_dropped_by_java() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();

    // Three distinct list entries that all decode to the same pixel: the
    // PolygonRoi local bounds are 0x0, and ImagePlus.setRoi deletes a
    // non-point/line selection whose width *and* height are zero
    // (ImagePlus.java 2073-2076).
    let dropped = tiff_with_roi_metadata(
        "roi_degenerate_polygon.tif",
        &roi_polygon_payload_for_test([5, 5, 5, 5], 0, &[(0, 0), (0, 0), (0, 0)], None, 0),
    );
    let oracle = bridge
        .eval_macro(&format!(
            "open(\"{path}\"); print(\"SEL:\"+selectionType());",
            path = dropped.display()
        ))
        .unwrap();
    assert_eq!(
        oracle_line(&oracle.stdout, "SEL:"),
        "SEL:-1",
        "original ImageJ must drop this selection"
    );

    // Rust must not silently install a selection that Java drops when the
    // same TIFF is opened.
    let error = load_tiff(&dropped).unwrap_err().to_string();
    assert!(
        error.contains("ROI") && error.contains("zero width and height"),
        "expected an explicit degenerate-polygon error, got: {error}"
    );

    // The encoder must likewise refuse to produce such a file.
    let mut doc = ImageDocument::new_ramp_8bit("degenerate.tif", 16, 16).unwrap();
    doc.roi = Some(Roi::Polygon(PolygonRoi {
        xpoints: vec![5, 5, 5],
        ypoints: vec![5, 5, 5],
    }));
    let path = temp_tiff("rust_degenerate_polygon.tif");
    let error = save_tiff(&path, &doc).unwrap_err().to_string();
    assert!(
        error.contains("ROI") && error.contains("zero width and height"),
        "expected the encoder to reject a zero-size polygon, got: {error}"
    );
}

#[test]
fn parity_zero_axis_subpixel_polygon_roi_is_rejected_before_java_rewrites_bounds() {
    // FloatPolygon.calculateBounds starts maxX/maxY at Float.MIN_VALUE
    // (FloatPolygon.java 89-105), a positive value. An axis whose points are
    // all zero therefore yields width/height 1, while an all-negative axis
    // yields the distance from the minimum to 0 instead of the true extent.
    // These records mimic what RoiEncoder writes for such subpixel polygons.
    let zero_x = tiff_with_roi_metadata(
        "roi_subpixel_zero_x.tif",
        &roi_polygon_payload_for_test(
            [0, 70000, 0, 70050],
            128,
            &[(0, 0), (0, 10), (0, 50)],
            Some(&[(0.0, 70000.0), (0.0, 70010.0), (0.0, 70050.0)]),
            100,
        ),
    );
    let zero_y = tiff_with_roi_metadata(
        "roi_subpixel_zero_y.tif",
        &roi_polygon_payload_for_test(
            [70000, 0, 70050, 0],
            128,
            &[(0, 0), (10, 0), (50, 0)],
            Some(&[(70000.0, 0.0), (70010.0, 0.0), (70050.0, 0.0)]),
            100,
        ),
    );

    let source = format!(
        r#"
import ij.ImagePlus;
import ij.gui.Roi;
import ij.io.Opener;
import java.awt.Polygon;
import java.awt.Rectangle;
public class ZeroAxisPolygonProbe {{
    static void dump(int index, String path) throws Exception {{
        ImagePlus imp = new Opener().openImage(path);
        Roi roi = imp.getRoi();
        Rectangle b = roi.getBounds();
        Polygon p = roi.getPolygon();
        System.out.print("CASE" + index + ":bounds=" + b.x + "," + b.y + "," + b.width + "," + b.height + "|points=");
        for (int i = 0; i < p.npoints; i++)
            System.out.print(p.xpoints[i] + "," + p.ypoints[i] + ";");
        System.out.println();
    }}
    public static void main(String[] args) throws Exception {{
        dump(0, "{zero_x}");
        dump(1, "{zero_y}");
    }}
}}
"#,
        zero_x = zero_x.display(),
        zero_y = zero_y.display(),
    );
    let stdout = run_java_harness("ZeroAxisPolygonProbe", &source);
    assert_eq!(
        oracle_line(&stdout, "CASE0:"),
        "CASE0:bounds=0,70000,1,50|points=0,70000;0,70010;0,70050;",
        "Java derives a width of 1 for an all-zero x axis"
    );
    assert_eq!(
        oracle_line(&stdout, "CASE1:"),
        "CASE1:bounds=70000,0,50,1|points=70000,0;70010,0;70050,0;",
        "Java derives a height of 1 for an all-zero y axis"
    );

    // The Rust polygon model reports the true vertex span, so these records
    // cannot be read or written without disagreeing with Java's bounds.
    let loaded = load_tiff(&zero_x).unwrap_err().to_string();
    assert!(
        loaded.contains("ROI") && loaded.contains("subpixel"),
        "expected an explicit subpixel zero-axis error, got: {loaded}"
    );
    let loaded = load_tiff(&zero_y).unwrap_err().to_string();
    assert!(
        loaded.contains("ROI") && loaded.contains("subpixel"),
        "expected an explicit subpixel zero-axis error, got: {loaded}"
    );

    for (name, xpoints, ypoints) in [
        (
            "rust_subpixel_zero_x.tif",
            vec![0, 0, 0],
            vec![70000, 70010, 70050],
        ),
        (
            "rust_subpixel_zero_y.tif",
            vec![70000, 70010, 70050],
            vec![0, 0, 0],
        ),
    ] {
        let mut doc = ImageDocument::new_ramp_8bit(name, 16, 16).unwrap();
        doc.roi = Some(Roi::Polygon(PolygonRoi { xpoints, ypoints }));
        let path = temp_tiff(name);
        let error = save_tiff(&path, &doc).unwrap_err().to_string();
        assert!(
            error.contains("ROI") && error.contains("subpixel"),
            "expected the encoder to reject a zero-axis subpixel polygon, got: {error}"
        );
    }
}

// ---------------------------------------------------------------------------
// TIFF reader hardening: scalar tag width and ImageDescription IFD selection.
//
// TiffDecoder.OpenIFD reads every entry as a Java int (getValue,
// TiffDecoder.java 148-157) and compares it with the full width, so e.g.
// Compression 0x00010001 is "compressed in this fashion (65537)"
// (TiffDecoder.java 501-516). The Rust reader must return a clear malformed
// error instead of masking the value down to a u16. TiffDecoder also decodes
// IMAGE_DESCRIPTION only when ifdCount==1 (TiffDecoder.java 555-559), so a
// description in a later IFD is never applied to the image.
// ---------------------------------------------------------------------------

fn push_big_endian_tiff_entry(
    out: &mut Vec<u8>,
    tag: u16,
    field_type: u16,
    count: u32,
    value: u32,
) {
    out.extend_from_slice(&tag.to_be_bytes());
    out.extend_from_slice(&field_type.to_be_bytes());
    out.extend_from_slice(&count.to_be_bytes());
    if field_type == 3 && count == 1 {
        // TIFF left-justifies an inline SHORT value in the four-byte slot.
        out.extend_from_slice(&(value as u16).to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes());
    } else {
        out.extend_from_slice(&value.to_be_bytes());
    }
}

/// Builds a big-endian single-IFD TIFF. Pixel `data` starts at offset 8.
fn build_big_endian_tiff_fixture(data: &[u8], entries: &[(u16, u16, u32, u32)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"MM");
    out.extend_from_slice(&42u16.to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(data);
    if out.len() % 2 == 1 {
        out.push(0);
    }
    let ifd_offset = out.len() as u32;
    out[4..8].copy_from_slice(&ifd_offset.to_be_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_be_bytes());
    for &(tag, field_type, count, value) in entries {
        push_big_endian_tiff_entry(&mut out, tag, field_type, count, value);
    }
    out.extend_from_slice(&0u32.to_be_bytes());
    out
}

/// Builds a little-endian two-IFD 8-bit stack with contiguous planes from
/// offset 8. Each IFD may carry its own ImageDescription; the blobs follow the
/// IFD chain.
fn build_two_ifd_tiff(
    width: u32,
    height: u32,
    planes: [&[u8]; 2],
    descriptions: [Option<&str>; 2],
) -> Vec<u8> {
    let plane_bytes = (width as usize) * (height as usize);
    assert_eq!(planes[0].len(), plane_bytes);
    assert_eq!(planes[1].len(), plane_bytes);

    let mut out = vec![0u8; 8];
    out[..2].copy_from_slice(b"II");
    out[2..4].copy_from_slice(&42u16.to_le_bytes());
    let mut strip_offsets = Vec::with_capacity(2);
    for plane in planes {
        strip_offsets.push(out.len() as u32);
        out.extend_from_slice(plane);
    }
    if out.len() % 2 == 1 {
        out.push(0);
    }

    let blobs: Vec<Option<Vec<u8>>> = descriptions
        .iter()
        .map(|text| text.map(description_blob))
        .collect();
    let entry_counts = [
        10 + usize::from(blobs[0].is_some()),
        10 + usize::from(blobs[1].is_some()),
    ];
    let ifd1 = out.len();
    let ifd1_size = 2 + entry_counts[0] * 12 + 4;
    let ifd2 = ifd1 + ifd1_size;
    let ifd2_size = 2 + entry_counts[1] * 12 + 4;
    let mut blob_offsets = Vec::with_capacity(2);
    let mut cursor = ifd2 + ifd2_size;
    for blob in blobs.iter().flatten() {
        if cursor % 2 == 1 {
            cursor += 1;
        }
        blob_offsets.push(cursor as u32);
        cursor += blob.len();
    }
    out[4..8].copy_from_slice(&(ifd1 as u32).to_le_bytes());

    for index in 0..2 {
        // Use the blobs in the order they were placed after the IFD chain.
        let description_entry = blobs[index].as_ref().map(|blob| {
            let placed = blobs[..index]
                .iter()
                .filter(|other| other.is_some())
                .count();
            (blob.len() as u32, blob_offsets[placed])
        });
        let next_ifd = if index == 0 { ifd2 as u32 } else { 0 };
        out.extend_from_slice(&(entry_counts[index] as u16).to_le_bytes());
        push_tiff_entry(&mut out, 256, 4, 1, width);
        push_tiff_entry(&mut out, 257, 4, 1, height);
        push_tiff_entry(&mut out, 258, 3, 1, 8);
        push_tiff_entry(&mut out, 259, 3, 1, 1);
        push_tiff_entry(&mut out, 262, 3, 1, 1);
        if let Some((count, offset)) = description_entry {
            push_tiff_entry(&mut out, 270, 2, count, offset);
        }
        push_tiff_entry(&mut out, 273, 4, 1, strip_offsets[index]);
        push_tiff_entry(&mut out, 277, 3, 1, 1);
        push_tiff_entry(&mut out, 278, 4, 1, height);
        push_tiff_entry(&mut out, 279, 4, 1, plane_bytes as u32);
        push_tiff_entry(&mut out, 339, 3, 1, 1);
        out.extend_from_slice(&next_ifd.to_le_bytes());
    }
    assert_eq!(out.len(), ifd2 + ifd2_size);
    for (blob, offset) in blobs.iter().flatten().zip(&blob_offsets) {
        while out.len() < *offset as usize {
            out.push(0);
        }
        out.extend_from_slice(blob);
    }
    out
}

#[test]
fn oversized_scalar_tiff_tag_values_are_rejected_like_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let data = [1u8, 2, 3, 4];

    let mut little_endian_entries = base_fixture_entries(2, 2, 8, 1, 1, 1, 1, data.len() as u32);
    little_endian_entries.retain(|entry| entry.0 != 259);
    little_endian_entries.push((259, 4, 1, FixtureValue::Literal(0x0001_0001)));
    let little_endian = build_tiff_fixture(&data, &[], &little_endian_entries);

    let big_endian = build_big_endian_tiff_fixture(
        &data,
        &[
            (256, 4, 1, 2),
            (257, 4, 1, 2),
            (258, 3, 1, 8),
            (259, 4, 1, 0x0001_0001),
            (262, 3, 1, 1),
            (273, 4, 1, 8),
            (277, 3, 1, 1),
            (278, 4, 1, 2),
            (279, 4, 1, 4),
            (339, 3, 1, 1),
        ],
    );

    for (name, bytes) in [("le", little_endian), ("be", big_endian)] {
        let path = write_fixture(&format!("oversized_compression_{name}.tif"), &bytes);
        let oracle = bridge
            .eval_macro(&format!("open(\"{p}\");", p = path.display()))
            .unwrap();
        assert!(
            oracle
                .stdout
                .contains("Unsupported format or file not found"),
            "Java unexpectedly opened Compression=65537 ({name}): {oracle:?}"
        );

        let error = load_tiff(&path).unwrap_err().to_string();
        assert!(
            error.contains("Compression") && error.contains("65537"),
            "expected an explicit Compression width error ({name}), got: {error}"
        );
    }
}

#[test]
fn image_description_in_a_later_ifd_is_ignored_like_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let width = 4u32;
    let height = 2u32;
    let first: Vec<u8> = (0..8).map(|i| i * 3).collect();
    let second: Vec<u8> = (0..8).map(|i| 200 - i * 5).collect();

    // Only IFD2 carries the 10..200 display range.
    let later_only = write_fixture(
        "description_second_ifd_only.tif",
        &build_two_ifd_tiff(
            width,
            height,
            [&first, &second],
            [None, Some("ImageJ=1.54u\nmin=10\nmax=200\n")],
        ),
    );
    // IFD1 wins when both IFDs carry a description.
    let both = write_fixture(
        "description_both_ifds.tif",
        &build_two_ifd_tiff(
            width,
            height,
            [&first, &second],
            [
                Some("ImageJ=1.54u\nmin=10\nmax=200\n"),
                Some("ImageJ=1.54u\nmin=30\nmax=90\n"),
            ],
        ),
    );

    let oracle = bridge
        .eval_macro(&format!(
            "open(\"{a}\"); getMinAndMax(lo,hi); print(\"LATER:\"+lo+\",\"+hi); close(); \
             open(\"{b}\"); getMinAndMax(lo,hi); print(\"BOTH:\"+lo+\",\"+hi);",
            a = later_only.display(),
            b = both.display()
        ))
        .unwrap();

    // TiffDecoder only reads IMAGE_DESCRIPTION for the first IFD.
    assert_eq!(oracle_line(&oracle.stdout, "LATER:"), "LATER:0,255");
    assert_eq!(oracle_line(&oracle.stdout, "BOTH:"), "BOTH:10,200");

    let loaded = load_tiff(&later_only).unwrap();
    assert_eq!(loaded.get_dimensions(), [4, 2, 1, 2, 1]);
    assert_eq!(
        loaded.display_range,
        DisplayRange {
            min: 0.0,
            max: 255.0
        },
        "IFD2's ImageDescription must not set the display range"
    );

    let loaded = load_tiff(&both).unwrap();
    assert_eq!(
        loaded.display_range,
        DisplayRange {
            min: 10.0,
            max: 200.0
        },
        "the first IFD's ImageDescription must win"
    );
}

#[test]
fn big_endian_inline_short_scalar_tags_match_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let pixels = [9u8, 8, 7, 6, 5, 4, 3, 2];
    let path = write_fixture(
        "big_endian_inline_short.tif",
        &build_big_endian_tiff_fixture(
            &pixels,
            &[
                (256, 4, 1, 4),
                (257, 4, 1, 2),
                (258, 3, 1, 8),
                (259, 3, 1, 1),
                (262, 3, 1, 1),
                (273, 4, 1, 8),
                (277, 3, 1, 1),
                (278, 4, 1, 2),
                (279, 4, 1, 8),
                (339, 3, 1, 1),
            ],
        ),
    );

    let oracle = bridge
        .eval_macro(&format!(
            "open(\"{p}\"); getDimensions(w,h,c,z,t); \
             print(\"DIMS:\"+w+\",\"+h+\",\"+c+\",\"+z+\",\"+t); \
             for (y=0;y<2;y++) {{ for (x=0;x<4;x++) {{ \
             print(\"PX:\"+x+\",\"+y+\",\"+getPixel(x,y)); }} }}",
            p = path.display()
        ))
        .unwrap();

    assert_eq!(oracle_line(&oracle.stdout, "DIMS:"), "DIMS:4,2,1,1,1");
    let doc = load_tiff(&path).unwrap();
    assert_eq!(doc.get_dimensions(), [4, 2, 1, 1, 1]);
    assert_eq!(doc.slices, vec![PixelPlane::Gray8(pixels.to_vec())]);
    for y in 0..2u32 {
        for x in 0..4u32 {
            assert_eq!(
                oracle_line(&oracle.stdout, &format!("PX:{x},{y},")),
                format!("PX:{x},{y},{}", pixels[(y * 4 + x) as usize])
            );
        }
    }
}

#[test]
fn parity_display_histogram_8bit_ramp_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    // Full image
    let oracle_full = bridge
        .eval_macro(
            "newImage('test', '8-bit ramp', 16, 16, 1); \
             getHistogram(values, counts, 256); \
             print('HMIN:0'); print('HMAX:255'); print('HIST'); Array.print(counts);",
        )
        .unwrap();
    let doc_full = ImageDocument::new_ramp_8bit("test", 16, 16).unwrap();
    let hist_full: DisplayHistogram = display_histogram(&doc_full);
    assert_eq!(hist_full.hmin, 0.0);
    assert_eq!(hist_full.hmax, 255.0);
    assert_eq!(
        &hist_full.counts[..],
        oracle_histogram(&oracle_full.stdout).as_slice()
    );

    // Rectangular ROI
    let oracle_roi = bridge
        .eval_macro(
            "newImage('test', '8-bit ramp', 16, 16, 1); \
             makeRectangle(2, 3, 8, 6); \
             getHistogram(values, counts, 256); \
             print('HMIN:0'); print('HMAX:255'); print('HIST'); Array.print(counts);",
        )
        .unwrap();
    let mut doc_roi = ImageDocument::new_ramp_8bit("test", 16, 16).unwrap();
    doc_roi.set_rect_roi(2, 3, 8, 6).unwrap();
    let hist_roi = display_histogram(&doc_roi);
    assert_eq!(hist_roi.hmin, 0.0);
    assert_eq!(hist_roi.hmax, 255.0);
    assert_eq!(
        &hist_roi.counts[..],
        oracle_histogram(&oracle_roi.stdout).as_slice()
    );
}

#[test]
fn parity_display_histogram_16bit_ramp_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    // Full image
    let oracle_full = bridge
        .eval_macro(
            "newImage('test', '16-bit ramp', 16, 16, 1); \
             getHistogram(values, counts, 256); \
             n = values.length; inc = (values[n-1]-values[0])/(n-1); \
             print('HMIN:' + values[0]); print('HMAX:' + (values[0] + inc*n)); \
             print('HIST'); Array.print(counts);",
        )
        .unwrap();
    let doc_full = ImageDocument::new_ramp_16bit("test", 16, 16).unwrap();
    let hist_full = display_histogram(&doc_full);
    let expected_hmin: f64 = oracle_line(&oracle_full.stdout, "HMIN:")
        .strip_prefix("HMIN:")
        .unwrap()
        .parse()
        .unwrap();
    let expected_hmax: f64 = oracle_line(&oracle_full.stdout, "HMAX:")
        .strip_prefix("HMAX:")
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(hist_full.hmin, expected_hmin);
    assert_eq!(hist_full.hmax, expected_hmax);
    assert_eq!(
        &hist_full.counts[..],
        oracle_histogram(&oracle_full.stdout).as_slice()
    );

    // Rectangular ROI
    let oracle_roi = bridge
        .eval_macro(
            "newImage('test', '16-bit ramp', 16, 16, 1); \
             makeRectangle(2, 3, 8, 6); \
             getHistogram(values, counts, 256); \
             n = values.length; inc = (values[n-1]-values[0])/(n-1); \
             print('HMIN:' + values[0]); print('HMAX:' + (values[0] + inc*n)); \
             print('HIST'); Array.print(counts);",
        )
        .unwrap();
    let mut doc_roi = ImageDocument::new_ramp_16bit("test", 16, 16).unwrap();
    doc_roi.set_rect_roi(2, 3, 8, 6).unwrap();
    let hist_roi = display_histogram(&doc_roi);
    let expected_hmin_roi: f64 = oracle_line(&oracle_roi.stdout, "HMIN:")
        .strip_prefix("HMIN:")
        .unwrap()
        .parse()
        .unwrap();
    let expected_hmax_roi: f64 = oracle_line(&oracle_roi.stdout, "HMAX:")
        .strip_prefix("HMAX:")
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(hist_roi.hmin, expected_hmin_roi);
    assert_eq!(hist_roi.hmax, expected_hmax_roi);
    assert_eq!(
        &hist_roi.counts[..],
        oracle_histogram(&oracle_roi.stdout).as_slice()
    );
}

#[test]
fn parity_display_histogram_32bit_ramp_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    // Full image
    let oracle_full = bridge
        .eval_macro(
            "newImage('test', '32-bit ramp', 16, 16, 1); \
             getHistogram(values, counts, 256); \
             n = values.length; inc = (values[n-1]-values[0])/(n-1); \
             print('HMIN:' + values[0]); print('HMAX:' + (values[0] + inc*n)); \
             print('HIST'); Array.print(counts);",
        )
        .unwrap();
    let doc_full = ImageDocument::new_ramp_32bit("test", 16, 16).unwrap();
    let hist_full = display_histogram(&doc_full);
    let expected_hmin: f64 = oracle_line(&oracle_full.stdout, "HMIN:")
        .strip_prefix("HMIN:")
        .unwrap()
        .parse()
        .unwrap();
    let expected_hmax: f64 = oracle_line(&oracle_full.stdout, "HMAX:")
        .strip_prefix("HMAX:")
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(hist_full.hmin, expected_hmin);
    assert!((hist_full.hmax - expected_hmax).abs() < 1e-6);
    assert_eq!(
        &hist_full.counts[..],
        oracle_histogram(&oracle_full.stdout).as_slice()
    );

    // Rectangular ROI
    let oracle_roi = bridge
        .eval_macro(
            "newImage('test', '32-bit ramp', 16, 16, 1); \
             makeRectangle(2, 3, 8, 6); \
             getHistogram(values, counts, 256); \
             n = values.length; inc = (values[n-1]-values[0])/(n-1); \
             print('HMIN:' + values[0]); print('HMAX:' + (values[0] + inc*n)); \
             print('HIST'); Array.print(counts);",
        )
        .unwrap();
    let mut doc_roi = ImageDocument::new_ramp_32bit("test", 16, 16).unwrap();
    doc_roi.set_rect_roi(2, 3, 8, 6).unwrap();
    let hist_roi = display_histogram(&doc_roi);
    let expected_hmin_roi: f64 = oracle_line(&oracle_roi.stdout, "HMIN:")
        .strip_prefix("HMIN:")
        .unwrap()
        .parse()
        .unwrap();
    let expected_hmax_roi: f64 = oracle_line(&oracle_roi.stdout, "HMAX:")
        .strip_prefix("HMAX:")
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(hist_roi.hmin, expected_hmin_roi);
    assert!((hist_roi.hmax - expected_hmax_roi).abs() < 1e-6);
    assert_eq!(
        &hist_roi.counts[..],
        oracle_histogram(&oracle_roi.stdout).as_slice()
    );
}

#[test]
fn parity_display_histogram_8bit_oval_roi_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let oracle = bridge
        .eval_macro(
            "newImage('test', '8-bit ramp', 16, 16, 1); \
             makeOval(2, 2, 12, 12); \
             getHistogram(values, counts, 256); \
             print('HMIN:0'); print('HMAX:255'); print('HIST'); Array.print(counts);",
        )
        .unwrap();
    let mut doc = ImageDocument::new_ramp_8bit("test", 16, 16).unwrap();
    doc.set_oval_roi(2, 2, 12, 12).unwrap();
    let hist = display_histogram(&doc);
    assert_eq!(hist.hmin, 0.0);
    assert_eq!(hist.hmax, 255.0);
    assert_eq!(
        &hist.counts[..],
        oracle_histogram(&oracle.stdout).as_slice()
    );
}

#[test]
fn parity_display_histogram_32bit_nonfinite_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let doc = ImageDocument::new(
        "nonfinite",
        4,
        1,
        PixelPlane::Gray32(vec![1.0, 2.0, f32::INFINITY, f32::NAN]),
    )
    .unwrap();
    let oracle = bridge
        .run_macro_on_image(&doc, "getHistogram(values, counts, 256); print('HIST'); Array.print(counts);")
        .unwrap();
    let hist = display_histogram(&doc);
    assert_eq!(hist.hmin, 1.0);
    assert_eq!(hist.hmax, 2.0);
    assert_eq!(&hist.counts[..], oracle_histogram(&oracle.stdout).as_slice());
}



// ---------------------------------------------------------------------------
// M1 Image > Crop (original_imagej/ij/plugin/Resizer.java 25-195). Crop keeps
// the ROI rectangle's pixel size as the new image size, so the calibration
// ratio origWidth/newWidth at Resizer.java 160 is 1 for every crop here.
// ---------------------------------------------------------------------------

fn distinct_stack_16bit(width: u32, height: u32, depth: u32) -> ImageDocument {
    let mut slices = Vec::new();
    for s in 0..depth {
        let mut pixels = vec![0u16; (width as usize) * (height as usize)];
        for y in 0..height {
            for x in 0..width {
                pixels[(y * width + x) as usize] =
                    ((x * 1237 + y * 7919 + s * 104729) % 65521) as u16;
            }
        }
        slices.push(PixelPlane::Gray16(pixels));
    }
    ImageDocument::new_stack("stack16.tif", width, height, slices).unwrap()
}

#[test]
fn parity_crop_gray8_rect_roi_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = ImageDocument::new_ramp_8bit("crop8.tif", 10, 8).unwrap();
    doc.set_rect_roi(2, 1, 4, 3).unwrap();

    let oracle = bridge
        .run_macro_on_image(&doc, "makeRectangle(2,1,4,3); run(\"Crop\");")
        .unwrap();
    let oracle_doc = oracle
        .output_image
        .expect("oracle must return saved TIFF");

    doc.crop().unwrap();
    assert_eq!(doc.slices, oracle_doc.slices);
    assert_eq!(doc.get_dimensions(), oracle_doc.get_dimensions());
    assert_eq!((doc.width, doc.height), (4, 3));
    // Resizer only deletes the ROI for non-rectangular selections, and
    // ImagePlus does not keep a rectangle whose bounds equal the new image
    // (measured with the 1.54u oracle).
    assert_eq!(doc.roi, None);
}

#[test]
fn parity_crop_gray16_stack_matches_java_oracle_and_preserves_labels() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = distinct_stack_16bit(8, 6, 3);
    doc.set_rect_roi(1, 2, 5, 3).unwrap();
    doc.slice_labels = vec![
        Some("first".into()),
        Some("second".into()),
        Some("third".into()),
    ];

    let oracle = bridge
        .run_macro_on_image(
            &doc,
            "Property.setSliceLabel(\"first\",1); Property.setSliceLabel(\"second\",2); Property.setSliceLabel(\"third\",3); \
             makeRectangle(1,2,5,3); run(\"Crop\");",
        )
        .unwrap();
    let oracle_doc = oracle
        .output_image
        .expect("oracle must return saved TIFF");

    doc.crop().unwrap();
    assert_eq!(doc.slices, oracle_doc.slices);
    assert_eq!(doc.get_dimensions(), oracle_doc.get_dimensions());
    assert_eq!(doc.slice_labels, oracle_doc.slice_labels);
    assert_eq!(
        doc.slice_labels,
        vec![
            Some("first".into()),
            Some("second".into()),
            Some("third".into())
        ]
    );
    assert_eq!((doc.width, doc.height), (5, 3));
    assert_eq!(doc.roi, None);
}

#[test]
fn parity_crop_oval_roi_is_relocated_to_origin() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = ImageDocument::new_ramp_8bit("crop_oval.tif", 10, 8).unwrap();
    doc.set_oval_roi(2, 1, 4, 3).unwrap();

    let oracle = bridge
        .run_macro_on_image(
            &doc,
            "makeOval(2,1,4,3); run(\"Crop\"); \
             getSelectionBounds(x,y,w,h); \
             print(\"TYPE:\"+selectionType()+\";BOUNDS:\"+x+\",\"+y+\",\"+w+\",\"+h);",
        )
        .unwrap();
    let oracle_roi = oracle_line(&oracle.stdout, "TYPE:");
    assert_eq!(oracle_roi, "TYPE:1;BOUNDS:0,0,4,3");

    doc.crop().unwrap();
    assert_eq!(
        doc.roi,
        Some(Roi::Oval(RectRoi {
            x: 0,
            y: 0,
            width: 4,
            height: 3
        }))
    );
    assert_eq!((doc.width, doc.height), (4, 3));
}

#[test]
fn parity_crop_preserves_calibration_like_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = ImageDocument::new_ramp_8bit("crop_cal.tif", 10, 8).unwrap();
    doc.calibration.pixel_width = 0.5;
    doc.calibration.pixel_height = 0.25;
    doc.calibration.unit = "mm".to_string();
    doc.set_rect_roi(2, 1, 4, 3).unwrap();

    let oracle = bridge
        .run_macro_on_image(&doc, "makeRectangle(2,1,4,3); run(\"Crop\");")
        .unwrap();
    let oracle_doc = oracle
        .output_image
        .expect("oracle must return saved TIFF");

    assert_eq!(oracle_doc.calibration.pixel_width, 0.5);
    assert_eq!(oracle_doc.calibration.pixel_height, 0.25);
    assert_eq!(oracle_doc.calibration.unit, "mm");

    doc.crop().unwrap();
    assert_eq!(doc.calibration, oracle_doc.calibration);
}

#[test]
fn parity_crop_float_rgb_and_indexed_pixels_match_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();

    let float_doc = ImageDocument::new_ramp_32bit("crop32.tif", 10, 8).unwrap();
    let rgb_doc = ImageDocument::new(
        "crop_rgb.tif",
        10,
        8,
        PixelPlane::ColorRgb(
            (0..80)
                .map(|i| 0xff00_0000 | ((i * 17) << 16) | ((i * 29) << 8) | (i * 43))
                .collect(),
        ),
    )
    .unwrap();
    let lut = Lut256 {
        r: (0..=255).map(|i| i as u8).collect(),
        g: (0..=255).map(|i| 255 - i as u8).collect(),
        b: (0..=255).map(|i| ((i * 7) % 256) as u8).collect(),
    };
    let indexed_doc = ImageDocument::new(
        "crop_indexed.tif",
        10,
        8,
        PixelPlane::Color256 {
            pixels: (0..80).map(|i| ((i * 13) % 256) as u8).collect(),
            lut: Box::new(lut.clone()),
        },
    )
    .unwrap();

    for (name, mut doc) in [
        ("32-bit", float_doc),
        ("RGB", rgb_doc),
        ("8-bit color", indexed_doc),
    ] {
        doc.set_rect_roi(2, 1, 6, 4).unwrap();
        let oracle = bridge
            .run_macro_on_image(&doc, "makeRectangle(2,1,6,4); run(\"Crop\");")
            .unwrap();
        let oracle_doc = oracle
            .output_image
            .expect("oracle must return saved TIFF");

        doc.crop().unwrap();
        assert_eq!(doc.slices, oracle_doc.slices, "{name}: crop pixels differ");
        assert_eq!(
            doc.get_dimensions(),
            oracle_doc.get_dimensions(),
            "{name}: crop dimensions differ"
        );
        if name == "8-bit color" {
            assert_eq!(
                match doc.plane() {
                    PixelPlane::Color256 { lut, .. } => lut.as_ref(),
                    other => panic!("expected Color256 after crop, got {other:?}"),
                },
                &lut,
                "crop must keep the Color256 LUT"
            );
        }
    }
}

#[test]
fn session_crop_records_macro_and_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let source = ImageDocument::new_ramp_8bit("session_crop.tif", 10, 8).unwrap();

    let oracle = bridge
        .run_macro_on_image(&source, "makeRectangle(2,1,4,3); run(\"Crop\");")
        .unwrap();
    let oracle_doc = oracle
        .output_image
        .expect("oracle must return saved TIFF");

    let path = temp_tiff("session_crop.tif");
    save_tiff(&path, &source).unwrap();
    let mut session = Session::new();
    session.dispatch(Command::OpenTiff { path }).unwrap();
    session.macro_recorder.clear();
    session
        .dispatch(Command::MakeRectRoi {
            x: 2,
            y: 1,
            width: 4,
            height: 3,
        })
        .unwrap();
    session.macro_recorder.clear();
    session.dispatch(Command::Crop).unwrap();

    let doc = session.active_doc().unwrap();
    assert_eq!(doc.slices, oracle_doc.slices);
    assert_eq!(doc.get_dimensions(), oracle_doc.get_dimensions());
    assert_eq!(doc.roi, None);
    assert_eq!(
        session.macro_recorder,
        vec!["run(\"Crop\");".to_string()],
        "Crop must record exactly one macro line without a trailing newline"
    );
}

#[test]
fn parity_crop_polygon_roi_is_relocated_to_origin() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = ImageDocument::new_ramp_8bit("crop_polygon.tif", 10, 8).unwrap();
    doc.set_polygon_roi(&[(3, 2), (7, 2), (7, 6), (3, 6)])
        .unwrap();

    let oracle = bridge
        .run_macro_on_image(
            &doc,
            "makePolygon(3,2,7,2,7,6,3,6); run(\"Crop\");              getSelectionBounds(x,y,w,h);              print(\"TYPE:\"+selectionType()+\";BOUNDS:\"+x+\",\"+y+\",\"+w+\",\"+h);",
        )
        .unwrap();
    assert_eq!(oracle_line(&oracle.stdout, "TYPE:"), "TYPE:2;BOUNDS:0,0,4,4");

    doc.crop().unwrap();
    assert_eq!(
        doc.roi,
        Some(Roi::Polygon(PolygonRoi {
            xpoints: vec![0, 4, 4, 0],
            ypoints: vec![0, 0, 4, 4],
        }))
    );
    assert_eq!((doc.width, doc.height), (4, 4));
}

#[test]
fn crop_rejects_missing_line_and_outside_selections() {
    let mut doc = ImageDocument::new_ramp_8bit("crop_errors.tif", 10, 8).unwrap();
    assert_eq!(
        doc.crop(),
        Err(CoreError::InvalidOperation("Area selection required"))
    );

    doc.set_line_roi(1, 1, 5, 4);
    assert_eq!(
        doc.crop(),
        Err(CoreError::InvalidOperation("Area selection required"))
    );

    // Resizer.java 52-61 rejects a selection whose intersection with the
    // image is empty.
    doc.set_rect_roi(20, 20, 4, 4).unwrap();
    assert_eq!(
        doc.crop(),
        Err(CoreError::InvalidOperation("Selection is outside image"))
    );
}

// ---------------------------------------------------------------------------
// M2 Image > Duplicate... (original_imagej/ij/plugin/Duplicator.java 36-135,
// 230-355 and WindowManager.java 357-385). Duplicator keeps the source
// calibration/display range through ImagePlus.createImagePlus (ImagePlus.java
// 2649-2654) and ImageProcessor.createProcessor, copies stack labels, and
// relocates non-rectangular ROIs to (0,0).
// ---------------------------------------------------------------------------

/// Runs Duplicate... against the real 1.54u oracle. `prelude` executes while
/// the source image is current; the helper then reselects the source by title
/// so a prelude may create extra windows (for the unique-title cases).
fn oracle_duplicate(
    doc: &ImageDocument,
    prelude: &str,
    options: &str,
) -> (ImageDocument, String) {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let title = doc.title.replace('"', "'");
    // GenericDialog only enters macro mode when Macro.getOptions() is
    // non-null, so the default dialog fields need a " " options string.
    let options = if options.is_empty() { " " } else { options };
    let macro_text = format!(
        "rename(\"{title}\"); {prelude} selectWindow(\"{title}\"); \
         run(\"Duplicate...\", \"{options}\"); \
         print(\"DUP_TITLE:\"+getTitle()); print(\"DUP_SLICE:\"+getSliceNumber()); \
         getDimensions(dw,dh,dc,dw2,df); \
         print(\"DUP_DIMS:\"+dw+\",\"+dh+\",\"+dc+\",\"+dw2+\",\"+df); \
         Property.set(\"UniqueName\",\"\"); Property.set(\"Slice_Label\",\"\");"
    );
    let output = bridge.run_macro_on_image(doc, &macro_text).unwrap();
    (
        output.output_image.expect("oracle must return saved TIFF"),
        output.stdout,
    )
}

fn oracle_duplicate_field<'a>(stdout: &'a str, prefix: &str) -> &'a str {
    oracle_line(stdout, prefix)
        .strip_prefix(prefix)
        .unwrap_or_else(|| panic!("missing {prefix} in oracle output:\n{stdout}"))
}

fn duplicate_stack_input() -> ImageDocument {
    let mut doc = distinct_stack_16bit(8, 6, 4);
    doc.title = "dupe16.tif".to_string();
    doc.slice_labels = vec![
        Some("L1".into()),
        Some("L2".into()),
        Some("L3".into()),
        Some("L4".into()),
    ];
    doc.display_range = DisplayRange { min: 100.0, max: 500.0 };
    doc
}

fn assert_duplicate_matches_oracle(
    actual: &ImageDocument,
    oracle_doc: &ImageDocument,
    oracle_stdout: &str,
    context: &str,
) {
    assert_eq!(
        actual.title,
        oracle_duplicate_field(oracle_stdout, "DUP_TITLE:"),
        "{context}: title"
    );
    assert_eq!(
        actual.current_slice,
        oracle_duplicate_field(oracle_stdout, "DUP_SLICE:")
            .parse::<u32>()
            .unwrap(),
        "{context}: current slice"
    );
    let dims: Vec<u32> = oracle_duplicate_field(oracle_stdout, "DUP_DIMS:")
        .split(',')
        .map(|value| value.parse::<u32>().unwrap())
        .collect();
    assert_eq!(
        actual.get_dimensions().to_vec(),
        dims,
        "{context}: dimensions"
    );
    assert_eq!(actual.slices, oracle_doc.slices, "{context}: pixels");
    assert_eq!(
        (actual.width, actual.height),
        (oracle_doc.width, oracle_doc.height),
        "{context}: size"
    );
    assert_eq!(
        actual.slice_labels, oracle_doc.slice_labels,
        "{context}: slice labels"
    );
    assert_eq!(
        actual.calibration, oracle_doc.calibration,
        "{context}: calibration"
    );
    assert_eq!(
        actual.display_range, oracle_doc.display_range,
        "{context}: display range"
    );
    assert_eq!(actual.roi, oracle_doc.roi, "{context}: ROI");
}

/// WindowManager.getUniqueName String overload (WindowManager.java 357-385):
/// the extension is split only when shorter than six characters, a trailing
/// -N with fewer than four characters remaining is stripped, digits count
/// only when the first one is not '0', and the first free -1..-99 wins.
#[test]
fn default_duplicate_title_matches_window_manager_rules() {
    assert_eq!(default_duplicate_title(&[], "blobs.tif"), "blobs-1.tif");
    assert_eq!(
        default_duplicate_title(&["blobs-1.tif"], "blobs.tif"),
        "blobs-2.tif"
    );
    assert_eq!(default_duplicate_title(&[], "ramp"), "ramp-1");
    // WindowManager.java 364: len-lastDot < 6, so ".jpeg" is an extension.
    assert_eq!(
        default_duplicate_title(&[], "blobs.jpeg"),
        "blobs-1.jpeg"
    );
    assert_eq!(
        default_duplicate_title(&[], "archive.tar.gz"),
        "archive.tar-1.gz"
    );
    // WindowManager.java 371-373: source "blobs-1.tif" loses "-1" first.
    assert_eq!(
        default_duplicate_title(&[], "blobs-1.tif"),
        "blobs-1.tif"
    );
    assert_eq!(
        default_duplicate_title(&["blobs-1.tif"], "blobs-1.tif"),
        "blobs-2.tif"
    );
    // A dot six or more characters from the end is not treated as an
    // extension (WindowManager.java 364).
    assert_eq!(
        default_duplicate_title(&[], "data.backup1"),
        "data.backup1-1"
    );
}

#[test]
fn parity_duplicate_single_image_matches_java_oracle() {
    let mut doc = ImageDocument::new_ramp_8bit("blobs.tif", 10, 8).unwrap();
    doc.calibration.pixel_width = 0.5;
    doc.calibration.pixel_height = 0.25;
    doc.calibration.unit = "mm".to_string();
    doc.display_range = DisplayRange { min: 10.0, max: 200.0 };

    let (oracle_doc, stdout) = oracle_duplicate(&doc, "", "");
    let title = default_duplicate_title(&[doc.title.as_str()], &doc.title);
    let actual = doc.duplicate(title, false, None).unwrap();
    assert_duplicate_matches_oracle(&actual, &oracle_doc, &stdout, "single");
}

#[test]
fn parity_duplicate_rect_roi_matches_java_oracle() {
    let mut doc = ImageDocument::new_ramp_8bit("blobs.tif", 10, 8).unwrap();
    doc.set_rect_roi(2, 1, 4, 3).unwrap();

    let (oracle_doc, stdout) =
        oracle_duplicate(&doc, "makeRectangle(2,1,4,3);", "");
    let title = default_duplicate_title(&[doc.title.as_str()], &doc.title);
    let actual = doc.duplicate(title, false, None).unwrap();
    assert_duplicate_matches_oracle(&actual, &oracle_doc, &stdout, "rect");
    // Duplicator.java 121-123 only re-installs non-rectangular area ROIs.
    assert_eq!(actual.roi, None);
}

#[test]
fn parity_duplicate_oval_and_polygon_rois_are_relocated() {
    let mut oval = ImageDocument::new_ramp_8bit("oval.tif", 10, 8).unwrap();
    oval.set_oval_roi(2, 1, 4, 3).unwrap();
    let (oracle_oval, oval_stdout) =
        oracle_duplicate(&oval, "makeOval(2,1,4,3);", "");
    let oval_title = default_duplicate_title(&[oval.title.as_str()], &oval.title);
    let actual_oval = oval.duplicate(oval_title, false, None).unwrap();
    assert_duplicate_matches_oracle(&actual_oval, &oracle_oval, &oval_stdout, "oval");
    assert_eq!(
        actual_oval.roi,
        Some(Roi::Oval(RectRoi {
            x: 0,
            y: 0,
            width: 4,
            height: 3
        }))
    );

    let mut polygon = ImageDocument::new_ramp_8bit("polygon.tif", 10, 8).unwrap();
    polygon
        .set_polygon_roi(&[(3, 2), (7, 2), (7, 6), (3, 6)])
        .unwrap();
    let (oracle_polygon, polygon_stdout) =
        oracle_duplicate(&polygon, "makePolygon(3,2,7,2,7,6,3,6);", "");
    let polygon_title = default_duplicate_title(&[polygon.title.as_str()], &polygon.title);
    let actual_polygon = polygon.duplicate(polygon_title, false, None).unwrap();
    assert_duplicate_matches_oracle(
        &actual_polygon,
        &oracle_polygon,
        &polygon_stdout,
        "polygon",
    );
    assert_eq!(
        actual_polygon.roi,
        Some(Roi::Polygon(PolygonRoi {
            xpoints: vec![0, 4, 4, 0],
            ypoints: vec![0, 0, 4, 4],
        }))
    );
}

#[test]
fn parity_duplicate_ignores_line_roi() {
    let mut doc = ImageDocument::new_ramp_8bit("line.tif", 10, 8).unwrap();
    doc.set_line_roi(2, 3, 8, 6);
    let (oracle_doc, stdout) =
        oracle_duplicate(&doc, "makeLine(2,3,8,6);", "");
    let title = default_duplicate_title(&[doc.title.as_str()], &doc.title);
    let actual = doc.duplicate(title, false, None).unwrap();
    assert_duplicate_matches_oracle(&actual, &oracle_doc, &stdout, "line");
    assert_eq!(actual.roi, None);
    assert_eq!((actual.width, actual.height), (10, 8));
}

#[test]
fn parity_duplicate_current_slice_only_matches_java_oracle() {
    let mut doc = duplicate_stack_input();
    doc.current_slice = 2;
    doc.position = [1, 2, 1];

    let prelude = "Property.setSliceLabel(\"L1\",1); Property.setSliceLabel(\"L2\",2); \
                   Property.setSliceLabel(\"L3\",3); Property.setSliceLabel(\"L4\",4); \
                   setSlice(2);";
    let (oracle_doc, stdout) = oracle_duplicate(&doc, prelude, "");
    let title = default_duplicate_title(&[doc.title.as_str()], &doc.title);
    let actual = doc.duplicate(title, false, None).unwrap();
    assert_duplicate_matches_oracle(&actual, &oracle_doc, &stdout, "current slice");
    assert_eq!(actual.get_dimensions(), [8, 6, 1, 1, 1]);
    assert_eq!(actual.current_slice, 1);
    assert_eq!(actual.slices[0], doc.slices[1]);
}

#[test]
fn parity_duplicate_full_stack_matches_java_oracle() {
    let mut doc = duplicate_stack_input();
    doc.current_slice = 3;
    doc.position = [1, 3, 1];

    let prelude = "Property.setSliceLabel(\"L1\",1); Property.setSliceLabel(\"L2\",2); \
                   Property.setSliceLabel(\"L3\",3); Property.setSliceLabel(\"L4\",4); \
                   setSlice(3);";
    let (oracle_doc, stdout) = oracle_duplicate(&doc, prelude, "duplicate");
    let title = default_duplicate_title(&[doc.title.as_str()], &doc.title);
    let actual = doc.duplicate(title, true, None).unwrap();
    assert_duplicate_matches_oracle(&actual, &oracle_doc, &stdout, "full stack");
    assert_eq!(actual.get_dimensions(), [8, 6, 1, 4, 1]);
    // Duplicator.java 114 restores the source slice only for a full copy.
    assert_eq!(actual.current_slice, 3);
}

#[test]
fn parity_duplicate_range_2_3_matches_java_oracle() {
    let mut doc = duplicate_stack_input();
    doc.current_slice = 2;
    doc.position = [1, 2, 1];

    let prelude = "Property.setSliceLabel(\"L1\",1); Property.setSliceLabel(\"L2\",2); \
                   Property.setSliceLabel(\"L3\",3); Property.setSliceLabel(\"L4\",4); \
                   setSlice(2);";
    let (oracle_doc, stdout) =
        oracle_duplicate(&doc, prelude, "duplicate range=2-3");
    let title = default_duplicate_title(&[doc.title.as_str()], &doc.title);
    let actual = doc.duplicate(title, true, Some((2, 3))).unwrap();
    assert_duplicate_matches_oracle(&actual, &oracle_doc, &stdout, "range 2-3");
    assert_eq!(actual.get_dimensions(), [8, 6, 1, 2, 1]);
    assert_eq!(actual.current_slice, 1);
    assert_eq!(actual.slices, doc.slices[1..3].to_vec());
    assert_eq!(actual.slice_labels, doc.slice_labels[1..3].to_vec());
}

#[test]
fn parity_duplicate_default_title_matches_java_oracle() {
    let doc = ImageDocument::new_ramp_8bit("blobs.tif", 10, 8).unwrap();
    let (_, stdout) = oracle_duplicate(&doc, "", "");
    let oracle_title = oracle_duplicate_field(&stdout, "DUP_TITLE:");
    assert_eq!(oracle_title, "blobs-1.tif");
    assert_eq!(
        default_duplicate_title(&[doc.title.as_str()], &doc.title),
        oracle_title
    );

    let ramp = ImageDocument::new_ramp_8bit("ramp", 10, 8).unwrap();
    let (_, stdout) = oracle_duplicate(&ramp, "", "");
    assert_eq!(oracle_duplicate_field(&stdout, "DUP_TITLE:"), "ramp-1");
}

#[test]
fn session_duplicate_opens_activates_and_records_duplicate_macro() {
    let source = ImageDocument::new_ramp_8bit("session_dup.tif", 10, 8).unwrap();
    let path = temp_tiff("session_dup.tif");
    save_tiff(&path, &source).unwrap();
    let mut session = Session::new();
    session.dispatch(Command::OpenTiff { path }).unwrap();
    let source_id = session.active_image_id().unwrap();
    session.macro_recorder.clear();

    let outcome = session
        .dispatch(Command::Duplicate {
            title: None,
            duplicate_stack: false,
            first_slice: None,
            last_slice: None,
        })
        .unwrap();
    let new_id = match outcome {
        CommandOutcome::ImageOpened(id) => id,
        other => panic!("expected ImageOpened, got {other:?}"),
    };
    assert_eq!(session.active_image_id(), Some(new_id));
    assert_ne!(new_id, source_id);
    assert_eq!(
        session.get_image(new_id).unwrap().title,
        "session_dup-1.tif"
    );
    assert_eq!(
        session.macro_recorder,
        vec!["run(\"Duplicate...\", \"title=session_dup-1.tif\");".to_string()]
    );

    session
        .dispatch(Command::Duplicate {
            title: None,
            duplicate_stack: false,
            first_slice: None,
            last_slice: None,
        })
        .unwrap();
    assert_eq!(
        session.get_image(new_id).unwrap().title,
        "session_dup-1.tif"
    );
    assert_eq!(
        session.active_doc().unwrap().title,
        "session_dup-2.tif"
    );
    assert_eq!(
        session.macro_recorder[1],
        "run(\"Duplicate...\", \"title=session_dup-2.tif\");"
    );
}

#[test]
fn session_duplicate_range_uses_java_recorder_options() {
    let mut source = duplicate_stack_input();
    source.title = "session_stack.tif".to_string();
    let path = temp_tiff("session_stack.tif");
    save_tiff(&path, &source).unwrap();
    let mut session = Session::new();
    session.dispatch(Command::OpenTiff { path }).unwrap();
    session.macro_recorder.clear();

    let outcome = session
        .dispatch(Command::Duplicate {
            title: Some("range-dup".to_string()),
            duplicate_stack: true,
            first_slice: Some(2),
            last_slice: Some(3),
        })
        .unwrap();
    let new_id = match outcome {
        CommandOutcome::ImageOpened(id) => id,
        other => panic!("expected ImageOpened, got {other:?}"),
    };
    let doc = session.get_image(new_id).unwrap();
    assert_eq!(doc.title, "range-dup");
    assert_eq!(doc.get_dimensions(), [8, 6, 1, 2, 1]);
    assert_eq!(doc.current_slice, 1);
    assert_eq!(
        doc.slices,
        source.slices[1..3].to_vec(),
        "range duplicate must copy only slices 2-3"
    );
    assert_eq!(doc.slice_labels, source.slice_labels[1..3].to_vec());
    // GenericDialog records the Title field, then the Duplicate stack
    // checkbox ("Duplicate stack" -> "duplicate"), then Range (Duplicator.java
    // 483-527; Recorder.trimKey strips at the first space/colon).
    assert_eq!(
        session.macro_recorder,
        vec!["run(\"Duplicate...\", \"title=range-dup duplicate range=2-3\");".to_string()]
    );
}

/// ImageJ's ShortProcessor.setMinAndMax rounds the display range to integers
/// (original_imagej/ij/process/ShortProcessor.java 267-277), so a 16-bit
/// conversion sees rounded bounds even when the model stores fractions.
fn convert_gray16_pattern(width: u32, height: u32) -> ImageDocument {
    let mut pixels = vec![0u16; (width * height) as usize];
    for y in 0..height {
        for x in 0..width {
            pixels[(y * width + x) as usize] = ((x * 4099 + y * 1237) % 65536) as u16;
        }
    }
    pixels[0] = 0;
    pixels[1] = 65535;
    pixels[2] = 1;
    pixels[3] = 255;
    pixels[4] = 256;
    ImageDocument::new("convert16.tif", width, height, PixelPlane::Gray16(pixels)).unwrap()
}

/// The standard 3-3-2 RGB palette. ImageProcessor.isPseudoColorLut returns
/// false for it (original_imagej/ij/process/ImageProcessor.java 351-386), so
/// ImageJ converts indexed images with this LUT through RGB.
fn rgb332_lut() -> Lut256 {
    let mut lut = Lut256 {
        r: Vec::with_capacity(256),
        g: Vec::with_capacity(256),
        b: Vec::with_capacity(256),
    };
    for i in 0..256u32 {
        lut.r.push((((i >> 5) & 7) * 255 / 7) as u8);
        lut.g.push((((i >> 2) & 7) * 255 / 7) as u8);
        lut.b.push(((i & 3) * 255 / 3) as u8);
    }
    lut
}

fn assert_grayscale_lut(lut: &Lut256) {
    for i in 0..=255u8 {
        assert_eq!(lut.r[i as usize], i, "grayscale LUT red[{i}]");
        assert_eq!(lut.g[i as usize], i, "grayscale LUT green[{i}]");
        assert_eq!(lut.b[i as usize], i, "grayscale LUT blue[{i}]");
    }
}

#[test]
fn parity_convert_to_8bit_gray16_display_ranges_match_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    for (min, max) in [
        (0.0, 65535.0),
        (1000.0, 20000.0),
        (1000.7, 20000.2),
        (30000.0, 40000.0),
    ] {
        let mut doc = convert_gray16_pattern(8, 4);
        doc.display_range = DisplayRange { min, max };
        let oracle = bridge
            .run_macro_on_image(&doc, "run(\"8-bit\");")
            .unwrap();
        let oracle_doc = oracle
            .output_image
            .expect("oracle must return saved TIFF");

        doc.convert_to_8bit();
        assert!(
            matches!(doc.plane(), PixelPlane::Gray8(_)),
            "16-bit conversion must produce 8-bit pixels"
        );
        assert_eq!(
            doc.slices, oracle_doc.slices,
            "16-bit pixels differ for display range {min}..{max}"
        );
        assert_eq!(
            doc.display_range, oracle_doc.display_range,
            "16-bit display range differs for {min}..{max}"
        );
    }
}

#[test]
fn parity_convert_to_8bit_gray32_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut pixels = vec![0f32; 32];
    for (i, value) in pixels.iter_mut().enumerate() {
        *value = (i as f32 - 4.0) * 0.25;
    }
    pixels[0] = -100.0;
    pixels[1] = 100.0;
    pixels[2] = 0.0;
    pixels[3] = 0.5;
    for (min, max) in [(-1.0, 1.0), (0.0, 1.0), (0.25, 0.75), (0.1, 1.1)] {
        let mut doc =
            ImageDocument::new("convert32.tif", 8, 4, PixelPlane::Gray32(pixels.clone())).unwrap();
        doc.display_range = DisplayRange { min, max };
        let oracle = bridge
            .run_macro_on_image(&doc, "run(\"8-bit\");")
            .unwrap();
        let oracle_doc = oracle
            .output_image
            .expect("oracle must return saved TIFF");

        doc.convert_to_8bit();
        assert_eq!(
            doc.slices, oracle_doc.slices,
            "32-bit pixels differ for display range {min}..{max}"
        );
        assert_eq!(doc.display_range, oracle_doc.display_range);
    }
}

#[test]
fn parity_convert_to_8bit_rgb_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut pixels = vec![0u32; 32];
    for (i, pixel) in pixels.iter_mut().enumerate() {
        let r = (i * 37) % 256;
        let g = (i * 53 + 7) % 256;
        let b = (i * 97 + 11) % 256;
        *pixel = 0xff00_0000 | ((r as u32) << 16) | ((g as u32) << 8) | b as u32;
    }
    pixels[0] = 0x00ff_0000;
    pixels[1] = 0x0000_ff00;
    pixels[2] = 0x0000_00ff;
    pixels[3] = 0x00ff_ffff;
    let mut doc =
        ImageDocument::new("convert_rgb.tif", 8, 4, PixelPlane::ColorRgb(pixels)).unwrap();

    let oracle = bridge
        .run_macro_on_image(&doc, "run(\"8-bit\");")
        .unwrap();
    let oracle_doc = oracle
        .output_image
        .expect("oracle must return saved TIFF");

    doc.convert_to_8bit();
    assert_eq!(doc.slices, oracle_doc.slices);
    assert_eq!(doc.display_range, oracle_doc.display_range);
}

#[test]
fn parity_convert_to_8bit_indexed_color_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let pixels: Vec<u8> = (0..32).map(|i| ((i * 13) % 256) as u8).collect();
    for (name, lut, keep_indices) in [
        ("pseudo-color", Lut256::default(), true),
        ("3-3-2 RGB", rgb332_lut(), false),
    ] {
        let mut doc = ImageDocument::new(
            format!("convert_{name}.tif"),
            8,
            4,
            PixelPlane::Color256 {
                pixels: pixels.clone(),
                lut: Box::new(lut.clone()),
            },
        )
        .unwrap();
        let oracle = bridge
            .run_macro_on_image(&doc, "run(\"8-bit\");")
            .unwrap();
        let oracle_doc = oracle
            .output_image
            .expect("oracle must return saved TIFF");

        doc.convert_to_8bit();
        let PixelPlane::Gray8(out) = doc.plane() else {
            panic!("{name}: expected Gray8 after conversion, got {:?}", doc.plane());
        };
        let oracle_pixels = match oracle_doc.plane() {
            PixelPlane::Gray8(v) => v.clone(),
            PixelPlane::Color256 { pixels, lut } => {
                // ImageConverter.java 42-45 keeps the Color256 image type but
                // swaps in a grayscale LUT, so the saved TIFF still carries
                // 8bitcolor=true.
                assert_grayscale_lut(lut);
                pixels.clone()
            }
            other => panic!("{name}: unexpected oracle plane {other:?}"),
        };
        assert_eq!(out, &oracle_pixels, "{name}: 8-bit pixels differ");
        if keep_indices {
            assert_eq!(out, &pixels, "{name}: pseudo-color indices must be kept");
        } else {
            assert_ne!(out, &pixels, "{name}: indexed color must go through RGB");
        }
        assert_eq!(doc.display_range, oracle_doc.display_range, "{name}: display range");
    }
}

#[test]
fn parity_convert_to_8bit_gray16_stack_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = distinct_stack_16bit(8, 6, 3);
    doc.display_range = DisplayRange {
        min: 500.0,
        max: 40000.0,
    };
    doc.slice_labels = vec![
        Some("first".into()),
        Some("second".into()),
        Some("third".into()),
    ];

    let oracle = bridge
        .run_macro_on_image(
            &doc,
            "Property.setSliceLabel(\"first\",1); Property.setSliceLabel(\"second\",2); \
             Property.setSliceLabel(\"third\",3); run(\"8-bit\");",
        )
        .unwrap();
    let oracle_doc = oracle
        .output_image
        .expect("oracle must return saved TIFF");

    doc.convert_to_8bit();
    assert_eq!(doc.slices, oracle_doc.slices);
    assert_eq!(doc.display_range, oracle_doc.display_range);
    assert_eq!(doc.slice_labels, oracle_doc.slice_labels);
    assert_eq!(doc.current_slice, oracle_doc.current_slice);
}

#[test]
fn session_convert_to_8bit_records_macro_and_clears_threshold() {
    let mut doc = convert_gray16_pattern(4, 2);
    doc.display_range = DisplayRange {
        min: 0.0,
        max: 65535.0,
    };
    doc.threshold = Some(ThresholdLevels {
        lower: 10.0,
        upper: 20.0,
    });
    let mut session = Session::new();
    session.insert_doc(doc);
    session.macro_recorder.clear();

    session.dispatch(Command::ConvertTo8Bit).unwrap();

    let doc = session.active_doc().unwrap();
    assert!(matches!(doc.plane(), PixelPlane::Gray8(_)));
    assert_eq!(doc.display_range, DisplayRange { min: 0.0, max: 255.0 });
    assert_eq!(
        doc.threshold, None,
        "Converter.java 55-56 resets the threshold before converting"
    );
    assert_eq!(
        session.macro_recorder,
        vec!["run(\"8-bit\");".to_string()],
        "8-bit must record exactly one macro line without a trailing newline"
    );
}

#[test]
fn convert_to_8bit_is_a_noop_for_gray8() {
    let mut doc = ImageDocument::new_ramp_8bit("already8.tif", 4, 3).unwrap();
    doc.display_range = DisplayRange {
        min: 10.0,
        max: 200.0,
    };
    let before = doc.slices.clone();
    doc.convert_to_8bit();
    assert_eq!(doc.slices, before);
    assert_eq!(
        doc.display_range,
        DisplayRange {
            min: 10.0,
            max: 200.0
        },
        "ImageConverter.java 42-45 only swaps the LUT for 8-bit images"
    );
}

/// ImageConverter.convertToGray16 (ImageConverter.java 58-77) for single
/// images: 8-bit pixels are copied unchanged (TypeConverter.java 149-160) and
/// the new ShortProcessor reports its own lazily computed min/max.
#[test]
fn parity_convert_to_16bit_gray8_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut pixels = vec![0u8; 32];
    for (i, pixel) in pixels.iter_mut().enumerate() {
        *pixel = ((i * 37 + 11) % 251) as u8;
    }
    let mut doc =
        ImageDocument::new("convert_to_16.tif", 8, 4, PixelPlane::Gray8(pixels)).unwrap();
    doc.display_range = DisplayRange {
        min: 10.0,
        max: 200.0,
    };
    let oracle = bridge
        .run_macro_on_image(&doc, "run(\"16-bit\");")
        .unwrap();
    let oracle_doc = oracle
        .output_image
        .expect("oracle must return saved TIFF");

    doc.convert_to_16bit().unwrap();
    let PixelPlane::Gray16(out) = doc.plane() else {
        panic!("expected Gray16 after conversion, got {:?}", doc.plane());
    };
    let oracle_pixels = match oracle_doc.plane() {
        PixelPlane::Gray16(v) => v.clone(),
        other => panic!("unexpected oracle plane {other:?}"),
    };
    assert_eq!(out, &oracle_pixels, "8-bit -> 16-bit pixels differ");
    assert_eq!(doc.display_range, oracle_doc.display_range);
}

/// TypeConverter.convertFloatToShort (TypeConverter.java 178-199) scales by
/// 65535/(max-min), clamps to 0..=65535 and rounds with +0.5, including the
/// max==min fallback scale of 1.0.
#[test]
fn parity_convert_to_16bit_gray32_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut pixels = vec![0f32; 32];
    for (i, value) in pixels.iter_mut().enumerate() {
        *value = (i as f32 - 4.0) * 0.25;
    }
    pixels[0] = -100.0;
    pixels[1] = 100.0;
    pixels[2] = 0.0;
    pixels[3] = 0.5;
    pixels[4] = f32::NAN;
    pixels[5] = f32::INFINITY;
    pixels[6] = f32::NEG_INFINITY;
    for (min, max) in [
        (-1.0, 1.0),
        (0.0, 1.0),
        (0.25, 0.75),
        (0.1, 1.1),
        (5.0, 5.0),
        (2.0, 1.0),
    ] {
        let mut doc = ImageDocument::new(
            "convert_to_16_from_32.tif",
            8,
            4,
            PixelPlane::Gray32(pixels.clone()),
        )
        .unwrap();
        doc.display_range = DisplayRange { min, max };
        let oracle = bridge
            .run_macro_on_image(&doc, "run(\"16-bit\");")
            .unwrap();
        let oracle_doc = oracle
            .output_image
            .expect("oracle must return saved TIFF");

        doc.convert_to_16bit().unwrap();
        assert_eq!(
            doc.slices, oracle_doc.slices,
            "32-bit -> 16-bit pixels differ for display range {min}..{max}"
        );
        assert_eq!(
            doc.display_range, oracle_doc.display_range,
            "32-bit -> 16-bit display range differs for {min}..{max}"
        );
    }
}

#[test]
fn parity_convert_to_16bit_gray32_zero_range_resets_from_pixels() {
    // FloatProcessor.setMinAndMax(0,0) calls resetMinAndMax
    // (FloatProcessor.java 123-125) before TypeConverter reads the range.
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = ImageDocument::new(
        "float_zero_range.tif",
        4,
        1,
        PixelPlane::Gray32(vec![-2.0, 0.0, 1.0, 3.0]),
    )
    .unwrap();
    let oracle = bridge
        .run_macro_on_image(&doc, "setMinAndMax(0,0); run(\"16-bit\");")
        .unwrap();
    let oracle_doc = oracle.output_image.expect("oracle must return saved TIFF");
    doc.display_range = DisplayRange { min: 0.0, max: 0.0 };
    doc.convert_to_16bit().unwrap();
    assert_eq!(doc.slices, oracle_doc.slices);
    assert_eq!(doc.display_range, oracle_doc.display_range);
}

/// TypeConverter.convertToShort (TypeConverter.java 149-160) sends RGB through
/// convertRGBToByte (equal 1/3 weights, TypeConverter.java 95-117) and then
/// convertByteToShort, so the result is the 0..=255 gray level as a u16.
#[test]
fn parity_convert_to_16bit_rgb_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut pixels = vec![0u32; 32];
    for (i, pixel) in pixels.iter_mut().enumerate() {
        let r = (i * 37) % 256;
        let g = (i * 53 + 7) % 256;
        let b = (i * 97 + 11) % 256;
        *pixel = ((r as u32) << 16) | ((g as u32) << 8) | b as u32;
    }
    pixels[0] = 0x00ff_0000;
    pixels[1] = 0x0000_ff00;
    pixels[2] = 0x0000_00ff;
    pixels[3] = 0x00ff_ffff;
    let mut doc =
        ImageDocument::new("convert_rgb_to_16.tif", 8, 4, PixelPlane::ColorRgb(pixels)).unwrap();
    let oracle = bridge
        .run_macro_on_image(&doc, "run(\"16-bit\");")
        .unwrap();
    let oracle_doc = oracle
        .output_image
        .expect("oracle must return saved TIFF");

    doc.convert_to_16bit().unwrap();
    assert_eq!(doc.slices, oracle_doc.slices);
    assert_eq!(doc.display_range, oracle_doc.display_range);
}

/// StackConverter.convertToGray16 (StackConverter.java 144-180) applies the
/// current processor's display range to every slice but only scales when the
/// source stack is 32-bit; a GRAY32 stack also gets setDisplayRange(0,65535).
#[test]
fn parity_convert_to_16bit_gray8_stack_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = distinct_stack_8bit(8, 6, 3);
    doc.current_slice = 2;
    doc.position = [1, 2, 1];
    doc.display_range = DisplayRange {
        min: 10.0,
        max: 200.0,
    };
    doc.slice_labels = vec![
        Some("first".into()),
        Some("second".into()),
        Some("third".into()),
    ];

    let oracle = bridge
        .run_macro_on_image(
            &doc,
            "Property.setSliceLabel(\"first\",1); Property.setSliceLabel(\"second\",2); \
             Property.setSliceLabel(\"third\",3); setSlice(2); run(\"16-bit\");",
        )
        .unwrap();
    let oracle_doc = oracle
        .output_image
        .expect("oracle must return saved TIFF");

    doc.convert_to_16bit().unwrap();
    assert_eq!(doc.slices, oracle_doc.slices);
    assert_eq!(doc.display_range, oracle_doc.display_range);
    assert_eq!(doc.slice_labels, oracle_doc.slice_labels);
    // The TIFF format does not persist the current slice, so compare it to the
    // pre-conversion position instead of the reloaded oracle document.
    assert_eq!(doc.current_slice, 2);
}

#[test]
fn parity_convert_to_16bit_gray32_stack_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = distinct_stack_gray32(8, 6, 3);
    doc.current_slice = 2;
    doc.position = [1, 2, 1];

    let oracle = bridge
        .run_macro_on_image(&doc, "setSlice(2); run(\"16-bit\");")
        .unwrap();
    let oracle_doc = oracle
        .output_image
        .expect("oracle must return saved TIFF");

    doc.convert_to_16bit().unwrap();
    assert_eq!(doc.slices, oracle_doc.slices);
    assert_eq!(
        doc.display_range,
        DisplayRange {
            min: 0.0,
            max: 65535.0
        },
        "StackConverter.java 178-179 sets the display range to 0..65535 for 32-bit stacks"
    );
    assert_eq!(doc.display_range, oracle_doc.display_range);
    // The TIFF format does not persist the current slice.
    assert_eq!(doc.current_slice, 2);
}

#[test]
fn parity_convert_to_16bit_gray32_stack_zero_range_uses_current_slice() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = ImageDocument::new_stack(
        "float_stack_zero_range.tif",
        2,
        1,
        vec![
            PixelPlane::Gray32(vec![100.0, 200.0]),
            PixelPlane::Gray32(vec![-2.0, 3.0]),
        ],
    )
    .unwrap();
    let oracle = bridge
        .run_macro_on_image(&doc, "setSlice(2); setMinAndMax(0,0); run(\"16-bit\");")
        .unwrap();
    let oracle_doc = oracle.output_image.expect("oracle must return saved TIFF");
    doc.current_slice = 2;
    doc.position = [1, 2, 1];
    doc.display_range = DisplayRange { min: 0.0, max: 0.0 };
    doc.convert_to_16bit().unwrap();
    assert_eq!(doc.slices, oracle_doc.slices);
    assert_eq!(doc.display_range, oracle_doc.display_range);
}

/// ImageConverter.convertToGray16 throws IllegalArgumentException for
/// COLOR_256 images (ImageConverter.java 59-62); Converter.java 136-143 turns
/// that into an aborted macro instead of a converted image.
#[test]
fn convert_to_16bit_rejects_color256_like_java() {
    let mut doc = ImageDocument::new(
        "indexed.tif",
        4,
        2,
        PixelPlane::Color256 {
            pixels: (0..8).collect(),
            lut: Box::new(Lut256::default()),
        },
    )
    .unwrap();
    assert_eq!(
        doc.convert_to_16bit(),
        Err(CoreError::InvalidOperation("Unsupported conversion"))
    );
    assert!(
        matches!(doc.plane(), PixelPlane::Color256 { .. }),
        "a rejected conversion must not modify the pixels"
    );

    let mut session = Session::new();
    session.insert_doc(doc);
    session.macro_recorder.clear();
    assert!(session.dispatch(Command::ConvertTo16Bit).is_err());
}

/// StackConverter.convertToGray16 rejects any stack that is not GRAY8/GRAY32
/// (StackConverter.java 145-148), so RGB stacks cannot become 16-bit even
/// though single RGB images can (ImageConverter.java 59-62 only admits
/// GRAY8, GRAY32 and COLOR_RGB before the stack branch).
#[test]
fn convert_to_16bit_rejects_rgb_stacks_like_java() {
    let stack = ImageDocument::new_stack(
        "rgb_stack.tif",
        2,
        1,
        vec![
            PixelPlane::ColorRgb(vec![0x00ff_0000, 0x0000_ff00]),
            PixelPlane::ColorRgb(vec![0x0000_00ff, 0x00ff_ffff]),
        ],
    )
    .unwrap();
    let mut doc = stack;
    assert_eq!(
        doc.convert_to_16bit(),
        Err(CoreError::InvalidOperation("Unsupported conversion"))
    );
    assert!(matches!(doc.plane(), PixelPlane::ColorRgb(_)));
}

#[test]
fn session_convert_to_16bit_records_macro_and_clears_threshold() {
    let mut doc = convert_gray16_pattern(4, 2);
    doc.display_range = DisplayRange {
        min: 0.0,
        max: 65535.0,
    };
    doc.threshold = Some(ThresholdLevels {
        lower: 10.0,
        upper: 20.0,
    });
    let mut session = Session::new();
    session.insert_doc(doc);
    session.macro_recorder.clear();

    session.dispatch(Command::ConvertTo16Bit).unwrap();

    let doc = session.active_doc().unwrap();
    assert!(matches!(doc.plane(), PixelPlane::Gray16(_)));
    assert_eq!(doc.threshold, None);
    assert_eq!(
        session.macro_recorder,
        vec!["run(\"16-bit\");".to_string()]
    );
}

#[test]
fn parity_convert_to_32bit_gray8_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut pixels = vec![0u8; 32];
    for (i, pixel) in pixels.iter_mut().enumerate() {
        *pixel = ((i * 37 + 11) % 251) as u8;
    }
    let mut doc =
        ImageDocument::new("convert_to_32.tif", 8, 4, PixelPlane::Gray8(pixels)).unwrap();
    doc.display_range = DisplayRange {
        min: 10.0,
        max: 200.0,
    };
    let oracle = bridge
        .run_macro_on_image(&doc, "run(\"32-bit\");")
        .unwrap();
    let oracle_doc = oracle
        .output_image
        .expect("oracle must return saved TIFF");

    doc.convert_to_32bit().unwrap();
    let PixelPlane::Gray32(out) = doc.plane() else {
        panic!("expected Gray32 after conversion, got {:?}", doc.plane());
    };
    let oracle_pixels = match oracle_doc.plane() {
        PixelPlane::Gray32(v) => v.clone(),
        other => panic!("unexpected oracle plane {other:?}"),
    };
    assert_eq!(out, &oracle_pixels, "8-bit -> 32-bit pixels differ");
    assert_eq!(doc.display_range, oracle_doc.display_range);
}

#[test]
fn parity_convert_to_32bit_gray16_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut pixels = vec![0u16; 32];
    for (i, pixel) in pixels.iter_mut().enumerate() {
        *pixel = ((i * 37 + 11) % 65521) as u16;
    }
    let mut doc =
        ImageDocument::new("convert_16_to_32.tif", 8, 4, PixelPlane::Gray16(pixels)).unwrap();
    doc.display_range = DisplayRange {
        min: 100.0,
        max: 500.0,
    };
    let oracle = bridge
        .run_macro_on_image(&doc, "run(\"32-bit\");")
        .unwrap();
    let oracle_doc = oracle
        .output_image
        .expect("oracle must return saved TIFF");

    doc.convert_to_32bit().unwrap();
    let PixelPlane::Gray32(out) = doc.plane() else {
        panic!("expected Gray32 after conversion, got {:?}", doc.plane());
    };
    let oracle_pixels = match oracle_doc.plane() {
        PixelPlane::Gray32(v) => v.clone(),
        other => panic!("unexpected oracle plane {other:?}"),
    };
    assert_eq!(out, &oracle_pixels, "16-bit -> 32-bit pixels differ");
    assert_eq!(doc.display_range, oracle_doc.display_range);
}

#[test]
fn parity_convert_to_32bit_gray16_fractional_range_matches_java_oracle() {
    // ShortProcessor.setMinAndMax rounds each bound with Math.round before
    // ImageConverter reads it (ShortProcessor.java 267-274).
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = ImageDocument::new(
        "fractional_range_16_to_32.tif",
        4,
        1,
        PixelPlane::Gray16(vec![10, 50, 100, 200]),
    )
    .unwrap();
    doc.display_range = DisplayRange { min: 10.6, max: 200.4 };
    let oracle = bridge.run_macro_on_image(&doc, "run(\"32-bit\");").unwrap();
    let oracle_doc = oracle.output_image.expect("oracle must return saved TIFF");
    doc.convert_to_32bit().unwrap();
    assert_eq!(doc.slices, oracle_doc.slices);
    assert_eq!(doc.display_range, oracle_doc.display_range);
}

#[test]
fn parity_convert_to_32bit_gray8_fractional_range_matches_java_oracle() {
    // ByteProcessor.setMinAndMax also rounds each bound with Math.round
    // (ByteProcessor.java 433-438).
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = ImageDocument::new(
        "fractional_range_8_to_32.tif",
        4,
        1,
        PixelPlane::Gray8(vec![10, 50, 100, 200]),
    )
    .unwrap();
    doc.display_range = DisplayRange { min: 10.6, max: 200.4 };
    let oracle = bridge.run_macro_on_image(&doc, "run(\"32-bit\");").unwrap();
    let oracle_doc = oracle.output_image.expect("oracle must return saved TIFF");
    doc.convert_to_32bit().unwrap();
    assert_eq!(doc.slices, oracle_doc.slices);
    assert_eq!(doc.display_range, oracle_doc.display_range);
}

#[test]
fn parity_convert_to_32bit_rgb_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut pixels = vec![0u32; 32];
    for (i, pixel) in pixels.iter_mut().enumerate() {
        let r = (i * 37) % 256;
        let g = (i * 53 + 7) % 256;
        let b = (i * 97 + 11) % 256;
        *pixel = ((r as u32) << 16) | ((g as u32) << 8) | b as u32;
    }
    pixels[0] = 0x00ff_0000;
    pixels[1] = 0x0000_ff00;
    pixels[2] = 0x0000_00ff;
    pixels[3] = 0x00ff_ffff;
    let mut doc =
        ImageDocument::new("convert_rgb_to_32.tif", 8, 4, PixelPlane::ColorRgb(pixels)).unwrap();
    let oracle = bridge
        .run_macro_on_image(&doc, "run(\"32-bit\");")
        .unwrap();
    let oracle_doc = oracle
        .output_image
        .expect("oracle must return saved TIFF");

    doc.convert_to_32bit().unwrap();
    assert_eq!(doc.slices, oracle_doc.slices);
    assert_eq!(doc.display_range, oracle_doc.display_range);
}

#[test]
fn parity_convert_to_32bit_gray8_stack_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = distinct_stack_8bit(8, 6, 3);
    doc.current_slice = 2;
    doc.position = [1, 2, 1];
    doc.display_range = DisplayRange {
        min: 10.0,
        max: 200.0,
    };
    doc.slice_labels = vec![
        Some("first".into()),
        Some("second".into()),
        Some("third".into()),
    ];

    let oracle = bridge
        .run_macro_on_image(
            &doc,
            "Property.setSliceLabel(\"first\",1); Property.setSliceLabel(\"second\",2); \
             Property.setSliceLabel(\"third\",3); setSlice(2); run(\"32-bit\");",
        )
        .unwrap();
    let oracle_doc = oracle
        .output_image
        .expect("oracle must return saved TIFF");

    doc.convert_to_32bit().unwrap();
    assert_eq!(doc.slices, oracle_doc.slices);
    assert_eq!(doc.display_range, oracle_doc.display_range);
    assert_eq!(doc.slice_labels, oracle_doc.slice_labels);
    assert_eq!(doc.current_slice, 2);
}

#[test]
fn parity_convert_to_32bit_gray16_stack_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = distinct_stack_16bit(8, 6, 3);
    doc.current_slice = 2;
    doc.position = [1, 2, 1];
    doc.display_range = DisplayRange {
        min: 500.0,
        max: 40000.0,
    };
    doc.slice_labels = vec![
        Some("first".into()),
        Some("second".into()),
        Some("third".into()),
    ];

    let oracle = bridge
        .run_macro_on_image(
            &doc,
            "Property.setSliceLabel(\"first\",1); Property.setSliceLabel(\"second\",2); \
             Property.setSliceLabel(\"third\",3); setSlice(2); run(\"32-bit\");",
        )
        .unwrap();
    let oracle_doc = oracle
        .output_image
        .expect("oracle must return saved TIFF");

    doc.convert_to_32bit().unwrap();
    assert_eq!(doc.slices, oracle_doc.slices);
    assert_eq!(doc.display_range, oracle_doc.display_range);
    assert_eq!(doc.slice_labels, oracle_doc.slice_labels);
    assert_eq!(doc.current_slice, 2);
}

#[test]
fn parity_convert_to_32bit_rgb_stack_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut slices = Vec::new();
    for s in 0..3 {
        let mut pixels = vec![0u32; 48];
        for y in 0..6 {
            for x in 0..8 {
                let r = (s * 41 + x * 17 + y * 7) % 256;
                let g = (s * 59 + x * 23 + y * 13) % 256;
                let b = (s * 83 + x * 31 + y * 29) % 256;
                pixels[(y * 8 + x) as usize] = (r << 16) | (g << 8) | b;
            }
        }
        slices.push(PixelPlane::ColorRgb(pixels));
    }
    let mut doc = ImageDocument::new_stack("stack_rgb_to_32.tif", 8, 6, slices).unwrap();
    doc.current_slice = 2;
    doc.position = [1, 2, 1];
    doc.slice_labels = vec![
        Some("slice1".into()),
        Some("slice2".into()),
        Some("slice3".into()),
    ];

    let oracle = bridge
        .run_macro_on_image(
            &doc,
            "Property.setSliceLabel(\"slice1\",1); Property.setSliceLabel(\"slice2\",2); \
             Property.setSliceLabel(\"slice3\",3); setSlice(2); run(\"32-bit\");",
        )
        .unwrap();
    let oracle_doc = oracle
        .output_image
        .expect("oracle must return saved TIFF");

    doc.convert_to_32bit().unwrap();
    assert_eq!(doc.slices, oracle_doc.slices);
    assert_eq!(doc.display_range, oracle_doc.display_range);
    assert_eq!(doc.slice_labels, oracle_doc.slice_labels);
    assert_eq!(doc.current_slice, 2);
}

#[test]
fn convert_to_32bit_rejects_color256_like_java() {
    let mut doc = ImageDocument::new(
        "indexed.tif",
        4,
        2,
        PixelPlane::Color256 {
            pixels: (0..8).collect(),
            lut: Box::new(Lut256::default()),
        },
    )
    .unwrap();
    assert_eq!(
        doc.convert_to_32bit(),
        Err(CoreError::InvalidOperation("Unsupported conversion"))
    );
    assert!(
        matches!(doc.plane(), PixelPlane::Color256 { .. }),
        "a rejected conversion must not modify the pixels"
    );

    let mut session = Session::new();
    session.insert_doc(doc);
    session.macro_recorder.clear();
    assert!(session.dispatch(Command::ConvertTo32Bit).is_err());
}

#[test]
fn session_convert_to_32bit_records_macro_and_clears_threshold() {
    let mut doc = convert_gray16_pattern(4, 2);
    doc.display_range = DisplayRange {
        min: 0.0,
        max: 65535.0,
    };
    doc.threshold = Some(ThresholdLevels {
        lower: 10.0,
        upper: 20.0,
    });
    let mut session = Session::new();
    session.insert_doc(doc);
    session.macro_recorder.clear();

    session.dispatch(Command::ConvertTo32Bit).unwrap();

    let doc = session.active_doc().unwrap();
    assert!(matches!(doc.plane(), PixelPlane::Gray32(_)));
    assert_eq!(doc.threshold, None);
    assert_eq!(
        session.macro_recorder,
        vec!["run(\"32-bit\");".to_string()]
    );
}

/// 8-bit pixel pattern that hits both ends and every residue the
/// ByteProcessor.setMinAndMax LUT (ByteProcessor.java 433-474) can bend.
fn rgb_convert_gray8_pattern() -> Vec<u8> {
    let mut pixels: Vec<u8> = (0..48).map(|i| ((i * 37 + 11) % 256) as u8).collect();
    pixels[0] = 0;
    pixels[1] = 255;
    pixels[2] = 1;
    pixels[3] = 254;
    pixels[4] = 128;
    pixels[5] = 127;
    pixels
}

fn custom_indexed_lut() -> Lut256 {
    // A non-grayscale, non-monotonic palette so the display-range LUT and the
    // RGB expansion cannot be confused with a gray ramp.
    let mut lut = Lut256 {
        r: Vec::with_capacity(256),
        g: Vec::with_capacity(256),
        b: Vec::with_capacity(256),
    };
    for i in 0..256u32 {
        lut.r.push(((i * 3) % 256) as u8);
        lut.g.push((255 - i) as u8);
        lut.b.push(((i * i) % 251) as u8);
    }
    lut
}

fn inverted_gray_lut() -> Lut256 {
    let mut lut = Lut256::default();
    lut.r.reverse();
    lut.g.reverse();
    lut.b.reverse();
    lut
}

fn assert_rgb_matches_oracle(doc: &ImageDocument, oracle_doc: &ImageDocument, context: &str) {
    assert!(
        doc.slices
            .iter()
            .all(|plane| matches!(plane, PixelPlane::ColorRgb(_))),
        "{context}: expected RGB planes, got {:?}",
        doc.slices
            .iter()
            .map(PixelPlane::bit_depth)
            .collect::<Vec<_>>()
    );
    assert_eq!(doc.slices, oracle_doc.slices, "{context}: pixels differ");
    assert_eq!(
        doc.display_range, oracle_doc.display_range,
        "{context}: display range differs"
    );
}

#[test]
fn parity_convert_to_rgb_gray8_display_ranges_match_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    for (min, max) in [
        (0.0, 255.0),
        (50.0, 200.0),
        (10.4, 200.6),
        (0.0, 128.0),
        (128.0, 255.0),
        (100.0, 100.0),
        (0.0, 0.0),
        (255.0, 255.0),
        (200.0, 50.0),
    ] {
        let mut doc = ImageDocument::new(
            "rgb_gray8.tif",
            8,
            6,
            PixelPlane::Gray8(rgb_convert_gray8_pattern()),
        )
        .unwrap();
        // The ImageJ TIFF format cannot carry a 0/0 range, so the macro sets
        // every range after opening the unscaled image.
        let oracle = bridge
            .run_macro_on_image(
                &doc,
                &format!("setMinAndMax({min},{max}); run(\"RGB Color\");"),
            )
            .unwrap();
        doc.display_range = DisplayRange { min, max };
        let oracle_doc = oracle
            .output_image
            .expect("oracle must return saved TIFF");

        doc.convert_to_rgb();
        assert_rgb_matches_oracle(&doc, &oracle_doc, &format!("gray8 range {min}..{max}"));
    }
}

#[test]
fn parity_convert_to_rgb_gray16_display_ranges_match_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    for (min, max) in [
        (0.0, 65535.0),
        (1000.0, 20000.0),
        (1000.7, 20000.2),
        (30000.0, 40000.0),
    ] {
        let mut doc = convert_gray16_pattern(8, 4);
        doc.display_range = DisplayRange { min, max };
        let oracle = bridge
            .run_macro_on_image(&doc, "run(\"RGB Color\");")
            .unwrap();
        let oracle_doc = oracle
            .output_image
            .expect("oracle must return saved TIFF");

        doc.convert_to_rgb();
        assert_rgb_matches_oracle(&doc, &oracle_doc, &format!("gray16 range {min}..{max}"));
    }
}

#[test]
fn parity_convert_to_rgb_gray32_display_ranges_match_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut pixels = vec![0f32; 32];
    for (i, value) in pixels.iter_mut().enumerate() {
        *value = (i as f32 - 4.0) * 0.25;
    }
    pixels[0] = -100.0;
    pixels[1] = 100.0;
    pixels[2] = 0.0;
    pixels[3] = 0.5;
    for (min, max) in [
        (-1.0, 1.0),
        (0.0, 1.0),
        (0.25, 0.75),
        (0.1, 1.1),
        (0.0, 0.0),
        (0.5, 0.5),
    ] {
        let mut doc =
            ImageDocument::new("rgb_gray32.tif", 8, 4, PixelPlane::Gray32(pixels.clone()))
                .unwrap();
        // 0/0 cannot be stored in an ImageJ TIFF; set it with the macro.
        let oracle = bridge
            .run_macro_on_image(
                &doc,
                &format!("setMinAndMax({min},{max}); run(\"RGB Color\");"),
            )
            .unwrap();
        doc.display_range = DisplayRange { min, max };
        let oracle_doc = oracle
            .output_image
            .expect("oracle must return saved TIFF");

        doc.convert_to_rgb();
        assert_rgb_matches_oracle(&doc, &oracle_doc, &format!("gray32 range {min}..{max}"));
    }
}

#[test]
fn parity_convert_to_rgb_indexed_color_luts_match_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    for (name, lut) in [
        ("default pseudo-color", Lut256::default()),
        ("3-3-2 RGB", rgb332_lut()),
        ("custom palette", custom_indexed_lut()),
        ("inverting gray", inverted_gray_lut()),
    ] {
        for (min, max) in [(0.0, 255.0), (50.0, 200.0), (10.4, 200.6)] {
            let mut doc = ImageDocument::new(
                format!("rgb_{name}.tif"),
                8,
                6,
                PixelPlane::Color256 {
                    pixels: rgb_convert_gray8_pattern(),
                    lut: Box::new(lut.clone()),
                },
            )
            .unwrap();
            doc.display_range = DisplayRange { min, max };
            let oracle = bridge
                .run_macro_on_image(&doc, "run(\"RGB Color\");")
                .unwrap();
            let oracle_doc = oracle
                .output_image
                .expect("oracle must return saved TIFF");

            doc.convert_to_rgb();
            assert_rgb_matches_oracle(&doc, &oracle_doc, &format!("{name} range {min}..{max}"));
        }
    }
}

#[test]
fn parity_convert_to_rgb_rgb_is_a_noop_like_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let pixels: Vec<u32> = (0..32u32)
        .map(|i| {
            0xff00_0000 | (((i * 37) % 256) << 16) | (((i * 53 + 7) % 256) << 8) | ((i * 97 + 11) % 256)
        })
        .collect();
    let mut doc =
        ImageDocument::new("rgb_noop.tif", 8, 4, PixelPlane::ColorRgb(pixels)).unwrap();
    let before = doc.slices.clone();
    let oracle = bridge
        .run_macro_on_image(&doc, "run(\"RGB Color\");")
        .unwrap();
    let oracle_doc = oracle
        .output_image
        .expect("oracle must return saved TIFF");

    doc.convert_to_rgb();
    assert_eq!(doc.slices, before);
    assert_rgb_matches_oracle(&doc, &oracle_doc, "rgb no-op");
}

fn rgb_conversion_stack(planes: Vec<PixelPlane>) -> ImageDocument {
    let mut doc = ImageDocument::new_stack("rgb_stack.tif", 8, 6, planes).unwrap();
    doc.current_slice = 2;
    doc.position = [1, 2, 1];
    doc.slice_labels = vec![
        Some("first".into()),
        Some("second".into()),
        Some("third".into()),
    ];
    doc
}

fn rgb_stack_macro(range: DisplayRange) -> String {
    // The ImageJ TIFF format cannot carry a 0/0 range, so every range is set
    // with the macro on the current slice (slice 2) after opening.
    format!(
        "Property.setSliceLabel(\"first\",1); Property.setSliceLabel(\"second\",2); \
         Property.setSliceLabel(\"third\",3); setSlice(2); setMinAndMax({},{}); \
         run(\"RGB Color\");",
        range.min, range.max
    )
}

#[test]
fn parity_convert_to_rgb_stacks_match_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let gray8_planes = |shift: usize| {
        (0..3)
            .map(|s| {
                PixelPlane::Gray8(
                    (0..48)
                        .map(|i| ((i * 37 + s * 91 + shift * 5 + 11) % 256) as u8)
                        .collect(),
                )
            })
            .collect::<Vec<_>>()
    };
    let gray32_planes = (0..3)
        .map(|s| {
            PixelPlane::Gray32(
                (0..48)
                    .map(|i| ((i as f32) - 10.0) * 0.125 + s as f32)
                    .collect(),
            )
        })
        .collect::<Vec<_>>();
    let indexed_planes = (0..3)
        .map(|s| PixelPlane::Color256 {
            pixels: (0..48).map(|i| ((i * 37 + s * 91 + 11) % 256) as u8).collect(),
            lut: Box::new(custom_indexed_lut()),
        })
        .collect::<Vec<_>>();
    let gray16_planes = distinct_stack_16bit(8, 6, 3).slices;

    let cases: Vec<(&str, Vec<PixelPlane>, DisplayRange)> = vec![
        (
            "gray8 stack",
            gray8_planes(0),
            DisplayRange { min: 40.0, max: 210.0 },
        ),
        (
            "gray8 stack fractional range",
            gray8_planes(1),
            DisplayRange { min: 40.4, max: 210.6 },
        ),
        (
            "gray16 stack",
            gray16_planes,
            DisplayRange { min: 500.0, max: 40000.0 },
        ),
        (
            "gray32 stack",
            gray32_planes.clone(),
            DisplayRange { min: -0.5, max: 4.0 },
        ),
        (
            "gray32 stack zero range",
            gray32_planes,
            DisplayRange { min: 0.0, max: 0.0 },
        ),
        (
            "color256 stack",
            indexed_planes,
            DisplayRange { min: 30.0, max: 220.0 },
        ),
    ];
    for (name, planes, range) in cases {
        let mut doc = rgb_conversion_stack(planes);
        let oracle = bridge
            .run_macro_on_image(&doc, &rgb_stack_macro(range))
            .unwrap();
        doc.display_range = range;
        let oracle_doc = oracle
            .output_image
            .expect("oracle must return saved TIFF");

        doc.convert_to_rgb();
        assert_rgb_matches_oracle(&doc, &oracle_doc, name);
        assert_eq!(doc.slice_labels, oracle_doc.slice_labels, "{name}: labels");
        assert_eq!(doc.current_slice, 2, "{name}: current slice");
    }
}

#[test]
fn convert_to_rgb_preserves_roi_and_undo_matches_java_oracle() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let mut doc = ImageDocument::new(
        "rgb_undo.tif",
        8,
        6,
        PixelPlane::Gray8(rgb_convert_gray8_pattern()),
    )
    .unwrap();
    doc.display_range = DisplayRange { min: 50.0, max: 200.0 };
    doc.set_rect_roi(1, 1, 3, 2).unwrap();
    let original_slices = doc.slices.clone();

    let oracle = bridge
        .run_macro_on_image(
            &doc,
            "makeRectangle(1,1,3,2); run(\"RGB Color\"); run(\"Undo\");",
        )
        .unwrap();
    let oracle_doc = oracle
        .output_image
        .expect("oracle must return saved TIFF");

    doc.convert_to_rgb();
    assert!(
        matches!(doc.plane(), PixelPlane::ColorRgb(_)),
        "RGB Color must change the pixel type"
    );
    assert!(doc.roi.is_some(), "Converter.java 143-144 restores the ROI");
    assert!(doc.undo());
    assert_eq!(doc.slices, original_slices);
    assert_eq!(doc.slices, oracle_doc.slices, "undo pixels");
    assert_eq!(doc.display_range, oracle_doc.display_range, "undo range");
    assert_eq!(
        doc.display_range,
        DisplayRange { min: 50.0, max: 200.0 },
        "TYPE_CONVERSION undo restores the 8-bit display range"
    );
}

#[test]
fn convert_to_rgb_of_a_stack_has_no_undo() {
    let mut doc = rgb_conversion_stack(vec![
        PixelPlane::Gray8(vec![1; 48]),
        PixelPlane::Gray8(vec![2; 48]),
        PixelPlane::Gray8(vec![3; 48]),
    ]);
    doc.convert_to_rgb();
    assert!(
        matches!(doc.plane(), PixelPlane::ColorRgb(_)),
        "stack conversion must produce RGB"
    );
    assert!(!doc.undo(), "whole-stack type conversions have no Undo buffer");
}

#[test]
fn session_convert_to_rgb_records_macro_and_clears_threshold() {
    let mut doc = ImageDocument::new(
        "rgb_session.tif",
        8,
        6,
        PixelPlane::Gray8(rgb_convert_gray8_pattern()),
    )
    .unwrap();
    doc.threshold = Some(ThresholdLevels {
        lower: 10.0,
        upper: 20.0,
    });
    let mut session = Session::new();
    session.insert_doc(doc);
    session.macro_recorder.clear();

    session.dispatch(Command::ConvertToRgb).unwrap();

    let doc = session.active_doc().unwrap();
    assert!(matches!(doc.plane(), PixelPlane::ColorRgb(_)));
    assert_eq!(doc.display_range, DisplayRange { min: 0.0, max: 255.0 });
    assert_eq!(
        doc.threshold, None,
        "Converter.java 55-56 resets the threshold before converting"
    );
    assert_eq!(
        session.macro_recorder,
        vec!["run(\"RGB Color\");".to_string()],
        "RGB Color must record exactly one macro line without a trailing newline"
    );
}

// ---------------------------------------------------------------------------
// M4b-1: ResultsTable dynamic columns and formatting
// (ResultsTable.java 20-38, 190-218, 826-960; Analyzer.java 556-703).
// ---------------------------------------------------------------------------

/// Every `Set Measurements...` checkbox that `ResultsTable` can render from
/// the modelled `MeasurementSummary` fields (Feret and LIMIT's MinThr/MaxThr
/// columns are not modelled yet).
const ALL_TABLE_MEASUREMENTS: &str = "area mean standard modal min centroid center perimeter \
     bounding fit shape integrated median skewness kurtosis area_fraction stack";

/// Parses the macro option string handed to `Set Measurements...` so the Rust
/// and Java sides are configured from one source of truth.
fn table_config(options: &str) -> MeasurementConfig {
    let mut config = MeasurementConfig {
        flags: 0,
        precision: 3,
    };
    for token in options.split_whitespace() {
        if let Some(decimals) = token.strip_prefix("decimal=") {
            config.precision = decimals.parse().unwrap();
            continue;
        }
        config.flags |= match token {
            "area" => MeasurementFlags::AREA,
            "mean" => MeasurementFlags::MEAN,
            "standard" => MeasurementFlags::STD_DEV,
            "modal" => MeasurementFlags::MODE,
            "min" => MeasurementFlags::MIN_MAX,
            "centroid" => MeasurementFlags::CENTROID,
            "center" => MeasurementFlags::CENTER_OF_MASS,
            "perimeter" => MeasurementFlags::PERIMETER,
            "bounding" => MeasurementFlags::RECT,
            "fit" => MeasurementFlags::ELLIPSE,
            "shape" => MeasurementFlags::SHAPE_DESCRIPTORS,
            "integrated" => MeasurementFlags::INTEGRATED_DENSITY,
            "median" => MeasurementFlags::MEDIAN,
            "skewness" => MeasurementFlags::SKEWNESS,
            "kurtosis" => MeasurementFlags::KURTOSIS,
            "area_fraction" => MeasurementFlags::AREA_FRACTION,
            "stack" => MeasurementFlags::STACK_POSITION,
            "scientific" => MeasurementFlags::SCIENTIFIC_NOTATION,
            other => panic!("unknown Set Measurements option {other}"),
        };
    }
    config
}

/// Runs `setup` in the Java oracle, saves the system Results table with
/// `saveAs('Results', ...)` and returns that file's text without its trailing
/// newline. Saving the table avoids relying on the per-measure stdout row,
/// which headless ImageJ trims to the newest line.
fn oracle_saved_results_tsv(setup: &str, tag: &str) -> String {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let path = std::env::temp_dir().join(format!("m4b1_{}_{tag}.tsv", std::process::id()));
    let path_text = path.to_str().unwrap();
    let output = bridge
        .eval_macro(&format!(
            "{setup} saveAs('Results', '{path_text}'); \
             print('M4B1_TSV_BEGIN'); print(File.openAsString('{path_text}'));"
        ))
        .unwrap();
    let _ = std::fs::remove_file(&path);
    let stdout = output.stdout.replace("\r\n", "\n");
    let (_, tsv) = stdout
        .split_once("M4B1_TSV_BEGIN\n")
        .unwrap_or_else(|| panic!("oracle did not print the saved table:\n{stdout}"));
    tsv.trim_end_matches('\n').to_string()
}

fn rust_results_tsv(rows: Vec<MeasurementSummary>, config: MeasurementConfig) -> String {
    let mut table = ResultsTable::new();
    table.config = config;
    for row in rows {
        table.push(row);
    }
    table.to_tsv().trim_end_matches('\n').to_string()
}

#[derive(Clone, Copy)]
enum TableRoi {
    WholeImage,
    Rect,
    Oval,
    Polygon,
    Line,
}

impl TableRoi {
    fn macro_text(self) -> &'static str {
        match self {
            TableRoi::WholeImage => "",
            TableRoi::Rect => "makeRectangle(2,3,8,6);",
            TableRoi::Oval => "makeOval(1,2,9,7);",
            TableRoi::Polygon => "makePolygon(2,2,10,3,8,10,3,9);",
            TableRoi::Line => "makeLine(2,3,10,7);",
        }
    }

    fn apply(self, doc: &mut ImageDocument) {
        match self {
            TableRoi::WholeImage => {}
            TableRoi::Rect => doc.set_rect_roi(2, 3, 8, 6).unwrap(),
            TableRoi::Oval => doc.set_oval_roi(1, 2, 9, 7).unwrap(),
            TableRoi::Polygon => doc.set_polygon_roi(&[(2, 2), (10, 3), (8, 10), (3, 9)]).unwrap(),
            TableRoi::Line => doc.set_line_roi(2, 3, 10, 7),
        }
    }
}

#[test]
fn parity_results_table_tsv_single_row_flag_combinations_match_java_oracle() {
    let all = ALL_TABLE_MEASUREMENTS;
    // (name, ROI, calibrated, options). decimal=2 and decimal=5 exercise the
    // precision-dependent fixed columns and AUTO_FORMAT integers; decimal=0
    // covers d2s's HALF_UP zero-decimal pattern and scientific covers its
    // notation switch.
    let cases: Vec<(&str, TableRoi, bool, String)> = vec![
        ("all rect d2", TableRoi::Rect, false, format!("{all} decimal=2")),
        ("all oval d5", TableRoi::Oval, false, format!("{all} decimal=5")),
        ("all polygon d3", TableRoi::Polygon, false, format!("{all} decimal=3")),
        ("all whole image d2", TableRoi::WholeImage, false, format!("{all} decimal=2")),
        (
            "stats only d5",
            TableRoi::Rect,
            false,
            "mean standard median skewness kurtosis decimal=5".to_string(),
        ),
        (
            "shape descriptors d2",
            TableRoi::Polygon,
            false,
            "area perimeter fit shape decimal=2".to_string(),
        ),
        (
            "calibrated bounding d2",
            TableRoi::Rect,
            true,
            "area mean min centroid bounding integrated decimal=2".to_string(),
        ),
        (
            "calibrated all d5",
            TableRoi::Oval,
            true,
            format!("{all} decimal=5"),
        ),
        (
            "default flags d3",
            TableRoi::Rect,
            false,
            "area mean min decimal=3".to_string(),
        ),
        (
            "half up d0",
            TableRoi::Rect,
            false,
            "area mean centroid center decimal=0".to_string(),
        ),
        (
            "scientific d3",
            TableRoi::Rect,
            false,
            "area mean standard skewness scientific decimal=3".to_string(),
        ),
        (
            "shape descriptors scientific d2",
            TableRoi::Rect,
            false,
            "area perimeter fit shape scientific decimal=2".to_string(),
        ),
    ];
    for (index, (name, roi, calibrated, options)) in cases.into_iter().enumerate() {
        let cal_macro = if calibrated {
            "setVoxelSize(0.5,0.25,1,'mm');"
        } else {
            ""
        };
        let expected = oracle_saved_results_tsv(
            &format!(
                "newImage('t','8-bit ramp',16,16,1); {cal_macro} {} \
                 run('Set Measurements...', '{options}'); run('Measure');",
                roi.macro_text()
            ),
            &format!("single{index}"),
        );
        let config = table_config(&options);
        let mut doc = ImageDocument::new_ramp_8bit("t", 16, 16).unwrap();
        if calibrated {
            doc.calibration.pixel_width = 0.5;
            doc.calibration.pixel_height = 0.25;
        }
        roi.apply(&mut doc);
        let actual = rust_results_tsv(vec![doc.measure_with_config(config).unwrap()], config);
        assert_eq!(actual, expected, "{name}: {options}");
    }
}

#[test]
fn parity_results_table_tsv_line_rows_match_java_oracle() {
    // A straight line writes Length (dynamic column) and Angle (index 17).
    // When none of Area/Mean/StdDev/Mode/Min/Max/Centroid/Median is selected
    // Analyzer.measureLength writes only those two cells (Analyzer.java
    // 468-476), so Perim. and the bounding box stay out of the table.
    for (index, options) in [
        "area mean min decimal=3",
        "area mean min decimal=5",
        "area mean perimeter bounding fit shape decimal=2",
        "mean standard median decimal=2",
        "perimeter bounding decimal=3",
        "fit shape decimal=4",
    ]
    .into_iter()
    .enumerate()
    {
        let expected = oracle_saved_results_tsv(
            &format!(
                "newImage('t','8-bit ramp',16,16,1); makeLine(2,3,10,7); \
                 run('Set Measurements...', '{options}'); run('Measure');"
            ),
            &format!("line{index}"),
        );
        let config = table_config(options);
        let mut doc = ImageDocument::new_ramp_8bit("t", 16, 16).unwrap();
        TableRoi::Line.apply(&mut doc);
        let actual = rust_results_tsv(vec![doc.measure_with_config(config).unwrap()], config);
        assert_eq!(actual, expected, "{options}");
    }
}

#[test]
fn parity_results_table_tsv_calibrated_line_bounds_match_java_oracle() {
    let options = "area mean bounding decimal=3";
    let expected = oracle_saved_results_tsv(
        &format!(
            "newImage('t','8-bit ramp',16,16,1); setVoxelSize(0.5,0.25,1,'mm'); \
             makeLine(2,3,10,7); run('Set Measurements...', '{options}'); run('Measure');"
        ),
        "calibrated_line_bounds",
    );
    let config = table_config(options);
    let mut doc = ImageDocument::new_ramp_8bit("t", 16, 16).unwrap();
    doc.calibration.pixel_width = 0.5;
    doc.calibration.pixel_height = 0.25;
    doc.set_line_roi(2, 3, 10, 7);
    let actual = rust_results_tsv(vec![doc.measure_with_config(config).unwrap()], config);
    assert_eq!(actual, expected);
}

#[test]
fn parity_results_table_tsv_stack_position_columns_match_java_oracle() {
    // (dimensions, position, macro setup) for Ch/Slice/Frame (Analyzer.java
    // 660-686): plain stacks only report Slice, hyperstacks report each
    // dimension larger than one.
    let options = "area mean stack decimal=2";
    let config = table_config(options);
    let plane = ramp8_plane(8, 8);
    let cases: [(&str, (i32, i32, i32), (i32, i32, i32), usize); 3] = [
        (
            "plain stack",
            (1, 5, 1),
            (1, 3, 1),
            5,
        ),
        (
            "hyperstack",
            (2, 3, 2),
            (2, 2, 2),
            12,
        ),
        (
            "no frame",
            (2, 3, 1),
            (1, 3, 1),
            6,
        ),
    ];
    for (index, (name, (c, z, t), (pc, pz, pt), count)) in cases.into_iter().enumerate() {
        let setup = if name == "plain stack" {
            format!("newImage('t','8-bit ramp',8,8,{count}); setSlice({pz});")
        } else {
            format!(
                "newImage('t','8-bit ramp',8,8,{count}); Stack.setDimensions({c},{z},{t}); \
                 Stack.setPosition({pc},{pz},{pt});"
            )
        };
        let expected = oracle_saved_results_tsv(
            &format!("{setup} run('Set Measurements...', '{options}'); run('Measure');"),
            &format!("stack{index}"),
        );
        let mut doc = ImageDocument::new_stack("t", 8, 8, vec![plane.clone(); count]).unwrap();
        if name == "plain stack" {
            doc.set_slice(pz).unwrap();
        } else {
            doc.set_dimensions(c as u32, z as u32, t as u32).unwrap();
            doc.set_position(pc, pz, pt);
        }
        let actual = rust_results_tsv(vec![doc.measure_with_config(config).unwrap()], config);
        assert_eq!(actual, expected, "{name}");
    }
}

/// Three rectangular particles in raster-scan order (Analyze Particles visits
/// the first pixel of each blob from the top-left). The first particle is a
/// constant 200 so its Mean is integral until the second row turns the column
/// into a fixed-decimal column; a few off-level pixels make the other columns
/// fractional.
fn particle_table_doc() -> (ImageDocument, &'static str, [(i32, i32, i32, i32); 3]) {
    let blobs = [(20, 1, 3, 7), (2, 2, 4, 3), (10, 4, 6, 5)];
    let extras = [(2, 2, 230u8), (10, 4, 201), (11, 5, 255)];
    let mut pixels = vec![0u8; 40 * 24];
    let mut macro_text = String::from("newImage('t','8-bit black',40,24,1); setColor(200);");
    for &(x, y, w, h) in &blobs {
        for yy in y..y + h {
            for xx in x..x + w {
                pixels[(yy * 40 + xx) as usize] = 200;
            }
        }
        macro_text.push_str(&format!(" fillRect({x},{y},{w},{h});"));
    }
    for &(x, y, value) in &extras {
        pixels[(y * 40 + x) as usize] = value;
        macro_text.push_str(&format!(" setPixel({x},{y},{value});"));
    }
    let mut doc = ImageDocument::new("t", 40, 24, PixelPlane::Gray8(pixels)).unwrap();
    doc.threshold = Some(ThresholdLevels {
        lower: 200.0,
        upper: 255.0,
    });
    // Leak the macro so the helper can hand out a 'static prelude shared by
    // every multi-row case.
    (doc, Box::leak(macro_text.into_boxed_str()), blobs)
}

#[test]
fn parity_results_table_tsv_multi_row_columns_match_java_oracle() {
    // Multi-row tables are where ResultsTable.addValue's first-24-rows rule
    // matters: once the second row makes Mean fractional, the first row is
    // retroactively saved as "200.000" instead of "200". Analyze Particles
    // writes all particle rows to the system table; a repeated "Measure"
    // would keep only the newest row in headless mode.
    let cases = [
        "area mean min decimal=3",
        "area mean min decimal=2",
        "area mean standard modal min centroid center integrated median \
         skewness kurtosis area_fraction stack decimal=2",
        "mean standard median skewness kurtosis integrated decimal=5",
    ];
    for (index, options) in cases.into_iter().enumerate() {
        let (doc, setup, blobs) = particle_table_doc();
        let expected = oracle_saved_results_tsv(
            &format!(
                "{setup} setThreshold(200,255); run('Set Measurements...', '{options}'); \
                 run('Analyze Particles...', 'size=0-Infinity display clear');"
            ),
            &format!("multi{index}"),
        );
        let config = table_config(options);
        let rows = blobs
            .iter()
            .map(|&(x, y, w, h)| {
                let mut blob_doc = doc.clone();
                blob_doc.set_rect_roi(x, y, w, h).unwrap();
                blob_doc.measure_with_config(config).unwrap()
            })
            .collect();
        assert_eq!(rust_results_tsv(rows, config), expected, "{options}");
    }
}

#[test]
fn default_config_results_table_to_tsv_matches_to_tsv_basic() {
    // The default Set Measurements state must keep the historical table
    // byte-for-byte so the Slice 1 verification is unaffected.
    let mut area_doc = ImageDocument::new_ramp_8bit("mixed_area.tif", 16, 16).unwrap();
    area_doc.set_rect_roi(2, 3, 8, 6).unwrap();
    let mut line_doc = ImageDocument::new_ramp_8bit("mixed_line.tif", 16, 16).unwrap();
    line_doc.set_line_roi(2, 3, 10, 7);
    let area_row = area_doc.measure().unwrap();
    let line_row = line_doc.measure().unwrap();

    let empty = ResultsTable::new();
    assert_eq!(empty.config, MeasurementConfig::default());
    assert_eq!(empty.to_tsv(), empty.to_tsv_basic());

    for rows in [
        vec![area_row.clone()],
        vec![line_row.clone()],
        vec![area_row.clone(), line_row.clone()],
        vec![line_row.clone(), area_row.clone(), area_row.clone()],
    ] {
        let mut table = ResultsTable::new();
        for row in rows {
            table.push(row);
        }
        assert_eq!(table.to_tsv(), table.to_tsv_basic());
    }

    let mut table = ResultsTable::new();
    table.push(area_row);
    table.push(line_row);
    assert_eq!(
        table.to_tsv(),
        " \tArea\tMean\tMin\tMax\tAngle\tLength\n\
         1\t48\t88.000\t32\t144\t0.000\t0.000\n\
         2\t10\t96.000\t32\t160\t-26.565\t8.944\n"
    );
}

#[test]
fn session_set_measurements_drives_java_compatible_results_tsv() {
    for (index, options) in [
        "area mean standard min centroid perimeter fit shape area_fraction decimal=2",
        "mean standard median skewness kurtosis decimal=5",
    ]
    .into_iter()
    .enumerate()
    {
        let expected = oracle_saved_results_tsv(
            &format!(
                "newImage('t','8-bit ramp',16,16,1); makeRectangle(2,3,8,6); \
                 run('Set Measurements...', '{options}'); run('Measure');"
            ),
            &format!("session_measurements_{index}"),
        );
        let config = table_config(options);
        let mut session = Session::new();
        session
            .dispatch(Command::SetMeasurements {
                flags: config.flags,
                precision: config.precision,
            })
            .unwrap();
        let mut doc = ImageDocument::new_ramp_8bit("t", 16, 16).unwrap();
        doc.set_rect_roi(2, 3, 8, 6).unwrap();
        session.insert_doc(doc);
        session.dispatch(Command::Measure).unwrap();
        assert_eq!(session.results.config, config);
        let path = std::env::temp_dir().join(format!(
            "m4b2_session_{}_{}.tsv",
            std::process::id(), index
        ));
        session
            .dispatch(Command::SaveResultsTsv { path: path.clone() })
            .unwrap();
        let actual = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(path);
        assert_eq!(actual.trim_end(), expected, "{options}");
        let recorded_options = options.replace(" decimal=", " redirect=None decimal=");
        assert_eq!(
            session.macro_recorder[0],
            format!("run(\"Set Measurements...\", \"{recorded_options}\");")
        );
        assert_eq!(session.macro_recorder[1], "run(\"Measure\");");
    }
}

#[test]
fn session_set_measurements_limit_uses_active_threshold() {
    let options = "area mean min area_fraction limit decimal=3";
    let expected = oracle_saved_results_tsv(
        &format!(
            "newImage('t','8-bit ramp',16,16,1); makeRectangle(2,3,8,6); \
             setThreshold(100,200); run('Set Measurements...', '{options}'); run('Measure');"
        ),
        "session_measurements_limit",
    );
    let config = table_config("area mean min area_fraction decimal=3");
    let mut session = Session::new();
    session
        .dispatch(Command::SetMeasurements {
            flags: config.flags | MeasurementFlags::LIMIT,
            precision: config.precision,
        })
        .unwrap();
    let mut doc = ImageDocument::new_ramp_8bit("t", 16, 16).unwrap();
    doc.set_rect_roi(2, 3, 8, 6).unwrap();
    doc.threshold = Some(ThresholdLevels { lower: 100.0, upper: 200.0 });
    session.insert_doc(doc);
    session.dispatch(Command::Measure).unwrap();
    assert_eq!(session.results.to_tsv().trim_end(), expected);
    assert_eq!(
        session.macro_recorder[0],
        "run(\"Set Measurements...\", \"area mean min area_fraction limit redirect=None decimal=3\");"
    );
}

#[test]
fn parity_results_table_limit_columns_without_threshold_match_java_oracle() {
    let config = MeasurementConfig {
        flags: MeasurementFlags::AREA | MeasurementFlags::LIMIT,
        precision: 3,
    };
    for (index, (image_type, doc)) in [
        ("8-bit ramp", ImageDocument::new_ramp_8bit("t", 16, 16).unwrap()),
        ("16-bit ramp", ImageDocument::new_ramp_16bit("t", 16, 16).unwrap()),
        ("32-bit ramp", ImageDocument::new_ramp_32bit("t", 16, 16).unwrap()),
        ("RGB ramp", ImageDocument::new("t", 16, 16, PixelPlane::ColorRgb(rgb_ramp(16, 16))).unwrap()),
    ]
    .into_iter()
    .enumerate()
    {
        let expected = oracle_saved_results_tsv(
            &format!(
                "newImage('t','{image_type}',16,16,1); \
                 run('Set Measurements...', 'area limit decimal=3'); run('Measure');"
            ),
            &format!("limit_no_threshold_{index}"),
        );
        let actual = rust_results_tsv(vec![doc.measure_with_config(config).unwrap()], config);
        assert_eq!(actual, expected, "{image_type}");
    }
}

#[test]
fn session_set_measurements_is_global_and_clamps_decimal_places() {
    let mut session = Session::new();
    let flags = MeasurementFlags::AREA | MeasurementFlags::MEAN;
    assert!(matches!(
        session.dispatch(Command::SetMeasurements { flags, precision: 15 }),
        Ok(CommandOutcome::Updated)
    ));
    assert_eq!(session.results.config.flags, flags);
    assert_eq!(session.results.config.precision, 9);
    assert_eq!(
        session.macro_recorder,
        vec!["run(\"Set Measurements...\", \"area mean redirect=None decimal=9\");"]
    );
    session.dispatch(Command::ClearResults).unwrap();
    assert_eq!(session.results.config.precision, 9);
}

// M6a: Analyze > Plot Profile. ProfilePlot.java 53-112 (constructor),
// 180-235 (getStraightLineProfile/getColumnAverageProfile) and
// ImageProcessor.java 1116-1139 (getLine).
fn oracle_profile(doc: &ImageDocument, roi_macro: &str) -> Vec<f64> {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let macro_text = format!(
        "{roi_macro} p=getProfile(); print(\"PROFILE_LEN:\"+p.length); \
         for(i=0;i<p.length;i++) print(d2s(p[i],9));"
    );
    let output = bridge.run_macro_on_image(doc, &macro_text).unwrap();
    let mut lines = output.stdout.lines().filter(|line| !line.trim().is_empty());
    let len = lines
        .next()
        .unwrap()
        .strip_prefix("PROFILE_LEN:")
        .unwrap()
        .parse::<usize>()
        .unwrap();
    let values = lines
        .map(|line| line.trim().parse::<f64>().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(values.len(), len);
    values
}

// ProfilePlot.findMinAndMax (ProfilePlot.java 323-338) uses `else if`, so a
// value below the running minimum is never considered for the maximum.
fn java_profile_min_max(values: &[f64]) -> (f64, f64) {
    let mut min = values[0];
    let mut max = values[0];
    for &value in &values[1..] {
        if value < min {
            min = value;
        } else if value > max {
            max = value;
        }
    }
    (min, max)
}

fn assert_profile_min_max(actual: (f64, f64), expected: (f64, f64), label: &str) {
    // The oracle prints nine decimals, so its min/max is a rounded copy of the
    // same profile; compare with the profile tolerance.
    assert!(
        (actual.0 - expected.0).abs() <= 1e-4,
        "{label} min: {} vs {}",
        actual.0,
        expected.0
    );
    assert!(
        (actual.1 - expected.1).abs() <= 1e-4,
        "{label} max: {} vs {}",
        actual.1,
        expected.1
    );
}

fn assert_profile_values(actual: &[f64], expected: &[f64], label: &str) {
    assert_eq!(actual.len(), expected.len(), "{label} length");
    for (index, (actual, expected)) in actual.iter().zip(expected.iter()).enumerate() {
        if expected.is_nan() {
            assert!(actual.is_nan(), "{label} index {index}: {actual} is not NaN");
        } else {
            assert!(
                (actual - expected).abs() <= 1e-4,
                "{label} index {index}: {actual} vs {expected}"
            );
        }
    }
}

fn profile_test_documents() -> Vec<(&'static str, ImageDocument)> {
    vec![
        (
            "8-bit",
            ImageDocument::new(
                "profile8",
                8,
                6,
                PixelPlane::Gray8((0..48).map(|i| ((i * 7 + 5) % 256) as u8).collect()),
            )
            .unwrap(),
        ),
        (
            "16-bit",
            ImageDocument::new(
                "profile16",
                8,
                6,
                PixelPlane::Gray16((0..48).map(|i| ((i * 1009 + 37) % 65536) as u16).collect()),
            )
            .unwrap(),
        ),
        (
            "32-bit",
            ImageDocument::new(
                "profile32",
                8,
                6,
                PixelPlane::Gray32((0..48).map(|i| (i as f32) * 0.5 - 3.25).collect()),
            )
            .unwrap(),
        ),
    ]
}

#[test]
fn parity_plot_profile_rect_column_averages_match_java_oracle() {
    for (name, doc) in profile_test_documents() {
        for (label, roi_macro, x, y, width, height) in [
            ("in-bounds", "makeRectangle(1,1,5,4);", 1, 1, 5, 4),
            // Out-of-bounds columns make ImageProcessor.getPixelValue return
            // NaN and getColumnAverageProfile skip them (ProfilePlot.java
            // 210-225); a column with no samples becomes 0/0 = NaN.
            ("clipped", "makeRectangle(5,4,6,4);", 5, 4, 6, 4),
        ] {
            let mut doc = doc.clone();
            doc.set_rect_roi(x, y, width, height).unwrap();
            let expected = oracle_profile(&doc, roi_macro);
            let actual = doc.plot_profile().unwrap();
            assert_profile_values(&actual.values, &expected, &format!("{name} {label}"));
            assert_profile_min_max(
                (actual.min, actual.max),
                java_profile_min_max(&expected),
                &format!("{name} {label}"),
            );
        }
    }
}

#[test]
fn parity_plot_profile_line_profiles_match_java_oracle() {
    for (name, doc) in profile_test_documents() {
        for (label, roi_macro, x1, y1, x2, y2) in [
            ("horizontal", "makeLine(1,2,6,2);", 1, 2, 6, 2),
            ("vertical", "makeLine(3,1,3,5);", 3, 1, 3, 5),
            ("slanted", "makeLine(1,1,7,5);", 1, 1, 7, 5),
            // Leaves the image: getInterpolatedValue returns 0.0 beyond the
            // one-pixel edge band (ImageProcessor.java 2008-2013).
            ("out-of-bounds", "makeLine(2,1,4,9);", 2, 1, 4, 9),
        ] {
            let mut doc = doc.clone();
            doc.set_line_roi(x1, y1, x2, y2);
            let expected = oracle_profile(&doc, roi_macro);
            let actual = doc.plot_profile().unwrap();
            assert_profile_values(&actual.values, &expected, &format!("{name} {label}"));
            assert_profile_min_max(
                (actual.min, actual.max),
                java_profile_min_max(&expected),
                &format!("{name} {label}"),
            );
        }
    }
}

#[test]
fn plot_profile_reports_calibrated_x_increment_and_units() {
    // ProfilePlot.java 66-68: xInc = cal.pixelWidth and units = cal.getUnits()
    // before the ROI branch. Calibration.getUnits() pluralizes "pixel",
    // "micron" and "inch" (Calibration.java 159-175).
    let mut doc = ImageDocument::new("profile-units", 8, 6, PixelPlane::Gray8(vec![0; 48])).unwrap();
    doc.set_rect_roi(1, 1, 5, 4).unwrap();
    let profile = doc.plot_profile().unwrap();
    assert_eq!(profile.x_inc, 1.0);
    assert_eq!(profile.units, "pixels");

    doc.calibration.pixel_width = 0.5;
    doc.calibration.pixel_height = 0.25;
    doc.calibration.unit = "mm".to_string();
    let profile = doc.plot_profile().unwrap();
    // getColumnAverageProfile does not change xInc (ProfilePlot.java 210-225).
    assert_eq!(profile.x_inc, 0.5);
    assert_eq!(profile.units, "mm");

    doc.calibration.unit = "micron".to_string();
    assert_eq!(doc.plot_profile().unwrap().units, "microns");

    // Equal pixel sizes keep xInc = pixelWidth for lines too
    // (getStraightLineProfile only recalibrates when they differ,
    // ProfilePlot.java 190-203).
    doc.calibration.pixel_width = 0.5;
    doc.calibration.pixel_height = 0.5;
    doc.calibration.unit = "mm".to_string();
    doc.set_line_roi(2, 3, 10, 9);
    assert_eq!(doc.plot_profile().unwrap().x_inc, 0.5);

    // pixelWidth != pixelHeight: xInc = calibratedLength / pixelLength.
    // dx=8, dy=6, pixelLength=10; calibrated dx=4, dy=1.5.
    doc.calibration.pixel_width = 0.5;
    doc.calibration.pixel_height = 0.25;
    let expected = (4.0_f64 * 4.0 + 1.5_f64 * 1.5).sqrt() / 10.0;
    assert_eq!(doc.plot_profile().unwrap().x_inc, expected);
}

#[test]
fn plot_profile_rejects_missing_or_unsupported_roi() {
    let mut doc = ImageDocument::new("profile-errors", 8, 6, PixelPlane::Gray8(vec![0; 48])).unwrap();
    assert_eq!(
        doc.plot_profile(),
        Err(CoreError::InvalidOperation(
            "Line or rectangular selection required."
        ))
    );
    doc.set_oval_roi(1, 1, 4, 4).unwrap();
    assert_eq!(
        doc.plot_profile(),
        Err(CoreError::InvalidOperation(
            "Line or rectangular selection required."
        ))
    );
    doc.set_polygon_roi(&[(1, 1), (6, 1), (4, 5)]).unwrap();
    assert_eq!(
        doc.plot_profile(),
        Err(CoreError::InvalidOperation(
            "Line or rectangular selection required."
        ))
    );
}

#[test]
fn session_plot_profile_preserves_document_and_records_macro() {
    let mut session = Session::new();
    let id = match session
        .dispatch(Command::NewRamp8Bit {
            title: "ramp".into(),
            width: 16,
            height: 16,
        })
        .unwrap()
    {
        CommandOutcome::ImageOpened(id) => id,
        other => panic!("unexpected outcome: {other:?}"),
    };
    session
        .dispatch(Command::MakeRectRoi {
            x: 2,
            y: 3,
            width: 4,
            height: 5,
        })
        .unwrap();
    let before = session.active_doc().unwrap().clone();
    session.set_dirty(id, false);

    let outcome = session.dispatch(Command::PlotProfile).unwrap();
    assert_eq!(
        session.macro_recorder.last().map(String::as_str),
        Some("run(\"Plot Profile\");")
    );
    match outcome {
        CommandOutcome::ProfileComputed(profile) => {
            assert_eq!(profile.values, vec![32.0, 48.0, 64.0, 80.0]);
            assert_eq!(profile.min, 32.0);
            assert_eq!(profile.max, 80.0);
            assert_eq!(profile.x_inc, 1.0);
            assert_eq!(profile.units, "pixels");
        }
        other => panic!("unexpected outcome: {other:?}"),
    }
    assert_eq!(session.active_doc().unwrap(), &before);
    assert!(!session.is_dirty(id));
}

// ---------------------------------------------------------------------------
// M7: whole-stack ("Process Stack?" -> Yes) grayscale filters.
//
// IJ.setupDialog adds PlugInFilter.DOES_STACKS for the "Yes" answer
// (original_imagej/ij/IJ.java 1252-1290), and PlugInFilterRunner.processStack
// then applies the same processor ROI to every slice
// (original_imagej/ij/plugin/filter/PlugInFilterRunner.java 181-199). A
// complete-stack filter calls Undo.reset() (PlugInFilterRunner.java 118-120),
// so no slice keeps an undo snapshot.
// ---------------------------------------------------------------------------

fn oracle_stack_filter(
    bridge: &JavaOracleBridge,
    doc: &ImageDocument,
    prelude: &str,
    macro_command: &str,
) -> ImageDocument {
    let out = bridge
        .run_macro_on_image(doc, &format!("{prelude} {macro_command}"))
        .unwrap();
    out.output_image.expect("oracle must return saved TIFF")
}

/// Runs the whole-stack filter in Java and Rust and requires every slice of the
/// three-slice stack to match exactly.
fn assert_whole_stack_filter_matches_oracle(
    mut doc: ImageDocument,
    prelude: &str,
    macro_command: &str,
    apply: impl FnOnce(&mut ImageDocument),
) {
    assert_eq!(doc.slices.len(), 3, "these cases use three-slice stacks");
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let oracle = oracle_stack_filter(&bridge, &doc, prelude, macro_command);
    let before = doc.slices.clone();

    apply(&mut doc);

    assert_eq!(doc.slices.len(), oracle.slices.len());
    assert_eq!(doc.slices, oracle.slices);
    assert!(
        doc.slices.iter().zip(before.iter()).any(|(a, b)| a != b),
        "the whole-stack filter must change at least one slice"
    );
}

fn stack_filter_with_rect_roi(mut doc: ImageDocument) -> ImageDocument {
    doc.set_rect_roi(1, 1, 4, 2).unwrap();
    doc
}

#[test]
fn parity_8bit_smooth_stack_with_and_without_roi_matches_java_oracle() {
    assert_whole_stack_filter_matches_oracle(
        distinct_stack_8bit(6, 4, 3),
        "",
        "run(\"Smooth\", \"stack\");",
        |doc| doc.smooth_stack().unwrap(),
    );
    assert_whole_stack_filter_matches_oracle(
        stack_filter_with_rect_roi(distinct_stack_8bit(6, 4, 3)),
        "makeRectangle(1,1,4,2);",
        "run(\"Smooth\", \"stack\");",
        |doc| doc.smooth_stack().unwrap(),
    );
}

#[test]
fn parity_8bit_sharpen_stack_with_and_without_roi_matches_java_oracle() {
    assert_whole_stack_filter_matches_oracle(
        distinct_stack_8bit(6, 4, 3),
        "",
        "run(\"Sharpen\", \"stack\");",
        |doc| doc.sharpen_stack().unwrap(),
    );
    assert_whole_stack_filter_matches_oracle(
        stack_filter_with_rect_roi(distinct_stack_8bit(6, 4, 3)),
        "makeRectangle(1,1,4,2);",
        "run(\"Sharpen\", \"stack\");",
        |doc| doc.sharpen_stack().unwrap(),
    );
}

#[test]
fn parity_8bit_find_edges_stack_with_and_without_roi_matches_java_oracle() {
    assert_whole_stack_filter_matches_oracle(
        distinct_stack_8bit(6, 4, 3),
        "",
        "run(\"Find Edges\", \"stack\");",
        |doc| doc.find_edges_stack().unwrap(),
    );
    assert_whole_stack_filter_matches_oracle(
        stack_filter_with_rect_roi(distinct_stack_8bit(6, 4, 3)),
        "makeRectangle(1,1,4,2);",
        "run(\"Find Edges\", \"stack\");",
        |doc| doc.find_edges_stack().unwrap(),
    );
}

#[test]
fn parity_8bit_gaussian_blur_stack_with_and_without_roi_matches_java_oracle() {
    assert_whole_stack_filter_matches_oracle(
        distinct_stack_8bit(6, 4, 3),
        "",
        "run(\"Gaussian Blur...\", \"sigma=1.5 stack\");",
        |doc| doc.gaussian_blur_stack(1.5).unwrap(),
    );
    assert_whole_stack_filter_matches_oracle(
        stack_filter_with_rect_roi(distinct_stack_8bit(6, 4, 3)),
        "makeRectangle(1,1,4,2);",
        "run(\"Gaussian Blur...\", \"sigma=1.5 stack\");",
        |doc| doc.gaussian_blur_stack(1.5).unwrap(),
    );
}

#[test]
fn parity_8bit_median_stack_with_and_without_roi_matches_java_oracle() {
    assert_whole_stack_filter_matches_oracle(
        distinct_stack_8bit(6, 4, 3),
        "",
        "run(\"Median...\", \"stack radius=2\");",
        |doc| doc.median_stack(2.0).unwrap(),
    );
    assert_whole_stack_filter_matches_oracle(
        stack_filter_with_rect_roi(distinct_stack_8bit(6, 4, 3)),
        "makeRectangle(1,1,4,2);",
        "run(\"Median...\", \"stack radius=2\");",
        |doc| doc.median_stack(2.0).unwrap(),
    );
}

#[test]
fn parity_8bit_minimum_stack_with_and_without_roi_matches_java_oracle() {
    assert_whole_stack_filter_matches_oracle(
        distinct_stack_8bit(6, 4, 3),
        "",
        "run(\"Minimum...\", \"stack radius=2\");",
        |doc| doc.minimum_stack(2.0).unwrap(),
    );
    assert_whole_stack_filter_matches_oracle(
        stack_filter_with_rect_roi(distinct_stack_8bit(6, 4, 3)),
        "makeRectangle(1,1,4,2);",
        "run(\"Minimum...\", \"stack radius=2\");",
        |doc| doc.minimum_stack(2.0).unwrap(),
    );
}

#[test]
fn parity_8bit_maximum_stack_with_and_without_roi_matches_java_oracle() {
    assert_whole_stack_filter_matches_oracle(
        distinct_stack_8bit(6, 4, 3),
        "",
        "run(\"Maximum...\", \"stack radius=2\");",
        |doc| doc.maximum_stack(2.0).unwrap(),
    );
    assert_whole_stack_filter_matches_oracle(
        stack_filter_with_rect_roi(distinct_stack_8bit(6, 4, 3)),
        "makeRectangle(1,1,4,2);",
        "run(\"Maximum...\", \"stack radius=2\");",
        |doc| doc.maximum_stack(2.0).unwrap(),
    );
}

#[test]
fn parity_8bit_despeckle_stack_with_and_without_roi_matches_java_oracle() {
    assert_whole_stack_filter_matches_oracle(
        distinct_stack_8bit(6, 4, 3),
        "",
        "run(\"Despeckle\", \"stack\");",
        |doc| doc.despeckle_stack().unwrap(),
    );
    assert_whole_stack_filter_matches_oracle(
        stack_filter_with_rect_roi(distinct_stack_8bit(6, 4, 3)),
        "makeRectangle(1,1,4,2);",
        "run(\"Despeckle\", \"stack\");",
        |doc| doc.despeckle_stack().unwrap(),
    );
}

#[test]
fn parity_16bit_stack_filters_match_java_oracle() {
    assert_whole_stack_filter_matches_oracle(
        distinct_stack_16bit(6, 4, 3),
        "",
        "run(\"Smooth\", \"stack\");",
        |doc| doc.smooth_stack().unwrap(),
    );
    assert_whole_stack_filter_matches_oracle(
        stack_filter_with_rect_roi(distinct_stack_16bit(6, 4, 3)),
        "makeRectangle(1,1,4,2);",
        "run(\"Gaussian Blur...\", \"sigma=1.5 stack\");",
        |doc| doc.gaussian_blur_stack(1.5).unwrap(),
    );
    assert_whole_stack_filter_matches_oracle(
        stack_filter_with_rect_roi(distinct_stack_16bit(6, 4, 3)),
        "makeRectangle(1,1,4,2);",
        "run(\"Median...\", \"stack radius=2\");",
        |doc| doc.median_stack(2.0).unwrap(),
    );
}

#[test]
fn parity_32bit_stack_filters_match_java_oracle() {
    let mut slices = Vec::new();
    for s in 0..3u32 {
        let mut pixels = Vec::new();
        for y in 0..4u32 {
            for x in 0..6u32 {
                let i = (y * 6 + x) as f32;
                pixels.push(i * 0.17 + 0.003 * x as f32 + 0.9 * s as f32 - 1.1);
            }
        }
        slices.push(PixelPlane::Gray32(pixels));
    }
    let doc = ImageDocument::new_stack("stack32.tif", 6, 4, slices).unwrap();

    assert_whole_stack_filter_matches_oracle(
        doc.clone(),
        "",
        "run(\"Sharpen\", \"stack\");",
        |doc| doc.sharpen_stack().unwrap(),
    );
    assert_whole_stack_filter_matches_oracle(
        stack_filter_with_rect_roi(doc),
        "makeRectangle(1,1,4,2);",
        "run(\"Find Edges\", \"stack\");",
        |doc| doc.find_edges_stack().unwrap(),
    );
}

#[test]
fn session_whole_stack_filters_record_stack_option_and_clear_undo() {
    let cases: Vec<(Command, &str)> = vec![
        (Command::SmoothStack, "run(\"Smooth\", \"stack\");"),
        (Command::SharpenStack, "run(\"Sharpen\", \"stack\");"),
        (Command::FindEdgesStack, "run(\"Find Edges\", \"stack\");"),
        (
            Command::GaussianBlurStack { sigma: 1.5 },
            "run(\"Gaussian Blur...\", \"sigma=1.5 stack\");",
        ),
        (
            Command::MedianStack { radius: 2.0 },
            "run(\"Median...\", \"stack radius=2\");",
        ),
        (
            Command::MinimumStack { radius: 2.0 },
            "run(\"Minimum...\", \"stack radius=2\");",
        ),
        (
            Command::MaximumStack { radius: 2.0 },
            "run(\"Maximum...\", \"stack radius=2\");",
        ),
        (Command::DespeckleStack, "run(\"Despeckle\", \"stack\");"),
    ];

    for (index, (command, expected)) in cases.into_iter().enumerate() {
        let path = temp_tiff(&format!("m7_stack_command_{index}.tif"));
        save_tiff(&path, &distinct_stack_8bit(6, 4, 3)).unwrap();
        let mut session = Session::new();
        session.dispatch(Command::OpenTiff { path }).unwrap();
        session.dispatch(Command::SetSlice { index: 2 }).unwrap();
        let before = session.active_doc().unwrap().slices.clone();
        let id = session.active_image_id().unwrap();

        session.dispatch(command).unwrap();

        let doc = session.active_doc().unwrap();
        assert_eq!(doc.current_slice, 2, "the selected slice must be preserved");
        assert_eq!(doc.slices.len(), 3);
        for (slice_index, (after, original)) in doc.slices.iter().zip(before.iter()).enumerate() {
            assert_ne!(
                after, original,
                "{expected}: slice {} must be filtered",
                slice_index + 1
            );
        }
        assert_eq!(
            session.macro_recorder.last().map(String::as_str),
            Some(expected),
            "one recorder run with the stack option"
        );
        assert!(session.is_dirty(id));
        // PlugInFilterRunner calls Undo.reset() for a complete stack.
        assert_eq!(
            session.dispatch(Command::Undo).unwrap(),
            CommandOutcome::NothingToUndo
        );
    }
}

#[test]
fn single_plane_stack_filter_keeps_undo_like_java_oracle() {
    // IJ.setupDialog only prompts (and only resets Undo) when the stack has
    // more than one slice, so a one-plane document stays undoable.
    let mut doc = smooth_pattern_8bit(6, 4);
    let before = doc.slices.clone();
    doc.smooth_stack().unwrap();
    assert_ne!(doc.slices, before);
    assert!(doc.undo());
    assert_eq!(doc.slices, before);
}

#[test]
fn whole_stack_filters_reject_rgb_and_indexed_stacks_without_touching_pixels() {
    let rgb = ImageDocument::new_stack(
        "rgb_stack.tif",
        4,
        4,
        vec![
            PixelPlane::ColorRgb(vec![0xff80_4020; 16]),
            PixelPlane::ColorRgb(vec![0xff11_2233; 16]),
        ],
    )
    .unwrap();
    let before = rgb.slices.clone();
    let error = rgb.clone().smooth_stack().unwrap_err();
    assert!(
        error.to_string().contains("RGB") && error.to_string().contains("Smooth"),
        "expected an explicit unsupported-type error, got: {error}"
    );
    assert_eq!(rgb.slices, before);

    let mut indexed = ImageDocument::new_stack(
        "indexed_stack.tif",
        4,
        4,
        vec![
            PixelPlane::Color256 {
                pixels: vec![3u8; 16],
                lut: Box::new(Lut256::default()),
            },
            PixelPlane::Color256 {
                pixels: vec![7u8; 16],
                lut: Box::new(Lut256::default()),
            },
        ],
    )
    .unwrap();
    let before = indexed.slices.clone();
    let error = indexed.clone().median_stack(2.0).unwrap_err();
    assert!(
        error.to_string().contains("8-bit color") && error.to_string().contains("Median"),
        "expected an explicit unsupported-type error, got: {error}"
    );
    assert_eq!(indexed.slices, before);
    // A single unsupported plane must also stop the filter before any slice is
    // modified.
    indexed.slices[1] = PixelPlane::Gray8(vec![9u8; 16]);
    let before = indexed.slices.clone();
    assert!(indexed.minimum_stack(1.0).is_err());
    assert_eq!(indexed.slices, before);
}

#[test]
fn session_select_all_and_restore_selection_geometry_and_lifecycle() {
    let mut session = Session::new();
    let doc = ImageDocument::new_ramp_8bit("doc1", 100, 80).unwrap();
    let id1 = session.insert_doc(doc);

    // Select All on empty ROI creates (0, 0, width, height)
    session.dispatch(Command::SelectAll).unwrap();
    assert_eq!(
        session.get_image(id1).unwrap().roi.as_ref().unwrap().bounds(),
        (0, 0, 100, 80)
    );
    assert_eq!(session.macro_recorder.last().unwrap(), "run(\"Select All\");");

    // Make an oval ROI inside doc1
    session
        .dispatch(Command::MakeOvalRoi {
            x: 10,
            y: 20,
            width: 30,
            height: 40,
        })
        .unwrap();
    assert_eq!(
        session.get_image(id1).unwrap().roi.as_ref().unwrap().bounds(),
        (10, 20, 30, 40)
    );

    // Select None saves previous_roi
    session.dispatch(Command::ClearRoi).unwrap();
    assert!(session.get_image(id1).unwrap().roi.is_none());
    assert_eq!(
        session.previous_roi.as_ref().unwrap().bounds(),
        (10, 20, 30, 40)
    );
    assert_eq!(session.macro_recorder.last().unwrap(), "run(\"Select None\");");

    // Restore Selection restores oval
    session.dispatch(Command::RestoreSelection).unwrap();
    assert!(matches!(
        session.get_image(id1).unwrap().roi.as_ref().unwrap(),
        Roi::Oval(_)
    ));
    assert_eq!(
        session.get_image(id1).unwrap().roi.as_ref().unwrap().bounds(),
        (10, 20, 30, 40)
    );
    assert_eq!(
        session.macro_recorder.last().unwrap(),
        "run(\"Restore Selection\");"
    );

    // Select All saves existing oval to previous_roi and sets full-image rect
    session.dispatch(Command::SelectAll).unwrap();
    assert_eq!(
        session.previous_roi.as_ref().unwrap().bounds(),
        (10, 20, 30, 40)
    );
    assert_eq!(
        session.get_image(id1).unwrap().roi.as_ref().unwrap().bounds(),
        (0, 0, 100, 80)
    );

    // Restore Selection restores the oval again
    session.dispatch(Command::RestoreSelection).unwrap();
    assert_eq!(
        session.get_image(id1).unwrap().roi.as_ref().unwrap().bounds(),
        (10, 20, 30, 40)
    );

    // Test Polygon ROI
    let pts = vec![(15, 25), (45, 25), (45, 55), (15, 55)];
    session
        .dispatch(Command::MakePolygonRoi { points: pts.clone() })
        .unwrap();
    session.dispatch(Command::ClearRoi).unwrap();
    assert_eq!(
        session.previous_roi.as_ref().unwrap().bounds(),
        (15, 25, 30, 30)
    );

    // Open a second, smaller image (50x50)
    let doc2 = ImageDocument::new_ramp_8bit("doc2", 50, 50).unwrap();
    let id2 = session.insert_doc(doc2);
    // On 50x50, polygon with bounds (15, 25, 30, 30) has r.x < 50 && r.y < 50, not out-of-bounds
    session.dispatch(Command::RestoreSelection).unwrap();
    assert_eq!(
        session.get_image(id2).unwrap().roi.as_ref().unwrap().bounds(),
        (15, 25, 30, 30)
    );

    // Now test out-of-bounds relocation:
    // Place ROI with r.x = 60, r.y = 60 on doc1 (bounds 100x80)
    session.set_active_image(id1).unwrap();
    session
        .dispatch(Command::MakeRectRoi {
            x: 60,
            y: 60,
            width: 20,
            height: 20,
        })
        .unwrap();
    session.dispatch(Command::ClearRoi).unwrap(); // previous_roi is at (60, 60, 20, 20)

    // Switch to doc2 (50x50): r.x (60) >= w (50) and r.y (60) >= h (50)
    session.set_active_image(id2).unwrap();
    session.dispatch(Command::RestoreSelection).unwrap();
    // Relocated to ((w - rw)/2, (h - rh)/2) = ((50 - 20)/2, (50 - 20)/2) = (15, 15)
    assert_eq!(
        session.get_image(id2).unwrap().roi.as_ref().unwrap().bounds(),
        (15, 15, 20, 20)
    );

    // Test same-size relocation: ROI of size (50, 50) restored on (50, 50) goes to (0, 0)
    session.dispatch(Command::SelectAll).unwrap();
    session.dispatch(Command::ClearRoi).unwrap();
    // Now move doc2's roi to (10, 10, 30, 30)
    session
        .dispatch(Command::MakeRectRoi {
            x: 10,
            y: 10,
            width: 30,
            height: 30,
        })
        .unwrap();
    // Restore the 50x50 previous_roi -> fits and rw == w && rh == h -> (0, 0)
    session.dispatch(Command::RestoreSelection).unwrap();
    assert_eq!(
        session.get_image(id2).unwrap().roi.as_ref().unwrap().bounds(),
        (0, 0, 50, 50)
    );
}

#[test]
fn parity_select_all_and_restore_selection() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();

    // 1. Rect ROI
    let rect_setup = "newImage('test', '8-bit ramp', 100, 100, 1);\
                      makeRectangle(10, 20, 30, 40);\
                      run('Select None');\
                      run('Restore Selection');\
                      run('Measure');";
    let rect_expected = oracle_saved_results_tsv(rect_setup, "restore_rect");
    let mut session = Session::new();
    let doc = ImageDocument::new_ramp_8bit("test", 100, 100).unwrap();
    session.insert_doc(doc);
    session
        .dispatch(Command::MakeRectRoi {
            x: 10,
            y: 20,
            width: 30,
            height: 40,
        })
        .unwrap();
    session.dispatch(Command::ClearRoi).unwrap();
    session.dispatch(Command::RestoreSelection).unwrap();
    session.dispatch(Command::Measure).unwrap();
    assert_eq!(session.results.to_tsv().trim_end(), rect_expected.trim_end());

    // 2. Oval ROI
    let oval_setup = "newImage('test', '8-bit ramp', 100, 100, 1);\
                      makeOval(15, 25, 30, 40);\
                      run('Select None');\
                      run('Restore Selection');\
                      run('Measure');";
    let oval_expected = oracle_saved_results_tsv(oval_setup, "restore_oval");
    let mut session = Session::new();
    let doc = ImageDocument::new_ramp_8bit("test", 100, 100).unwrap();
    session.insert_doc(doc);
    session
        .dispatch(Command::MakeOvalRoi {
            x: 15,
            y: 25,
            width: 30,
            height: 40,
        })
        .unwrap();
    session.dispatch(Command::ClearRoi).unwrap();
    session.dispatch(Command::RestoreSelection).unwrap();
    session.dispatch(Command::Measure).unwrap();
    assert_eq!(session.results.to_tsv().trim_end(), oval_expected.trim_end());

    // 3. Polygon ROI
    let poly_setup = "newImage('test', '8-bit ramp', 100, 100, 1);\
                      makePolygon(10, 10, 40, 15, 35, 50, 15, 45);\
                      run('Select None');\
                      run('Restore Selection');\
                      run('Measure');";
    let poly_expected = oracle_saved_results_tsv(poly_setup, "restore_poly");
    let mut session = Session::new();
    let doc = ImageDocument::new_ramp_8bit("test", 100, 100).unwrap();
    session.insert_doc(doc);
    session
        .dispatch(Command::MakePolygonRoi {
            points: vec![(10, 10), (40, 15), (35, 50), (15, 45)],
        })
        .unwrap();
    session.dispatch(Command::ClearRoi).unwrap();
    session.dispatch(Command::RestoreSelection).unwrap();
    session.dispatch(Command::Measure).unwrap();
    assert_eq!(session.results.to_tsv().trim_end(), poly_expected.trim_end());

    // 4. Center relocation on different-sized image
    let center_setup = "newImage('big', '8-bit ramp', 100, 100, 1);\
                        makeRectangle(80, 80, 30, 30);\
                        run('Select None');\
                        newImage('small', '8-bit ramp', 50, 50, 1);\
                        run('Restore Selection');\
                        getSelectionBounds(x, y, w, h);\
                        print('CENTERED:' + x + ',' + y + ',' + w + ',' + h);";
    let output = bridge.eval_macro(center_setup).unwrap();
    let oracle_line = output
        .stdout
        .lines()
        .find(|l| l.starts_with("CENTERED:"))
        .expect("oracle must print CENTERED bounds");
    let mut session = Session::new();
    session
        .dispatch(Command::NewRamp8Bit {
            title: "big".into(),
            width: 100,
            height: 100,
        })
        .unwrap();
    session
        .dispatch(Command::MakeRectRoi {
            x: 80,
            y: 80,
            width: 30,
            height: 30,
        })
        .unwrap();
    session.dispatch(Command::ClearRoi).unwrap();
    session
        .dispatch(Command::NewRamp8Bit {
            title: "small".into(),
            width: 50,
            height: 50,
        })
        .unwrap();
    session.dispatch(Command::RestoreSelection).unwrap();
    let (x, y, w, h) = session.active_doc().unwrap().roi.as_ref().unwrap().bounds();
    assert_eq!(oracle_line, format!("CENTERED:{x},{y},{w},{h}"));
}

#[test]
fn parity_png_gray8_gray16_and_rgb_roundtrip_with_java() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let cases = [
        ("gray8", PixelPlane::Gray8(vec![0, 17, 128, 255, 91, 200]), 8),
        (
            "gray16",
            PixelPlane::Gray16(vec![0, 257, 32768, 65535, 1000, 54321]),
            16,
        ),
        (
            "rgb",
            PixelPlane::ColorRgb(vec![
                0xff000000, 0xff123456, 0xffabcdef, 0xffffffff, 0xff00ff00, 0xffff0000,
            ]),
            24,
        ),
    ];
    for (name, plane, bit_depth) in cases {
        let doc = ImageDocument::new(name, 3, 2, plane.clone()).unwrap();
        let path = dir.path().join(format!("{name}.png"));
        save_png(&path, &doc).unwrap();
        let reloaded = load_image(&path).unwrap();
        assert_eq!((reloaded.width, reloaded.height), (3, 2));
        assert_eq!(reloaded.plane(), &plane, "Rust PNG roundtrip {name}");

        let macro_text = format!(
            "open(\"{}\"); print(\"TYPE:\"+bitDepth()); \
             for(y=0;y<getHeight();y++) for(x=0;x<getWidth();x++) print(\"PX:\"+getPixel(x,y));",
            path.display()
        );
        let output = bridge.eval_macro(&macro_text).unwrap();
        assert_eq!(oracle_line(&output.stdout, "TYPE:"), format!("TYPE:{bit_depth}"));
        let java_pixels: Vec<u32> = output
            .stdout
            .lines()
            .filter_map(|line| line.strip_prefix("PX:"))
            .map(|value| {
                let pixel = value.parse::<i64>().unwrap() as u32;
                if bit_depth == 24 { pixel & 0xffffff } else { pixel }
            })
            .collect();
        let expected: Vec<u32> = match plane {
            PixelPlane::Gray8(values) => values.into_iter().map(u32::from).collect(),
            PixelPlane::Gray16(values) => values.into_iter().map(u32::from).collect(),
            PixelPlane::ColorRgb(values) => values.into_iter().map(|v| v & 0xffffff).collect(),
            _ => unreachable!(),
        };
        assert_eq!(java_pixels, expected, "Java PNG decode {name}");
    }
}

#[test]
fn parity_jpeg_dimensions_and_lossy_pixels_with_java() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut source = Vec::new();
    for y in 0..16u32 {
        for x in 0..16u32 {
            let r = x * 13 + y * 2;
            let g = y * 13 + x * 2;
            let b = (x + y) * 7;
            source.push(0xff000000 | (r << 16) | (g << 8) | b);
        }
    }
    let doc = ImageDocument::new("jpeg", 16, 16, PixelPlane::ColorRgb(source.clone())).unwrap();
    let path = dir.path().join("rust.jpg");
    save_jpeg(&path, &doc, 90).unwrap();
    let reloaded = load_image(&path).unwrap();
    assert_eq!((reloaded.width, reloaded.height), (16, 16));
    let PixelPlane::ColorRgb(actual) = reloaded.plane() else {
        panic!("RGB JPEG must decode as RGB");
    };
    let squared_error: f64 = source
        .iter()
        .zip(actual)
        .flat_map(|(&a, b)| {
            [16, 8, 0].map(move |shift| {
                let delta = ((a >> shift) & 255) as f64 - ((b >> shift) & 255) as f64;
                delta * delta
            })
        })
        .sum();
    let mse = squared_error / (source.len() * 3) as f64;
    assert!(mse < 100.0, "JPEG MSE {mse}");
    let output = bridge
        .eval_macro(&format!(
            "open(\"{}\"); print(\"JPEG:\"+getWidth()+\",\"+getHeight()+\",\"+bitDepth());",
            path.display()
        ))
        .unwrap();
    assert_eq!(oracle_line(&output.stdout, "JPEG:"), "JPEG:16,16,24");

    let java_path = dir.path().join("java.jpg");
    bridge
        .eval_macro(&format!(
            "newImage(\"gray\", \"8-bit ramp\", 16, 16, 1); saveAs(\"Jpeg\", \"{}\");",
            java_path.display()
        ))
        .unwrap();
    let java_jpeg = load_image(&java_path).unwrap();
    assert_eq!((java_jpeg.width, java_jpeg.height), (16, 16));
    let PixelPlane::Gray8(decoded) = java_jpeg.plane() else {
        panic!("Java grayscale JPEG must decode as Gray8");
    };
    let PixelPlane::Gray8(ramp) = ImageDocument::new_ramp_8bit("gray", 16, 16)
        .unwrap()
        .plane()
        .clone()
    else {
        unreachable!()
    };
    let mse = ramp
        .iter()
        .zip(decoded)
        .map(|(&a, &b)| (f64::from(a) - f64::from(b)).powi(2))
        .sum::<f64>()
        / ramp.len() as f64;
    assert!(mse < 100.0, "Java JPEG MSE {mse}");
}

#[test]
fn png_and_jpeg_failed_save_keep_previous_file() {
    let dir = tempfile::tempdir().unwrap();
    let mut doc = ImageDocument::new("bad", 2, 2, PixelPlane::Gray8(vec![1; 4])).unwrap();
    doc.slices[0] = PixelPlane::Gray8(vec![1]);
    for extension in ["png", "jpg"] {
        let path = dir.path().join(format!("previous.{extension}"));
        std::fs::write(&path, b"previous contents").unwrap();
        let saved = if extension == "png" {
            save_png(&path, &doc)
        } else {
            save_jpeg(&path, &doc, 75)
        };
        assert!(saved.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"previous contents");
    }
}

#[test]
fn parity_java_png_files_decode_to_matching_rust_planes() {
    let bridge = JavaOracleBridge::locate_from_repo_root(repo_root()).unwrap();
    let dir = tempfile::tempdir().unwrap();
    for (name, kind, pixels, expected) in [
        (
            "gray8",
            "8-bit black",
            "setPixel(0,0,17); setPixel(1,0,128); setPixel(2,0,255);",
            PixelPlane::Gray8(vec![17, 128, 255]),
        ),
        (
            "gray16",
            "16-bit black",
            "setPixel(0,0,257); setPixel(1,0,32768); setPixel(2,0,65535);",
            PixelPlane::Gray16(vec![257, 32768, 65535]),
        ),
        (
            "rgb",
            "RGB black",
            "setPixel(0,0,1193046); setPixel(1,0,11259375); setPixel(2,0,16711680);",
            PixelPlane::ColorRgb(vec![0xff123456, 0xffabcdef, 0xffff0000]),
        ),
    ] {
        let path = dir.path().join(format!("java_{name}.png"));
        bridge
            .eval_macro(&format!(
                "newImage(\"{name}\", \"{kind}\", 3, 1, 1); {pixels} saveAs(\"PNG\", \"{}\");",
                path.display()
            ))
            .unwrap();
        let doc = load_image(&path).unwrap();
        assert_eq!((doc.width, doc.height), (3, 1));
        assert_eq!(doc.plane(), &expected, "Java PNG {name}");
    }
}

#[test]
fn session_image_format_commands_use_magic_and_preserve_revert() {
    let dir = tempfile::tempdir().unwrap();
    let source = ImageDocument::new("source", 2, 2, PixelPlane::Gray8(vec![1, 2, 3, 4])).unwrap();
    let misleading_path = dir.path().join("source.jpg");
    save_png(&misleading_path, &source).unwrap();
    let mut session = Session::new();
    session
        .dispatch(Command::OpenImage {
            path: misleading_path.clone(),
        })
        .unwrap();
    assert_eq!(session.active_doc().unwrap().plane(), source.plane());
    let id = session.active_image_id().unwrap();
    session.save_image(id, &misleading_path).unwrap();
    assert!(std::fs::read(&misleading_path)
        .unwrap()
        .starts_with(b"\x89PNG\r\n\x1a\n"));

    let extensionless_path = dir.path().join("extensionless");
    session
        .dispatch(Command::SavePng {
            path: extensionless_path.clone(),
        })
        .unwrap();
    session.save_image(id, &extensionless_path).unwrap();
    assert!(std::fs::read(&extensionless_path)
        .unwrap()
        .starts_with(b"\x89PNG\r\n\x1a\n"));

    let png_path = dir.path().join("saved.png");
    session
        .dispatch(Command::SavePng {
            path: png_path.clone(),
        })
        .unwrap();
    assert!(std::fs::read(&png_path)
        .unwrap()
        .starts_with(b"\x89PNG\r\n\x1a\n"));
    assert_eq!(session.image_path(id), Some(png_path.as_path()));
    session.dispatch(Command::Invert).unwrap();
    assert!(session.is_dirty(id));
    session.dispatch(Command::Revert).unwrap();
    assert_eq!(session.active_doc().unwrap().plane(), source.plane());
    assert!(!session.is_dirty(id));

    let jpeg_path = dir.path().join("saved.jpeg");
    session
        .dispatch(Command::SaveJpeg {
            path: jpeg_path.clone(),
        })
        .unwrap();
    assert!(std::fs::read(&jpeg_path).unwrap().starts_with(&[0xff, 0xd8, 0xff]));
    assert_eq!(session.image_path(id), Some(jpeg_path.as_path()));
}
