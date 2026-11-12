//! Pure single-image binary conversion: `Thresholder.applyThreshold` and the
//! `ISODATA2` auto threshold it uses, for the P3.3a commands
//! `Process > Binary > Make Binary`, `Convert to Mask` and the threshold
//! palette's `Apply` button (original_imagej/ij/plugin/Thresholder.java,
//! original_imagej/ij/process/ImageProcessor.java lines 680-848).
//!
//! Everything here is pure pixel work: no I/O, no UI, no JVM.

use crate::filters::{FilterRegion, auto_threshold_histogram_16bit, blur_one_direction, gray16_min_max};
use crate::{AutoThresholdMethod, CoreError, ImageDocument, PixelPlane, ThresholdLevels};

/// Thresholder's "Convert Stack to Binary" dialog options
/// (original_imagej/ij/plugin/Thresholder.java 82-90). The GUI dialog is
/// exchanged for this struct like the P3.11 GenericDialog swap; the value
/// names mirror the smart-recorded macro string
/// `method=<Method> background=<Background> calculate only black`, where the
/// "Default" choices are suppressed by GenericDialog.setSmartRecording.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StackBinaryOptions {
    /// "Method:" choice; `"Default"` on this port, or one of the
    /// `AutoThresholder` methods implemented so far ("IsoData", "Otsu").
    pub method: String,
    /// "Background:" choice: "Default", "Dark" or "Light". Any value other
    /// than "Light" is a dark background, like
    /// `!background.equals("Light")` (Thresholder.java 364).
    pub background: String,
    /// "Calculate threshold for each image" checkbox (Thresholder's
    /// `useLocal`). A bare option-less `run("Make Binary")` macro never
    /// passes it, which forces the whole-stack path (Thresholder.java 66).
    pub calculate: bool,
    /// "Only convert current image" checkbox (Thresholder's `oneSlice`).
    pub only_current: bool,
}

