use ij_core::{ImageDocument, PixelPlane};

pub const ZOOM_LEVELS: &[f64] = &[
    0.25, 0.333, 0.5, 0.75, 1.0, 1.5, 2.0, 3.0, 4.0, 6.0, 8.0, 12.0, 16.0,
];

#[derive(Debug, Clone)]
pub struct ImageView {
    pub width: u32,
    pub height: u32,
    pub zoom: f64,
    pub pan: (f32, f32),
    pub is_dragging: bool,
    pub drag_start_pixel: Option<(i32, i32)>,
    pub drag_current_pixel: Option<(i32, i32)>,
    pub polygon_points: Vec<(i32, i32)>,
}

impl ImageView {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            zoom: 1.0,
            pan: (0.0, 0.0),
            is_dragging: false,
            drag_start_pixel: None,
            drag_current_pixel: None,
            polygon_points: Vec::new(),
        }
    }

    pub fn zoom_in(&mut self) {
        for &lvl in ZOOM_LEVELS {
            if lvl > self.zoom + 1e-4 {
                self.zoom = lvl;
                return;
            }
        }
    }

    pub fn zoom_out(&mut self) {
        for &lvl in ZOOM_LEVELS.iter().rev() {
            if lvl < self.zoom - 1e-4 {
                self.zoom = lvl;
                return;
            }
        }
    }

    pub fn set_zoom(&mut self, zoom: f64) {
        self.zoom = zoom.clamp(0.1, 32.0);
    }

    /// Maps coordinates relative to the image canvas origin to integer pixel coordinates.
    pub fn screen_to_pixel(&self, sx: f32, sy: f32) -> (i32, i32) {
        let z = self.zoom as f32;
        let px = ((sx - self.pan.0) / z).floor() as i32;
        let py = ((sy - self.pan.1) / z).floor() as i32;
        (px, py)
    }

    /// Maps integer pixel coordinates to screen coordinates relative to image canvas origin.
    pub fn pixel_to_screen(&self, px: f32, py: f32) -> (f32, f32) {
        let z = self.zoom as f32;
        (px * z + self.pan.0, py * z + self.pan.1)
    }

    /// Creates subtitle string matching original ImageWindow.java lines 309-379.
    pub fn create_subtitle(doc: &ImageDocument) -> String {
        let mut s = String::new();
        let stack_size = doc.stack_size();
        if stack_size > 1 {
            s.push_str(&format!("{}/{}", doc.current_slice, stack_size));
            if let Some(label) = doc
                .slice_labels
                .get((doc.current_slice - 1) as usize)
                .and_then(|label| label.as_deref())
                .and_then(|label| short_slice_label(label, 60))
            {
                if !label.is_empty() {
                    s.push_str(&format!(" ({label})"));
                }
            }
            s.push_str("; ");
        } else if let Some(label) = doc
            .slice_labels
            .first()
            .and_then(|label| label.as_deref())
            .filter(|label| !label.is_empty())
        {
            let first_line = label
                .split_once('\n')
                .and_then(|(first, _)| (!first.is_empty()).then_some(first))
                .unwrap_or(label);
            let chars: Vec<char> = first_line.chars().collect();
            let trimmed = strip_extension(&chars);
            let mut label: String = trimmed.iter().take(60).collect();
            if trimmed.len() > 60 {
                label.push_str("...");
            }
            s.push_str(&format!("\"{label}\"; "));
        }

        let cal = &doc.calibration;
        // Calibration.scaled (original_imagej/ij/measure/Calibration.java line 92)
        let scaled = cal.pixel_width != 1.0
            || cal.pixel_height != 1.0
            || cal.pixel_depth != 1.0
            || cal.unit != "pixel";

        if scaled {
            let cwidth = doc.width as f64 * cal.pixel_width;
            let cheight = doc.height as f64 * cal.pixel_height;
            // Tools.getDecimalPlaces (original_imagej/ij/gui/ImageWindow.java lines 349-350)
            let mut digits = get_decimal_places(cwidth, cheight);
            if digits > 2 {
                digits = 2;
            }
            let units = plural_unit(&cal.unit);
            s.push_str(&format!(
                "{}x{} {} ({}x{}); ",
                d2s(cwidth, digits),
                d2s(cheight, digits),
                units,
                doc.width,
                doc.height
            ));
        } else {
            s.push_str(&format!("{}x{} pixels; ", doc.width, doc.height));
        }

        // ImagePlus type formatting (original_imagej/ij/gui/ImageWindow.java lines 361-375)
        match doc.plane() {
            PixelPlane::Gray8(_) | PixelPlane::Color256 { .. } => s.push_str("8-bit"),
            PixelPlane::Gray16(_) => s.push_str("16-bit"),
            PixelPlane::Gray32(_) => s.push_str("32-bit"),
            PixelPlane::ColorRgb(_) => s.push_str("RGB"),
        }

        // Inverting LUT detection (original_imagej/ij/gui/ImageWindow.java lines 376-377)
        if is_inverted_lut(doc) {
            s.push_str(" (inverting LUT)");
        }

        s.push_str("; ");
        s.push_str(&Self::image_size(doc));

        s
    }

    /// Formats the image size string matching ImageWindow.getImageSize (lines 381-396)
    /// and ImagePlus.getSizeInBytes (lines 1226-1235).
    pub fn image_size(doc: &ImageDocument) -> String {
        let bytes_per_pixel = match doc.plane() {
            PixelPlane::Gray8(_) | PixelPlane::Color256 { .. } => 1.0,
            PixelPlane::Gray16(_) => 2.0,
            PixelPlane::Gray32(_) | PixelPlane::ColorRgb(_) => 4.0,
        };
        let total_bytes =
            (doc.width as f64) * (doc.height as f64) * (doc.stack_size() as f64) * bytes_per_pixel;
        let size = total_bytes / 1024.0;
        let (mut s2, s3) = if size < 1024.0 {
            (d2s(size, 0), "K")
        } else if size < 10000.0 {
            (d2s(size / 1024.0, 1), "MB")
        } else if size < 1048576.0 {
            let rounded = (size / 1024.0).round();
            (d2s(rounded, 0), "MB")
        } else {
            (d2s(size / 1048576.0, 1), "GB")
        };
        if s2.ends_with(".0") {
            s2.truncate(s2.len() - 2);
        }
        format!("{s2}{s3}")
    }
}

