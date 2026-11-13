//! `Image > Stacks > Images to Stack` and `Image > Stacks > Stack to Images`,
//! mirroring `ij.plugin.ImagesToStack`
//! (original_imagej/ij/plugin/ImagesToStack.java) and
//! `ij.plugin.StackEditor.convertStackToImages`
//! (original_imagej/ij/plugin/StackEditor.java 240-328).
//!
//! Only ordinary 2D images and plain stacks are modelled. Hyperstacks and
//! composites are rejected explicitly instead of being flattened, because
//! Java's `CompositeImage` channel-LUT branch (StackEditor.java 262-270) needs
//! the composite display foundations that this unit does not implement.
//!
//! Two Java behaviours are deliberately not modelled because the document
//! model has no equivalent:
//!
//! * The `Info` property appended to a slice label (ImagesToStack.java
//!   142-146) and re-attached by Stack to Images (StackEditor.java 276-278).
//! * Overlays (ImagesToStack.java 202-210, 221-228) and `FileInfo`
//!   (ImagesToStack.java 150-151, 165-169, 245-249).
//!
//! `ImagePlus.getProp("Slice_Label")` *is* modelled: a single-slice document
//! keeps that property in `slice_label_property`, independently of
//! `slice_labels[0]`. Delete Slice sets it when the stack collapses to one;
//! a one-slice stack made by Images to Stack does not. The fallback
//! (ImagesToStack.java 137-141) reads the property. Stack to Images never sets it
//! (StackEditor.java 276-278 stores `Info` instead), so its outputs stay
//! label-free.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::edit::{best_gray_index, gray16_value, gray32_value, rgb_value};
use crate::transform::{resize_plane, Interpolation};
use crate::{Calibration, DisplayRange, ImageDocument, PixelPlane};

/// `ImagesToStack.methods` in dialog order (ImagesToStack.java 20).
pub const IMAGES_TO_STACK_METHODS: [ImagesToStackMethod; 4] = [
    ImagesToStackMethod::CopyCenter,
    ImagesToStackMethod::CopyTopLeft,
    ImagesToStackMethod::ScaleSmallest,
    ImagesToStackMethod::ScaleLargest,
];

/// The "Method:" choice of the Images to Stack dialog
/// (ImagesToStack.java 19-20).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImagesToStackMethod {
    CopyCenter,
    CopyTopLeft,
    ScaleSmallest,
    ScaleLargest,
}

impl ImagesToStackMethod {
    /// The choice label (ImagesToStack.java 20).
    pub fn label(self) -> &'static str {
        match self {
            Self::CopyCenter => "Copy (center)",
            Self::CopyTopLeft => "Copy (top-left)",
            Self::ScaleSmallest => "Scale (smallest)",
            Self::ScaleLargest => "Scale (largest)",
        }
    }

    /// Parses a choice label, as `GenericDialog.getNextChoiceIndex` matches it.
    pub fn from_label(label: &str) -> Option<Self> {
        IMAGES_TO_STACK_METHODS
            .into_iter()
            .find(|method| method.label() == label)
    }
}

/// The Images to Stack dialog fields (ImagesToStack.java 76-116). `name`,
/// `title_filter` and `fill` are reset on every run; `method`, `bicubic`,
/// `titles_as_labels` and `keep` are the persisted statics when the dialog is
/// shown outside a macro (ImagesToStack.java 108-113).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImagesToStackOptions {
    pub method: ImagesToStackMethod,
    /// The Method choice's initial index, i.e. `staticMethod`
    /// (ImagesToStack.java 22, 85). Only the recorder reads it: with
    /// `setSmartRecording(true)` a choice is recorded only when the user
    /// changed it (GenericDialog.java 1428-1436).
    pub method_default: ImagesToStackMethod,
    /// "Name:" field; the output window title before uniquification.
    pub name: String,
    /// "Title contains:" field. Empty and `*` both mean "no filter"
    /// (ImagesToStack.java 101-102).
    pub title_filter: String,
    /// Raw "Fill color:" text, decoded by [`decode_fill_color`] exactly like
    /// `Colors.decode(fillc, null)` (ImagesToStack.java 96). An unrecognized
    /// value leaves the new processor's background untouched.
    pub fill: String,
    /// "Bicubic interpolation" checkbox; only used by the scale methods.
    pub bicubic: bool,
    /// "Use titles as labels" checkbox.
    pub titles_as_labels: bool,
    /// "Keep source images" checkbox.
    pub keep: bool,
}

