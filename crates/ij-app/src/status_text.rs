use ij_core::{ImageDocument, PixelPlane};

/// Formats the status bar string for the pixel at `(x, y)` to match ImageJ 1.54u.
///
/// Corresponds to `ImagePlus.getLocationAsString` (lines 2823-2842) concatenated
/// with `ImagePlus.getValueAsString` (lines 2848-2880).
pub fn pixel_status(doc: &ImageDocument, x: i32, y: i32) -> Option<String> {
    if x < 0 || y < 0 || x >= doc.width as i32 || y >= doc.height as i32 {
        return None;
    }

    let cal = &doc.calibration;
    // Calibration.scaled (original_imagej/ij/measure/Calibration.java line 92):
    // pixelWidth!=1.0 || pixelHeight!=1.0 || pixelDepth!=1.0 || !unit.equals("pixel")
    let scaled = cal.pixel_width != 1.0
        || cal.pixel_height != 1.0
        || cal.pixel_depth != 1.0
        || cal.unit != "pixel";

    let xx = if scaled {
        format!(" ({x})")
    } else {
        String::new()
    };
    let yy = if scaled {
        format!(" ({y})")
    } else {
        String::new()
    };

    let cal_x = x as f64 * cal.pixel_width;
    let cal_y = if cal.invert_y {
        (doc.height as f64 - y as f64 - 1.0) * cal.pixel_height
    } else {
        y as f64 * cal.pixel_height
    };

    let mut s = format!(" x={}{xx}, y={}{yy}", d2s_coord(cal_x), d2s_coord(cal_y));

    // Stack z coordinate (ImagePlus.java lines 2833-2840)
    if doc.slices.len() > 1 {
        let z = if doc.is_hyperstack() {
            doc.get_slice().saturating_sub(1) as i32
        } else {
            doc.current_slice.saturating_sub(1) as i32
        };
        let cal_z = z as f64 * cal.pixel_depth;
        let zz = if scaled && (cal_z != z as f64) {
            format!(" ({z})")
        } else {
            String::new()
        };
        s.push_str(&format!(", z={}{zz}", d2s_coord(cal_z)));
    }

    // Pixel value string (ImagePlus.java lines 2848-2880)
    let plane = doc.plane();
    let idx = (y as usize) * (doc.width as usize) + (x as usize);

    match plane {
        PixelPlane::Gray8(pixels) => {
            // Rust's Calibration currently has no density calibration function or origin,
            // so cValue == v always holds (ImagePlus.java lines 2855-2866).
            let v = pixels[idx];
            s.push_str(&format!(", value={v}"));
        }
        PixelPlane::Gray16(pixels) => {
            // Density calibration uncalibrated path (ImagePlus.java lines 2855-2866)
            let v = pixels[idx];
            s.push_str(&format!(", value={v}"));
        }
        PixelPlane::Color256 { pixels, lut } => {
            // Color256 uncalibrated path (ImagePlus.java lines 2856-2861):
            // Returns index and LUT-mapped R, G, B.
            let index = pixels[idx] as usize;
            let r = lut.r.get(index).copied().unwrap_or(0);
            let g = lut.g.get(index).copied().unwrap_or(0);
            let b = lut.b.get(index).copied().unwrap_or(0);
            s.push_str(&format!(", index={index}, value={r},{g},{b}"));
        }
        PixelPlane::Gray32(pixels) => {
            // ImagePlus.java lines 2868-2870:
            // String s = (int)value==value?IJ.d2s(value,0)+".0":IJ.d2s(value,4,7);
            let val = pixels[idx] as f64;
            let val_str = if val.is_nan() {
                "NaN".to_string()
            } else if val.is_infinite() {
                if val > 0.0 {
                    "Infinity".to_string()
                } else {
                    "-Infinity".to_string()
                }
            } else if val == 0.0 && val.is_sign_negative() {
                "-0.0".to_string()
            } else if (val as i32 as f64) == val {
                format!("{}.0", val as i32)
            } else {
                ij_d2s_sig(val, 4, 7)
            };
            s.push_str(&format!(", value={val_str}"));
        }
        PixelPlane::ColorRgb(pixels) => {
            // ImagePlus.java lines 2871-2877:
            // String hex = Colors.colorToString(new Color(v[0],v[1],v[2]));
            // return(", value=" + IJ.pad(v[0],3) + "," + IJ.pad(v[1],3) + "," + IJ.pad(v[2],3) + " ("+hex + ")");
            let c = pixels[idx];
            let r = ((c >> 16) & 0xff) as u8;
            let g = ((c >> 8) & 0xff) as u8;
            let b = (c & 0xff) as u8;
            let hex = color_to_string(r, g, b);
            s.push_str(&format!(", value={r:03},{g:03},{b:03} ({hex})"));
        }
    }

    Some(s)
}

