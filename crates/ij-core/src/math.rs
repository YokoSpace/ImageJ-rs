//! Process > Math, following ImageMath and the four ImageProcessor implementations.

use crate::{CoreError, DisplayRange, ImageDocument, PixelPlane, float_processor_range};
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum MathOperation {
    Add(f64),
    Subtract(f64),
    Multiply(f64),
    Divide(f64),
    Min(f64),
    Max(f64),
    Gamma(f64),
    Log,
    Exp,
    Square,
    SquareRoot,
    Reciprocal,
    Abs,
    Set(f64),
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum CalculatorOperation {
    Add,
    Subtract,
    Multiply,
    Divide,
    And,
    Or,
    Xor,
    Min,
    Max,
    Average,
    Difference,
    Copy,
    TransparentZero,
}

impl CalculatorOperation {
    /// ImageCalculator.java 21: the dialog choice labels and the first
    /// `imageCalculator` option string.
    pub fn java_name(self) -> &'static str {
        match self {
            Self::Add => "Add",
            Self::Subtract => "Subtract",
            Self::Multiply => "Multiply",
            Self::Divide => "Divide",
            Self::And => "AND",
            Self::Or => "OR",
            Self::Xor => "XOR",
            Self::Min => "Min",
            Self::Max => "Max",
            Self::Average => "Average",
            Self::Difference => "Difference",
            Self::Copy => "Copy",
            Self::TransparentZero => "Transparent-zero",
        }
    }
}

impl MathOperation {
    pub fn name(self) -> &'static str {
        match self {
            Self::Add(_) => "Add",
            Self::Subtract(_) => "Subtract",
            Self::Multiply(_) => "Multiply",
            Self::Divide(_) => "Divide",
            Self::Min(_) => "Min",
            Self::Max(_) => "Max",
            Self::Gamma(_) => "Gamma",
            Self::Log => "Log",
            Self::Exp => "Exp",
            Self::Square => "Square",
            Self::SquareRoot => "Square Root",
            Self::Reciprocal => "Reciprocal",
            Self::Abs => "Abs",
            Self::Set(_) => "Set",
        }
    }

    pub fn value(self) -> Option<f64> {
        match self {
            Self::Add(v)
            | Self::Subtract(v)
            | Self::Multiply(v)
            | Self::Divide(v)
            | Self::Min(v)
            | Self::Max(v)
            | Self::Gamma(v)
            | Self::Set(v) => Some(v),
            _ => None,
        }
    }

    pub fn with_value(self, value: f64) -> Self {
        match self {
            Self::Add(_) => Self::Add(value),
            Self::Subtract(_) => Self::Subtract(value),
            Self::Multiply(_) => Self::Multiply(value),
            Self::Divide(_) => Self::Divide(value),
            Self::Min(_) => Self::Min(value),
            Self::Max(_) => Self::Max(value),
            Self::Gamma(_) => Self::Gamma(value),
            Self::Set(_) => Self::Set(value),
            _ => self,
        }
    }

    pub fn validate(self) -> Result<(), CoreError> {
        if matches!(self, Self::Gamma(v) if v < 0.05 || v > 5.0) {
            return Err(CoreError::InvalidOperation(
                "Gamma must be between 0.05 and 5.0",
            ));
        }
        Ok(())
    }
}

