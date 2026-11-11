//! Pure single-image binary conversion: `Thresholder.applyThreshold` and the
//! `ISODATA2` auto threshold it uses, for the P3.3a commands
//! `Process > Binary > Make Binary`, `Convert to Mask` and the threshold
//! palette's `Apply` button (original_imagej/ij/plugin/Thresholder.java,
//! original_imagej/ij/process/ImageProcessor.java lines 680-848).
//!
//! Everything here is pure pixel work: no I/O, no UI, no JVM.

use crate::filters::{FilterRegion, blur_one_direction};
use crate::{CoreError, ImageDocument, PixelPlane, ThresholdLevels};

impl ImageDocument {
    /// Port of the single-image `Thresholder.applyThreshold(imp, false)` path
    /// behind `Make Binary`, `Convert to Mask` and the Threshold palette's
    /// `Apply` button (original_imagej/ij/plugin/Thresholder.java 151-263).
    ///
    /// `black_background` (ImageJ's `Prefs.blackBackground`) only inverts the
    /// display LUT in this Java path; the stored pixel values are
    /// 255 (inside the threshold bounds) and 0 (outside) for both settings.
    /// The model has no LUT state, so the flag is accepted for API shape but
    /// cannot change pixels; the oracle pairs in
    /// `parity_convert_to_mask_gray8_ignores_black_background_flag_like_java`
    /// pin both behaviours.
    ///
    /// Gray16/Gray32 sources become Gray8 (`applyShortOrFloatThreshold`), and
    /// the threshold state ends as `None`: ImageJ resets the threshold and
    /// leaves an *invisible* 255-255 bilevel threshold behind
    /// (`setThreshold(fcolor, fcolor, NO_LUT_UPDATE)`, Thresholder.java
    /// 262-266), which this model cannot represent (same documented
    /// `lutUpdateMode` limitation as the filters module).
    pub fn apply_threshold(&mut self, black_background: bool) -> Result<(), CoreError> {
        let _ = black_background;
        // Single image only: the stack path (`convertStack`) is P3.3b.
        if self.slices.len() > 1 {
            return Err(CoreError::UnsupportedThresholdStack {
                stack_size: self.slices.len(),
            });
        }
        let plane = self.slices[0].clone();
        // imp.deleteRoi() (Thresholder.java 152): Make Binary and Convert to
        // Mask drop the selection before computing the mask. The session
        // layer saves it to previous_roi first, like ImagePlus.deleteRoi's
        // saveRoi() call.
        if self.roi.is_some() {
            self.roi = None;
        }
        let converted = !matches!(plane, PixelPlane::Gray8(_));
        let display_range = self.display_range;
        let mask = match plane {
            PixelPlane::Gray8(pixels) => gray8_mask(&pixels, self.threshold),
            PixelPlane::Gray16(pixels) => {
                gray16_mask(&pixels, self.threshold, display_range)
            }
            PixelPlane::Gray32(pixels) => gray32_mask(&pixels, self.threshold, display_range),
            PixelPlane::Color256 { .. } => {
                return Err(CoreError::UnsupportedThreshold {
                    pixel_type: "8-bit color (indexed)",
                });
            }
            PixelPlane::ColorRgb(_) => {
                return Err(CoreError::UnsupportedThreshold { pixel_type: "RGB" });
            }
        };
        // Undo.setup(Undo.TRANSFORM, imp) before the conversion
        // (Thresholder.java 39).
        self.save_snapshot();
        self.slices[0] = mask;
        // ip.resetThreshold() (Thresholder.java 266): the active threshold is
        // cleared; the invisible 255-255 bilevel threshold set afterwards is
        // NO_LUT_UPDATE state this model cannot represent.
        self.threshold = None;
        if converted {
            // applyShortOrFloatThreshold installs a fresh ByteProcessor via
            // imp.setStack, which starts with the default 0..255 range.
            self.display_range = crate::DisplayRange { min: 0.0, max: 255.0 };
        }
        Ok(())
    }
}

/// The threshold bounds in original pixel units for each source type.
///
/// Gray8 compares the byte value against the stored (or ISODATA2-derived)
/// double levels, like the `i>=minThreshold && i<=maxThreshold` lut loop
/// (Thresholder.java 235-245 with the macro-path defaults `fcolor=255`,
/// `bcolor=0`, `fill1=fill2=true`).
fn gray8_mask(pixels: &[u8], threshold: Option<ThresholdLevels>) -> PixelPlane {
    let (lower, upper) = match threshold {
        Some(levels) => (levels.lower, levels.upper),
        None => {
            // Thresholder.autoThreshold -> setAutoThreshold(ISODATA2,
            // NO_LUT_UPDATE) (Thresholder.java 277). ISODATA2 takes its
            // direction from the LUT only, so a plain default LUT gives
            // 0..threshold.
            let histogram = byte_histogram_256(pixels.iter().copied());
            isodata2_threshold(&histogram)
        }
    };
    PixelPlane::Gray8(
        pixels
            .iter()
            .map(|&p| in_threshold_f64(f64::from(p), lower, upper))
            .collect(),
    )
}