impl Default for ImagesToStackOptions {
    fn default() -> Self {
        // ImagesToStack.java 22-28: staticMethod=COPY_CENTER, staticBicubic
        // and staticKeep false, staticTitlesAsLabels true, name "Stack".
        Self {
            method: ImagesToStackMethod::CopyCenter,
            method_default: ImagesToStackMethod::CopyCenter,
            name: "Stack".to_string(),
            title_filter: String::new(),
            fill: String::new(),
            bicubic: false,
            titles_as_labels: true,
            keep: false,
        }
    }
}

/// `Colors.decode(text, null)` (original_imagej/ij/plugin/Colors.java 100-171):
/// a named color matched by substring, a `#`/`0x` prefixed 6- or 8-digit hex
/// value, or `None` when the text names no color.
pub fn decode_fill_color(text: &str) -> Option<[u8; 3]> {
    if text.chars().count() < 2 {
        return None;
    }
    let lower = text.to_lowercase();
    // Colors.getColor checks the names in this order (Colors.java 113-131).
    let named = if lower.contains("black") {
        Some([0, 0, 0])
    } else if lower.contains("white") {
        Some([255, 255, 255])
    } else if lower.contains("red") {
        Some([255, 0, 0])
    } else if lower.contains("blue") {
        Some([0, 0, 255])
    } else if lower.contains("yellow") {
        Some([255, 255, 0])
    } else if lower.contains("green") {
        Some([0, 255, 0])
    } else if lower.contains("magenta") {
        Some([255, 0, 255])
    } else if lower.contains("cyan") {
        Some([0, 255, 255])
    } else if lower.contains("orange") {
        Some([255, 200, 0])
    } else if lower.contains("pink") {
        Some([255, 175, 175])
    } else if lower.contains("gray") || lower.contains("grey") {
        if lower.contains("light") {
            Some([192, 192, 192])
        } else if lower.contains("dark") {
            Some([64, 64, 64])
        } else {
            Some([128, 128, 128])
        }
    } else {
        None
    };
    if named.is_some() {
        return named;
    }
    let hex = text
        .strip_prefix('#')
        .or_else(|| text.strip_prefix("0x"))
        .unwrap_or(text);
    // Colors.decode accepts exactly 6 or 8 hex digits (Colors.java 158-160).
    if hex.len() != 6 && hex.len() != 8 {
        return None;
    }
    let value = u32::from_str_radix(hex, 16).ok()?;
    Some([
        ((value >> 16) & 0xff) as u8,
        ((value >> 8) & 0xff) as u8,
        (value & 0xff) as u8,
    ])
}

/// The processor type the output stack is built with
/// (ImagesToStack.findMinMaxSize, ImagesToStack.java 246-296).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StackType {
    Byte,
    Short,
    Float,
    Rgb,
}

impl StackType {
    /// `ImagePlus.getBitDepth` for the matching processor.
    pub fn bit_depth(self) -> u8 {
        match self {
            Self::Byte => 8,
            Self::Short => 16,
            Self::Float => 32,
            Self::Rgb => 24,
        }
    }
}

/// `IJ.error` texts and the explicit boundaries of this unit.
#[derive(Debug, PartialEq, Error)]
pub enum StackConvertError {
    /// `IJ.error("Images to Stack", "At least two open 2D images required.")`
    /// (ImagesToStack.java 52-63).
    #[error("At least two open 2D images required.")]
    NeedsTwoImages,
    /// The same error with the Concatenate hint appended, used when more than
    /// one open window is a stack (ImagesToStack.java 54-58).
    #[error(
        "At least two open 2D images required.\n \nUse the Image>Stacks>Tools>Concatenate\ncommand to combine stacks."
    )]
    NeedsTwoImagesWithStacks,
    /// `IJ.error("Images are not all the same size")` for a macro run without
    /// options (ImagesToStack.java 66-74).
    #[error("Images are not all the same size")]
    SizeMismatch,
    /// `IJ.error("Images to Stack", "None of the images have a title
    /// containing \"...\"")` (ImagesToStack.java 96-100).
    #[error("None of the images have a title containing \"{0}\"")]
    NoMatchingTitles(String),
    /// `IJ.error("\"Convert Stack to Images\" requires a stack\n"+imp)`
    /// (StackEditor.java 245-248).
    #[error("\"Convert Stack to Images\" requires a stack")]
    StackRequired,
    /// This unit does not implement Java's composite/hyperstack branch
    /// (StackEditor.java 262-270); flattening C/Z/T silently would be wrong.
    #[error("Stack to Images does not support hyperstacks or composite images")]
    UnsupportedHyperstack,
    /// The output document could not be built (invalid dimensions or plane
    /// lengths); unreachable for well-formed inputs.
    #[error("core error: {0}")]
    Core(#[from] crate::CoreError),
}