/// ImagePlus.d2s helper for coordinates (ImagePlus.java line 2844-2846):
/// `n==(int)n?Integer.toString((int)n):IJ.d2s(n)`
fn d2s_coord(n: f64) -> String {
    if (n as i32 as f64) == n {
        format!("{}", n as i32)
    } else {
        ij_d2s(n, 2)
    }
}

/// Converts an RGB color into a lowercase string or hex representation
/// matching `ij.plugin.Colors.colorToString` (Colors.java lines 88-104 & 208-214).
fn color_to_string(r: u8, g: u8, b: u8) -> String {
    match (r, g, b) {
        (255, 0, 0) => "red".to_string(),
        (0, 255, 0) => "green".to_string(),
        (0, 0, 255) => "blue".to_string(),
        (255, 0, 255) => "magenta".to_string(),
        (0, 255, 255) => "cyan".to_string(),
        (255, 255, 0) => "yellow".to_string(),
        (255, 200, 0) => "orange".to_string(),
        (0, 0, 0) => "black".to_string(),
        (255, 255, 255) => "white".to_string(),
        (128, 128, 128) => "gray".to_string(),
        (192, 192, 192) => "lightgray".to_string(),
        (64, 64, 64) => "darkgray".to_string(),
        (255, 175, 175) => "pink".to_string(),
        _ => format!("#{r:02x}{g:02x}{b:02x}"),
    }
}

/// Formats a float with significant/max digits matching `IJ.d2s(x, sig, max)`
/// (IJ.java lines 1038-1050).
pub fn ij_d2s_sig(x: f64, significant_digits: i32, max_digits: i32) -> String {
    if x.is_nan() {
        return "NaN".to_string();
    }
    if x.is_infinite() {
        return if x > 0.0 {
            "Infinity".to_string()
        } else {
            "-Infinity".to_string()
        };
    }
    if x == f32::MAX as f64 {
        return "3.4e38".to_string();
    }
    let abs_x = x.abs();
    let log10 = abs_x.log10();
    let round_error_at_max = 0.223 * 10f64.powi(-max_digits);
    let magnitude = (log10 + round_error_at_max).ceil() as i32;
    let mut decimals = if x == 0.0 { 0 } else { max_digits - magnitude };
    if decimals < 0 || magnitude < significant_digits + 1 - max_digits {
        ij_d2s(x, -significant_digits)
    } else {
        if decimals > significant_digits {
            decimals = significant_digits.max(decimals - max_digits + significant_digits);
        }
        ij_d2s(x, decimals)
    }
}

/// Formats a number matching `IJ.d2s(n, decimalPlaces)` (IJ.java lines 1000-1029).
pub fn ij_d2s(n: f64, mut decimal_places: i32) -> String {
    if n.is_nan() {
        return "NaN".to_string();
    }
    if n.is_infinite() {
        return if n > 0.0 {
            "Infinity".to_string()
        } else {
            "-Infinity".to_string()
        };
    }
    if n == f32::MAX as f64 {
        return "3.4e38".to_string();
    }
    if decimal_places < 0 {
        decimal_places = (-decimal_places).min(9);
        format!("{:.places$e}", n, places = decimal_places as usize).replace('e', "E")
    } else {
        let places = decimal_places.clamp(0, 9) as usize;
        format!("{:.places$}", n, places = places)
    }
}