fn gray16_mask(
    pixels: &[u16],
    threshold: Option<ThresholdLevels>,
    display_range: crate::DisplayRange,
) -> PixelPlane {
    let (t1, t2) = short_float_thresholds(pixels, threshold, display_range);
    PixelPlane::Gray8(
        pixels
            .iter()
            .map(|&p| in_threshold_f32(f32::from(p), t1, t2))
            .collect(),
    )
}

fn gray32_mask(
    pixels: &[f32],
    threshold: Option<ThresholdLevels>,
    display_range: crate::DisplayRange,
) -> PixelPlane {
    let (t1, t2) = float_thresholds(pixels, threshold, display_range);
    PixelPlane::Gray8(
        pixels
            .iter()
            .map(|&p| in_threshold_f32(p, t1, t2))
            .collect(),
    )
}

/// Gray16 bounds: the stored levels cast to float, or — when no threshold is
/// set — the `convertToByte(true)` + ISODATA2 round trip mapped back through
/// `min + (max-min)*(threshold/255.0)` (Thresholder.java 279-296). `min`/`max`
/// are the processor's current display bounds (`ip.getMin()/getMax()`), which
/// for ShortProcessor are its min/max fields, not a fresh scan.
fn short_float_thresholds(
    pixels: &[u16],
    threshold: Option<ThresholdLevels>,
    display_range: crate::DisplayRange,
) -> (f32, f32) {
    match threshold {
        Some(levels) => (levels.lower as f32, levels.upper as f32),
        None => {
            // TypeConverter.convertShortToByte truncates the display bounds
            // (TypeConverter.java 54: `int min=(int)ip.getMin(), ...`).
            let min = display_range.min as i32;
            let max = display_range.max as i32;
            let scale = 256.0 / (max - min + 1) as f64;
            let histogram = byte_histogram_256(
                pixels
                    .iter()
                    .map(|&p| crate::short_to_byte(p, min, scale)),
            );
            let (lo255, hi255) = isodata2_threshold(&histogram);
            let (min_f, max_f) = (min as f64, max as f64);
            (
                rescale_threshold(min_f, max_f, lo255) as f32,
                rescale_threshold(min_f, max_f, hi255) as f32,
            )
        }
    }
}

/// Gray32 bounds, using the display bounds (`getMin()/getMax()`, stored as
/// float in FloatProcessor) and `FloatProcessor.create8BitImage` scaling
/// (FloatProcessor.java 190-208) for the no-threshold path.
fn float_thresholds(
    pixels: &[f32],
    threshold: Option<ThresholdLevels>,
    display_range: crate::DisplayRange,
) -> (f32, f32) {
    match threshold {
        Some(levels) => (levels.lower as f32, levels.upper as f32),
        None => {
            let min = f64::from(display_range.min as f32);
            let max = f64::from(display_range.max as f32);
            let scale = 255.0 / (max - min);
            let histogram = byte_histogram_256(
                pixels.iter().map(|&p| crate::float_to_byte(p, min, scale)),
            );
            let (lo255, hi255) = isodata2_threshold(&histogram);
            (
                rescale_threshold(min, max, lo255) as f32,
                rescale_threshold(min, max, hi255) as f32,
            )
        }
    }
}

/// `t = min + (max-min)*(threshold/255.0)` (Thresholder.java 295-296).
fn rescale_threshold(min: f64, max: f64, threshold: f64) -> f64 {
    min + (max - min) * (threshold / 255.0)
}

fn in_threshold_f64(value: f64, lower: f64, upper: f64) -> u8 {
    if value >= lower && value <= upper {
        255
    } else {
        0
    }
}

/// `value>=t1 && value<=t2` on float pixels and float bounds
/// (Thresholder.java 302-307).
fn in_threshold_f32(value: f32, lower: f32, upper: f32) -> u8 {
    if value >= lower && value <= upper {
        255
    } else {
        0
    }
}

fn byte_histogram_256<I: Iterator<Item = u8>>(values: I) -> [i64; 256] {
    let mut histogram = [0i64; 256];
    for value in values {
        histogram[value as usize] += 1;
    }
    histogram
}