/// The result of `ImagesToStack.findMinMaxSize` (ImagesToStack.java 246-296)
/// plus the display range `convert` accumulates (ImagesToStack.java 118-124).
#[derive(Debug, Clone, PartialEq)]
pub struct ImagesToStackPlan {
    pub stack_type: StackType,
    /// Output width/height: the largest included image
    /// (ImagesToStack.java 272-275).
    pub width: u32,
    pub height: u32,
    /// Smallest included image by area (ImagesToStack.java 276-281).
    pub min_width: u32,
    pub min_height: u32,
    /// Largest included image by area (ImagesToStack.java 282-286).
    pub max_width: u32,
    pub max_height: u32,
    /// True when every included image uses an inverting LUT
    /// (ImagesToStack.java 264-266).
    pub all_inverted_luts: bool,
    /// `cal2`: the first 2D image's calibration, or `None` when any included
    /// image differs (ImagesToStack.java 259, 287-289).
    pub calibration: Option<Calibration>,
    /// Positions in the *eligible* 2D images, i.e. the `images` slice with the
    /// stacks removed (ImagesToStack.java 60-66), in window order. The caller
    /// must build its id list from the same filter before closing sources.
    pub included: Vec<usize>,
    /// Smallest `getMin()` over the included images (ImagesToStack.java
    /// 119-124).
    pub display_min: f64,
    /// Largest `getMax()` over the included images.
    pub display_max: f64,
}

impl ImagesToStackPlan {
    /// `sizesDiffer` (ImagesToStack.java 65).
    pub fn sizes_differ(&self) -> bool {
        self.width != self.min_width || self.height != self.min_height
    }

    /// Number of included images (`count` after filtering).
    pub fn count(&self) -> usize {
        self.included.len()
    }
}

/// The output of `ImagesToStack.convert` (ImagesToStack.java 118-253).
#[derive(Debug, Clone, PartialEq)]
pub struct ImagesToStackOutput {
    pub document: ImageDocument,
    /// Positions in the eligible 2D images of the images that were merged.
    /// Java closes exactly these when "Keep source images" is off
    /// (ImagesToStack.java 208-211).
    pub included: Vec<usize>,
}

/// `ImagePlus.getType()==ImagePlus.COLOR_256`: a byte image whose LUT is a
/// color LUT that is not pseudo-color, which is what `setTypeToColor256`
/// requires (ImagePlus.java 1422-1429).
///
/// Known limit: Java's `imageType` is sticky, so an image that was COLOR_256
/// when it was opened stays COLOR_256 even after its LUT is replaced or a
/// threshold is set; `setTypeToColor256`'s `!ip2.isThreshold()` guard only
/// applies at the moment the type is (re)computed. This model has no stored
/// type, so the predicate is recomputed from the plane. No reachable mismatch
/// was found for the conversion path (a thresholded color-LUT image is still
/// COLOR_256 in Java), so the predicate is left as-is rather than guessed at.
fn is_color256(plane: &PixelPlane) -> bool {
    match plane {
        PixelPlane::Color256 { lut, .. } => lut.is_color() && !crate::is_pseudo_color_lut(lut),
        _ => false,
    }
}

/// `ImageProcessor.isInvertedLut` for the plane's color model. Only indexed
/// planes carry a LUT in this model; every other processor uses ImageJ's
/// default grayscale LUT, which is not inverting.
fn plane_is_inverted_lut(plane: &PixelPlane) -> bool {
    match plane {
        PixelPlane::Color256 { lut, .. } => lut.is_inverted(),
        _ => false,
    }
}

/// `ImagesToStack.exclude` (ImagesToStack.java 298-300).
fn excluded(title: &str, filter: Option<&str>) -> bool {
    match filter {
        Some(filter) => !title.contains(filter),
        None => false,
    }
}

