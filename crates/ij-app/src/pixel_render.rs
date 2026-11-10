use ij_core::{DisplayRange, PixelPlane, ThresholdLevels};

/// Converts an ImageJ PixelPlane into an RGBA8 buffer of size (width * height * 4).
/// Respects DisplayRange (brightness & contrast) and ThresholdLevels (red highlight).
pub fn plane_to_rgba(
    plane: &PixelPlane,
    width: u32,
    height: u32,
    range: DisplayRange,
    threshold: Option<ThresholdLevels>,
) -> Vec<u8> {
    let count = (width as usize).saturating_mul(height as usize);
    let mut rgba = vec![255u8; count * 4];
    let span = (range.max - range.min).max(1e-5);

    match plane {
        PixelPlane::Gray8(pixels) => {
            for (i, &p) in pixels.iter().take(count).enumerate() {
                let off = i * 4;
                if let Some(t) = threshold {
                    let val = p as f64;
                    if val >= t.lower && val <= t.upper {
                        // ImageJ classic RED_LUT threshold highlight
                        rgba[off] = 255;
                        rgba[off + 1] = 0;
                        rgba[off + 2] = 0;
                        rgba[off + 3] = 255;
                        continue;
                    }
                }
                let norm = ((p as f64 - range.min) / span * 255.0).clamp(0.0, 255.0) as u8;
                rgba[off] = norm;
                rgba[off + 1] = norm;
                rgba[off + 2] = norm;
                rgba[off + 3] = 255;
            }
        }
        PixelPlane::Color256 { pixels, lut } => {
            let has_lut = lut.r.len() == 256 && lut.g.len() == 256 && lut.b.len() == 256;
            for (i, &p) in pixels.iter().take(count).enumerate() {
                let off = i * 4;
                if let Some(t) = threshold {
                    let val = p as f64;
                    if val >= t.lower && val <= t.upper {
                        rgba[off] = 255;
                        rgba[off + 1] = 0;
                        rgba[off + 2] = 0;
                        rgba[off + 3] = 255;
                        continue;
                    }
                }
                let norm = ((p as f64 - range.min) / span * 255.0).clamp(0.0, 255.0) as usize;
                if has_lut {
                    rgba[off] = lut.r[norm];
                    rgba[off + 1] = lut.g[norm];
                    rgba[off + 2] = lut.b[norm];
                } else {
                    let n = norm as u8;
                    rgba[off] = n;
                    rgba[off + 1] = n;
                    rgba[off + 2] = n;
                }
                rgba[off + 3] = 255;
            }
        }
        PixelPlane::Gray16(pixels) => {
            for (i, &p) in pixels.iter().take(count).enumerate() {
                let off = i * 4;
                if let Some(t) = threshold {
                    let val = p as f64;
                    if val >= t.lower && val <= t.upper {
                        rgba[off] = 255;
                        rgba[off + 1] = 0;
                        rgba[off + 2] = 0;
                        rgba[off + 3] = 255;
                        continue;
                    }
                }
                let norm = ((p as f64 - range.min) / span * 255.0).clamp(0.0, 255.0) as u8;
                rgba[off] = norm;
                rgba[off + 1] = norm;
                rgba[off + 2] = norm;
                rgba[off + 3] = 255;
            }
        }
        PixelPlane::Gray32(pixels) => {
            for (i, &p) in pixels.iter().take(count).enumerate() {
                let off = i * 4;
                if let Some(t) = threshold {
                    let val = p as f64;
                    if val >= t.lower && val <= t.upper {
                        rgba[off] = 255;
                        rgba[off + 1] = 0;
                        rgba[off + 2] = 0;
                        rgba[off + 3] = 255;
                        continue;
                    }
                }
                let norm = ((p as f64 - range.min) / span * 255.0).clamp(0.0, 255.0) as u8;
                rgba[off] = norm;
                rgba[off + 1] = norm;
                rgba[off + 2] = norm;
                rgba[off + 3] = 255;
            }
        }
        PixelPlane::ColorRgb(pixels) => {
            for (i, &p) in pixels.iter().take(count).enumerate() {
                let off = i * 4;
                rgba[off] = ((p >> 16) & 0xff) as u8;
                rgba[off + 1] = ((p >> 8) & 0xff) as u8;
                rgba[off + 2] = (p & 0xff) as u8;
                rgba[off + 3] = 255;
            }
        }
    }

    rgba
}