/// The dialog defaults for a fresh `Thresholder` instance: the first entries
/// of `AutoThresholder.getMethods()` and `backgrounds` with the static
/// checkbox defaults (Thresholder.java 28-33, staticUseLocal defaults true).
impl Default for StackBinaryOptions {
    fn default() -> Self {
        Self {
            method: "Default".to_string(),
            background: "Default".to_string(),
            calculate: false,
            only_current: false,
        }
    }
}

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

    /// Port of the stack branches of `Thresholder.run` and
    /// `Thresholder.convertStack` (original_imagej/ij/plugin/Thresholder.java
    /// 39-147), behind `Make Binary` / `Convert to Mask` on multi-slice
    /// images ("Convert Stack to Binary").
    ///
    /// Only macro semantics are modelled: `run("Convert to Mask", "options")`
    /// styled strings, mirroring what GenericDialog's smart recorder emits.
    /// `options.method`/`background` of "Default" mirror the suppressed
    /// default choices; a `None`-options command (or `calculate == false`)
    /// runs the whole-stack fixed path, like an option-less macro whose
    /// checkbox defaults are all false (Thresholder.java 66).
    ///
    /// Undo behaviour differs from the single-image path: convertStack calls
    /// `Undo.reset()` instead of `Undo.setup` (Thresholder.java 118), so no
    /// snapshot is taken and an existing one is dropped. The per-slice path
    /// (`calculate`) keeps the ROI and computes whole-image histograms,
    /// because the processors fetched via `ImageStack.getProcessor` never
    /// carry the selection (ImageStack.java 308-336); the whole-stack and
    /// only-current paths delete it first (`Thresholder.applyThreshold`,
    /// Thresholder.java 152).
    ///
    /// Known limits, mirroring the engine's dialog: the remaining
    /// AutoThresholder methods, "List thresholds" and "Create new stack"
    /// (Thresholder.java 257-370) are unsupported, and the only-current
    /// option is rejected for stacks that are not 8-bit
    /// (Thresholder.java 129-131).
    pub fn convert_stack_to_binary(
        &mut self,
        options: &StackBinaryOptions,
    ) -> Result<(), CoreError> {
        // AutoThreshover method validation happens inside setAutoThreshold
        // in Java; validating first avoids dropping the undo state for an
        // option error.
        let method = if options.method == "Default" {
            None
        } else {
            Some(AutoThresholdMethod::parse(&options.method)?)
        };
        let dark_background = options.background != "Light";
        let one_slice = options.only_current;
        let mut use_local = options.calculate;
        if one_slice {
            // "Only convert current image" forces the single-slice fixed path
            // and requires an 8-bit stack (Thresholder.java 127-131).
            use_local = false;
            let current = self.current_slice_index_for_binary();
            if !matches!(self.slices[current], PixelPlane::Gray8(_)) {
                return Err(CoreError::Gray8StackRequiredForOnlyCurrentSlice);
            }
        }
        // Undo.reset() (Thresholder.java 118).
        self.snapshot = None;
        self.snapshot_dimensions = None;
        self.snapshot_display_range = None;

        if use_local {
            self.convert_stack_to_binary_slice_local(method, dark_background)
        } else {
            self.apply_threshold_stack_fixed(one_slice)
        }
    }

    /// Mirror of `ImageDocument::current_slice_index` in lib.rs, which is
    /// private to the core module.
    fn current_slice_index_for_binary(&self) -> usize {
        let size = self.slices.len() as u32;
        (self.current_slice.clamp(1, size) - 1) as usize
    }

    /// Port of `Thresholder.applyThreshold(imp, oneSlice)` reached from
    /// `convertStack` (Thresholder.java 151-276): the ROI is deleted, the LUT
    /// (or 16-bit threshold bounds) comes from the current slice's fixed
    /// threshold or its ISODATA2 auto threshold, and one LUT table is applied
    /// to all slices (or only the current slice).
    fn apply_threshold_stack_fixed(&mut self, one_slice: bool) -> Result<(), CoreError> {
        // imp.deleteRoi() (Thresholder.java 152).
        self.roi = None;
        let size = self.slices.len();
        let current = self.current_slice_index_for_binary();
        let indices: Vec<usize> = if one_slice {
            vec![current]
        } else {
            (0..size).collect()
        };
        let converted = !matches!(self.slices[current], PixelPlane::Gray8(_));
        let display_range = self.display_range;
        // int type = imp.getType() (Thresholder.java 155).
        match &self.slices[current] {
            PixelPlane::Gray16(pixels) => {
                let (t1, t2) = match self.threshold {
                    Some(levels) => (levels.lower as f32, levels.upper as f32),
                    None => {
                        let (t1, t2) = short_float_thresholds(pixels, None, display_range);
                        (t1, t2)
                    }
                };
                let converted_planes = {
                    let converted: Vec<PixelPlane> = indices
                        .iter()
                        .filter_map(|&index| match &self.slices[index] {
                            PixelPlane::Gray16(pixels) => {
                                Some(gray16_mask_with(pixels, t1, t2))
                            }
                            _ => None,
                        })
                        .collect();
                    converted
                };
                for (position, plane) in converted_planes.into_iter().enumerate() {
                    self.slices[indices[position]] = plane;
                }
            }
            PixelPlane::Gray32(pixels) => {
                let (t1, t2) = match self.threshold {
                    Some(levels) => (levels.lower as f32, levels.upper as f32),
                    None => {
                        let (t1, t2) = float_thresholds(pixels, None, display_range);
                        (t1, t2)
                    }
                };
                let converted: Vec<PixelPlane> = indices
                    .iter()
                    .filter_map(|&index| match &self.slices[index] {
                        PixelPlane::Gray32(pixels) => {
                            Some(gray32_mask_with(pixels, t1, t2))
                        }
                        _ => None,
                    })
                    .collect();
                for (position, plane) in converted.into_iter().enumerate() {
                    self.slices[indices[position]] = plane;
                }
            }
            PixelPlane::Gray8(_) => {
                // The threshold bounds (Thresholder.java 274-278): the saved
                // level pair, or the ISODATA2 auto threshold of the current
                // slice's histogram.
                let (lower, upper) = match self.threshold {
                    Some(levels) => (levels.lower, levels.upper),
                    None => {
                        let histogram =
                            byte_histogram_256(self.gray8_pixels_at(current).iter().copied());
                        isodata2_threshold(&histogram)
                    }
                };
                let converted: Vec<Vec<u8>> = indices
                    .iter()
                    .map(|&index| match &self.slices[index] {
                        PixelPlane::Gray8(pixels) => pixels
                            .iter()
                            .map(|&p| in_threshold_f64(f64::from(p), lower, upper))
                            .collect(),
                        _ => Vec::new(),
                    })
                    .collect();
                for (position, mask) in converted.into_iter().enumerate() {
                    self.slices[indices[position]] = PixelPlane::Gray8(mask);
                }
            }
            PixelPlane::Color256 { .. } => {
                return Err(CoreError::UnsupportedThreshold {
                    pixel_type: "8-bit color (indexed)",
                });
            }
            PixelPlane::ColorRgb(_) => {
                return Err(CoreError::UnsupportedThreshold { pixel_type: "RGB" });
            }
        }
        // ip.resetThreshold() plus the invisible 255-255 NO_LUT_UPDATE
        // threshold (Thresholder.java 263, 266 and 393), which this model
        // stores as None.
        self.threshold = None;
        if converted && !one_slice {
            self.display_range = crate::DisplayRange { min: 0.0, max: 255.0 };
        }
        Ok(())
    }

    fn gray8_pixels_at(&self, index: usize) -> Vec<u8> {
        match &self.slices[index] {
            PixelPlane::Gray8(pixels) => pixels.clone(),
            _ => Vec::new(),
        }
    }

    /// Port of `Thresholder.convertStackToBinary` (Thresholder.java 333-410):
    /// per-slice auto thresholding. Non-8-bit slices are first converted to
    /// byte with a per-slice `resetMinAndMax()` + `convertToByte(true)`.
    fn convert_stack_to_binary_slice_local(
        &mut self,
        method: Option<AutoThresholdMethod>,
        dark_background: bool,
    ) -> Result<(), CoreError> {
        let mut converted_any = false;
        for index in 0..self.slices.len() {
            let bytes = match &self.slices[index] {
                PixelPlane::Gray8(_) => continue,
                PixelPlane::Gray16(pixels) => {
                    // ip.resetMinAndMax(); ip.convertToByte(true)
                    // (Thresholder.java 346-350). ShortProcessor.findMinAndMax
                    // scans the whole plane (ShortProcessor.java 70-87).
                    let (min, max) = gray16_min_max(pixels);
                    let scale =
                        256.0 / ((max as i64 - min as i64) + 1) as f64;
                    pixels
                        .iter()
                        .map(|&p| crate::short_to_byte(p, min as i32, scale))
                        .collect::<Vec<u8>>()
                }
                PixelPlane::Gray32(_) => {
                    return Err(CoreError::UnsupportedThreshold {
                        pixel_type: "32-bit float",
                    });
                }
                PixelPlane::Color256 { .. } => {
                    return Err(CoreError::UnsupportedThreshold {
                        pixel_type: "8-bit color (indexed)",
                    });
                }
                PixelPlane::ColorRgb(_) => {
                    return Err(CoreError::UnsupportedThreshold { pixel_type: "RGB" });
                }
            };
            self.slices[index] = PixelPlane::Gray8(bytes);
            converted_any = true;
        }
        for index in 0..self.slices.len() {
            let (lower, upper) = {
                let histogram = match &self.slices[index] {
                    PixelPlane::Gray8(pixels) => {
                        byte_histogram_256(pixels.iter().copied())
                    }
                    _ => unreachable!("converted above"),
                };
                match method {
                    None => isodata2_threshold(&histogram),
                    Some(method) => {
                        // setAutoThreshold(Method, !background.equals("Light"),
                        // NO_LUT_UPDATE) on a fresh ByteProcessor
                        // (Thresholder.java 364, ImageProcessor.java 629-676):
                        // addOne is the untouched instance field default
                        // (false), so dark backgrounds keep the threshold
                        // itself as the lower level.
                        let t =
                            auto_threshold_histogram_16bit(method, &histogram, false)? as f64;
                        if dark_background {
                            (t, 255.0)
                        } else {
                            (0.0, t)
                        }
                    }
                }
            };
            let mask: Vec<u8> = match &self.slices[index] {
                PixelPlane::Gray8(pixels) => pixels
                    .iter()
                    .map(|&p| in_threshold_f64(f64::from(p), lower, upper))
                    .collect(),
                _ => unreachable!("converted above"),
            };
            self.slices[index] = PixelPlane::Gray8(mask);
        }
        // stack.setColorModel + setThreshold(255, 255, NO_LUT_UPDATE)
        // (Thresholder.java 387-393): display-only.
        self.threshold = None;
        if converted_any {
            self.display_range = crate::DisplayRange { min: 0.0, max: 255.0 };
        }
        Ok(())
    }
}

/// `(value>=t1 && value<=t2) ? 255 : 0` for Gray16 sources with float bounds
/// (Thresholder.java 302-307).
fn gray16_mask_with(pixels: &[u16], t1: f32, t2: f32) -> PixelPlane {
    PixelPlane::Gray8(
        pixels
            .iter()
            .map(|&p| in_threshold_f32(f32::from(p), t1, t2))
            .collect(),
    )
}

fn gray32_mask_with(pixels: &[f32], t1: f32, t2: f32) -> PixelPlane {
    PixelPlane::Gray8(
        pixels
            .iter()
            .map(|&p| in_threshold_f32(p, t1, t2))
            .collect(),
    )
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
    blur_one_direction(&mut hist_f, 256, 1, &full, 2.0, 0.01, true, 0, 1);
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
