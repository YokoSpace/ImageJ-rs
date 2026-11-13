//! Z-projection of grayscale stacks, mirroring `ij.plugin.ZProjector`
//! (original_imagej/ij/plugin/ZProjector.java). Only the plain-stack path is
//! implemented: RGB stacks (24-bit) and C/Z/T hyperstacks follow Java's
//! `doRGBProjection`/`doHyperStackProjection` branches, which are out of
//! scope for this unit.

use serde::{Deserialize, Serialize};

use crate::{CoreError, DisplayRange, ImageDocument, PixelPlane};

/// `ZProjector.METHODS` in dialog order (ZProjector.java 24-25).
pub const Z_PROJECT_METHODS: [ZProjectionMethod; 6] = [
    ZProjectionMethod::Average,
    ZProjectionMethod::Max,
    ZProjectionMethod::Min,
    ZProjectionMethod::Sum,
    ZProjectionMethod::StandardDeviation,
    ZProjectionMethod::Median,
];

/// The six projection methods (ZProjector.java 18-23).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ZProjectionMethod {
    Average,
    Max,
    Min,
    Sum,
    StandardDeviation,
    Median,
}

impl ZProjectionMethod {
    /// The "Projection type" choice label (ZProjector.java 25).
    pub fn label(self) -> &'static str {
        match self {
            Self::Average => "Average Intensity",
            Self::Max => "Max Intensity",
            Self::Min => "Min Intensity",
            Self::Sum => "Sum Slices",
            Self::StandardDeviation => "Standard Deviation",
            Self::Median => "Median",
        }
    }

    /// Output title prefix (ZProjector.java 606-615).
    pub fn title_prefix(self) -> &'static str {
        match self {
            Self::Average => "AVG_",
            Self::Max => "MAX_",
            Self::Min => "MIN_",
            Self::Sum => "SUM_",
            Self::StandardDeviation => "STD_",
            Self::Median => "MED_",
        }
    }
}

/// Parameters matching the ZProjection GenericDialog (ZProjector.java
/// 293-301): two numeric fields and the method choice.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ZProjectionOptions {
    pub method: ZProjectionMethod,
    /// Raw "Start slice:" field value, cast like `(int)gd.getNextNumber()`.
    pub start_slice: f64,
    /// Raw "Stop slice:" field value.
    pub stop_slice: f64,
}

/// `IJ.error("Z Project", "Stack required")` (ZProjector.java 144-146).
pub const Z_PROJECT_STACK_REQUIRED: &str = "Stack required";
/// Plain `ZProjector.doProjection` never handles these (ZProjector.java
/// 309-311, 383-388); they need the P4.4d1 RGB/hyperstack branches.
pub const Z_PROJECT_UNSUPPORTED: &str =
    "Z Project requires a grayscale stack; RGB and hyperstack projection are not implemented";
/// `doMedianProjection` indexes `slices[0]` of an empty array and crashes
/// (ZProjector.java 621-625); we fail instead of producing a broken image.
pub const Z_PROJECT_EMPTY_RANGE: &str = "Z Project: the slice range contains no slices";

/// Warning dialog text `ZProjector.lutMessage` (ZProjector.java 33-36). The
/// app shows it through `IJ.showMessageWithCancel` before the dialog when the
/// source has an inverting LUT (ZProjector.java 149-153).
pub const Z_PROJECT_INVERTED_LUT_MESSAGE: &str =
    "Stacks with inverting LUTs may not project correctly.\n\
     To create a standard LUT, invert the stack (Edit/Invert)\n\
     and invert the LUT (Image/Lookup Tables/Invert LUT).";

/// Reason `run("Z Project...")` cannot run on `doc`, or `None`.
///
/// Java only errors up front for a single-slice image (ZProjector.java
/// 144-147); the RGB and hyperstack branches are rejected here because this
/// unit does not implement them.
pub fn z_project_block_reason(doc: &ImageDocument) -> Option<&'static str> {
    if doc.stack_size() == 1 {
        return Some(Z_PROJECT_STACK_REQUIRED);
    }
    if matches!(doc.plane(), PixelPlane::ColorRgb(_)) || doc.is_hyperstack() {
        return Some(Z_PROJECT_UNSUPPORTED);
    }
    None
}

/// Which `RayFunction.projectSlice` overload Java picks from the first
/// slice's processor (ZProjector.java 339-346).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlaneKind {
    Byte,
    Short,
    Float,
}