impl ImageDocument {
    /// A complete-stack filter discards Undo; a current-slice filter snapshots
    /// pixels and display bounds (PlugInFilterRunner.java 107-129).
    pub fn apply_math(
        &mut self,
        operation: MathOperation,
        whole_stack: bool,
    ) -> Result<(), CoreError> {
        operation.validate()?;
        let stack = whole_stack && self.stack_size() > 1;
        let (x, y, w, h, mask) = match self.roi.as_ref().filter(|roi| roi.is_area()) {
            Some(roi) => {
                let (x, y, w, h) = roi
                    .clipped_bounds(self.width, self.height)
                    .unwrap_or((0, 0, 0, 0));
                (x, y, w, h, roi.raster_mask(self.width, self.height)?)
            }
            None => (0, 0, self.width, self.height, None),
        };
        let processor_range = match self.plane() {
            PixelPlane::Gray16(pixels) => {
                // ShortProcessor.setMinAndMax rounds both bounds; 0/0 resets them (267-275).
                if self.display_range.min == 0.0 && self.display_range.max == 0.0 {
                    DisplayRange {
                        min: pixels.iter().copied().min().unwrap_or(0) as f64,
                        max: pixels.iter().copied().max().unwrap_or(0) as f64,
                    }
                } else {
                    DisplayRange {
                        min: round_to_i32(self.display_range.min) as f64,
                        max: round_to_i32(self.display_range.max) as f64,
                    }
                }
            }
            _ => self.display_range,
        };
        if matches!(self.plane(), PixelPlane::Gray16(_)) {
            self.display_range = processor_range;
        }
        if stack {
            self.snapshot = None;
            self.snapshot_dimensions = None;
            self.snapshot_display_range = None;
        } else {
            self.save_snapshot();
            self.snapshot_display_range = Some(self.display_range);
        }
        let current = self.current_slice_index();
        for index in 0..self.slices.len() {
            if !whole_stack && index != current {
                continue;
            }
            let plane = &mut self.slices[index];
            let original = mask.as_ref().map(|_| plane.clone());
            for py in y..y + h {
                for px in x..x + w {
                    let i = (py * self.width + px) as usize;
                    match plane {
                        PixelPlane::Gray8(v) | PixelPlane::Color256 { pixels: v, .. } => {
                            v[i] = integer_math(v[i] as i32, operation, self.display_range, false)
                                as u8;
                        }
                        PixelPlane::Gray16(v) => {
                            v[i] =
                                integer_math(v[i] as i32, operation, processor_range, true) as u16;
                        }
                        PixelPlane::Gray32(v) => v[i] = float_math(v[i], operation),
                        PixelPlane::ColorRgb(v) => {
                            if let MathOperation::Set(value) = operation {
                                v[i] = value as i32 as u32;
                            } else {
                                let r = integer_math(
                                    ((v[i] >> 16) & 255) as i32,
                                    operation,
                                    self.display_range,
                                    false,
                                ) as u32;
                                let g = integer_math(
                                    ((v[i] >> 8) & 255) as i32,
                                    operation,
                                    self.display_range,
                                    false,
                                ) as u32;
                                let b = integer_math(
                                    (v[i] & 255) as i32,
                                    operation,
                                    self.display_range,
                                    false,
                                ) as u32;
                                v[i] = 0xff00_0000 | (r << 16) | (g << 8) | b;
                            }
                        }
                    }
                }
            }
            // ImageMath resets bounds before PlugInFilterRunner restores the mask.
            if !stack
                && (matches!(operation, MathOperation::Min(_) | MathOperation::Max(_))
                    || matches!(
                        (operation, &*plane),
                        (MathOperation::Abs, PixelPlane::Gray32(_))
                    ))
            {
                match &*plane {
                    PixelPlane::Gray16(v) => {
                        self.display_range = DisplayRange {
                            min: v.iter().copied().min().unwrap_or(0) as f64,
                            max: v.iter().copied().max().unwrap_or(0) as f64,
                        };
                        self.threshold = None;
                    }
                    PixelPlane::Gray32(v) => {
                        self.display_range = float_processor_range(v);
                        self.threshold = None;
                    }
                    _ => {}
                }
            }
            if let (Some(mask), Some(original)) = (&mask, original) {
                for py in y..y + h {
                    for px in x..x + w {
                        if mask.contains(px, py) {
                            continue;
                        }
                        let i = (py * self.width + px) as usize;
                        match (&mut *plane, &original) {
                            (PixelPlane::Gray8(a), PixelPlane::Gray8(b)) => a[i] = b[i],
                            (
                                PixelPlane::Color256 { pixels: a, .. },
                                PixelPlane::Color256 { pixels: b, .. },
                            ) => a[i] = b[i],
                            (PixelPlane::Gray16(a), PixelPlane::Gray16(b)) => a[i] = b[i],
                            (PixelPlane::Gray32(a), PixelPlane::Gray32(b)) => a[i] = b[i],
                            (PixelPlane::ColorRgb(a), PixelPlane::ColorRgb(b)) => a[i] = b[i],
                            _ => unreachable!(),
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

// ImageProcessor.java 1034-1100 and ShortProcessor.java 486-579 differ in
// gamma/log/exp scaling. Math.round narrows through long, while SQR casts directly.
fn integer_math(v: i32, operation: MathOperation, range: DisplayRange, short: bool) -> i32 {
    use MathOperation::*;
    let n = v as f64;
    let max = if short {
        range.max as i32 as f64
    } else {
        255.0
    };
    let result = match operation {
        Add(c) => v.wrapping_add(c as i32),
        Subtract(c) => v.wrapping_add(-c as i32),
        Multiply(c) => round_to_i32(n * c),
        Divide(0.0) => v,
        Divide(c) => round_to_i32(n * (1.0 / c)),
        Min(c) => {
            if n < c {
                c as i32
            } else {
                v
            }
        }
        Max(c) => {
            if n > c {
                c as i32
            } else {
                v
            }
        }
        Gamma(c) if short => {
            let min = range.min as i32 as f64;
            let span = range.max - range.min;
            if span <= 0.0 || n == min {
                v
            } else {
                round_to_i32((c * ((n - min) / span).ln()).exp() * span + min)
            }
        }
        Gamma(c) => round_to_i32(((n / 255.0).ln() * c).exp() * 255.0),
        Log => {
            if v <= 0 {
                0
            } else {
                round_to_i32(n.ln() * (max / max.ln()))
            }
        }
        Exp if short => round_to_i32((n * (max.ln() / max)).exp()),
        Exp => round_to_i32((n / (255.0 / 255.0_f64.ln())).exp()),
        Square => (n * n) as i32,
        SquareRoot => round_to_i32(n.sqrt()),
        Set(c) => c as i32,
        Reciprocal | Abs => v,
    };
    result.clamp(0, if short { 65535 } else { 255 })
}

// FloatProcessor.java 445-510 first casts the operand to float; gamma/log/exp
// alone use double intermediates. ImageMath.java 121-129 maps reciprocal(0) to NaN.
fn float_math(v: f32, operation: MathOperation) -> f32 {
    use MathOperation::*;
    match operation {
        Add(c) => v + c as f32,
        Subtract(c) => v + (-c) as f32,
        Multiply(c) => v * c as f32,
        Divide(c) => v * (1.0 / c) as f32,
        Min(c) => {
            if (v as f64) < c {
                c as f32
            } else {
                v
            }
        }
        Max(c) => {
            if (v as f64) > c {
                c as f32
            } else {
                v
            }
        }
        Gamma(c) => {
            if v <= 0.0 {
                0.0
            } else {
                ((c as f32 as f64) * (v as f64).ln()).exp() as f32
            }
        }
        Log => (v as f64).ln() as f32,
        Exp => (v as f64).exp() as f32,
        Square => v * v,
        SquareRoot => {
            if v <= 0.0 {
                0.0
            } else {
                (v as f64).sqrt() as f32
            }
        }
        Reciprocal => {
            if v == 0.0 {
                f32::NAN
            } else {
                1.0 / v
            }
        }
        Abs => v.abs(),
        Set(c) => c as f32,
    }
}

// Math.round(double) rounds halfway toward +infinity. Adding 0.5 first loses
// the distinction between 0.5 and its preceding representable double.
fn round_to_i32(value: f64) -> i32 {
    let floor = value.floor();
    let rounded = if value - floor >= 0.5 {
        (floor as i64).saturating_add(1)
    } else {
        floor as i64
    };
    rounded as i32
}

// ---------------------------------------------------------------------------
// Process > Image Calculator... (ImageCalculator.java 19-220)
//
// The calculator delegates all arithmetic to ImageProcessor.copyBits, i.e. the
// Byte/Short/Float/Color Blitter classes. A processor ROI never enters the
// blitters, so every operation covers the whole (0,0)-anchored overlap of the
// two images.
// ---------------------------------------------------------------------------

impl ImageDocument {
    /// `ImageCalculator.calculate(img1, img2, apiCall=true)`
    /// (ImageCalculator.java 144-160). Returns the new "Result of ..." document
    /// when the effective options create a window, and mutates `self` in place
    /// otherwise. `self` is image1 (the destination), `other` is image2 (the
    /// source).
    pub fn image_calculate(
        &mut self,
        other: &ImageDocument,
        operation: CalculatorOperation,
        create_window: bool,
        float_result: bool,
        process_stack: bool,
    ) -> Result<Option<ImageDocument>, CoreError> {
        // ImageCalculator.java 148-149: a float result always creates a window
        // unless both inputs are already 32-bit.
        let both_gray32 = matches!(self.plane(), PixelPlane::Gray32(_))
            && matches!(other.plane(), PixelPlane::Gray32(_));
        let create_window = create_window || (float_result && !both_gray32);
        let size1 = self.stack_size();
        let size2 = other.stack_size();
        // ImageCalculator.java 152-156.
        if process_stack && (size1 > 1 || size2 > 1) {
            self.calculator_stack_operation(other, operation, create_window, float_result)
        } else {
            self.calculator_single_operation(other, operation, create_window, float_result)
        }
    }

    /// `ImageCalculator.doOperation` (238-276): the current-slice pair.
    fn calculator_single_operation(
        &mut self,
        other: &ImageDocument,
        operation: CalculatorOperation,
        create_window: bool,
        float_result: bool,
    ) -> Result<Option<ImageDocument>, CoreError> {
        let (src_w, src_h) = (other.width, other.height);
        let src_display = other.display_range;
        let src_slice = other.plane().clone();
        if create_window {
            // createNewImage (278-288): a blank processor of image1's type at
            // the min dimensions, then ip1 is inserted at (0,0).
            let width = self.width.min(src_w);
            let height = self.height.min(src_h);
            let mut plane = crop_plane(self.plane(), self.width, width, height);
            let is_color = matches!(plane, PixelPlane::ColorRgb(_));
            if float_result && !is_color {
                plane = plane_to_gray32(&plane);
            }
            let temp_display = blit_plane(
                &mut plane,
                width,
                height,
                &src_slice,
                src_w,
                src_h,
                src_display,
                operation,
            );
            // ImageCalculator.java 266-267: with an RGB source ("rgb") the
            // finished blend is converted to a 32-bit weighted-gray result.
            if float_result && matches!(src_slice, PixelPlane::ColorRgb(_)) {
                plane = rgb_plane_to_gray32(&plane);
            }
            let display_range = temp_display.unwrap_or_else(|| plane_display_range(&plane));
            let mut result = ImageDocument::new_stack(
                format!("Result of {}", self.title),
                width,
                height,
                vec![plane],
            )?;
            result.calibration = self.calibration.clone();
            result.display_range = display_range;
            return Ok(Some(result));
        }
        // In-place path: processor snapshot and FILTER undo (253-254).
        self.save_snapshot();
        self.snapshot_display_range = Some(self.display_range);
        let index = self.current_slice_index();
        let (width, height) = (self.width, self.height);
        let temp_display = blit_plane(
            &mut self.slices[index],
            width,
            height,
            &src_slice,
            src_w,
            src_h,
            src_display,
            operation,
        );
        // doOperation (268-269): every non-ByteProcessor result resets its
        // display bounds (and with them the threshold, ShortProcessor.java
        // 282-286). ByteProcessor keeps its range unless copyBits' temporary
        // float path stored the float bounds (ByteProcessor.java 1268-1277).
        self.calculator_apply_display(index, temp_display);
        Ok(None)
    }

    /// `ImageCalculator.doStackOperation` (194-236).
    fn calculator_stack_operation(
        &mut self,
        other: &ImageDocument,
        operation: CalculatorOperation,
        create_window: bool,
        float_result: bool,
    ) -> Result<Option<ImageDocument>, CoreError> {
        let size1 = self.stack_size();
        let size2 = other.stack_size();
        // ImageCalculator.java 198-201 error dialog text.
        if size1 > 1 && size2 > 1 && size1 != size2 {
            return Err(CoreError::InvalidOperation(
                "'Image1' and 'image2' must be stacks with the same\nnumber of slices, or 'image2' must be a single image.",
            ));
        }
        let src_display = other.display_range;
        let src_current = other.plane().clone();
        let src_slices = &other.slices;
        if create_window {
            // duplicateStack (310-347): image1's stack copied slice by slice
            // (ROI-free crop), converted to float for a 32-bit result.
            let converted: Vec<PixelPlane> = self
                .slices
                .iter()
                .map(|plane| {
                    if float_result && !matches!(plane, PixelPlane::ColorRgb(_)) {
                        plane_to_gray32(plane)
                    } else {
                        plane.clone()
                    }
                })
                .collect();
            let mut result = ImageDocument::new_stack(
                format!("Result of {}", self.title),
                self.width,
                self.height,
                converted,
            )?;
            result.calibration = self.calibration.clone();
            result.slice_labels = self.slice_labels.clone();
            // duplicateStack 336-338 keeps image1's hyperstack dimensions.
            (result.n_channels, result.n_slices, result.n_frames) =
                (self.n_channels, self.n_slices, self.n_frames);
            for index in 0..result.slices.len() {
                // StackProcessor.copyBits (194-211): image2's slice i, or its
                // single processor for a single-image source.
                let src = if size2 == 1 {
                    &src_current
                } else {
                    &src_slices[index]
                };
                blit_plane(
                    &mut result.slices[index],
                    result.width,
                    result.height,
                    src,
                    other.width,
                    other.height,
                    src_display,
                    operation,
                );
            }
            result.display_range = plane_display_range(result.plane());
            Ok(Some(result))
        } else {
            // Undo.reset() (216): whole-stack edits have no undo.
            self.snapshot = None;
            self.snapshot_display_range = None;
            for index in 0..self.slices.len() {
                let src = if size2 == 1 {
                    &src_current
                } else {
                    &src_slices[index]
                };
                blit_plane(
                    &mut self.slices[index],
                    self.width,
                    self.height,
                    src,
                    other.width,
                    other.height,
                    src_display,
                    operation,
                );
            }
            // doStackOperation (230-232): a GRAY8 image (= Gray8 and Color256)
            // keeps its display bounds, everything else resets them.
            if !matches!(
                self.plane(),
                PixelPlane::Gray8(_) | PixelPlane::Color256 { .. }
            ) {
                let index = self.current_slice_index();
                self.calculator_apply_display(index, None);
            }
            Ok(None)
        }
    }

    /// Applies doOperation's resetMinAndMax (268-269) to the current document
    /// state for the slice at `index`.
    fn calculator_apply_display(&mut self, index: usize, temp_display: Option<DisplayRange>) {
        match &self.slices[index] {
            PixelPlane::Gray8(_) | PixelPlane::Color256 { .. } => {
                if let Some(range) = temp_display {
                    self.display_range = range;
                }
            }
            PixelPlane::Gray16(v) => {
                self.display_range = DisplayRange {
                    min: v.iter().copied().min().unwrap_or(0) as f64,
                    max: v.iter().copied().max().unwrap_or(0) as f64,
                };
                self.threshold = None;
            }
            PixelPlane::Gray32(v) => {
                self.display_range = float_processor_range(v);
                self.threshold = None;
            }
            // ColorProcessor inherits the empty resetMinAndMax
            // (ImageProcessor.java 500).
            PixelPlane::ColorRgb(_) => {}
        }
    }
}

// The (0,0)-anchored overlap of the two images (Blitter.copyBits rectangle
// intersection).
fn overlap(w1: u32, h1: u32, w2: u32, h2: u32) -> (usize, usize) {
    (w1.min(w2) as usize, h1.min(h2) as usize)
}

/// True for ByteProcessor/ShortProcessor.copyBits' temporary-float path for a
/// 32-bit source under MULTIPLY or DIVIDE (ByteProcessor.java 490-499,
/// ShortProcessor.java 451-459).
fn calculator_temporary_float(operation: CalculatorOperation, src32: bool) -> bool {
    src32
        && matches!(
            operation,
            CalculatorOperation::Multiply | CalculatorOperation::Divide
        )
}

/// `ImageProcessor.copyBits` for the calculator's (0,0) location: converts the
/// source through the destination type's TypeConverter route, then runs the
/// matching blitter. Returns the display range the temporary-float path
/// stored, if it ran.
fn blit_plane(
    dst: &mut PixelPlane,
    dst_w: u32,
    dst_h: u32,
    src: &PixelPlane,
    src_w: u32,
    src_h: u32,
    src_display: DisplayRange,
    operation: CalculatorOperation,
) -> Option<DisplayRange> {
    let (w, h) = overlap(dst_w, dst_h, src_w, src_h);
    match dst {
        PixelPlane::Gray8(dst_pixels) => {
            if calculator_temporary_float(
                operation,
                matches!(src, PixelPlane::Gray32(_)),
            ) {
                float_detour_u8(dst_pixels, dst_w as usize, src, src_w as usize, w, h, operation)
            } else {
                let src_pixels = plane_to_byte(src, src_display);
                for y in 0..h {
                    for x in 0..w {
                        let (d, s) = (y * dst_w as usize + x, y * src_w as usize + x);
                        dst_pixels[d] =
                            byte_op(operation, dst_pixels[d], src_pixels[s]);
                    }
                }
                None
            }
        }
        PixelPlane::Color256 { pixels, .. } => {
            if calculator_temporary_float(
                operation,
                matches!(src, PixelPlane::Gray32(_)),
            ) {
                float_detour_u8(pixels, dst_w as usize, src, src_w as usize, w, h, operation)
            } else {
                let src_pixels = plane_to_byte(src, src_display);
                for y in 0..h {
                    for x in 0..w {
                        let (d, s) = (y * dst_w as usize + x, y * src_w as usize + x);
                        pixels[d] = byte_op(operation, pixels[d], src_pixels[s]);
                    }
                }
                None
            }
        }
        PixelPlane::Gray16(dst_pixels) => {
            if calculator_temporary_float(
                operation,
                matches!(src, PixelPlane::Gray32(_)),
            ) {
                float_detour_u16(dst_pixels, dst_w as usize, src, src_w as usize, w, h, operation)
            } else {
                let src_pixels = plane_to_short(src);
                for y in 0..h {
                    for x in 0..w {
                        let (d, s) = (y * dst_w as usize + x, y * src_w as usize + x);
                        dst_pixels[d] =
                            short_op(operation, dst_pixels[d], src_pixels[s]);
                    }
                }
                None
            }
        }
        PixelPlane::Gray32(dst_pixels) => {
            // FloatProcessor.copyBits (439-442): any source converts to float.
            let src_pixels = plane_to_float(src);
            for y in 0..h {
                for x in 0..w {
                    let (d, s) = (y * dst_w as usize + x, y * src_w as usize + x);
                    dst_pixels[d] = float_op(operation, dst_pixels[d], src_pixels[s]);
                }
            }
            None
        }
        PixelPlane::ColorRgb(dst_pixels) => {
            // ColorProcessor.copyBits (622-625): any source converts to RGB.
            let src_pixels = plane_to_rgb(src, src_display);
            for y in 0..h {
                for x in 0..w {
                    let (d, s) = (y * dst_w as usize + x, y * src_w as usize + x);
                    dst_pixels[d] = color_op(operation, dst_pixels[d], src_pixels[s]);
                }
            }
            None
        }
    }
}

/// ByteProcessor.copyBits' temporary-float route with `setPixels`
/// (ByteProcessor.java 490-499, 1268-1277).
fn float_detour_u8(
    dst: &mut [u8],
    dst_w: usize,
    src: &PixelPlane,
    src_w: usize,
    w: usize,
    h: usize,
    operation: CalculatorOperation,
) -> Option<DisplayRange> {
    let src_pixels = plane_to_float(src);
    let mut values: Vec<f32> = dst.iter().map(|&v| v as f32).collect();
    for y in 0..h {
        for x in 0..w {
            let (d, s) = (y * dst_w + x, y * src_w + x);
            values[d] = float_op(operation, values[d], src_pixels[s]);
        }
    }
    let range = float_processor_range(&values);
    for y in 0..h {
        for x in 0..w {
            let i = y * dst_w + x;
            let value = values[i] + 0.5f32;
            let value = if value < 0.0 { 0.0 } else { value };
            let value = if value > 255.0 { 255.0 } else { value };
            dst[i] = value as u8;
        }
    }
    Some(range)
}

/// ShortProcessor.copyBits' temporary-float route with `setPixels`
/// (ShortProcessor.java 451-459, 1241-1248).
fn float_detour_u16(
    dst: &mut [u16],
    dst_w: usize,
    src: &PixelPlane,
    src_w: usize,
    w: usize,
    h: usize,
    operation: CalculatorOperation,
) -> Option<DisplayRange> {
    let src_pixels = plane_to_float(src);
    let mut values: Vec<f32> = dst.iter().map(|&v| v as f32).collect();
    for y in 0..h {
        for x in 0..w {
            let (d, s) = (y * dst_w + x, y * src_w + x);
            values[d] = float_op(operation, values[d], src_pixels[s]);
        }
    }
    let range = float_processor_range(&values);
    for y in 0..h {
        for x in 0..w {
            let i = y * dst_w + x;
            let value = values[i] + 0.5f32;
            let value = if value < 0.0 { 0.0 } else { value };
            let value = if value > 65535.0 { 65535.0 } else { value };
            dst[i] = value as u16;
        }
    }
    Some(range)
}

// ByteBlitter.java 88-165.
fn byte_op(operation: CalculatorOperation, dst: u8, src: u8) -> u8 {
    let (d, s) = (dst as i32, src as i32);
    let value = match operation {
        CalculatorOperation::Add => (d + s).min(255),
        CalculatorOperation::Subtract => (d - s).max(0),
        CalculatorOperation::Multiply => (d * s).min(255),
        CalculatorOperation::Divide => {
            if s == 0 {
                255
            } else {
                d / s
            }
        }
        CalculatorOperation::And => d & s,
        CalculatorOperation::Or => d | s,
        CalculatorOperation::Xor => d ^ s,
        CalculatorOperation::Min => d.min(s),
        CalculatorOperation::Max => d.max(s),
        CalculatorOperation::Average => (d + s) / 2,
        CalculatorOperation::Difference => (d - s).abs(),
        CalculatorOperation::Copy => s,
        CalculatorOperation::TransparentZero => {
            if s == 0 {
                d
            } else {
                s
            }
        }
    };
    value as u8
}

// ShortBlitter.java 46-140.
fn short_op(operation: CalculatorOperation, dst: u16, src: u16) -> u16 {
    let (d, s) = (dst as i32, src as i32);
    let value = match operation {
        CalculatorOperation::Add => (d + s).clamp(0, 65535),
        CalculatorOperation::Subtract => (d - s).clamp(0, 65535),
        CalculatorOperation::Multiply => (d * s).clamp(0, 65535),
        CalculatorOperation::Divide => {
            if s == 0 {
                65535
            } else {
                d / s
            }
        }
        CalculatorOperation::And => d & s,
        CalculatorOperation::Or => d | s,
        CalculatorOperation::Xor => d ^ s,
        CalculatorOperation::Min => d.min(s),
        CalculatorOperation::Max => d.max(s),
        CalculatorOperation::Average => (d + s) / 2,
        CalculatorOperation::Difference => (d - s).abs().min(65535),
        CalculatorOperation::Copy => s,
        CalculatorOperation::TransparentZero => {
            if s == 0 {
                d
            } else {
                s
            }
        }
    };
    value as u16
}

// FloatBlitter.java 56-138. `as i32` reproduces Java's (int) float casts
// (saturating conversion, NaN to 0).
fn float_op(operation: CalculatorOperation, dst: f32, src: f32) -> f32 {
    match operation {
        CalculatorOperation::Add => src + dst,
        CalculatorOperation::Subtract => dst - src,
        CalculatorOperation::Multiply => src * dst,
        CalculatorOperation::Divide => dst / src,
        CalculatorOperation::And => ((src as i32) & (dst as i32)) as f32,
        CalculatorOperation::Or => ((src as i32) | (dst as i32)) as f32,
        CalculatorOperation::Xor => ((src as i32) ^ (dst as i32)) as f32,
        CalculatorOperation::Min => {
            if src < dst {
                src
            } else {
                dst
            }
        }
        CalculatorOperation::Max => {
            if src > dst {
                src
            } else {
                dst
            }
        }
        CalculatorOperation::Average => (src + dst) / 2.0,
        CalculatorOperation::Difference => {
            let value = dst - src;
            if value < 0.0 {
                -value
            } else {
                value
            }
        }
        CalculatorOperation::Copy => src,
        CalculatorOperation::TransparentZero => {
            if src == 0.0 {
                dst
            } else {
                src
            }
        }
    }
}

// ColorBlitter.java 53-145 (COPY modes live in blit_plane via Copy/TransparentZero).
fn color_op(operation: CalculatorOperation, dst: u32, src: u32) -> u32 {
    let (r1, g1, b1) = ((src >> 16) & 255, (src >> 8) & 255, src & 255);
    let (r2, g2, b2) = ((dst >> 16) & 255, (dst >> 8) & 255, dst & 255);
    let (r, g, b) = match operation {
        CalculatorOperation::Add => (
            (r1 + r2).min(255),
            (g1 + g2).min(255),
            (b1 + b2).min(255),
        ),
        CalculatorOperation::Subtract => (
            r2.saturating_sub(r1),
            g2.saturating_sub(g1),
            b2.saturating_sub(b1),
        ),
        CalculatorOperation::Multiply => (
            (r1 * r2).min(255),
            (g1 * g2).min(255),
            (b1 * b2).min(255),
        ),
        CalculatorOperation::Divide => (
            if r1 == 0 { 255 } else { r2 / r1 },
            if g1 == 0 { 255 } else { g2 / g1 },
            if b1 == 0 { 255 } else { b2 / b1 },
        ),
        CalculatorOperation::And => (r1 & r2, g1 & g2, b1 & b2),
        CalculatorOperation::Or => (r1 | r2, g1 | g2, b1 | b2),
        CalculatorOperation::Xor => (r1 ^ r2, g1 ^ g2, b1 ^ b2),
        CalculatorOperation::Min => (r1.min(r2), g1.min(g2), b1.min(b2)),
        CalculatorOperation::Max => (r1.max(r2), g1.max(g2), b1.max(b2)),
        CalculatorOperation::Average => ((r1 + r2) / 2, (g1 + g2) / 2, (b1 + b2) / 2),
        CalculatorOperation::Difference => (
            (r2 as i32 - r1 as i32).unsigned_abs(),
            (g2 as i32 - g1 as i32).unsigned_abs(),
            (b2 as i32 - b1 as i32).unsigned_abs(),
        ),
        CalculatorOperation::Copy => return src | 0xff00_0000,
        CalculatorOperation::TransparentZero => {
            if (src & 0xff_ff_ff) == 0 {
                return dst;
            }
            return src | 0xff00_0000;
        }
    };
    0xff00_0000 | (r << 16) | (g << 8) | b
}

// convertRGBToByte/convertRGBToFloat weighting (TypeConverter.java 95-139):
// the oracle batch session keeps the default 1/3 weights.
fn weighted_gray_byte(rgb: u32) -> u8 {
    let (r, g, b) = ((rgb >> 16) & 255, (rgb >> 8) & 255, rgb & 255);
    let gray = r as f64 / 3.0 + g as f64 / 3.0 + b as f64 / 3.0 + 0.5;
    (gray as i32).min(255) as u8
}

fn weighted_gray_float(rgb: u32) -> f32 {
    let (r, g, b) = ((rgb >> 16) & 255, (rgb >> 8) & 255, rgb & 255);
    (r as f64 / 3.0 + g as f64 / 3.0 + b as f64 / 3.0) as f32
}

/// TypeConverter.convertShortToByte with scaling (TypeConverter.java 48-63),
/// using the source image's stored display bounds.
fn short_to_byte_scaled(value: u16, min: i32, max: i32) -> u8 {
    let scale = 256.0 / f64::from(max.wrapping_sub(min).wrapping_add(1));
    let mut value = (value as i32) - min;
    if value < 0 {
        value = 0;
    }
    let value = (value as f64 * scale + 0.5) as i32;
    value.min(255) as u8
}

/// FloatProcessor.create8BitImage (FloatProcessor.java 190-208).
fn float_to_byte_scaled(value: f32, min: f64, scale: f64) -> u8 {
    let shifted = f64::from(value) - min;
    let shifted = if shifted < 0.0 { 0.0 } else { shifted };
    ((shifted * scale + 0.5) as i32).min(255) as u8
}

/// `ImageProcessor.convertToByte(true)` for every source type the calculator
/// can pair with an 8-bit destination (TypeConverter.java 32-117).
fn plane_to_byte(src: &PixelPlane, src_display: DisplayRange) -> Vec<u8> {
    match src {
        // BYTE: returns the processor itself.
        PixelPlane::Gray8(v) => v.clone(),
        PixelPlane::Color256 { pixels, .. } => pixels.clone(),
        // convertShortToByte scales by the source's stored display bounds.
        PixelPlane::Gray16(v) => {
            let min = round_to_i32(src_display.min);
            let max = round_to_i32(src_display.max);
            v.iter().map(|&px| short_to_byte_scaled(px, min, max)).collect()
        }
        // convertFloatToByte with scaling calls create8BitImage.
        PixelPlane::Gray32(v) => {
            // FloatProcessor.setMinAndMax stores the bounds as float
            // (FloatProcessor.java 128-129).
            let min = f64::from(src_display.min as f32);
            let max = f64::from(src_display.max as f32);
            let scale = 255.0 / (max - min);
            v.iter().map(|&px| float_to_byte_scaled(px, min, scale)).collect()
        }
        // convertRGBToByte with the default 1/3 weights.
        PixelPlane::ColorRgb(v) => v.iter().map(|&px| weighted_gray_byte(px)).collect(),
    }
}

/// `ImageProcessor.convertToShort(false)`: values are copied without scaling
/// (TypeConverter.java 140-207); an RGB source goes through weighted gray.
fn plane_to_short(src: &PixelPlane) -> Vec<u16> {
    match src {
        PixelPlane::Gray8(v) => v.iter().map(|&px| px as u16).collect(),
        PixelPlane::Color256 { pixels, .. } => pixels.iter().map(|&px| px as u16).collect(),
        PixelPlane::Gray16(v) => v.clone(),
        // convertFloatToShort without scaling: clamp, +0.5, truncate.
        PixelPlane::Gray32(v) => v
            .iter()
            .map(|&px| {
                if px.is_nan() {
                    // (short)(NaN+0.5f) is 0.
                    0
                } else {
                    let value = f64::from(px.clamp(0.0, 65535.0)) + 0.5;
                    value as u16
                }
            })
            .collect(),
        // RGB goes through convertRGBToByte, then convertByteToShort.
        PixelPlane::ColorRgb(v) => v
            .iter()
            .map(|&px| weighted_gray_byte(px) as u16)
            .collect(),
    }
}

/// `ImageProcessor.convertToFloat()`: byte/short values map to raw values when
/// no calibration table exists; RGB converts to weighted gray.
fn plane_to_float(src: &PixelPlane) -> Vec<f32> {
    match src {
        PixelPlane::Gray8(v) => v.iter().map(|&px| px as f32).collect(),
        PixelPlane::Color256 { pixels, .. } => pixels.iter().map(|&px| px as f32).collect(),
        PixelPlane::Gray16(v) => v.iter().map(|&px| px as f32).collect(),
        PixelPlane::Gray32(v) => v.clone(),
        PixelPlane::ColorRgb(v) => v.iter().map(|&px| weighted_gray_float(px)).collect(),
    }
}

/// The float conversion used by createNewImage/duplicateStack for a 32-bit
/// result (ImageCalculator.java 282-285, 322-325): grayscale only.
fn plane_to_gray32(src: &PixelPlane) -> PixelPlane {
    PixelPlane::Gray32(plane_to_float(src))
}

/// `ImageProcessor.convertToFloat()` over a finished RGB blend
/// (ImageCalculator.java 266-267).
fn rgb_plane_to_gray32(src: &PixelPlane) -> PixelPlane {
    match src {
        PixelPlane::ColorRgb(v) => {
            PixelPlane::Gray32(v.iter().map(|&px| weighted_gray_float(px)).collect())
        }
        other => plane_to_gray32(other),
    }
}

/// `ImageProcessor.convertToRGB()` (ImageProcessor.java 2500-2503): byte/short
/// and float sources scale to 8-bit first, then grey-expand; an indexed source
/// expands through its own LUT.
fn plane_to_rgb(src: &PixelPlane, src_display: DisplayRange) -> Vec<u32> {
    match src {
        PixelPlane::Gray8(v) => v
            .iter()
            .map(|&px| 0xff00_0000 | ((px as u32) << 16) | ((px as u32) << 8) | px as u32)
            .collect(),
        PixelPlane::Color256 { pixels, lut } => pixels
            .iter()
            .map(|&px| {
                0xff00_0000
                    | ((lut.r[px as usize] as u32) << 16)
                    | ((lut.g[px as usize] as u32) << 8)
                    | lut.b[px as usize] as u32
            })
            .collect(),
        PixelPlane::Gray16(_) | PixelPlane::Gray32(_) => {
            // Gray16/Gray32 sources first scale to byte (TypeConverter path
            // convertToByte with doScaling=true), then expand to gray RGB.
            plane_to_byte(src, src_display)
                .into_iter()
                .map(|px| {
                    0xff00_0000 | ((px as u32) << 16) | ((px as u32) << 8) | px as u32
                })
                .collect()
        }
        PixelPlane::ColorRgb(v) => v.clone(),
    }
}

/// The top-left `width` x `height` crop of a plane, `ImageProcessor.crop`
/// without a processor ROI, as used by createNewImage's insert and
/// duplicateStack.
fn crop_plane(plane: &PixelPlane, src_w: u32, width: u32, height: u32) -> PixelPlane {
    fn cropped<T: Copy>(src: &[T], src_w: u32, width: u32, height: u32) -> Vec<T> {
        if width == src_w && height == src.len() as u32 / src_w.max(1) {
            return src.to_vec();
        }
        let mut out = Vec::with_capacity((width * height) as usize);
        for y in 0..height {
            let start = (y * src_w) as usize;
            out.extend_from_slice(&src[start..start + width as usize]);
        }
        out
    }
    match plane {
        PixelPlane::Gray8(v) => PixelPlane::Gray8(cropped(v, src_w, width, height)),
        PixelPlane::Color256 { pixels, lut } => PixelPlane::Color256 {
            pixels: cropped(pixels, src_w, width, height),
            lut: lut.clone(),
        },
        PixelPlane::Gray16(v) => PixelPlane::Gray16(cropped(v, src_w, width, height)),
        PixelPlane::Gray32(v) => PixelPlane::Gray32(cropped(v, src_w, width, height)),
        PixelPlane::ColorRgb(v) => PixelPlane::ColorRgb(cropped(v, src_w, width, height)),
    }
}

/// The display bounds a fresh ImagePlus shows for a processor:
/// doOperation's resetMinAndMax (ImageCalculator.java 268-269) with
/// ByteProcessor's (0,255) special case.
fn plane_display_range(plane: &PixelPlane) -> DisplayRange {
    match plane {
        PixelPlane::Gray8(_) | PixelPlane::Color256 { .. } | PixelPlane::ColorRgb(_) => {
            DisplayRange {
                min: 0.0,
                max: 255.0,
            }
        }
        PixelPlane::Gray16(v) => DisplayRange {
            min: v.iter().copied().min().unwrap_or(0) as f64,
            max: v.iter().copied().max().unwrap_or(0) as f64,
        },
        PixelPlane::Gray32(v) => float_processor_range(v),
    }
}