/// Port of the `setAutoThreshold(ISODATA2, lutUpdate)` level selection for an
/// already-8-bit processor (ImageProcessor.java 686-748): dominant-mode
/// flattening, the `ImageProcessor.getAutoThreshold(int[])` isodata
/// iteration, a Gaussian-blurred histogram mode test, then
/// `scaleAndSetThreshold`, which for 8-bit data stores the bounds unchanged.
fn isodata2_threshold(histogram: &[i64; 256]) -> (f64, f64) {
    // ByteStatistics.getRawStatistics/getRawMinAndMax over the full image,
    // because Thresholder.applyThreshold deleted the ROI first
    // (ImageStatistics.java).
    let mut mode = 0usize;
    let mut max_count = 0i64;
    for (i, &count) in histogram.iter().enumerate() {
        if count > max_count {
            max_count = count;
            mode = i;
        }
    }
    let mut min = 0usize;
    while min < 255 && histogram[min] == 0 {
        min += 1;
    }
    let mut max = 255usize;
    while max > 0 && histogram[max] == 0 {
        max -= 1;
    }

    // Dominant-mode flattening (ImageProcessor.java 700-710).
    let mut max_count2 = 0i64;
    for (i, &count) in histogram.iter().enumerate() {
        if count > max_count2 && i != mode {
            max_count2 = count;
        }
    }
    let mut work = *histogram;
    if max_count > max_count2 * 2 && max_count2 != 0 {
        work[mode] = (max_count2 as f64 * 1.5) as i64;
    }
    let threshold = image_processor_isodata(&mut work);

    // The mode of the Gaussian-blurred histogram decides the branch
    // (ImageProcessor.java 716-745).
    let mut hist_f: Vec<f32> = histogram.iter().map(|&count| count as f32).collect();
    let full = FilterRegion {
        x: 0,
        y: 0,
        width: 256,
        height: 1,
        mask: None,
    };
    blur_one_direction(&mut hist_f, 256, 1, &full, 2.0, 0.01, true, 0);
    let mut blurred_max = 0f32;
    let mut blurred_mode = 0usize;
    let mut sum = 0f32;
    for (i, &count) in hist_f.iter().enumerate() {
        sum += count;
        if count > blurred_max {
            blurred_max = count;
            blurred_mode = i;
        }
    }
    let avg = f64::from(sum) / 256.0;
    if f64::from(blurred_max) / avg > 1.5 {
        // The dominant blurred peak sits on one side of the mode; that side is
        // foreground (ImageProcessor.java 726-730).
        if (max as i64 - blurred_mode as i64) > (blurred_mode as i64 - min as i64) {
            (threshold as f64, 255.0)
        } else {
            (0.0, threshold as f64)
        }
    } else {
        // Non-dominant peak: a default (non-inverted) LUT keeps 0..threshold
        // (ImageProcessor.java 735-745).
        (0.0, threshold as f64)
    }
}

/// Port of `ImageProcessor.getAutoThreshold(int[])` (ImageProcessor.java
/// 808-848), the isodata iteration the two-argument `setAutoThreshold` uses.
/// `histogram[0]` and `histogram[maxValue]` are excluded from the iteration
/// ("so erased areas aren't included") and restored afterwards.
fn image_processor_isodata(histogram: &mut [i64]) -> i64 {
    let max_value = histogram.len() - 1;
    let count0 = histogram[0];
    histogram[0] = 0;
    let count_max = histogram[max_value];
    histogram[max_value] = 0;

    // Java polls histogram[min] before the bound, so an all-zero histogram
    // walks min to maxValue; the min>=max branch then returns length/2.
    let mut min = 0usize;
    while min < max_value && histogram[min] == 0 {
        min += 1;
    }
    let mut max = max_value;
    while max > 0 && histogram[max] == 0 {
        max -= 1;
    }
    if min >= max {
        histogram[0] = count0;
        histogram[max_value] = count_max;
        return (histogram.len() / 2) as i64;
    }

    let mut moving_index = min as i64;
    // Java's do-while always evaluates the body once before the condition.
    let mut result;
    loop {
        let (mut sum1, mut sum2, mut sum3, mut sum4) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
        for i in (min as i64)..=moving_index {
            sum1 += i as f64 * histogram[i as usize] as f64;
            sum2 += histogram[i as usize] as f64;
        }
        for i in (moving_index + 1)..=(max as i64) {
            sum3 += i as f64 * histogram[i as usize] as f64;
            sum4 += histogram[i as usize] as f64;
        }
        result = (sum1 / sum2 + sum3 / sum4) / 2.0;
        moving_index += 1;
        // while ((movingIndex+1)<=result && movingIndex<max-1)
        if !((moving_index + 1) as f64 <= result && moving_index < max as i64 - 1) {
            break;
        }
    }
    histogram[0] = count0;
    histogram[max_value] = count_max;
    // `(int)Math.round(result)`; a NaN result casts to 0 like the JVM.
    (result + 0.5).floor() as i64
}
