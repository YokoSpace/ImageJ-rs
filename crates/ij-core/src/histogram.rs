use crate::{ImageDocument, PixelPlane};

/// A 256-bin display histogram and its min/max intensity range, matching
/// ImageJ 1.54u `ContrastAdjuster.plotHistogram` and `ImageStatistics`.
#[derive(Debug, Clone, PartialEq)]
pub struct DisplayHistogram {
    pub counts: [u64; 256],
    pub hmin: f64,
    pub hmax: f64,
}

/// Computes the 256-bin display histogram of the current slice of `doc`,
/// taking any active area ROI into account.
///
/// Follows ImageJ 1.54u behavior:
/// - 8-bit (Gray8 / Color256): 256 unit-sized bins with `hmin = 0.0, hmax = 255.0`
///   (`ByteStatistics.java` lines 84-85).
/// - 16-bit (Gray16): `hmin` is the minimum pixel value and `hmax` is the maximum
///   pixel value in the ROI/image. Bins are formed with
///   `binSize = (hmax - hmin + 1) / 256` (`ShortStatistics.java` lines 88-95).
/// - 32-bit (Gray32): `hmin` is the minimum finite pixel value and `hmax` is the
///   maximum finite pixel value in the ROI/image. Bins are formed with
///   `scale = 256 / (hmax - hmin)` (`FloatStatistics.java` lines 80-92).
pub fn display_histogram(doc: &ImageDocument) -> DisplayHistogram {
    let width = doc.width;
    let height = doc.height;
    let plane = doc.plane();

    let (rx, ry, rw, rh, mask) = match &doc.roi {
        None => (0, 0, width, height, None),
        Some(roi) if !roi.is_area() => (0, 0, width, height, None),
        Some(roi) => {
            let (rx, ry, rw, rh) = roi.clipped_bounds(width, height).unwrap_or((0, 0, 0, 0));
            let mask = roi.raster_mask(width, height).ok().flatten();
            (rx, ry, rw, rh, mask)
        }
    };

    if rw == 0 || rh == 0 {
        return match plane {
            PixelPlane::Gray16(_) => DisplayHistogram {
                counts: [0; 256],
                hmin: 65535.0,
                hmax: 0.0,
            },
            PixelPlane::Gray32(_) => DisplayHistogram {
                counts: [0; 256],
                hmin: f64::MAX,
                hmax: -f64::MAX,
            },
            _ => DisplayHistogram {
                counts: [0; 256],
                hmin: 0.0,
                hmax: 255.0,
            },
        };
    }

    match plane {
        PixelPlane::Gray8(pixels) | PixelPlane::Color256 { pixels, .. } => {
            let mut counts = [0u64; 256];
            for y in ry..(ry + rh) {
                for x in rx..(rx + rw) {
                    if mask.as_ref().is_some_and(|m| !m.contains(x, y)) {
                        continue;
                    }
                    let idx = (y * width + x) as usize;
                    counts[pixels[idx] as usize] += 1;
                }
            }
            DisplayHistogram {
                counts,
                hmin: 0.0,
                hmax: 255.0,
            }
        }
        PixelPlane::Gray16(pixels) => {
            let mut hist16 = [0u64; 65536];
            let mut min = 65535usize;
            let mut max = 0usize;
            let mut has_pixel = false;
            for y in ry..(ry + rh) {
                for x in rx..(rx + rw) {
                    if mask.as_ref().is_some_and(|m| !m.contains(x, y)) {
                        continue;
                    }
                    let idx = (y * width + x) as usize;
                    let v = pixels[idx] as usize;
                    hist16[v] += 1;
                    if v < min {
                        min = v;
                    }
                    if v > max {
                        max = v;
                    }
                    has_pixel = true;
                }
            }
            if !has_pixel {
                return DisplayHistogram {
                    counts: [0; 256],
                    hmin: 65535.0,
                    hmax: 0.0,
                };
            }
            let hist_min = min as f64;
            let hist_max = max as f64;
            // ShortStatistics.java lines 94-95: binSize = (histMax-histMin+1)/nBins
            let bin_size = (hist_max - hist_min + 1.0) / 256.0;
            let scale = 1.0 / bin_size;
            let mut counts = [0u64; 256];
            for i in min..=max {
                let count = hist16[i];
                if count == 0 {
                    continue;
                }
                let mut index = (scale * (i as f64 - hist_min)) as usize;
                if index >= 256 {
                    index = 255;
                }
                counts[index] += count;
            }
            DisplayHistogram {
                counts,
                hmin: hist_min,
                hmax: hist_max,
            }
        }
        PixelPlane::Gray32(pixels) => {
            let mut min = f64::MAX;
            let mut max = -f64::MAX;
            let mut has_pixel = false;
            for y in ry..(ry + rh) {
                for x in rx..(rx + rw) {
                    if mask.as_ref().is_some_and(|m| !m.contains(x, y)) {
                        continue;
                    }
                    let idx = (y * width + x) as usize;
                    let v = pixels[idx] as f64;
                    if v.is_finite() {
                        if v < min {
                            min = v;
                        }
                        if v > max {
                            max = v;
                        }
                        has_pixel = true;
                    }
                }
            }
            if !has_pixel {
                return DisplayHistogram {
                    counts: [0; 256],
                    hmin: f64::MAX,
                    hmax: -f64::MAX,
                };
            }
            let hist_min = min;
            let hist_max = max;
            let mut counts = [0u64; 256];
            if hist_max == hist_min {
                let mut total = 0u64;
                for y in ry..(ry + rh) {
                    for x in rx..(rx + rw) {
                        if mask.as_ref().is_some_and(|m| !m.contains(x, y)) {
                            continue;
                        }
                        let idx = (y * width + x) as usize;
                        let v = pixels[idx] as f64;
                        if v.is_finite() && v >= hist_min && v <= hist_max {
                            total += 1;
                        }
                    }
                }
                counts[0] = total;
            } else {
                // FloatStatistics.java line 92: double scale = nBins/(histMax-histMin)
                let scale = 256.0 / (hist_max - hist_min);
                for y in ry..(ry + rh) {
                    for x in rx..(rx + rw) {
                        if mask.as_ref().is_some_and(|m| !m.contains(x, y)) {
                            continue;
                        }
                        let idx = (y * width + x) as usize;
                        let v = pixels[idx] as f64;
                        if v.is_finite() && v >= hist_min && v <= hist_max {
                            let mut index = (scale * (v - hist_min)) as usize;
                            if index >= 256 {
                                index = 255;
                            }
                            counts[index] += 1;
                        }
                    }
                }
            }
            DisplayHistogram {
                counts,
                hmin: hist_min,
                hmax: hist_max,
            }
        }
        PixelPlane::ColorRgb(pixels) => {
            // ColorProcessor.java lines 1260-1275: Color conversion using standard weights
            let mut counts = [0u64; 256];
            for y in ry..(ry + rh) {
                for x in rx..(rx + rw) {
                    if mask.as_ref().is_some_and(|m| !m.contains(x, y)) {
                        continue;
                    }
                    let idx = (y * width + x) as usize;
                    let c = pixels[idx];
                    let r = ((c >> 16) & 0xff) as f64;
                    let g = ((c >> 8) & 0xff) as f64;
                    let b = (c & 0xff) as f64;
                    let v = (r * 0.299 + g * 0.587 + b * 0.114 + 0.5) as usize;
                    let v = v.min(255);
                    counts[v] += 1;
                }
            }
            DisplayHistogram {
                counts,
                hmin: 0.0,
                hmax: 255.0,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LineRoi;

    #[test]
    fn display_histogram_constant_images() {
        let doc16 = ImageDocument::new("t", 4, 4, PixelPlane::Gray16(vec![1000u16; 16])).unwrap();
        let hist16 = display_histogram(&doc16);
        assert_eq!(hist16.hmin, 1000.0);
        assert_eq!(hist16.hmax, 1000.0);
        assert_eq!(hist16.counts[0], 16);
        assert_eq!(hist16.counts[1..], [0; 255]);

        let doc32 = ImageDocument::new("t", 4, 4, PixelPlane::Gray32(vec![5.0f32; 16])).unwrap();
        let hist32 = display_histogram(&doc32);
        assert_eq!(hist32.hmin, 5.0);
        assert_eq!(hist32.hmax, 5.0);
        assert_eq!(hist32.counts[0], 16);
        assert_eq!(hist32.counts[1..], [0; 255]);
    }

    #[test]
    fn display_histogram_line_roi_ignored() {
        let mut doc = ImageDocument::new_ramp_8bit("t", 16, 16).unwrap();
        let full = display_histogram(&doc);
        doc.roi = Some(crate::Roi::Line(LineRoi {
            x1: 2,
            y1: 3,
            x2: 10,
            y2: 10,
        }));
        let with_line = display_histogram(&doc);
        assert_eq!(full, with_line);
    }

    #[test]
    fn display_histogram_float_excludes_nonfinite_values() {
        // FloatStatistics.java lines 24-30 bound measurements to +/-Float.MAX_VALUE.
        let doc = ImageDocument::new(
            "nonfinite",
            4,
            1,
            PixelPlane::Gray32(vec![1.0, 2.0, f32::INFINITY, f32::NAN]),
        )
        .unwrap();
        let hist = display_histogram(&doc);
        assert_eq!(hist.hmin, 1.0);
        assert_eq!(hist.hmax, 2.0);
        assert_eq!(hist.counts.iter().sum::<u64>(), 2);

        let all_nan = ImageDocument::new("nan", 1, 1, PixelPlane::Gray32(vec![f32::NAN]))
            .unwrap();
        let empty = display_histogram(&all_nan);
        assert_eq!(empty.hmin, f64::MAX);
        assert_eq!(empty.hmax, -f64::MAX);
        assert_eq!(empty.counts.iter().sum::<u64>(), 0);
    }
}

