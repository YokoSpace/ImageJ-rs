use crate::edit::{best_gray_index, best_lut_index};
use crate::{CoreError, DisplayRange, ImageDocument, PixelPlane};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Interpolation {
    None,
    Bilinear,
    Bicubic,
}

impl Interpolation {
    pub fn java_name(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Bilinear => "Bilinear",
            Self::Bicubic => "Bicubic",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RotateOptions {
    pub angle: f64,
    pub grid_lines: u32,
    pub interpolation: Interpolation,
    pub fill_with_background: bool,
    pub enlarge: bool,
    pub whole_stack: bool,
}

impl Default for RotateOptions {
    fn default() -> Self {
        Self {
            angle: 15.0,
            grid_lines: 1,
            interpolation: Interpolation::Bilinear,
            fill_with_background: false,
            enlarge: false,
            whole_stack: false,
        }
    }
}

impl ImageDocument {
    /// Rotator.run delegates to each processor's rotate(angle). The processor
    /// rotates its rectangular ROI about the ROI centre; the plugin runner
    /// restores pixels outside a non-rectangular ROI mask.
    pub fn rotate_arbitrary(
        &mut self,
        options: RotateOptions,
        background_color: [u8; 3],
    ) -> Result<(), CoreError> {
        if !options.angle.is_finite() {
            return Err(CoreError::InvalidOperation("Angle is invalid."));
        }
        let mut bounds = self
            .roi
            .as_ref()
            .filter(|roi| roi.is_area())
            .and_then(|roi| roi.clipped_bounds(self.width, self.height).ok())
            .unwrap_or((0, 0, self.width, self.height));
        let mask = self
            .roi
            .as_ref()
            .filter(|roi| roi.is_area())
            .and_then(|roi| roi.raster_mask(self.width, self.height).ok())
            .flatten();
        if self.stack_size() == 1 {
            self.save_snapshot();
        } else {
            self.snapshot = None;
            self.snapshot_dimensions = None;
            self.snapshot_display_range = None;
        }
        let indices: Vec<usize> = if options.whole_stack {
            (0..self.slices.len()).collect()
        } else {
            vec![self.current_slice_index()]
        };
        if options.enlarge && bounds == (0, 0, self.width, self.height) {
            let theta = options.angle.to_radians();
            let new_width = ((self.width as f64 * theta.cos().abs()
                + self.height as f64 * theta.sin().abs())
            .round() as u32)
                .max(self.width);
            let new_height = ((self.width as f64 * theta.sin().abs()
                + self.height as f64 * theta.cos().abs())
            .round() as u32)
                .max(self.height);
            let x_offset = (new_width - self.width) / 2;
            let y_offset = (new_height - self.height) / 2;
            self.slices = self
                .slices
                .iter()
                .map(|plane| {
                    expand_plane(
                        plane,
                        self.width,
                        self.height,
                        new_width,
                        new_height,
                        x_offset,
                        y_offset,
                        options.fill_with_background,
                        background_color,
                        self.display_range,
                    )
                })
                .collect();
            self.width = new_width;
            self.height = new_height;
            self.roi = None;
            bounds = (0, 0, new_width, new_height);
        }
        for index in indices {
            let source = self.slices[index].clone();
            self.slices[index] = rotate_plane(
                &source,
                self.width as usize,
                self.height as usize,
                bounds,
                mask.as_ref(),
                options.angle,
                options.interpolation,
                options.fill_with_background,
                background_color,
                self.display_range,
            );
        }
        Ok(())
    }

    pub fn resize_image(
        &mut self,
        dst_width: u32,
        dst_height: u32,
        interpolation: Interpolation,
        average_when_downsizing: bool,
    ) -> Result<(), CoreError> {
        if dst_width == 0 || dst_height == 0 {
            return Err(CoreError::InvalidOperation("Width or height are invalid."));
        }
        let bounds = self
            .roi
            .as_ref()
            .filter(|roi| roi.is_area())
            .and_then(|roi| roi.clipped_bounds(self.width, self.height).ok())
            .unwrap_or((0, 0, self.width, self.height));
        let (_, _, src_width, src_height) = bounds;
        if self.stack_size() == 1 {
            self.save_snapshot();
        } else {
            self.snapshot = None;
            self.snapshot_dimensions = None;
            self.snapshot_display_range = None;
        }
        let planes = self
            .slices
            .iter()
            .map(|plane| {
                resize_plane(
                    plane,
                    self.width as usize,
                    self.height as usize,
                    bounds,
                    dst_width as usize,
                    dst_height as usize,
                    interpolation,
                    average_when_downsizing,
                )
            })
            .collect();
        self.slices = planes;
        self.width = dst_width;
        self.height = dst_height;
        self.roi = None;
        if self.calibration.scaled() {
            self.calibration.pixel_width *= src_width as f64 / dst_width as f64;
            self.calibration.pixel_height *= src_height as f64 / dst_height as f64;
        }
        Ok(())
    }

    pub fn resize_depth(
        &mut self,
        depth: u32,
        method: Interpolation,
        average: bool,
    ) -> Result<(), CoreError> {
        if depth == 0 {
            return Err(CoreError::InvalidOperation("Depth is invalid."));
        }
        if depth as usize == self.stack_size() {
            return Ok(());
        }
        if self.n_channels != 1 || self.n_frames != 1 {
            return Err(CoreError::InvalidOperation(
                "Hyperstack depth scaling is not implemented.",
            ));
        }
        let old_depth = self.stack_size();
        let width = self.width as usize;
        let height = self.height as usize;
        let new_depth = depth as usize;
        self.slices = match &self.slices[0] {
            PixelPlane::Gray8(_) => {
                let planes: Vec<Vec<u8>> = self
                    .slices
                    .iter()
                    .map(|p| match p {
                        PixelPlane::Gray8(v) => v.clone(),
                        _ => unreachable!(),
                    })
                    .collect();
                resize_z_typed(
                    &planes,
                    width,
                    height,
                    new_depth,
                    method,
                    average,
                    PixelPlane::Gray8,
                    |p| match p {
                        PixelPlane::Gray8(v) => v,
                        _ => unreachable!(),
                    },
                )
            }
            PixelPlane::Color256 { lut, .. } => {
                let lut = lut.clone();
                let planes: Vec<Vec<u8>> = self
                    .slices
                    .iter()
                    .map(|p| match p {
                        PixelPlane::Color256 { pixels, .. } => pixels.clone(),
                        _ => unreachable!(),
                    })
                    .collect();
                resize_z_typed(
                    &planes,
                    width,
                    height,
                    new_depth,
                    method,
                    average,
                    |pixels| PixelPlane::Color256 {
                        pixels,
                        lut: lut.clone(),
                    },
                    |p| match p {
                        PixelPlane::Color256 { pixels, .. } => pixels,
                        _ => unreachable!(),
                    },
                )
            }
            PixelPlane::Gray16(_) => {
                let planes: Vec<Vec<u16>> = self
                    .slices
                    .iter()
                    .map(|p| match p {
                        PixelPlane::Gray16(v) => v.clone(),
                        _ => unreachable!(),
                    })
                    .collect();
                resize_z_typed(
                    &planes,
                    width,
                    height,
                    new_depth,
                    method,
                    average,
                    PixelPlane::Gray16,
                    |p| match p {
                        PixelPlane::Gray16(v) => v,
                        _ => unreachable!(),
                    },
                )
            }
            PixelPlane::Gray32(_) => {
                let planes: Vec<Vec<f32>> = self
                    .slices
                    .iter()
                    .map(|p| match p {
                        PixelPlane::Gray32(v) => v.clone(),
                        _ => unreachable!(),
                    })
                    .collect();
                resize_z_typed(
                    &planes,
                    width,
                    height,
                    new_depth,
                    method,
                    average,
                    PixelPlane::Gray32,
                    |p| match p {
                        PixelPlane::Gray32(v) => v,
                        _ => unreachable!(),
                    },
                )
            }
            PixelPlane::ColorRgb(_) => {
                let planes: Vec<Vec<u32>> = self
                    .slices
                    .iter()
                    .map(|p| match p {
                        PixelPlane::ColorRgb(v) => v.clone(),
                        _ => unreachable!(),
                    })
                    .collect();
                resize_z_typed(
                    &planes,
                    width,
                    height,
                    new_depth,
                    method,
                    average,
                    PixelPlane::ColorRgb,
                    |p| match p {
                        PixelPlane::ColorRgb(v) => v,
                        _ => unreachable!(),
                    },
                )
            }
        };
        self.slice_labels = vec![None; new_depth];
        self.n_slices = depth;
        self.current_slice = self.current_slice.min(depth);
        self.position[1] = self.current_slice;
        self.snapshot = None;
        self.snapshot_dimensions = None;
        if self.calibration.scaled() {
            self.calibration.pixel_depth *= old_depth as f64 / new_depth as f64;
        }
        Ok(())
    }
}

fn resize_z_typed<T: Copy + Default>(
    planes: &[Vec<T>],
    width: usize,
    height: usize,
    new_depth: usize,
    method: Interpolation,
    average: bool,
    make: impl Fn(Vec<T>) -> PixelPlane,
    extract: impl Fn(PixelPlane) -> Vec<T>,
) -> Vec<PixelPlane> {
    let old_depth = planes.len();
    let mut out = vec![vec![T::default(); width * height]; new_depth];
    for y in 0..height {
        let mut xz = Vec::with_capacity(width * old_depth);
        for plane in planes {
            xz.extend_from_slice(&plane[y * width..(y + 1) * width]);
        }
        let scaled = extract(resize_plane(
            &make(xz),
            width,
            old_depth,
            (0, 0, width as u32, old_depth as u32),
            width,
            new_depth,
            method,
            average,
        ));
        for z in 0..new_depth {
            out[z][y * width..(y + 1) * width].copy_from_slice(&scaled[z * width..(z + 1) * width]);
        }
    }
    out.into_iter().map(make).collect()
}

fn expand_plane(
    plane: &PixelPlane,
    width: u32,
    height: u32,
    new_width: u32,
    new_height: u32,
    x_offset: u32,
    y_offset: u32,
    fill_background: bool,
    color: [u8; 3],
    display_range: DisplayRange,
) -> PixelPlane {
    let count = (new_width * new_height) as usize;
    let bg = if fill_background {
        best_gray_index(color)
    } else {
        0
    };
    let scaled = display_range.min + (display_range.max - display_range.min) * (bg as f64 / 255.0);
    let rgb = if fill_background {
        0xff00_0000 | ((color[0] as u32) << 16) | ((color[1] as u32) << 8) | color[2] as u32
    } else {
        0xff00_0000
    };
    match plane {
        PixelPlane::Gray8(src) => {
            let mut dst = vec![bg; count];
            copy_to_canvas(src, &mut dst, width, height, new_width, x_offset, y_offset);
            PixelPlane::Gray8(dst)
        }
        PixelPlane::Color256 { pixels, lut } => {
            let mut dst = vec![
                if fill_background {
                    best_lut_index(color, lut)
                } else {
                    0
                };
                count
            ];
            copy_to_canvas(
                pixels, &mut dst, width, height, new_width, x_offset, y_offset,
            );
            PixelPlane::Color256 {
                pixels: dst,
                lut: lut.clone(),
            }
        }
        PixelPlane::Gray16(src) => {
            let mut dst = vec![scaled as u16; count];
            copy_to_canvas(src, &mut dst, width, height, new_width, x_offset, y_offset);
            PixelPlane::Gray16(dst)
        }
        PixelPlane::Gray32(src) => {
            let mut dst = vec![scaled as f32; count];
            copy_to_canvas(src, &mut dst, width, height, new_width, x_offset, y_offset);
            PixelPlane::Gray32(dst)
        }
        PixelPlane::ColorRgb(src) => {
            let mut dst = vec![rgb; count];
            copy_to_canvas(src, &mut dst, width, height, new_width, x_offset, y_offset);
            PixelPlane::ColorRgb(dst)
        }
    }
}

fn copy_to_canvas<T: Copy>(
    src: &[T],
    dst: &mut [T],
    width: u32,
    height: u32,
    new_width: u32,
    x_offset: u32,
    y_offset: u32,
) {
    for y in 0..height as usize {
        for x in 0..width as usize {
            dst[(y + y_offset as usize) * new_width as usize + x + x_offset as usize] =
                src[y * width as usize + x];
        }
    }
}

fn resize_plane(
    plane: &PixelPlane,
    width: usize,
    height: usize,
    bounds: (u32, u32, u32, u32),
    dst_width: usize,
    dst_height: usize,
    method: Interpolation,
    average: bool,
) -> PixelPlane {
    let method = if dst_width == bounds.2 as usize && dst_height == bounds.3 as usize {
        Interpolation::None
    } else {
        method
    };
    let averaged = average && (dst_width < bounds.2 as usize || dst_height < bounds.3 as usize);
    let integer = |v: f64, max: f64| -> f64 {
        if averaged {
            ((v as f32 + 0.5_f32).clamp(0.0, max as f32)) as f64
        } else {
            (v + 0.5).clamp(0.0, max)
        }
    };
    let convert = |source: Vec<f64>| -> Vec<f64> {
        if averaged {
            downsize_samples(
                &source, width, height, bounds, dst_width, dst_height, method,
            )
        } else {
            resize_samples(
                &source, width, height, bounds, dst_width, dst_height, method,
            )
        }
    };
    match plane {
        PixelPlane::Gray8(p) => PixelPlane::Gray8(
            convert(p.iter().map(|&v| v as f64).collect())
                .into_iter()
                .map(|v| integer(v, 255.0) as u8)
                .collect(),
        ),
        PixelPlane::Color256 { pixels, lut } => PixelPlane::Color256 {
            pixels: convert(pixels.iter().map(|&v| v as f64).collect())
                .into_iter()
                .map(|v| integer(v, 255.0) as u8)
                .collect(),
            lut: lut.clone(),
        },
        PixelPlane::Gray16(p) => PixelPlane::Gray16(
            convert(p.iter().map(|&v| v as f64).collect())
                .into_iter()
                .map(|v| integer(v, 65535.0) as u16)
                .collect(),
        ),
        PixelPlane::Gray32(p) => PixelPlane::Gray32(
            convert(p.iter().map(|&v| v as f64).collect())
                .into_iter()
                .map(|v| v as f32)
                .collect(),
        ),
        PixelPlane::ColorRgb(p) => {
            let channels: [Vec<u8>; 3] = std::array::from_fn(|i| {
                let shift = [16, 8, 0][i];
                convert(p.iter().map(|&v| ((v >> shift) & 255) as f64).collect())
                    .into_iter()
                    .map(|v| integer(v, 255.0) as u8)
                    .collect()
            });
            let alpha = if !averaged && method != Interpolation::None {
                0xff00_0000
            } else {
                0
            };
            PixelPlane::ColorRgb(
                (0..dst_width * dst_height)
                    .map(|i| {
                        alpha
                            | ((channels[0][i] as u32) << 16)
                            | ((channels[1][i] as u32) << 8)
                            | channels[2][i] as u32
                    })
                    .collect(),
            )
        }
    }
}

fn resize_samples(
    source: &[f64],
    width: usize,
    height: usize,
    bounds: (u32, u32, u32, u32),
    dst_width: usize,
    dst_height: usize,
    method: Interpolation,
) -> Vec<f64> {
    let (rx, ry, rw, rh) = bounds;
    let src_center_x = rx as f64 + rw as f64 / 2.0;
    let src_center_y = ry as f64 + rh as f64 / 2.0;
    let x_scale = dst_width as f64 / rw as f64;
    let y_scale = dst_height as f64 / rh as f64;
    let mut dst_center_x = dst_width as f64 / 2.0;
    let mut dst_center_y = dst_height as f64 / 2.0;
    if method != Interpolation::None {
        if dst_width != width {
            dst_center_x += x_scale / 4.0;
        }
        if dst_height != height {
            dst_center_y += y_scale / 4.0;
        }
    }
    let mut out = vec![0.0; dst_width * dst_height];
    for y in 0..dst_height {
        let mut ys = (y as f64 - dst_center_y) / y_scale + src_center_y;
        if method == Interpolation::Bilinear && height >= 2 {
            ys = ys.clamp(0.0, height as f64 - 1.001);
        }
        for x in 0..dst_width {
            let mut xs = (x as f64 - dst_center_x) / x_scale + src_center_x;
            out[y * dst_width + x] = match method {
                Interpolation::None => {
                    let ix = (xs as usize).min(width - 1);
                    let iy = (ys as usize).min(height - 1);
                    source[iy * width + ix]
                }
                Interpolation::Bilinear => {
                    if width < 2 || height < 2 {
                        source[(ys.max(0.0) as usize).min(height - 1) * width
                            + (xs.max(0.0) as usize).min(width - 1)]
                    } else {
                        xs = xs.clamp(0.0, width as f64 - 1.001);
                        bilinear(source, width, xs, ys)
                    }
                }
                Interpolation::Bicubic => bicubic(source, width, height, xs, ys, 0.0),
            };
        }
    }
    out
}

fn downsize_samples(
    source: &[f64],
    width: usize,
    height: usize,
    bounds: (u32, u32, u32, u32),
    dst_width: usize,
    dst_height: usize,
    method: Interpolation,
) -> Vec<f64> {
    let (rx, ry, rw, rh) = bounds;
    let mut values: Vec<f32> = source.iter().map(|&v| v as f32).collect();
    let mut w = width;
    let mut h = height;
    let mut roi = (rx as usize, ry as usize, rw as usize, rh as usize);
    if dst_width < roi.2 {
        values = downsize_1d(&values, w, h, roi, dst_width, true, method);
        w = dst_width;
        h = roi.3;
        roi = (0, 0, w, h);
    }
    if dst_height < roi.3 {
        values = downsize_1d(&values, w, h, roi, dst_height, false, method);
        h = dst_height;
    }
    let values: Vec<f64> = values.into_iter().map(f64::from).collect();
    if w != dst_width || h != dst_height {
        resize_samples(
            &values,
            w,
            h,
            (0, 0, w as u32, h as u32),
            dst_width,
            dst_height,
            method,
        )
    } else {
        values
    }
}

fn downsize_1d(
    source: &[f32],
    width: usize,
    _height: usize,
    roi: (usize, usize, usize, usize),
    dst_size: usize,
    x_direction: bool,
    method: Interpolation,
) -> Vec<f32> {
    let (origin, length, src_size, lines, output_width, output_height) = if x_direction {
        (roi.0, roi.2, width, roi.3, dst_size, roi.3)
    } else {
        (roi.1, roi.3, source.len() / width, roi.2, roi.2, dst_size)
    };
    let scale = length as f64 / dst_size as f64;
    let kernel_size = match method {
        Interpolation::None => 1,
        Interpolation::Bilinear => 2,
        Interpolation::Bicubic => 4,
    };
    let src_index = |d: f64| origin as f64 - 0.5 + (d + 0.5) * scale;
    let dst_index = |s: i32| (s as f64 - origin as f64 + 0.5) / scale - 0.5;
    let kernel = |d: f64| -> f32 {
        match method {
            Interpolation::None => 1.0,
            Interpolation::Bilinear => 1.0_f32 - (d.abs() as f32),
            Interpolation::Bicubic => cubic(d) as f32,
        }
    };
    let mut out = vec![0.0f32; output_width * output_height];
    for dst in 0..dst_size {
        let low = (1e-8 + src_index(dst as f64 - 0.5 * kernel_size as f64)).ceil() as i32;
        let high = (-1e-8 + src_index(dst as f64 + 0.5 * kernel_size as f64)).floor() as i32;
        let mut weights = vec![0.0f32; src_size];
        let mut sum = 0.0f64;
        for src in low..=high {
            let clamped = src.clamp(0, src_size as i32 - 1) as usize;
            let weight = kernel(dst as f64 - dst_index(src));
            weights[clamped] += weight;
            sum += weight as f64;
        }
        for line in 0..lines {
            let source_line = if x_direction {
                roi.1 + line
            } else {
                roi.0 + line
            };
            let mut accum = 0.0f32;
            for (src, &weight) in weights.iter().enumerate() {
                if weight != 0.0 {
                    let index = if x_direction {
                        source_line * width + src
                    } else {
                        src * width + source_line
                    };
                    accum += source[index] * (weight as f64 / sum) as f32;
                }
            }
            let output_index = if x_direction {
                line * output_width + dst
            } else {
                dst * output_width + line
            };
            out[output_index] = accum;
        }
    }
    out
}

fn rotate_plane(
    plane: &PixelPlane,
    width: usize,
    height: usize,
    bounds: (u32, u32, u32, u32),
    mask: Option<&crate::RoiMask>,
    angle: f64,
    interpolation: Interpolation,
    fill_with_background: bool,
    background_color: [u8; 3],
    display_range: DisplayRange,
) -> PixelPlane {
    let gray_index = if fill_with_background {
        best_gray_index(background_color)
    } else {
        0
    };
    let gray_background = gray_index as f64;
    let scaled_background = if fill_with_background {
        display_range.min + (display_range.max - display_range.min) * (gray_index as f64 / 255.0)
    } else {
        0.0
    };
    match plane {
        PixelPlane::Gray8(pixels) => {
            let values: Vec<f64> = pixels.iter().map(|&v| f64::from(v)).collect();
            PixelPlane::Gray8(
                rotate_samples(
                    &values,
                    width,
                    height,
                    bounds,
                    mask,
                    angle,
                    interpolation,
                    gray_background,
                )
                .into_iter()
                .map(|v| (v + 0.5) as u8)
                .collect(),
            )
        }
        PixelPlane::Color256 { pixels, lut } => {
            let values: Vec<f64> = pixels.iter().map(|&v| f64::from(v)).collect();
            PixelPlane::Color256 {
                pixels: rotate_samples(
                    &values,
                    width,
                    height,
                    bounds,
                    mask,
                    angle,
                    interpolation,
                    if fill_with_background {
                        best_lut_index(background_color, lut) as f64
                    } else {
                        0.0
                    },
                )
                .into_iter()
                .map(|v| (v + 0.5) as u8)
                .collect(),
                lut: lut.clone(),
            }
        }
        PixelPlane::Gray16(pixels) => {
            let values: Vec<f64> = pixels.iter().map(|&v| f64::from(v)).collect();
            PixelPlane::Gray16(
                rotate_samples(
                    &values,
                    width,
                    height,
                    bounds,
                    mask,
                    angle,
                    interpolation,
                    scaled_background,
                )
                .into_iter()
                .map(|v| (v + 0.5) as u16)
                .collect(),
            )
        }
        PixelPlane::Gray32(pixels) => {
            let values: Vec<f64> = pixels.iter().map(|&v| f64::from(v)).collect();
            PixelPlane::Gray32(
                rotate_samples(
                    &values,
                    width,
                    height,
                    bounds,
                    mask,
                    angle,
                    interpolation,
                    scaled_background,
                )
                .into_iter()
                .map(|v| v as f32)
                .collect(),
            )
        }
        PixelPlane::ColorRgb(pixels) => PixelPlane::ColorRgb(rotate_rgb(
            pixels,
            width,
            height,
            bounds,
            mask,
            angle,
            interpolation,
            if fill_with_background {
                0xff00_0000
                    | (u32::from(background_color[0]) << 16)
                    | (u32::from(background_color[1]) << 8)
                    | u32::from(background_color[2])
            } else {
                0
            },
        )),
    }
}

fn rotate_rgb(
    source: &[u32],
    width: usize,
    height: usize,
    bounds: (u32, u32, u32, u32),
    mask: Option<&crate::RoiMask>,
    angle: f64,
    interpolation: Interpolation,
    background: u32,
) -> Vec<u32> {
    if angle % 360.0 == 0.0 {
        return source.to_vec();
    }
    let (rx, ry, rw, rh) = bounds;
    let center_x = rx as f64 + (rw as f64 - 1.0) / 2.0;
    let center_y = ry as f64 + (rh as f64 - 1.0) / 2.0;
    let radians = -angle / (180.0 / std::f64::consts::PI);
    let ca = radians.cos();
    let sa = radians.sin();
    let tmp1 = center_y * sa - center_x * ca;
    let tmp2 = -center_x * sa - center_y * ca;
    let mut output: Vec<u32> = if interpolation == Interpolation::Bicubic {
        source.iter().map(|value| value | 0xff00_0000).collect()
    } else {
        source.to_vec()
    };
    let channels: [Vec<f64>; 3] = std::array::from_fn(|index| {
        let shift = [16, 8, 0][index];
        source
            .iter()
            .map(|&v| ((v >> shift) & 255) as f64)
            .collect()
    });
    for y in ry..ry + rh {
        let tmp3 = tmp1 - y as f64 * sa + center_x;
        let tmp4 = tmp2 + y as f64 * ca + center_y;
        for x in rx..rx + rw {
            if mask.is_some_and(|m| !m.contains(x, y)) {
                continue;
            }
            let mut xs = x as f64 * ca + tmp3;
            let mut ys = x as f64 * sa + tmp4;
            let value = if interpolation == Interpolation::Bicubic {
                let mut color = 0xff00_0000;
                for (shift, values) in [16, 8, 0].into_iter().zip(channels.iter()) {
                    let channel = (bicubic(
                        values,
                        width,
                        height,
                        xs,
                        ys,
                        (background >> shift & 255) as f64,
                    ) + 0.5)
                        .clamp(0.0, 255.0) as u32;
                    color |= channel << shift;
                }
                color
            } else if xs >= -0.01 && xs < width as f64 && ys >= -0.01 && ys < height as f64 {
                match interpolation {
                    Interpolation::None => {
                        let xi = ((xs + 0.5) as usize).min(width - 1);
                        let yi = ((ys + 0.5) as usize).min(height - 1);
                        source[yi * width + xi]
                    }
                    Interpolation::Bilinear => {
                        xs = xs.max(0.0).min(width as f64 - 1.001);
                        ys = ys.max(0.0).min(height as f64 - 1.001);
                        let mut color = 0xff00_0000;
                        for (shift, values) in [16, 8, 0].into_iter().zip(channels.iter()) {
                            let channel = (bilinear(&values, width, xs, ys) + 0.5) as u32;
                            color |= (channel & 255) << shift;
                        }
                        color
                    }
                    Interpolation::Bicubic => unreachable!(),
                }
            } else {
                background
            };
            output[y as usize * width + x as usize] = value;
        }
    }
    output
}

/// Byte/Short/Float/ColorProcessor.rotate all use these source coordinates
/// (e.g. ByteProcessor.java 1054-1114). The bilinear edge is clamped to
/// width-1.001/height-1.001 before the four-neighbour sample.
fn rotate_samples(
    source: &[f64],
    width: usize,
    height: usize,
    bounds: (u32, u32, u32, u32),
    mask: Option<&crate::RoiMask>,
    angle: f64,
    interpolation: Interpolation,
    background: f64,
) -> Vec<f64> {
    if angle % 360.0 == 0.0 {
        return source.to_vec();
    }
    let (rx, ry, rw, rh) = bounds;
    let center_x = rx as f64 + (rw as f64 - 1.0) / 2.0;
    let center_y = ry as f64 + (rh as f64 - 1.0) / 2.0;
    let radians = -angle / (180.0 / std::f64::consts::PI);
    let ca = radians.cos();
    let sa = radians.sin();
    let tmp1 = center_y * sa - center_x * ca;
    let tmp2 = -center_x * sa - center_y * ca;
    let mut output = source.to_vec();
    for y in ry..ry + rh {
        let tmp3 = tmp1 - y as f64 * sa + center_x;
        let tmp4 = tmp2 + y as f64 * ca + center_y;
        for x in rx..rx + rw {
            if mask.is_some_and(|m| !m.contains(x, y)) {
                continue;
            }
            let mut xs = x as f64 * ca + tmp3;
            let mut ys = x as f64 * sa + tmp4;
            let value = if interpolation == Interpolation::Bicubic {
                bicubic(source, width, height, xs, ys, background)
            } else if xs >= -0.01 && xs < width as f64 && ys >= -0.01 && ys < height as f64 {
                match interpolation {
                    Interpolation::None => {
                        let xi = ((xs + 0.5) as usize).min(width - 1);
                        let yi = ((ys + 0.5) as usize).min(height - 1);
                        source[yi * width + xi]
                    }
                    Interpolation::Bilinear => {
                        xs = xs.max(0.0).min(width as f64 - 1.001);
                        ys = ys.max(0.0).min(height as f64 - 1.001);
                        bilinear(source, width, xs, ys)
                    }
                    Interpolation::Bicubic => unreachable!(),
                }
            } else {
                background
            };
            output[y as usize * width + x as usize] = value;
        }
    }
    output
}

fn bilinear(source: &[f64], width: usize, x: f64, y: f64) -> f64 {
    let height = source.len() / width;
    if width < 2 || height < 2 {
        let xi = (x.max(0.0) as usize).min(width - 1);
        let yi = (y.max(0.0) as usize).min(height - 1);
        return source[yi * width + xi];
    }
    let xb = x as usize;
    let yb = y as usize;
    let xf = x - xb as f64;
    let yf = y - yb as f64;
    let offset = yb * width + xb;
    let lower_left = source[offset];
    let lower_right = source[offset + 1];
    let upper_left = source[offset + width];
    let upper_right = source[offset + width + 1];
    let upper_average = upper_left + xf * (upper_right - upper_left);
    let lower_average = lower_left + xf * (lower_right - lower_left);
    lower_average + yf * (upper_average - lower_average)
}

/// ImageProcessor.getBicubicInterpolatedPixel, Catmull-Rom with a=0.5.
/// At the two-pixel border Java falls back to bilinear interpolation.
fn bicubic(source: &[f64], width: usize, height: usize, x: f64, y: f64, background: f64) -> f64 {
    let x0 = x.floor() as isize;
    let y0 = y.floor() as isize;
    if x0 <= 0 || x0 >= width as isize - 2 || y0 <= 0 || y0 >= height as isize - 2 {
        if x >= -1.0 && x < width as f64 && y >= -1.0 && y < height as f64 {
            if width < 2 || height < 2 {
                let xi = (x.max(0.0) as usize).min(width - 1);
                let yi = (y.max(0.0) as usize).min(height - 1);
                return source[yi * width + xi];
            }
            let x = x.max(0.0).min(width as f64 - 1.001);
            let y = y.max(0.0).min(height as f64 - 1.001);
            return bilinear(source, width, x, y);
        }
        return background;
    }
    let mut sum = 0.0;
    for j in 0..=3 {
        let row = (y0 - 1 + j) as usize;
        let mut row_sum = 0.0;
        for i in 0..=3 {
            let col = (x0 - 1 + i) as usize;
            row_sum += source[row * width + col] * cubic(x - col as f64);
        }
        sum += row_sum * cubic(y - row as f64);
    }
    sum
}

fn cubic(mut x: f64) -> f64 {
    if x < 0.0 {
        x = -x;
    }
    if x < 1.0 {
        x * x * (x * (-0.5 + 2.0) + (0.5 - 3.0)) + 1.0
    } else if x < 2.0 {
        -0.5 * x * x * x + 2.5 * x * x - 4.0 * x + 2.0
    } else {
        0.0
    }
}