/// `ImagesToStack.findMinMaxSize` (ImagesToStack.java 246-296) followed by the
/// display-range accumulation of `convert` (ImagesToStack.java 119-124).
///
/// `images` must be every open window in `WindowManager.getIDList` order; the
/// "at least two open 2D images" check counts stacks too
/// (ImagesToStack.java 44-63).
pub fn images_to_stack_plan(
    images: &[&ImageDocument],
    options: &ImagesToStackOptions,
) -> Result<ImagesToStackPlan, StackConvertError> {
    let two_d: Vec<&ImageDocument> = images
        .iter()
        .copied()
        .filter(|doc| doc.stack_size() == 1)
        .collect();
    let stack_count = images.len() - two_d.len();
    if two_d.len() < 2 {
        return Err(if stack_count > 1 {
            StackConvertError::NeedsTwoImagesWithStacks
        } else {
            StackConvertError::NeedsTwoImages
        });
    }
    // ImagesToStack.java 101-102: "" and "*" both clear the filter.
    let filter = match options.title_filter.as_str() {
        "" | "*" => None,
        other => Some(other),
    };

    // ImagesToStack.java 247-258.
    let mut stack_type = 8i32;
    let mut width = 0u32;
    let mut height = 0u32;
    // cal2 starts from the first 2D image, even when the filter excludes it
    // (ImagesToStack.java 259).
    let mut calibration = Some(two_d[0].calibration.clone());
    let mut max_width = 0u32;
    let mut max_height = 0u32;
    let mut min_width = u32::MAX;
    let mut min_height = u32::MAX;
    let mut min_size = u32::MAX;
    let mut max_size = 0u32;
    let mut all_inverted_luts = true;
    let mut included = Vec::new();
    for (index, doc) in two_d.iter().enumerate() {
        if excluded(&doc.title, filter) {
            continue;
        }
        // ImagesToStack.java 262-264: COLOR_256 assigns rgb (33) rather than
        // taking a maximum, so it can lower a 32-bit stackType.
        if is_color256(doc.plane()) {
            stack_type = 33;
        }
        if !plane_is_inverted_lut(doc.plane()) {
            all_inverted_luts = false;
        }
        let mut bit_depth = i32::from(doc.plane().bit_depth());
        if bit_depth == 24 {
            bit_depth = 33;
        }
        if bit_depth > stack_type {
            stack_type = bit_depth;
        }
        let (w, h) = (doc.width, doc.height);
        if w > width {
            width = w;
        }
        if h > height {
            height = h;
        }
        let size = w.saturating_mul(h);
        if size < min_size {
            min_size = size;
            min_width = w;
            min_height = h;
        }
        if size > max_size {
            max_size = size;
            max_width = w;
            max_height = h;
        }
        // Calibration.equals compares pixel sizes, unit, value unit and
        // function (Calibration.java 475-486).
        if calibration
            .as_ref()
            .is_some_and(|cal| !same_calibration(cal, &doc.calibration))
        {
            calibration = None;
        }
        included.push(index);
    }
    if included.is_empty() {
        return Err(StackConvertError::NoMatchingTitles(
            options.title_filter.clone(),
        ));
    }

    // ImagesToStack.java 119-124: the display range spans the included
    // images' current processor ranges.
    let mut display_min = f64::MAX;
    let mut display_max = -f64::MAX;
    for &index in &included {
        let range = two_d[index].display_range;
        if range.min < display_min {
            display_min = range.min;
        }
        if range.max > display_max {
            display_max = range.max;
        }
    }

    Ok(ImagesToStackPlan {
        stack_type: match stack_type {
            16 => StackType::Short,
            32 => StackType::Float,
            33 => StackType::Rgb,
            _ => StackType::Byte,
        },
        width,
        height,
        min_width,
        min_height,
        max_width,
        max_height,
        all_inverted_luts,
        calibration,
        included,
        display_min,
        display_max,
    })
}

/// `Calibration.equals` (Calibration.java 475-486) restricted to the fields
/// this model stores.
fn same_calibration(a: &Calibration, b: &Calibration) -> bool {
    a.pixel_width == b.pixel_width
        && a.pixel_height == b.pixel_height
        && a.pixel_depth == b.pixel_depth
        && a.unit == b.unit
}

/// `ImageProcessor.convertToShort(false)` (TypeConverter.java 142-166).
fn to_short(plane: &PixelPlane) -> PixelPlane {
    match plane {
        // convertByteToShort keeps the byte values and the color model.
        PixelPlane::Gray8(pixels) | PixelPlane::Color256 { pixels, .. } => {
            PixelPlane::Gray16(pixels.iter().map(|&v| u16::from(v)).collect())
        }
        PixelPlane::Gray16(pixels) => PixelPlane::Gray16(pixels.clone()),
        // convertFloatToShort without scaling clamps to [0,65535] and rounds
        // with +0.5 (TypeConverter.java 168-190).
        PixelPlane::Gray32(pixels) => PixelPlane::Gray16(
            pixels
                .iter()
                .map(|&v| {
                    let value = f64::from(v).clamp(0.0, 65535.0);
                    (value + 0.5) as i32 as u16
                })
                .collect(),
        ),
        // convertRGBToByte then convertByteToShort (TypeConverter.java
        // 95-117, 159-166).
        PixelPlane::ColorRgb(pixels) => PixelPlane::Gray16(
            pixels
                .iter()
                .map(|&pixel| u16::from(crate::rgb_to_byte(pixel)))
                .collect(),
        ),
    }
}