fn short_slice_label(label: &str, max: usize) -> Option<String> {
    let first_line = label.split('\n').next()?;
    if label.starts_with('\n') {
        return None;
    }
    let chars: Vec<char> = first_line.chars().collect();
    let trimmed = strip_extension(&chars);
    Some(trimmed.iter().take(max).collect())
}

fn strip_extension(chars: &[char]) -> &[char] {
    if chars.len() > 4 && chars[chars.len() - 4] == '.' && !chars[chars.len() - 1].is_ascii_digit()
    {
        &chars[..chars.len() - 4]
    } else {
        chars
    }
}

/// Formats a number matching IJ.d2s(n, decimalPlaces).
///
/// Java's DecimalFormat("0") rounds 0.5 up to 1 (half-away-from-zero),
/// whereas IEEE 754 float formatting in Rust rounds 0.5 to even (0).
fn d2s(n: f64, decimal_places: i32) -> String {
    if decimal_places == 0 {
        format!("{}", n.round() as i64)
    } else {
        crate::status_text::ij_d2s(n, decimal_places)
    }
}

/// Plural form of spatial calibration unit matching Calibration.pluralForm (lines 159-170).
fn plural_unit(unit: &str) -> &str {
    match unit {
        "pixel" => "pixels",
        "micron" => "microns",
        "inch" => "inches",
        other => other,
    }
}

/// Checks whether the active plane has an inverting LUT matching
/// ImageProcessor.isInvertedLut (original_imagej/ij/process/ImageProcessor.java lines 299-318).
fn is_inverted_lut(doc: &ImageDocument) -> bool {
    match doc.plane() {
        PixelPlane::Color256 { lut, .. } => {
            if lut.r.len() < 256 || lut.g.len() < 256 || lut.b.len() < 256 {
                return false;
            }
            let mut has_ascending_step = false;
            for i in 1..255 {
                let v1 = lut.r[i - 1] as i32 + lut.g[i - 1] as i32 + lut.b[i - 1] as i32;
                let v2 = lut.r[i] as i32 + lut.g[i] as i32 + lut.b[i] as i32;
                if v1 < v2 {
                    has_ascending_step = true;
                    break;
                }
            }
            !has_ascending_step
        }
        _ => false,
    }
}

/// Computes decimal places matching Tools.getDecimalPlaces(double) (lines 209-222).
fn get_decimal_places_single(n: f64) -> i32 {
    if (n as i32 as f64) == n || n.is_nan() {
        return 0;
    }
    let mut s = n.to_string();
    if s.contains('e') || s.contains('E') {
        return -2;
    }
    while s.ends_with('0') {
        s.pop();
    }
    let Some(index) = s.find('.') else {
        return 0;
    };
    let digits = (s.len() - index - 1) as i32;
    if digits > 4 {
        4
    } else {
        digits
    }
}

/// Computes decimal places for two dimensions matching Tools.getDecimalPlaces(double, double)
/// (original_imagej/ij/util/Tools.java lines 226-240).
fn get_decimal_places(n1: f64, n2: f64) -> i32 {
    if (n1 as i32 as f64) == n1 && (n2 as i32 as f64) == n2 {
        return 0;
    }
    let digits = get_decimal_places_single(n1);
    let digits2 = get_decimal_places_single(n2);
    if digits == 0 {
        return digits2;
    }
    if digits2 == 0 {
        return digits;
    }
    if digits < 0 || digits2 < 0 {
        return digits;
    }
    if digits2 > digits {
        return digits2;
    }
    digits
}