fn plane_kind(plane: &PixelPlane) -> Option<PlaneKind> {
    match plane {
        PixelPlane::Gray8(_) | PixelPlane::Color256 { .. } => Some(PlaneKind::Byte),
        PixelPlane::Gray16(_) => Some(PlaneKind::Short),
        PixelPlane::Gray32(_) => Some(PlaneKind::Float),
        PixelPlane::ColorRgb(_) => None,
    }
}

/// `ImageProcessor.getPixelValue` equivalents used by the projection loops:
/// `pixels[i]&0xff`, `pixels[i]&0xffff`, or the raw float (ZProjector.java
/// 714-727, 751-769, 788-805, 825-850).
fn slice_value(plane: &PixelPlane, index: usize) -> f32 {
    match plane {
        PixelPlane::Gray8(p) | PixelPlane::Color256 { pixels: p, .. } => p[index] as f32,
        PixelPlane::Gray16(p) => p[index] as f32,
        PixelPlane::Gray32(p) => p[index],
        PixelPlane::ColorRgb(_) => unreachable!("RGB is rejected before projection"),
    }
}

/// `FloatProcessor.findMinAndMax` (FloatProcessor.java 92-117): the first
/// non-NaN pixel seeds min and max; an all-NaN plane reports NaN,NaN.
fn float_min_max(pixels: &[f32]) -> DisplayRange {
    let mut min = f32::NAN;
    let mut max = f32::NAN;
    let mut seeded = false;
    for &value in pixels {
        if !seeded {
            if value.is_nan() {
                continue;
            }
            min = value;
            max = value;
            seeded = true;
        } else if value < min {
            min = value;
        } else if value > max {
            max = value;
        }
    }
    DisplayRange {
        min: min as f64,
        max: max as f64,
    }
}

/// `ShortProcessor.findMinAndMax` reports the actual pixel extremes.
fn short_min_max(pixels: &[u16]) -> DisplayRange {
    let mut min = u16::MAX;
    let mut max = u16::MIN;
    for &value in pixels {
        min = min.min(value);
        max = max.max(value);
    }
    DisplayRange {
        min: min as f64,
        max: max as f64,
    }
}

/// `makeOutputImage` narrowing casts `(byte|short)(pixels[i]+0.5f)`
/// (ZProjector.java 558-574). JLS 5.1.3 narrows float→byte/short in two
/// steps (float→int saturating with NaN→0, then int→byte/short truncating),
/// which `as i32 as u8/u16` reproduces exactly.
fn narrow_to_byte(value: f32) -> u8 {
    (value + 0.5) as i32 as u8
}

fn narrow_to_short(value: f32) -> u16 {
    (value + 0.5) as i32 as u16
}

/// `Arrays.sort` on floats uses `Float.compare`: NaN is greater than every
/// real value and equal to itself, and -0.0 < 0.0 (Float.compare docs;
/// ZProjector.median at ZProjector.java 645-652).
fn java_float_compare(a: f32, b: f32) -> std::cmp::Ordering {
    match (a.is_nan(), b.is_nan()) {
        (true, true) => std::cmp::Ordering::Equal,
        (true, false) => std::cmp::Ordering::Greater,
        (false, true) => std::cmp::Ordering::Less,
        (false, false) => a.total_cmp(&b),
    }
}

/// `ZProjector.doMedianProjection` (ZProjector.java 618-643): medians are
/// computed in a FloatProcessor; only 8-bit input converts back to byte
/// (`convertToByte(false)` → `ByteProcessor.setPixels(0, fp)`, clamping
/// `value+0.5` into [0,255], ByteProcessor.java 1268-1279). 16-bit input
/// therefore produces 32-bit output.
fn median_projection(
    source: &ImageDocument,
    start: usize,
    stop: usize,
    slice_count: usize,
) -> Result<(PixelPlane, DisplayRange), CoreError> {
    if slice_count == 0 {
        return Err(CoreError::InvalidOperation(Z_PROJECT_EMPTY_RANGE));
    }
    let len = (source.width * source.height) as usize;
    let mut out = vec![0f32; len];
    let mut values = vec![0f32; slice_count];
    for (i, pixel) in out.iter_mut().enumerate() {
        for (j, slice) in source.slices[start..stop].iter().enumerate() {
            values[j] = slice_value(slice, i);
        }
        values.sort_by(|a, b| java_float_compare(*a, *b));
        let middle = slice_count / 2;
        *pixel = if slice_count % 2 == 0 {
            // (a[middle-1] + a[middle]) / 2f in f32 (ZProjector.java 649).
            (values[middle - 1] + values[middle]) / 2.0
        } else {
            values[middle]
        };
    }
    match plane_kind(&source.slices[0]) {
        Some(PlaneKind::Byte) => {
            let pixels = out
                .iter()
                .map(|&v| {
                    // ByteProcessor.setPixels(0, fp): value+0.5 clamped to
                    // [0,255] then (byte) cast; NaN survives the clamps and
                    // becomes 0 (ByteProcessor.java 1268-1279).
                    let mut value = v + 0.5;
                    if value < 0.0 {
                        value = 0.0;
                    }
                    if value > 255.0 {
                        value = 255.0;
                    }
                    value as u8
                })
                .collect();
            Ok((
                PixelPlane::Gray8(pixels),
                DisplayRange { min: 0.0, max: 255.0 },
            ))
        }
        Some(PlaneKind::Short) => {
            // convertShortToFloat builds a fresh FloatProcessor, so
            // getMinAndMax lazily finds the true range (ZProjector.java
            // 627, TypeConverter.java convertToFloat).
            let range = float_min_max(&out);
            Ok((PixelPlane::Gray32(out), range))
        }
        _ => {
            // For FLOAT input convertToFloat returns the slices[0]
            // duplicate itself, carrying its stored min/max instead of a
            // recomputed range (TypeConverter.java FLOAT case).
            Ok((PixelPlane::Gray32(out), source.display_range))
        }
    }
}