/// `ImageProcessor.convertToFloat()` (TypeConverter.java 192-246).
fn to_float(plane: &PixelPlane) -> PixelPlane {
    match plane {
        PixelPlane::Gray8(pixels) | PixelPlane::Color256 { pixels, .. } => {
            PixelPlane::Gray32(pixels.iter().map(|&v| f32::from(v)).collect())
        }
        PixelPlane::Gray16(pixels) => {
            PixelPlane::Gray32(pixels.iter().map(|&v| f32::from(v)).collect())
        }
        PixelPlane::Gray32(pixels) => PixelPlane::Gray32(pixels.clone()),
        PixelPlane::ColorRgb(pixels) => PixelPlane::Gray32(
            pixels
                .iter()
                .map(|&pixel| crate::rgb_to_float(pixel))
                .collect(),
        ),
    }
}

/// `ImageProcessor.convertToRGB()` (TypeConverter.java 248-255): convert to
/// the displayed 8-bit image, then read it back through its color model.
fn to_rgb(plane: &PixelPlane, range: DisplayRange) -> PixelPlane {
    crate::plane_to_displayed_rgb(plane, range, false)
}

/// `ImageProcessor.invert()` for the converted processor
/// (ByteProcessor.java 1194-1196, ShortProcessor.java 582-598,
/// FloatProcessor.java 524-526, ColorProcessor.java 1386-1388).
///
/// The 16/32-bit forms invert about the processor's own min/max, which for a
/// freshly converted processor is the actual pixel range.
fn invert_plane(plane: &PixelPlane) -> PixelPlane {
    match plane {
        PixelPlane::Gray8(pixels) | PixelPlane::Color256 { pixels, .. } => {
            let inverted: Vec<u8> = pixels.iter().map(|&v| 255 - v).collect();
            match plane {
                PixelPlane::Color256 { lut, .. } => PixelPlane::Color256 {
                    pixels: inverted,
                    lut: lut.clone(),
                },
                _ => PixelPlane::Gray8(inverted),
            }
        }
        PixelPlane::Gray16(pixels) => {
            let (min, max) = match (pixels.iter().min(), pixels.iter().max()) {
                (Some(&min), Some(&max)) => (i64::from(min), i64::from(max)),
                _ => (0, 0),
            };
            PixelPlane::Gray16(
                pixels
                    .iter()
                    .map(|&v| (max - (i64::from(v) - min)) as u16)
                    .collect(),
            )
        }
        PixelPlane::Gray32(pixels) => {
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
            PixelPlane::Gray32(pixels.iter().map(|&v| max - (v - min)).collect())
        }
        PixelPlane::ColorRgb(pixels) => PixelPlane::ColorRgb(
            pixels
                .iter()
                .map(|&pixel| {
                    let r = 255 - ((pixel >> 16) & 0xff);
                    let g = 255 - ((pixel >> 8) & 0xff);
                    let b = 255 - (pixel & 0xff);
                    (pixel & 0xff00_0000) | (r << 16) | (g << 8) | b
                })
                .collect(),
        ),
    }
}

/// The typed fill value of a fresh processor of `stack_type`
/// (`ImageProcessor.setColor` + `fill`), plus the display range that
/// `setColor` installs for an all-zero 16/32-bit processor
/// (ShortProcessor.java 987-999, FloatProcessor.java 1020-1030).
fn fill_value(stack_type: StackType, color: [u8; 3]) -> (FillValue, Option<DisplayRange>) {
    match stack_type {
        StackType::Byte => (FillValue::Byte(best_gray_index(color)), None),
        StackType::Short => {
            let (value, range) = gray16_value(color, DisplayRange { min: 0.0, max: 0.0 });
            (FillValue::Short(value as u16), range)
        }
        StackType::Float => {
            let (value, range) = gray32_value(color, DisplayRange { min: 0.0, max: 0.0 });
            (FillValue::Float(value), range)
        }
        StackType::Rgb => (FillValue::Rgb(rgb_value(color)), None),
    }
}

#[derive(Debug, Clone, Copy)]
enum FillValue {
    Byte(u8),
    Short(u16),
    Float(f32),
    Rgb(u32),
}

/// A fresh processor of `stack_type` filled with `fill`, matching
/// `new ByteProcessor(width, height)` + `setColor` + `fill`
/// (ImagesToStack.java 174-178).
fn filled_plane(
    stack_type: StackType,
    width: u32,
    height: u32,
    fill: Option<[u8; 3]>,
) -> (PixelPlane, Option<DisplayRange>) {
    let len = (width as usize) * (height as usize);
    let Some(color) = fill else {
        return (
            match stack_type {
                StackType::Byte => PixelPlane::Gray8(vec![0; len]),
                StackType::Short => PixelPlane::Gray16(vec![0; len]),
                StackType::Float => PixelPlane::Gray32(vec![0.0; len]),
                StackType::Rgb => PixelPlane::ColorRgb(vec![0; len]),
            },
            None,
        );
    };
    let (value, range) = fill_value(stack_type, color);
    let plane = match value {
        FillValue::Byte(v) => PixelPlane::Gray8(vec![v; len]),
        FillValue::Short(v) => PixelPlane::Gray16(vec![v; len]),
        FillValue::Float(v) => PixelPlane::Gray32(vec![v; len]),
        FillValue::Rgb(v) => PixelPlane::ColorRgb(vec![v; len]),
    };
    (plane, range)
}

