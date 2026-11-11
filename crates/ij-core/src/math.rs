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