/// `ZProjector.doProjection` accumulation for Average/Sum/Max/Min
/// (ZProjector.java 325-372 and the RayFunction classes at 700-808).
fn accumulate(
    source: &ImageDocument,
    method: ZProjectionMethod,
    start: usize,
    stop: usize,
    kind: PlaneKind,
) -> Vec<f32> {
    let len = (source.width * source.height) as usize;
    let init = match method {
        ZProjectionMethod::Max => -f32::MAX,
        ZProjectionMethod::Min => f32::MAX,
        _ => 0.0,
    };
    let mut fp = vec![init; len];
    for slice in &source.slices[start..stop] {
        match method {
            ZProjectionMethod::Average | ZProjectionMethod::Sum => {
                for (i, acc) in fp.iter_mut().enumerate() {
                    *acc += slice_value(slice, i);
                }
            }
            ZProjectionMethod::Max => match kind {
                PlaneKind::Float => {
                    for (i, acc) in fp.iter_mut().enumerate() {
                        let v = slice_value(slice, i);
                        if !v.is_nan() && v > *acc {
                            *acc = v;
                        }
                    }
                }
                _ => {
                    for (i, acc) in fp.iter_mut().enumerate() {
                        let v = slice_value(slice, i);
                        if v > *acc {
                            *acc = v;
                        }
                    }
                }
            },
            ZProjectionMethod::Min => {
                for (i, acc) in fp.iter_mut().enumerate() {
                    let v = slice_value(slice, i);
                    if v < *acc {
                        *acc = v;
                    }
                }
            }
            _ => unreachable!("SD and Median do not share the f32 accumulator"),
        }
    }
    fp
}

/// `ZProjector.StandardDeviation` (ZProjector.java 811-867): double
/// accumulators, `sqrt((n*sum2 - sum*sum)/n/(n-1))` when positive, else 0.
fn standard_deviation_projection(
    source: &ImageDocument,
    start: usize,
    stop: usize,
    slice_count: usize,
) -> Vec<f32> {
    let len = (source.width * source.height) as usize;
    let mut sum = vec![0f64; len];
    let mut sum2 = vec![0f64; len];
    for slice in &source.slices[start..stop] {
        for i in 0..len {
            let v = slice_value(slice, i) as f64;
            sum[i] += v;
            sum2[i] += v * v;
        }
    }
    let n = slice_count as f64;
    let mut result = vec![0f32; len];
    for i in 0..len {
        if slice_count > 1 {
            let std_dev = (n * sum2[i] - sum[i] * sum[i]) / n;
            if std_dev > 0.0 {
                result[i] = (std_dev / (n - 1.0)).sqrt() as f32;
            }
        }
    }
    result
}

/// `ZProjector.doAverageFloatProjection` (ZProjector.java 654-681): the
/// 32-bit-only average accumulates in double and ignores NaNs, so a pixel
/// that is NaN in every slice becomes 0.0/0 → NaN.
fn average_float_projection(source: &ImageDocument, start: usize, stop: usize) -> Vec<f32> {
    let len = (source.width * source.height) as usize;
    let mut sum = vec![0f64; len];
    let mut count = vec![0u32; len];
    for slice in &source.slices[start..stop] {
        for i in 0..len {
            let value = slice_value(slice, i);
            if !value.is_nan() {
                sum[i] += value as f64;
                count[i] += 1;
            }
        }
    }
    let mut out = vec![0f32; len];
    for (i, pixel) in out.iter_mut().enumerate() {
        *pixel = (sum[i] / count[i] as f64) as f32;
    }
    out
}