/// `ImageProcessor.insert(ip, xoff, yoff)` for a source that fits inside the
/// destination (Blitter.COPY).
fn insert_plane(
    destination: &mut PixelPlane,
    source: &PixelPlane,
    dst_width: u32,
    src_width: u32,
    src_height: u32,
    x_offset: u32,
    y_offset: u32,
) {
    let dst_width = dst_width as usize;
    let src_width = src_width as usize;
    let src_height = src_height as usize;
    let x_offset = x_offset as usize;
    let y_offset = y_offset as usize;
    match (destination, source) {
        (PixelPlane::Gray8(dst), PixelPlane::Gray8(src))
        | (PixelPlane::Gray8(dst), PixelPlane::Color256 { pixels: src, .. })
        | (PixelPlane::Color256 { pixels: dst, .. }, PixelPlane::Gray8(src))
        | (PixelPlane::Color256 { pixels: dst, .. }, PixelPlane::Color256 { pixels: src, .. }) => {
            for y in 0..src_height {
                let start = (y + y_offset) * dst_width + x_offset;
                dst[start..start + src_width]
                    .copy_from_slice(&src[y * src_width..(y + 1) * src_width]);
            }
        }
        (PixelPlane::Gray16(dst), PixelPlane::Gray16(src)) => {
            for y in 0..src_height {
                let start = (y + y_offset) * dst_width + x_offset;
                dst[start..start + src_width]
                    .copy_from_slice(&src[y * src_width..(y + 1) * src_width]);
            }
        }
        (PixelPlane::Gray32(dst), PixelPlane::Gray32(src)) => {
            for y in 0..src_height {
                let start = (y + y_offset) * dst_width + x_offset;
                dst[start..start + src_width]
                    .copy_from_slice(&src[y * src_width..(y + 1) * src_width]);
            }
        }
        (PixelPlane::ColorRgb(dst), PixelPlane::ColorRgb(src)) => {
            for y in 0..src_height {
                let start = (y + y_offset) * dst_width + x_offset;
                dst[start..start + src_width]
                    .copy_from_slice(&src[y * src_width..(y + 1) * src_width]);
            }
        }
        _ => unreachable!("insert_plane requires matching plane types"),
    }
}