/// `makeOutputImage` converts the float accumulator back to the input's
/// processor type for Average/Max/Min (ZProjector.java 549-586). Byte output
/// gets `ByteProcessor.resetMinAndMax` → 0..255; short/float get the actual
/// pixel extremes.
fn same_type_output(
    fp: &[f32],
    kind: PlaneKind,
) -> (PixelPlane, DisplayRange) {
    match kind {
        PlaneKind::Byte => (
            PixelPlane::Gray8(fp.iter().map(|&v| narrow_to_byte(v)).collect()),
            DisplayRange { min: 0.0, max: 255.0 },
        ),
        PlaneKind::Short => {
            let pixels: Vec<u16> = fp.iter().map(|&v| narrow_to_short(v)).collect();
            let range = short_min_max(&pixels);
            (PixelPlane::Gray16(pixels), range)
        }
        PlaneKind::Float => (PixelPlane::Gray32(fp.to_vec()), float_min_max(fp)),
    }
}

/// Runs `ZProjector.doProjection` on a plain grayscale stack and returns the
/// new output document. `title` is the already-unique `makeTitle()` result
/// (ZProjector.java 606-616); the caller owns window-name uniquification.
/// Calibration is copied like `doProjection(boolean)` (ZProjector.java
/// 393-395); ROI and overlay are not carried over.
pub fn z_project(
    source: &ImageDocument,
    options: &ZProjectionOptions,
    title: impl Into<String>,
) -> Result<ImageDocument, CoreError> {
    if let Some(reason) = z_project_block_reason(source) {
        return Err(CoreError::InvalidOperation(reason));
    }
    let stack_size = source.stack_size();
    // setStartSlice/setStopSlice silently keep the previous value (the
    // dialog defaults 1 and stackSize) when the argument is out of bounds
    // (ZProjector.java 110-120).
    let mut start = options.start_slice as i64;
    if start < 1 || start as usize > stack_size {
        start = 1;
    }
    let mut stop = options.stop_slice as i64;
    if stop < 1 || stop as usize > stack_size {
        stop = stack_size as i64;
    }
    let (start, stop) = (start as usize, stop as usize);
    // Java's `for (slice=startSlice; slice<=stopSlice; slice+=increment)`
    // visits zero slices when start > stop (ZProjector.java 318-319, 350).
    let slice_count = if start <= stop { stop - start + 1 } else { 0 };
    // Rust slices are zero-based; Java's start..stop is inclusive one-based.
    // Keep the range valid when it is empty so `slices[range]` never panics.
    let range = if slice_count == 0 {
        0..0
    } else {
        (start - 1)..stop
    };

    let kind = plane_kind(&source.slices[0])
        .ok_or(CoreError::InvalidOperation(Z_PROJECT_UNSUPPORTED))?;
    let (plane, range_display) = match options.method {
        ZProjectionMethod::Median => {
            median_projection(source, range.start, range.end, slice_count)?
        }
        ZProjectionMethod::Average if kind == PlaneKind::Float => {
            let out = average_float_projection(source, range.start, range.end);
            let range = float_min_max(&out);
            (PixelPlane::Gray32(out), range)
        }
        ZProjectionMethod::Sum => {
            let fp = accumulate(source, options.method, range.start, range.end, kind);
            // fp.subtract(sliceCount*32768) applies only to signed 16-bit
            // calibration (ZProjector.java 361-362), which the document
            // model does not represent.
            let range = float_min_max(&fp);
            (PixelPlane::Gray32(fp), range)
        }
        ZProjectionMethod::StandardDeviation => {
            let result =
                standard_deviation_projection(source, range.start, range.end, slice_count);
            let range = float_min_max(&result);
            (PixelPlane::Gray32(result), range)
        }
        _ => {
            let mut fp = accumulate(source, options.method, range.start, range.end, kind);
            if options.method == ZProjectionMethod::Average {
                // AverageIntensity.postProcess divides by the projected
                // slice count in f32 (ZProjector.java 729-733).
                let fnum = slice_count as f32;
                for acc in fp.iter_mut() {
                    *acc /= fnum;
                }
            }
            same_type_output(&fp, kind)
        }
    };

    let mut doc = ImageDocument::new(title, source.width, source.height, plane)?;
    doc.display_range = range_display;
    doc.calibration = source.calibration.clone();
    Ok(doc)
}