/// `ImagesToStack.convert` (ImagesToStack.java 118-253).
///
/// `images` must be the same window-ordered slice passed to
/// [`images_to_stack_plan`]. `title` is the output window title; the caller
/// owns `WindowManager` uniquification.
pub fn images_to_stack(
    images: &[&ImageDocument],
    options: &ImagesToStackOptions,
    title: impl Into<String>,
) -> Result<ImagesToStackOutput, StackConvertError> {
    let plan = images_to_stack_plan(images, options)?;
    let fill_color = decode_fill_color(&options.fill);
    let two_d: Vec<&ImageDocument> = images
        .iter()
        .copied()
        .filter(|doc| doc.stack_size() == 1)
        .collect();
    // ImagesToStack.java 114-121: the scale methods resize the output to the
    // smallest or largest included image.
    let (width, height) = match options.method {
        ImagesToStackMethod::ScaleSmallest => (plan.min_width, plan.min_height),
        ImagesToStackMethod::ScaleLargest => (plan.max_width, plan.max_height),
        _ => (plan.width, plan.height),
    };

    let mut slices = Vec::with_capacity(plan.included.len());
    let mut labels = Vec::with_capacity(plan.included.len());
    let mut first_display_range = None;
    for (position, &index) in plan.included.iter().enumerate() {
        let source = two_d[index];
        let inverted_lut = plane_is_inverted_lut(source.plane());
        // ImagesToStack.java 137-141: the title is the label when "Use titles
        // as labels" is on; otherwise `ImagePlus.getProp("Slice_Label")` is the
        // fallback, distinct from the label on a one-slice stack.
        let label = if options.titles_as_labels {
            Some(source.title.clone())
        } else {
            source.slice_label_property.clone()
        };
        let mut plane = match plan.stack_type {
            StackType::Byte => source.plane().clone(),
            StackType::Short => to_short(source.plane()),
            StackType::Float => to_float(source.plane()),
            StackType::Rgb => to_rgb(source.plane(), source.display_range),
        };
        // ImagesToStack.java 148-152.
        if inverted_lut && !plan.all_inverted_luts {
            plane = invert_plane(&plane);
        }
        if source.width != width || source.height != height {
            match options.method {
                ImagesToStackMethod::CopyCenter | ImagesToStackMethod::CopyTopLeft => {
                    let (mut destination, range) =
                        filled_plane(plan.stack_type, width, height, fill_color);
                    let (x_offset, y_offset) = if options.method == ImagesToStackMethod::CopyCenter
                    {
                        ((width - source.width) / 2, (height - source.height) / 2)
                    } else {
                        (0, 0)
                    };
                    insert_plane(
                        &mut destination,
                        &plane,
                        width,
                        source.width,
                        source.height,
                        x_offset,
                        y_offset,
                    );
                    plane = destination;
                    // The 16/32-bit output range is set from the inputs below,
                    // overriding a range installed by the fill operation.
                    let _ = range;
                }
                ImagesToStackMethod::ScaleSmallest | ImagesToStackMethod::ScaleLargest => {
                    let interpolation = if options.bicubic {
                        Interpolation::Bicubic
                    } else {
                        Interpolation::Bilinear
                    };
                    plane = resize_plane(
                        &plane,
                        source.width as usize,
                        source.height as usize,
                        (0, 0, source.width, source.height),
                        width as usize,
                        height as usize,
                        interpolation,
                        false,
                    );
                }
            }
        }
        if position == 0 {
            // ImageStack.addSlice stores the first slice's processor range
            // (ImageStack.java 116-119).
            first_display_range = Some(
                if plan.stack_type == StackType::Byte
                    && (source.width != width || source.height != height)
                    && matches!(
                        options.method,
                        ImagesToStackMethod::CopyCenter | ImagesToStackMethod::CopyTopLeft
                    )
                {
                    // The copy branch uses new ByteProcessor, whose limits are
                    // 0..255 (ImagesToStack.java 186); resizing preserves them.
                    DisplayRange {
                        min: 0.0,
                        max: 255.0,
                    }
                } else {
                    source.display_range
                },
            );
        }
        slices.push(plane);
        labels.push(label);
    }

    // ImageStack.addSlice takes the first output processor's color model,
    // including a fresh ByteProcessor's grayscale LUT after a padded copy
    // (ImageStack.java 116-119). ImagesToStack clears it when the first
    // source LUT is inverting but not every input is
    // (ImagesToStack.java 205-207). Every byte slice then shares that model.
    if plan.stack_type == StackType::Byte {
        let reset_lut =
            plane_is_inverted_lut(two_d[plan.included[0]].plane()) && !plan.all_inverted_luts;
        let kept_lut = match &slices[0] {
            PixelPlane::Color256 { lut, .. } if !reset_lut => Some(lut.clone()),
            _ => None,
        };
        for plane in &mut slices {
            match (&kept_lut, &*plane) {
                (Some(lut), PixelPlane::Gray8(pixels)) => {
                    *plane = PixelPlane::Color256 {
                        pixels: pixels.clone(),
                        lut: lut.clone(),
                    };
                }
                (None, PixelPlane::Color256 { pixels, .. }) => {
                    *plane = PixelPlane::Gray8(pixels.clone());
                }
                _ => {}
            }
        }
    }

    let mut doc = ImageDocument::new_stack(title, width, height, slices)?;
    doc.slice_labels = labels;
    doc.calibration = plan
        .calibration
        .clone()
        .unwrap_or_else(Calibration::default);
    doc.display_range = match plan.stack_type {
        // ImageStack.getProcessor does not restore limits on ColorProcessor
        // (ImageStack.java 330), whose fresh range is always 0..255.
        StackType::Rgb => DisplayRange {
            min: 0.0,
            max: 255.0,
        },
        // ImagesToStack.java 214-215: only 16/32-bit output gets the global
        // range; 8-bit keeps the first slice's range.
        StackType::Short | StackType::Float => {
            if plan.display_min == 0.0 && plan.display_max == 0.0 {
                // setMinAndMax(0,0) resets to the first slice's actual range
                // (ShortProcessor.java 267-277, FloatProcessor.java 123-133).
                actual_range(&doc.slices[0])
            } else {
                DisplayRange {
                    min: plan.display_min,
                    max: plan.display_max,
                }
            }
        }
        StackType::Byte => first_display_range.unwrap_or(doc.display_range),
    };
    Ok(ImagesToStackOutput {
        document: doc,
        included: plan.included,
    })
}

/// `findMinAndMax` for a freshly built 16/32-bit processor.
fn actual_range(plane: &PixelPlane) -> DisplayRange {
    match plane {
        PixelPlane::Gray16(pixels) => {
            let mut min = u16::MAX;
            let mut max = u16::MIN;
            for &value in pixels {
                min = min.min(value);
                max = max.max(value);
            }
            DisplayRange {
                min: f64::from(min),
                max: f64::from(max),
            }
        }
        PixelPlane::Gray32(pixels) => {
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
                min: f64::from(min),
                max: f64::from(max),
            }
        }
        _ => DisplayRange {
            min: 0.0,
            max: 255.0,
        },
    }
}

/// `ImageStack.getShortSliceLabel(n)` (ImageStack.java 282-295): cut at the
/// first newline, drop a four-character extension, cap at 60 characters.
fn short_slice_label(label: Option<&str>) -> Option<String> {
    let label = label?;
    let chars: Vec<char> = label.chars().collect();
    let newline = chars.iter().position(|&c| c == '\n');
    if newline == Some(0) {
        return None;
    }
    let mut short: Vec<char> = match newline {
        Some(index) => chars[..index].to_vec(),
        None => chars,
    };
    let len = short.len();
    if len > 4 && short[len - 4] == '.' && !short[len - 1].is_ascii_digit() {
        short.truncate(len - 4);
    }
    if short.len() > 60 {
        short.truncate(60);
    }
    Some(short.into_iter().collect())
}

/// `StackEditor.getShortTitle` (StackEditor.java 319-328): cut at the first
/// space, then drop the extension at the last dot.
fn short_title(title: &str) -> String {
    let mut short = title;
    if let Some(index) = short.find(' ') {
        short = &short[..index];
    }
    if let Some(index) = short.rfind('.') {
        if index > 0 {
            short = &short[..index];
        }
    }
    short.to_string()
}

/// `StackEditor.getTitle` (StackEditor.java 312-315): `shortTitle-000n`.
/// Java pads `n` to at least eight digits and keeps the last four characters,
/// so the suffix wraps at 10000 rather than widening.
fn generated_title(source: &ImageDocument, n: usize) -> String {
    format!("{}-{:04}", short_title(&source.title), n % 10000)
}

/// `StackEditor.convertStackToImages` (StackEditor.java 240-310) for a plain
/// stack. Every slice becomes a single-slice document with the stack's
/// calibration and display range; the caller closes the source and owns
/// window-title uniquification.
pub fn stack_to_images(source: &ImageDocument) -> Result<Vec<ImageDocument>, StackConvertError> {
    if source.stack_size() < 2 {
        return Err(StackConvertError::StackRequired);
    }
    if source.is_hyperstack() {
        return Err(StackConvertError::UnsupportedHyperstack);
    }
    let mut images = Vec::with_capacity(source.slices.len());
    for (index, plane) in source.slices.iter().enumerate() {
        let n = index + 1;
        // StackEditor.java 269-272: a label containing a path separator is
        // rejected and the generated title is used instead.
        let label = short_slice_label(source.slice_labels.get(index).and_then(|l| l.as_deref()))
            .filter(|label| !label.contains('/') && !label.contains('\\') && !label.contains(':'));
        let title = match label {
            Some(label) if !label.is_empty() => label,
            _ => generated_title(source, n),
        };
        let mut doc = ImageDocument::new(title, source.width, source.height, plane.clone())?;
        // StackEditor.java 274-275: the stack calibration is copied.
        doc.calibration = source.calibration.clone();
        // ImageStack.getProcessor applies the stack's stored range, which
        // ImagePlus.getStack refreshes from the current processor
        // (ImageStack.java 330-331, ImagePlus.java 1791-1797).
        doc.display_range = source.display_range;
        images.push(doc);
    }
    Ok(images)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `StackEditor.getTitle` (StackEditor.java 312-315) pads `n` to at least
    /// eight digits and then takes the *last four* characters, so the suffix
    /// wraps at 10000 instead of widening. A source-based boundary test keeps
    /// this cheap: opening 10000 windows would be the only way to reach it
    /// through `stack_to_images`.
    #[test]
    fn generated_title_takes_the_last_four_digits() {
        let source = ImageDocument::new("blobs.tif", 1, 1, PixelPlane::Gray8(vec![0])).unwrap();
        assert_eq!(generated_title(&source, 1), "blobs-0001");
        assert_eq!(generated_title(&source, 999), "blobs-0999");
        assert_eq!(generated_title(&source, 9999), "blobs-9999");
        assert_eq!(generated_title(&source, 10000), "blobs-0000");
        assert_eq!(generated_title(&source, 12345), "blobs-2345");
        assert_eq!(generated_title(&source, 100000000), "blobs-0000");
    }
}
