pub mod contrast;
pub mod filters;
pub mod histogram;
pub mod threshold_palette;
pub mod wand;

pub use contrast::{
    adjust_brightness, adjust_contrast, adjust_max, adjust_min, auto_contrast_range,
    default_contrast_range, reset, set_min_and_max,
    slider_positions, slider_range_for, ContrastState, SLIDER_RANGE,
};
pub use filters::{AutoThresholdMethod, RankFilterKind};
pub use histogram::{display_histogram, DisplayHistogram};
pub use threshold_palette::{
    adjust_max_threshold, adjust_min_threshold, format_threshold_label, plot_threshold_bins,
    scale_down, scale_up, scale_up_and_set, threshold_to_sliders, ThresholdSliders,
    DEFAULT_MAX_SLIDER, DEFAULT_MAX_THRESHOLD, DEFAULT_MIN_SLIDER, DEFAULT_MIN_THRESHOLD,
    FLOAT_NEG_LIMIT, FLOAT_POS_LIMIT, NO_THRESHOLD_PLOT_BIN, THRESHOLD_SLIDER_RANGE,
};
pub use wand::{Wand, WandMode};

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
pub enum CoreError {
    #[error("invalid image dimensions: {width}x{height}")]
    InvalidDimensions { width: u32, height: u32 },
    #[error("pixel buffer length {actual} does not match expected {expected} ({width}x{height})")]
    BufferLengthMismatch {
        width: u32,
        height: u32,
        expected: usize,
        actual: usize,
    },
    #[error("coordinate ({x}, {y}) out of bounds for {width}x{height}")]
    OutOfBounds {
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    },
    #[error("invalid ROI rectangle: x={x}, y={y}, width={width}, height={height}")]
    InvalidRoi {
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    },
    #[error("empty selection or no pixels inside threshold")]
    EmptySelection,
    #[error("{0}")]
    InvalidOperation(&'static str),
    #[error("polygon ROI needs at least 3 non-collinear points")]
    InvalidPolygon,
    #[error("measurement of {0} ROIs is not implemented")]
    UnsupportedRoi(&'static str),
    #[error("filter {filter} is not implemented for {pixel_type} images")]
    UnsupportedFilter {
        filter: &'static str,
        pixel_type: &'static str,
    },
    #[error("filter {filter} does not support {parameter}={value}")]
    InvalidFilterParameter {
        filter: &'static str,
        parameter: &'static str,
        value: String,
    },
    #[error("image stack must contain at least one slice")]
    EmptyStack,
    #[error("invalid stack dimensions {channels}x{slices}x{frames} for {stack_size} slices")]
    InvalidStackDimensions {
        channels: u32,
        slices: u32,
        frames: u32,
        stack_size: usize,
    },
    #[error("stack index {index} out of range 1..={stack_size}")]
    StackIndexOutOfRange { index: i32, stack_size: usize },
    #[error("auto-threshold method \"{0}\" is not implemented (supported: IsoData, Otsu)")]
    UnsupportedThresholdMethod(String),
    #[error("auto-threshold is not implemented for {pixel_type} images")]
    UnsupportedThreshold { pixel_type: &'static str },
    #[error("auto-threshold on a {stack_size}-slice stack is not implemented yet")]
    UnsupportedThresholdStack { stack_size: usize },
    #[error("auto-threshold needs a 256-bin histogram, got {bins} bins")]
    UnsupportedThresholdHistogram { bins: usize },
    #[error("16-bit auto-threshold needs a 256- to 65536-bin histogram, got {bins} bins")]
    UnsupportedThresholdHistogram16 { bins: usize },
    #[error("auto-threshold option \"{option}\" requires a 16-bit image, got {pixel_type}")]
    UnsupportedThresholdOption {
        option: &'static str,
        pixel_type: &'static str,
    },
    #[error(
        "auto-threshold cannot select a bin range from a {bins}-bin histogram with no nonzero bins"
    )]
    EmptyThresholdHistogram { bins: usize },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lut256 {
    pub r: Vec<u8>,
    pub g: Vec<u8>,
    pub b: Vec<u8>,
}

impl Default for Lut256 {
    fn default() -> Self {
        let r: Vec<u8> = (0..=255).collect();
        let g: Vec<u8> = (0..=255).collect();
        let b: Vec<u8> = (0..=255).collect();
        Self { r, g, b }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PixelPlane {
    Gray8(Vec<u8>),
    Color256 { pixels: Vec<u8>, lut: Box<Lut256> },
    Gray16(Vec<u16>),
    Gray32(Vec<f32>),
    ColorRgb(Vec<u32>),
}

impl PixelPlane {
    pub fn len(&self) -> usize {
        match self {
            Self::Gray8(v) => v.len(),
            Self::Color256 { pixels, .. } => pixels.len(),
            Self::Gray16(v) => v.len(),
            Self::Gray32(v) => v.len(),
            Self::ColorRgb(v) => v.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn bit_depth(&self) -> u8 {
        match self {
            Self::Gray8(_) | Self::Color256 { .. } => 8,
            Self::Gray16(_) => 16,
            Self::ColorRgb(_) => 24,
            Self::Gray32(_) => 32,
        }
    }

    pub fn sample_f64(&self, idx: usize) -> f64 {
        match self {
            Self::Gray8(v) => v[idx] as f64,
            Self::Color256 { pixels, .. } => pixels[idx] as f64,
            Self::Gray16(v) => v[idx] as f64,
            Self::Gray32(v) => v[idx] as f64,
            Self::ColorRgb(v) => {
                let c = v[idx];
                let r = ((c >> 16) & 0xff) as f64;
                let g = ((c >> 8) & 0xff) as f64;
                let b = (c & 0xff) as f64;
                (r + g + b) / 3.0
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DisplayRange {
    pub min: f64,
    pub max: f64,
}

impl Default for DisplayRange {
    fn default() -> Self {
        Self {
            min: 0.0,
            max: 255.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Calibration {
    pub pixel_width: f64,
    pub pixel_height: f64,
    pub pixel_depth: f64,
    pub unit: String,
    pub invert_y: bool,
}

impl Default for Calibration {
    fn default() -> Self {
        Self {
            pixel_width: 1.0,
            pixel_height: 1.0,
            pixel_depth: 1.0,
            unit: "pixel".to_string(),
            invert_y: false,
        }
    }
}

impl Calibration {
    pub fn transform_x(&self, x: f64) -> f64 {
        x * self.pixel_width
    }

    pub fn transform_y(&self, y: f64, image_height: u32) -> f64 {
        if self.invert_y {
            (image_height as f64 - y - 1.0) * self.pixel_height
        } else {
            y * self.pixel_height
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RectRoi {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl RectRoi {
    pub fn new(x: i32, y: i32, width: i32, height: i32) -> Result<Self, CoreError> {
        if width <= 0 || height <= 0 {
            return Err(CoreError::InvalidRoi {
                x,
                y,
                width,
                height,
            });
        }
        Ok(Self {
            x,
            y,
            width,
            height,
        })
    }

    pub fn clamp_to(
        &self,
        img_width: u32,
        img_height: u32,
    ) -> Result<(u32, u32, u32, u32), CoreError> {
        let x0 = self.x.max(0) as u32;
        let y0 = self.y.max(0) as u32;
        let x1 = (self.x.saturating_add(self.width)).clamp(0, img_width as i32) as u32;
        let y1 = (self.y.saturating_add(self.height)).clamp(0, img_height as i32) as u32;
        if x1 <= x0 || y1 <= y0 {
            return Err(CoreError::EmptySelection);
        }
        Ok((x0, y0, x1 - x0, y1 - y0))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolygonRoi {
    pub xpoints: Vec<i32>,
    pub ypoints: Vec<i32>,
}

impl PolygonRoi {
    pub fn new(points: &[(i32, i32)]) -> Result<Self, CoreError> {
        if points.len() < 3 {
            return Err(CoreError::InvalidPolygon);
        }
        let xpoints: Vec<i32> = points.iter().map(|p| p.0).collect();
        let ypoints: Vec<i32> = points.iter().map(|p| p.1).collect();
        Ok(Self { xpoints, ypoints })
    }

    pub fn bounds(&self) -> (i32, i32, i32, i32) {
        polygon_bounds(&self.xpoints, &self.ypoints)
    }
}

/// A straight line selection. Line ROIs are not areas: ImageJ resets the
/// processor ROI before filters such as Invert (ImagePlus.setRoi lines
/// 2080-2083), and Analyzer measures a sampled profile instead of a mask.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineRoi {
    pub x1: i32,
    pub y1: i32,
    pub x2: i32,
    pub y2: i32,
}

impl LineRoi {
    pub fn bounds(&self) -> (i32, i32, i32, i32) {
        // Line.updateCoordinates sets width/height to |dx|/|dy| (Line.java
        // lines 355-359).
        (
            self.x1.min(self.x2),
            self.y1.min(self.y2),
            (self.x2 - self.x1).abs(),
            (self.y2 - self.y1).abs(),
        )
    }
}

/// A rasterized ROI mask in image coordinates. `pixels` is row-major over
/// `(width, height)` and is already clipped to the image, matching
/// ImageProcessor.setRoi(Rectangle), which crops a mask to the image.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoiMask {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<bool>,
}

impl RoiMask {
    pub fn contains(&self, x: u32, y: u32) -> bool {
        if x < self.x || y < self.y {
            return false;
        }
        let lx = x - self.x;
        let ly = y - self.y;
        if lx >= self.width || ly >= self.height {
            return false;
        }
        self.pixels[(ly * self.width + lx) as usize]
    }
}

fn polygon_bounds(xpoints: &[i32], ypoints: &[i32]) -> (i32, i32, i32, i32) {
    let mut min_x = i32::MAX;
    let mut max_x = i32::MIN;
    let mut min_y = i32::MAX;
    let mut max_y = i32::MIN;
    for (&x, &y) in xpoints.iter().zip(ypoints.iter()) {
        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_y = min_y.min(y);
        max_y = max_y.max(y);
    }
    (min_x, min_y, max_x - min_x, max_y - min_y)
}

fn convex_hull_area(points: &[(i32, i32)]) -> f64 {
    if points.len() < 3 {
        return 0.0;
    }
    let mut sorted = points.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    let cross = |a: (i32, i32), b: (i32, i32), c: (i32, i32)| -> i128 {
        let abx = i128::from(b.0) - i128::from(a.0);
        let aby = i128::from(b.1) - i128::from(a.1);
        let acx = i128::from(c.0) - i128::from(a.0);
        let acy = i128::from(c.1) - i128::from(a.1);
        abx * acy - aby * acx
    };
    let mut hull = Vec::with_capacity(sorted.len() * 2);
    for &point in &sorted {
        while hull.len() >= 2 && cross(hull[hull.len() - 2], hull[hull.len() - 1], point) <= 0 {
            hull.pop();
        }
        hull.push(point);
    }
    let lower_len = hull.len();
    for &point in sorted.iter().rev().skip(1) {
        while hull.len() > lower_len
            && cross(hull[hull.len() - 2], hull[hull.len() - 1], point) <= 0
        {
            hull.pop();
        }
        hull.push(point);
    }
    let twice_area: i128 = (0..hull.len())
        .map(|i| {
            let j = (i + 1) % hull.len();
            i128::from(hull[i].0) * i128::from(hull[j].1)
                - i128::from(hull[j].0) * i128::from(hull[i].1)
        })
        .sum();
    twice_area.abs() as f64 / 2.0
}

fn oval_convex_area(oval: RectRoi) -> f64 {
    // OvalRoi.getFloatPolygon passes a slightly expanded Ellipse2D through
    // ShapeRoi's 0.1-flatness path iterator, then Roi.getConvexHull rounds
    // those points before Analyzer.getArea uses the shoelace formula.
    let x = f64::from(oval.x) - 0.0004;
    let y = f64::from(oval.y) - 0.0004;
    let w = f64::from(oval.width) + 0.0008;
    let h = f64::from(oval.height) + 0.0008;
    let cx = x + w / 2.0;
    let cy = y + h / 2.0;
    let rx = w / 2.0;
    let ry = h / 2.0;
    let k = 0.5522847498307933;
    let arcs = [
        ([(cx + rx, cy), (cx + rx, cy + k * ry), (cx + k * rx, cy + ry), (cx, cy + ry)]),
        ([(cx, cy + ry), (cx - k * rx, cy + ry), (cx - rx, cy + k * ry), (cx - rx, cy)]),
        ([(cx - rx, cy), (cx - rx, cy - k * ry), (cx - k * rx, cy - ry), (cx, cy - ry)]),
        ([(cx, cy - ry), (cx + k * rx, cy - ry), (cx + rx, cy - k * ry), (cx + rx, cy)]),
    ];
    let mut points = vec![arcs[0][0]];
    for arc in arcs {
        flatten_cubic(arc, 0, &mut points);
    }
    let mut points: Vec<(f64, f64)> = points
        .into_iter()
        .map(|(x, y)| (x as f32 as f64, y as f32 as f64))
        .collect();
    points.sort_by(|a, b| a.partial_cmp(b).unwrap());
    points.dedup();
    let cross = |a: (f64, f64), b: (f64, f64), c: (f64, f64)| {
        (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)
    };
    let mut hull = Vec::new();
    for &point in &points {
        while hull.len() >= 2 && cross(hull[hull.len() - 2], hull[hull.len() - 1], point) <= 0.0 {
            hull.pop();
        }
        hull.push(point);
    }
    let lower_len = hull.len();
    for &point in points.iter().rev().skip(1) {
        while hull.len() > lower_len && cross(hull[hull.len() - 2], hull[hull.len() - 1], point) <= 0.0 {
            hull.pop();
        }
        hull.push(point);
    }
    let rounded: Vec<(i32, i32)> = hull
        .into_iter()
        .map(|(x, y)| ((x + 0.5).floor() as i32, (y + 0.5).floor() as i32))
        .collect();
    let twice_area: i128 = (0..rounded.len())
        .map(|i| {
            let j = (i + 1) % rounded.len();
            i128::from(rounded[i].0) * i128::from(rounded[j].1)
                - i128::from(rounded[j].0) * i128::from(rounded[i].1)
        })
        .sum();
    twice_area.abs() as f64 / 2.0
}

fn flatten_cubic(curve: [(f64, f64); 4], depth: u8, points: &mut Vec<(f64, f64)>) {
    let (x0, y0) = curve[0];
    let (x3, y3) = curve[3];
    let dx = x3 - x0;
    let dy = y3 - y0;
    let distance2 = |(x, y): (f64, f64)| {
        if dx == 0.0 && dy == 0.0 {
            (x - x0).powi(2) + (y - y0).powi(2)
        } else {
            let t = ((x - x0) * dx + (y - y0) * dy) / (dx * dx + dy * dy);
            (x - x0 - t * dx).powi(2) + (y - y0 - t * dy).powi(2)
        }
    };
    if depth >= 10 || distance2(curve[1]).max(distance2(curve[2])) < 0.01 {
        points.push(curve[3]);
        return;
    }
    let midpoint = |a: (f64, f64), b: (f64, f64)| ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
    let p01 = midpoint(curve[0], curve[1]);
    let p12 = midpoint(curve[1], curve[2]);
    let p23 = midpoint(curve[2], curve[3]);
    let p012 = midpoint(p01, p12);
    let p123 = midpoint(p12, p23);
    let center = midpoint(p012, p123);
    flatten_cubic([curve[0], p01, p012, center], depth + 1, points);
    flatten_cubic([center, p123, p23, curve[3]], depth + 1, points);
}

fn polygon_perimeter(poly: &PolygonRoi, pixel_width: f64, pixel_height: f64) -> f64 {
    let n = poly.xpoints.len();
    if n < 2 {
        return 0.0;
    }
    let dx = |i: usize, j: usize| i64::from(poly.xpoints[j]) - i64::from(poly.xpoints[i]);
    let dy = |i: usize, j: usize| i64::from(poly.ypoints[j]) - i64::from(poly.ypoints[i]);
    let traced = (0..n).all(|i| {
        let j = (i + 1) % n;
        dx(i, j) == 0 || dy(i, j) == 0
    });
    if traced {
        // PolygonRoi.getTracedPerimeter shortens non-adjacent corners by
        // (w+h)-sqrt(w²+h²), even for an orthogonal user polygon.
        if n < 4 {
            return 0.0;
        }
        let mut sum_dx = 0i64;
        let mut sum_dy = 0i64;
        let mut corners = 0u32;
        let mut side = dx(n - 1, 0).abs() + dy(n - 1, 0).abs();
        let mut previous_dx = dx(n - 1, 0);
        let mut previous_dy = dy(n - 1, 0);
        let mut corner = false;
        for i in 0..n {
            let j = (i + 1) % n;
            let next_dx = dx(i, j);
            let next_dy = dy(i, j);
            sum_dx += previous_dx.abs();
            sum_dy += previous_dy.abs();
            if side > 1 || !corner {
                corner = true;
                corners += 1;
            } else {
                corner = false;
            }
            previous_dx = next_dx;
            previous_dy = next_dy;
            side = next_dx.abs() + next_dy.abs();
        }
        let corner_loss = pixel_width + pixel_height
            - (pixel_width * pixel_width + pixel_height * pixel_height).sqrt();
        return sum_dx as f64 * pixel_width + sum_dy as f64 * pixel_height
            - f64::from(corners) * corner_loss;
    }
    (0..n)
        .map(|i| {
            let j = (i + 1) % n;
            let x = dx(i, j) as f64 * pixel_width;
            let y = dy(i, j) as f64 * pixel_height;
            (x * x + y * y).sqrt()
        })
        .sum()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Roi {
    Rect(RectRoi),
    Oval(RectRoi),
    Polygon(PolygonRoi),
    Line(LineRoi),
}

impl Roi {
    pub fn is_area(&self) -> bool {
        !matches!(self, Self::Line(_))
    }

    pub fn bounds(&self) -> (i32, i32, i32, i32) {
        match self {
            Self::Rect(r) | Self::Oval(r) => (r.x, r.y, r.width, r.height),
            Self::Polygon(p) => p.bounds(),
            Self::Line(l) => l.bounds(),
        }
    }

    /// Returns the intersection of the ROI bounds with the image. This is the
    /// rectangle that ImageProcessor.setRoi(Rectangle) leaves in roiX/roiY/
    /// roiWidth/roiHeight (ImageProcessor.java lines 855-877).
    pub fn clipped_bounds(
        &self,
        img_width: u32,
        img_height: u32,
    ) -> Result<(u32, u32, u32, u32), CoreError> {
        let (bx, by, bw, bh) = self.bounds();
        let x0 = bx.max(0) as u32;
        let y0 = by.max(0) as u32;
        let x1 = bx.saturating_add(bw).clamp(0, img_width as i32) as u32;
        let y1 = by.saturating_add(bh).clamp(0, img_height as i32) as u32;
        if x1 <= x0 || y1 <= y0 {
            return Err(CoreError::EmptySelection);
        }
        Ok((x0, y0, x1 - x0, y1 - y0))
    }

    /// Returns `None` for rectangular ROIs, whose processor has no mask, and
    /// for line ROIs, which `ImagePlus.setRoi` turns into `resetRoi`
    /// (ImagePlus.java 2080-2083). Those cases return `None` even when the
    /// bounds lie outside the image; only area masks can fail with
    /// `EmptySelection`.
    pub fn raster_mask(
        &self,
        img_width: u32,
        img_height: u32,
    ) -> Result<Option<RoiMask>, CoreError> {
        let (bx, by, bw, bh) = self.bounds();
        let local = match self {
            Self::Rect(_) | Self::Line(_) => return Ok(None),
            Self::Oval(_) => oval_mask(bw, bh),
            Self::Polygon(p) => polygon_mask(&p.xpoints, &p.ypoints, bw, bh),
        };
        let (ox, oy, mw, mh) = self.clipped_bounds(img_width, img_height)?;

        let mut pixels = vec![false; (mw * mh) as usize];
        for ly in 0..bh {
            let gy = by + ly;
            if gy < oy as i32 || gy >= (oy + mh) as i32 {
                continue;
            }
            for lx in 0..bw {
                let gx = bx + lx;
                if gx < ox as i32 || gx >= (ox + mw) as i32 {
                    continue;
                }
                if local[(ly * bw + lx) as usize] {
                    pixels[(((gy - oy as i32) as u32) * mw + (gx - ox as i32) as u32) as usize] =
                        true;
                }
            }
        }
        Ok(Some(RoiMask {
            x: ox,
            y: oy,
            width: mw,
            height: mh,
            pixels,
        }))
    }
}

/// OvalRoi.getMask() (original_imagej/ij/gui/OvalRoi.java lines 403-425).
fn oval_mask(width: i32, height: i32) -> Vec<bool> {
    let mut pixels = vec![false; (width * height) as usize];
    if width <= 0 || height <= 0 {
        return pixels;
    }
    let a = width as f64 / 2.0;
    let b = height as f64 / 2.0;
    let a2 = a * a;
    let b2 = b * b;
    let a = a - 0.5;
    let b = b - 0.5;
    for y in 0..height {
        for x in 0..width {
            let xx = x as f64 - a;
            let yy = y as f64 - b;
            if xx * xx / a2 + yy * yy / b2 <= 1.0 {
                pixels[(y * width + x) as usize] = true;
            }
        }
    }
    pixels
}

/// Port of ij.process.PolygonFiller.fillByteProcessorMask() for integer
/// polygon coordinates. Coordinates are relative to the polygon bounds.
fn polygon_mask(xpoints: &[i32], ypoints: &[i32], width: i32, height: i32) -> Vec<bool> {
    let mut pixels = vec![false; (width * height) as usize];
    let n = xpoints.len();
    if n < 3 || width <= 0 || height <= 0 {
        return pixels;
    }
    let (bx, by, _, _) = polygon_bounds(xpoints, ypoints);
    let xs: Vec<i32> = xpoints.iter().map(|&x| x - bx).collect();
    let ys: Vec<i32> = ypoints.iter().map(|&y| y - by).collect();

    let mut edges: Vec<Edge> = Vec::with_capacity(n);
    let mut y_min = i32::MAX;
    let mut y_max = i32::MIN;
    for i in 0..n {
        let iplus1 = if i == n - 1 { 0 } else { i + 1 };
        let mut y1 = ys[i];
        let mut y2 = ys[iplus1];
        let mut x1 = xs[i];
        let mut x2 = xs[iplus1];
        if y1 == y2 {
            continue;
        }
        if y1 > y2 {
            std::mem::swap(&mut y1, &mut y2);
            std::mem::swap(&mut x1, &mut x2);
        }
        let slope = (x2 - x1) as f64 / (y2 - y1) as f64;
        edges.push(Edge {
            x: x1 as f64 + 0.5 * slope + 1e-8,
            y1,
            y2,
            slope,
        });
        y_min = y_min.min(y1);
        y_max = y_max.max(y2);
    }

    let mut active: Vec<usize> = Vec::new();
    let y_start = y_min.max(0);
    if y_min != 0 {
        for (i, edge) in edges.iter_mut().enumerate() {
            if edge.y1 < y_start && edge.y2 >= y_start {
                edge.x += edge.slope * (y_start - edge.y1) as f64;
                active.push(i);
            }
        }
        sort_active_edges(&mut active, &edges);
    }

    for y in y_start..height.min(y_max + 1) {
        active.retain(|&i| !(y < edges[i].y1 || y >= edges[i].y2));
        for (edge_index, edge) in edges.iter().enumerate() {
            if y == edge.y1 {
                let mut index = 0;
                while index < active.len() && edge.x > edges[active[index]].x {
                    index += 1;
                }
                active.insert(index, edge_index);
            }
        }
        let offset = y * width;
        let mut i = 0;
        while i + 1 < active.len() {
            let mut x1 = (edges[active[i]].x + 0.5) as i32;
            let mut x2 = (edges[active[i + 1]].x + 0.5) as i32;
            x1 = x1.clamp(0, width);
            x2 = x2.clamp(0, width);
            for x in x1..x2 {
                pixels[(offset + x) as usize] = true;
            }
            i += 2;
        }
        for &index in &active {
            edges[index].x += edges[index].slope;
        }
        sort_active_edges(&mut active, &edges);
    }
    pixels
}

struct Edge {
    x: f64,
    y1: i32,
    y2: i32,
    slope: f64,
}

fn sort_active_edges(active: &mut [usize], edges: &[Edge]) {
    active.sort_by(|&a, &b| {
        edges[a]
            .x
            .partial_cmp(&edges[b].x)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}

fn default_display_range(plane: &PixelPlane) -> DisplayRange {
    match plane {
        PixelPlane::Gray8(_) | PixelPlane::Color256 { .. } | PixelPlane::ColorRgb(_) => {
            DisplayRange {
                min: 0.0,
                max: 255.0,
            }
        }
        PixelPlane::Gray16(_) => DisplayRange {
            min: 0.0,
            max: 65535.0,
        },
        PixelPlane::Gray32(v) => {
            let mut min = f64::INFINITY;
            let mut max = f64::NEG_INFINITY;
            for &val in v {
                let f = val as f64;
                if f < min {
                    min = f;
                }
                if f > max {
                    max = f;
                }
            }
            if !min.is_finite() || !max.is_finite() {
                DisplayRange { min: 0.0, max: 1.0 }
            } else {
                DisplayRange { min, max }
            }
        }
    }
}

/// Java's `(int)Math.round(double)`: `floor(value + 0.5)` narrowed through a
/// long, with NaN mapping to 0 like the JVM cast.
fn java_round_to_i32(value: f64) -> i32 {
    ((value + 0.5).floor() as i64) as i32
}

/// TypeConverter.convertShortToByte (TypeConverter.java 49-63) with scaling.
fn short_to_byte(pixel: u16, min: i32, scale: f64) -> u8 {
    let mut value = i32::from(pixel).wrapping_sub(min);
    if value < 0 {
        value = 0;
    }
    let scaled = (f64::from(value) * scale + 0.5) as i32;
    if scaled > 255 {
        255
    } else {
        // Java casts to byte, which wraps negative results.
        scaled as u8
    }
}

/// TypeConverter.convertFloatToShort (TypeConverter.java 178-199) with
/// scaling. Java clamps before adding 0.5 and casting through a short, which
/// keeps 65535 representable as the unsigned value.
fn float_to_short(pixel: f32, min: f64, scale: f64) -> u16 {
    let mut value = (f64::from(pixel) - min) * scale;
    if value < 0.0 {
        value = 0.0;
    }
    if value > 65535.0 {
        value = 65535.0;
    }
    (value + 0.5) as u16
}

/// ShortProcessor.findMinAndMax (ShortProcessor.java 70-87) over a converted
/// 16-bit plane. ImageJ never stores NaN in a ShortProcessor, so the lazy
/// processor range is just the pixel minimum and maximum.
fn gray16_display_range(pixels: &[u16]) -> DisplayRange {
    let mut min = u16::MAX;
    let mut max = 0u16;
    for &value in pixels {
        if value < min {
            min = value;
        }
        if value > max {
            max = value;
        }
    }
    DisplayRange {
        min: f64::from(min),
        max: f64::from(max),
    }
}

fn float_processor_range(pixels: &[f32]) -> DisplayRange {
    // FloatProcessor.findMinAndMax skips leading NaNs, then compares every
    // remaining value (FloatProcessor.java 92-114).
    let first = pixels
        .iter()
        .copied()
        .find(|value| !value.is_nan())
        .unwrap_or(f32::NAN);
    let (mut min, mut max) = (first, first);
    for &value in pixels {
        if value < min {
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

fn range_after_float_conversion(
    source: &PixelPlane,
    converted: &PixelPlane,
    display_range: DisplayRange,
) -> DisplayRange {
    // ByteProcessor and ShortProcessor round display bounds to integers;
    // FloatProcessor stores them as floats and treats 0/0 as resetMinAndMax.
    let source_range = match source {
        PixelPlane::Gray8(_) | PixelPlane::Gray16(_) => DisplayRange {
            min: f64::from(java_round_to_i32(display_range.min)),
            max: f64::from(java_round_to_i32(display_range.max)),
        },
        _ => display_range,
    };
    if source_range.min == 0.0 && source_range.max == 0.0 {
        let PixelPlane::Gray32(pixels) = converted else {
            unreachable!("32-bit conversion must yield Gray32")
        };
        float_processor_range(pixels)
    } else {
        DisplayRange {
            min: f64::from(source_range.min as f32),
            max: f64::from(source_range.max as f32),
        }
    }
}

/// FloatProcessor.create8BitImage (FloatProcessor.java 190-208).
fn float_to_byte(pixel: f32, min: f64, scale: f64) -> u8 {
    let mut value = f64::from(pixel) - min;
    if value < 0.0 {
        value = 0.0;
    }
    let scaled = (value * scale + 0.5) as i32;
    if scaled > 255 {
        255
    } else {
        scaled as u8
    }
}

/// TypeConverter.convertRGBToByte (TypeConverter.java 95-117) with
/// ColorProcessor's default equal weights (ColorProcessor.java 26).
fn rgb_to_byte(pixel: u32) -> u8 {
    let r = f64::from((pixel >> 16) & 0xff);
    let g = f64::from((pixel >> 8) & 0xff);
    let b = f64::from(pixel & 0xff);
    let weight = 1.0 / 3.0;
    (r * weight + g * weight + b * weight + 0.5) as u8
}

/// TypeConverter.convertRGBToFloat (TypeConverter.java 123-139) with
/// ColorProcessor's default equal weights (ColorProcessor.java 26).
fn rgb_to_float(pixel: u32) -> f32 {
    let r = f64::from((pixel >> 16) & 0xff);
    let g = f64::from((pixel >> 8) & 0xff);
    let b = f64::from(pixel & 0xff);
    let weight = 1.0 / 3.0;
    (r * weight + g * weight + b * weight) as f32
}

fn lut_rgb(lut: &Lut256, index: u8) -> u32 {
    let i = index as usize;
    (u32::from(lut.r[i]) << 16) | (u32::from(lut.g[i]) << 8) | u32::from(lut.b[i])
}

/// ByteProcessor.setMinAndMax (ByteProcessor.java 433-474): the display range
/// of an 8-bit image is baked into a copy of its LUT, indexed by pixel value.
/// `max < min` is ignored by Java and leaves the base LUT in place. Values are
/// opaque ARGB like every `ColorRgb` plane loaded from ImageJ.
fn byte_display_rgb_table(lut: &Lut256, min: f64, max: f64) -> [u32; 256] {
    let mut table = [0u32; 256];
    let rounded_min = java_round_to_i32(min);
    for (i, entry) in table.iter_mut().enumerate() {
        let index = if max < min {
            i
        } else if (i as f64) < min {
            0
        } else if (i as f64) > max {
            255
        } else {
            let offset = i as i32 - rounded_min;
            // Java's double-to-int cast saturates and maps NaN to 0, as Rust's does.
            ((256.0 * f64::from(offset) / (max - min)) as i32).clamp(0, 255) as usize
        };
        *entry = 0xff00_0000 | lut_rgb(lut, index as u8);
    }
    table
}

/// `ImageProcessor.convertToRGB` (TypeConverter.java 248-255): convert to the
/// displayed 8-bit image with scaling, then read it back through its color
/// model. 8-bit planes display through their LUT with the range baked in; a
/// stack's slices take that range from the current processor, which has
/// already rounded it (ImageStack.java 205-208, 330-331).
fn plane_to_displayed_rgb(plane: &PixelPlane, range: DisplayRange, stack: bool) -> PixelPlane {
    let gray = |value: u8| 0xff00_0000 | u32::from(value) * 0x0001_0101;
    match plane {
        PixelPlane::Gray8(pixels) | PixelPlane::Color256 { pixels, .. } => {
            let default_lut = Lut256::default();
            let lut = match plane {
                PixelPlane::Color256 { lut, .. } => lut.as_ref(),
                _ => &default_lut,
            };
            let (min, max) = if stack {
                (
                    f64::from(java_round_to_i32(range.min)),
                    f64::from(java_round_to_i32(range.max)),
                )
            } else {
                (range.min, range.max)
            };
            let table = byte_display_rgb_table(lut, min, max);
            PixelPlane::ColorRgb(pixels.iter().map(|&px| table[usize::from(px)]).collect())
        }
        PixelPlane::Gray16(pixels) => {
            // convertShortToByte (TypeConverter.java 49-63); the resulting
            // ByteProcessor keeps ImageJ's default grayscale LUT.
            let min = java_round_to_i32(range.min);
            let max = java_round_to_i32(range.max);
            let scale = 256.0 / f64::from(max.wrapping_sub(min).wrapping_add(1));
            PixelPlane::ColorRgb(
                pixels
                    .iter()
                    .map(|&px| gray(short_to_byte(px, min, scale)))
                    .collect(),
            )
        }
        PixelPlane::Gray32(pixels) => {
            // FloatProcessor.create8BitImage (FloatProcessor.java 190-208).
            let min = f64::from(range.min as f32);
            let max = f64::from(range.max as f32);
            let scale = 255.0 / (max - min);
            PixelPlane::ColorRgb(
                pixels
                    .iter()
                    .map(|&px| gray(float_to_byte(px, min, scale)))
                    .collect(),
            )
        }
        PixelPlane::ColorRgb(pixels) => PixelPlane::ColorRgb(pixels.clone()),
    }
}

/// ImageProcessor.isPseudoColorLut (ImageProcessor.java 351-386). The active
/// threshold case cannot occur because `Converter.java 55-56` resets it first.
fn is_pseudo_color_lut(lut: &Lut256) -> bool {
    let mut r2 = i32::from(lut.r[0]);
    let mut g2 = i32::from(lut.g[0]);
    let mut b2 = i32::from(lut.b[0]);
    let mut sum = 0.0;
    let mut sum2 = 0.0;
    for i in 0..256 {
        let r = i32::from(lut.r[i]);
        let g = i32::from(lut.g[i]);
        let b = i32::from(lut.b[i]);
        for (value, previous) in [(r, r2), (g, g2), (b, b2)] {
            let d = f64::from(value - previous);
            sum += d;
            sum2 += d * d;
        }
        r2 = r;
        g2 = g;
        b2 = b;
    }
    let mut std_dev = (768.0 * sum2 - sum * sum) / 768.0;
    if std_dev > 0.0 {
        std_dev = (std_dev / 767.0).sqrt();
    } else {
        std_dev = 0.0;
    }
    std_dev < 20.0 || std_dev as i32 == 67
}

/// The lower/upper levels of `ImageProcessor.minThreshold`/`maxThreshold`
/// (original_imagej/ij/process/ImageProcessor.java lines 31-33). Thresholding
/// never rewrites pixels; only this state (and the display LUT in ImageJ) is
/// updated by `setAutoThreshold`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ThresholdLevels {
    pub lower: f64,
    pub upper: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImageDocument {
    pub title: String,
    pub width: u32,
    pub height: u32,
    /// Pixel planes in stack order: `slices[n-1]` is the one-based stack index n.
    pub slices: Vec<PixelPlane>,
    /// Optional ImageJ slice labels in stack order.
    pub slice_labels: Vec<Option<String>>,
    /// One-based dimensions matching ImagePlus.nChannels/nSlices/nFrames.
    pub n_channels: u32,
    pub n_slices: u32,
    pub n_frames: u32,
    /// One-based current stack index (ImagePlus.currentSlice).
    pub current_slice: u32,
    /// One-based (channel, slice, frame) (ImagePlus.position).
    pub position: [u32; 3],
    pub display_range: DisplayRange,
    pub calibration: Calibration,
    pub roi: Option<Roi>,
    /// Active threshold levels (`ImageProcessor.NO_THRESHOLD` is modelled as
    /// `None`). `set_auto_threshold` changes this state without touching
    /// pixels.
    pub threshold: Option<ThresholdLevels>,
    /// Undo snapshot of all pixel planes.
    pub snapshot: Option<Vec<PixelPlane>>,
    /// Dimensions of the planes in `snapshot`. A transform can change image
    /// dimensions, so restoring only its pixels would invalidate the document.
    #[serde(default)]
    pub snapshot_dimensions: Option<(u32, u32)>,
    /// Original processor range saved for TYPE_CONVERSION undo only.
    #[serde(default)]
    pub snapshot_display_range: Option<DisplayRange>,
}

impl ImageDocument {
    pub fn new(
        title: impl Into<String>,
        width: u32,
        height: u32,
        plane: PixelPlane,
    ) -> Result<Self, CoreError> {
        Self::new_stack(title, width, height, vec![plane])
    }

    /// Builds a stack from planes in stack order. A plain N-slice stack has
    /// `(C,Z,T) = (1,N,1)`, matching ImagePlus.verifyDimensions
    /// (original_imagej/ij/ImagePlus.java lines 1351-1364).
    pub fn new_stack(
        title: impl Into<String>,
        width: u32,
        height: u32,
        slices: Vec<PixelPlane>,
    ) -> Result<Self, CoreError> {
        if width == 0 || height == 0 {
            return Err(CoreError::InvalidDimensions { width, height });
        }
        if slices.is_empty() {
            return Err(CoreError::EmptyStack);
        }
        let expected = (width as usize).saturating_mul(height as usize);
        for plane in &slices {
            let actual = plane.len();
            if actual != expected {
                return Err(CoreError::BufferLengthMismatch {
                    width,
                    height,
                    expected,
                    actual,
                });
            }
        }
        let stack_size = slices.len() as u32;
        let slice_labels = vec![None; slices.len()];
        let (n_channels, n_slices, n_frames) = if stack_size == 1 {
            (1, 1, 1)
        } else {
            (1, stack_size, 1)
        };
        let display_range = default_display_range(&slices[0]);
        Ok(Self {
            title: title.into(),
            width,
            height,
            slices,
            slice_labels,
            n_channels,
            n_slices,
            n_frames,
            current_slice: 1,
            position: [1, 1, 1],
            display_range,
            calibration: Calibration::default(),
            roi: None,
            threshold: None,
            snapshot: None,
            snapshot_dimensions: None,
            snapshot_display_range: None,
        })
    }

    /// Builds a stack with explicit one-based C/Z/T dimensions. The product
    /// must equal the number of planes.
    pub fn new_hyperstack(
        title: impl Into<String>,
        width: u32,
        height: u32,
        slices: Vec<PixelPlane>,
        n_channels: u32,
        n_slices: u32,
        n_frames: u32,
    ) -> Result<Self, CoreError> {
        let mut doc = Self::new_stack(title, width, height, slices)?;
        let stack_size = doc.slices.len() as u32;
        if n_channels == 0
            || n_slices == 0
            || n_frames == 0
            || n_channels.saturating_mul(n_slices).saturating_mul(n_frames) != stack_size
        {
            return Err(CoreError::InvalidStackDimensions {
                channels: n_channels,
                slices: n_slices,
                frames: n_frames,
                stack_size: stack_size as usize,
            });
        }
        doc.set_dimensions(n_channels, n_slices, n_frames)?;
        Ok(doc)
    }

    pub fn new_ramp_8bit(
        title: impl Into<String>,
        width: u32,
        height: u32,
    ) -> Result<Self, CoreError> {
        if width == 0 || height == 0 {
            return Err(CoreError::InvalidDimensions { width, height });
        }
        let mut pixels = vec![0u8; (width * height) as usize];
        for x in 0..width {
            let val = if width <= 1 {
                0u8
            } else {
                ((x as f64 * 256.0 / width as f64).min(255.0)) as u8
            };
            for y in 0..height {
                pixels[(y * width + x) as usize] = val;
            }
        }
        Self::new(title, width, height, PixelPlane::Gray8(pixels))
    }

    pub fn new_ramp_16bit(
        title: impl Into<String>,
        width: u32,
        height: u32,
    ) -> Result<Self, CoreError> {
        if width == 0 || height == 0 {
            return Err(CoreError::InvalidDimensions { width, height });
        }
        // NewImage.java line 286: (short)(((i*65536.0)/width)+0.5)
        let mut ramp = Vec::with_capacity(width as usize);
        for x in 0..width {
            ramp.push((((x as f64 * 65536.0) / width as f64) + 0.5) as u16);
        }
        let mut pixels = vec![0u16; (width * height) as usize];
        for y in 0..height {
            for x in 0..width {
                pixels[(y * width + x) as usize] = ramp[x as usize];
            }
        }
        Self::new(title, width, height, PixelPlane::Gray16(pixels))
    }

    pub fn new_ramp_32bit(
        title: impl Into<String>,
        width: u32,
        height: u32,
    ) -> Result<Self, CoreError> {
        if width == 0 || height == 0 {
            return Err(CoreError::InvalidDimensions { width, height });
        }
        // NewImage.java line 334: (float)((i*1.0)/width)
        let mut ramp = Vec::with_capacity(width as usize);
        for x in 0..width {
            ramp.push((x as f64 / width as f64) as f32);
        }
        let mut pixels = vec![0f32; (width * height) as usize];
        for y in 0..height {
            for x in 0..width {
                pixels[(y * width + x) as usize] = ramp[x as usize];
            }
        }
        let mut doc = Self::new(title, width, height, PixelPlane::Gray32(pixels))?;
        // NewImage.java line 354: default display range of a new float image
        doc.display_range = DisplayRange { min: 0.0, max: 1.0 };
        Ok(doc)
    }

    pub fn stack_size(&self) -> usize {
        self.slices.len().max(1)
    }

    /// The processor for the current stack index.
    pub fn plane(&self) -> &PixelPlane {
        &self.slices[self.current_slice_index()]
    }

    pub fn plane_mut(&mut self) -> &mut PixelPlane {
        let index = self.current_slice_index();
        &mut self.slices[index]
    }

    fn current_slice_index(&self) -> usize {
        let size = self.slices.len().max(1) as u32;
        (self.current_slice.clamp(1, size) - 1) as usize
    }

    pub fn get_n_channels(&self) -> u32 {
        self.n_channels
    }

    pub fn get_n_slices(&self) -> u32 {
        self.n_slices
    }

    pub fn get_n_frames(&self) -> u32 {
        self.n_frames
    }

    /// Returns `[width, height, channels, slices, frames]` like
    /// ImagePlus.getDimensions.
    pub fn get_dimensions(&self) -> [u32; 5] {
        [
            self.width,
            self.height,
            self.n_channels,
            self.n_slices,
            self.n_frames,
        ]
    }

    pub fn get_slice_number(&self) -> u32 {
        self.current_slice
    }

    pub fn get_channel(&self) -> u32 {
        self.position[0]
    }

    pub fn get_slice(&self) -> u32 {
        self.position[1]
    }

    pub fn get_frame(&self) -> u32 {
        self.position[2]
    }

    pub fn get_position(&self) -> (u32, u32, u32) {
        (self.position[0], self.position[1], self.position[2])
    }

    /// ImagePlus.getNDimensions (original_imagej/ij/ImagePlus.java 1301-1309):
    /// starts at 2 and adds one for every C/Z/T dimension greater than 1.
    pub fn get_n_dimensions(&self) -> u32 {
        2 + u32::from(self.n_channels > 1)
            + u32::from(self.n_slices > 1)
            + u32::from(self.n_frames > 1)
    }

    /// ImagePlus.isHyperStack: more than three dimensions (ImagePlus.java
    /// 1294-1297).
    pub fn is_hyperstack(&self) -> bool {
        self.get_n_dimensions() > 3
    }

    /// ImagePlus.verifyDimensions (original_imagej/ij/ImagePlus.java
    /// 1349-1364). Dimensions are cached, so ImageJ re-derives them from the
    /// stack size before reading or positioning.
    fn verify_dimensions(&mut self) {
        let stack_size = self.slices.len().max(1) as u32;
        if self.n_slices == 1 {
            if self.n_channels > 1 && self.n_frames == 1 {
                self.n_channels = stack_size;
            } else if self.n_frames > 1 && self.n_channels == 1 {
                self.n_frames = stack_size;
            }
        }
        if self
            .n_channels
            .saturating_mul(self.n_slices)
            .saturating_mul(self.n_frames)
            != stack_size
        {
            self.n_slices = stack_size;
            self.n_channels = 1;
            self.n_frames = 1;
        }
    }

    /// ImagePlus.setDimensions (original_imagej/ij/ImagePlus.java 1262-1288).
    ///
    /// When `channels*slices*frames` does not match the stack size, ImageJ
    /// silently falls back to a plain Z-stack `(1, stackSize, 1)` rather than
    /// raising. Without a StackWindow the `updateWin` branch never runs, so
    /// neither `currentSlice` nor `position` is touched; both may therefore be
    /// stale after a shape change, exactly as in headless ImageJ.
    pub fn set_dimensions(
        &mut self,
        channels: u32,
        slices: u32,
        frames: u32,
    ) -> Result<(), CoreError> {
        let stack_size = self.slices.len().max(1) as u32;
        if channels.saturating_mul(slices).saturating_mul(frames) != stack_size {
            self.n_channels = 1;
            self.n_slices = stack_size;
            self.n_frames = 1;
        } else {
            self.n_channels = channels;
            self.n_slices = slices;
            self.n_frames = frames;
        }
        Ok(())
    }

    /// ImagePlus.getStackIndex (original_imagej/ij/ImagePlus.java lines
    /// 1916-1924): channel varies fastest, then Z, then T.
    pub fn get_stack_index(&self, channel: i32, slice: i32, frame: i32) -> u32 {
        let c = channel.clamp(1, self.n_channels as i32) as u32;
        let z = slice.clamp(1, self.n_slices as i32) as u32;
        let t = frame.clamp(1, self.n_frames as i32) as u32;
        (t - 1) * self.n_channels * self.n_slices + (z - 1) * self.n_channels + c
    }

    /// ImagePlus.convertIndexToPosition (original_imagej/ij/ImagePlus.java
    /// lines 1948-1957).
    pub fn convert_index_to_position(&self, n: i32) -> Result<[u32; 3], CoreError> {
        let stack_size = self.slices.len().max(1);
        if n < 1 || n as usize > stack_size {
            return Err(CoreError::StackIndexOutOfRange {
                index: n,
                stack_size,
            });
        }
        let n = n as u32;
        let channel = (n - 1) % self.n_channels + 1;
        let slice = ((n - 1) / self.n_channels) % self.n_slices + 1;
        let frame = ((n - 1) / (self.n_channels * self.n_slices)) % self.n_frames + 1;
        Ok([channel, slice, frame])
    }

    /// ImagePlus.setPosition (original_imagej/ij/ImagePlus.java lines
    /// 1850-1875): negatives become zero, zero keeps the current coordinate,
    /// and values above the dimension are clamped. Like ImageJ, changing the
    /// displayed stack index discards the processor snapshot used by Undo.
    pub fn set_position(&mut self, channel: i32, slice: i32, frame: i32) {
        self.verify_dimensions();
        let mut c = channel.max(0) as u32;
        let mut z = slice.max(0) as u32;
        let mut t = frame.max(0) as u32;
        if c == 0 {
            c = self.position[0];
        }
        if z == 0 {
            z = self.position[1];
        }
        if t == 0 {
            t = self.position[2];
        }
        c = c.min(self.n_channels);
        z = z.min(self.n_slices);
        t = t.min(self.n_frames);
        let index = self.get_stack_index(c as i32, z as i32, t as i32);
        let changed = index != self.current_slice;
        self.current_slice = index;
        self.position = [c, z, t];
        if changed {
            // ImagePlus.setSlice swaps in different stack pixels and calls
            // ip.setSnapshotPixels(null) (ImagePlus.java 1997-1999).
            self.snapshot = None;
            self.snapshot_display_range = None;
        }
    }

    /// ImagePlus.setPosition(int n) (original_imagej/ij/ImagePlus.java lines
    /// 1942-1946): converts a one-based stack index to C/Z/T and applies it.
    pub fn set_position_from_index(&mut self, n: i32) -> Result<(), CoreError> {
        self.verify_dimensions();
        let [c, z, t] = self.convert_index_to_position(n)?;
        self.set_position(c as i32, z as i32, t as i32);
        Ok(())
    }

    /// ImagePlus.setSlice (original_imagej/ij/ImagePlus.java lines
    /// 1966-2024): only a stack shaped like a single dimension updates the
    /// hyperstack position (setCurrentSlice, ImagePlus.java 1823-1829). An
    /// out-of-range index does nothing in ImageJ; callers that model the macro
    /// function should use [`Self::set_slice_macro`] for the error check.
    pub fn set_slice(&mut self, index: i32) -> Result<(), CoreError> {
        let stack_size = self.slices.len().max(1);
        if index < 1 || index as usize > stack_size {
            return Ok(());
        }
        if self.current_slice as i32 == index {
            return Ok(());
        }
        self.set_current_slice(index as u32);
        // ImagePlus.setSlice calls ip.setSnapshotPixels(null) when it switches
        // processors, so Undo no longer has a snapshot (ImagePlus.java
        // 1997-1999).
        self.snapshot = None;
        self.snapshot_display_range = None;
        Ok(())
    }

    fn set_current_slice(&mut self, slice: u32) {
        self.current_slice = slice;
        let stack_size = self.slices.len().max(1) as u32;
        if self.n_channels == stack_size {
            self.position = [slice, 1, 1];
        }
        if self.n_slices == stack_size {
            self.position = [1, slice, 1];
        }
        if self.n_frames == stack_size {
            self.position = [1, 1, slice];
        }
    }

    /// Port of the macro function `setSlice` (original_imagej/ij/macro/
    /// Functions.java lines 3544-3560). Unlike ImagePlus.setSlice, a macro
    /// `setSlice(n)` on a hyperstack calls `setPosition(n)`, converting the
    /// stack index to C/Z/T. The macro also rejects out-of-range indices.
    pub fn set_slice_macro(&mut self, index: i32) -> Result<(), CoreError> {
        let n_slices = self.slices.len().max(1);
        if index == 1 && n_slices == 1 {
            return Ok(());
        }
        if index < 1 || index as usize > n_slices {
            return Err(CoreError::StackIndexOutOfRange {
                index,
                stack_size: n_slices,
            });
        }
        if self.is_hyperstack() {
            self.set_position_from_index(index)?;
        } else {
            self.set_slice(index)?;
        }
        Ok(())
    }

    /// Port of `IJ.makeRectangle` plus `ImagePlus.setRoi` (original_imagej
    /// lines IJ.java 1298-1308 and ImagePlus.java 2048-2084). A width <= 0 or
    /// a negative height deletes the current selection and records nothing;
    /// a zero height is legal and `Roi` clamps it to one pixel (Roi.java 151-152).
    pub fn set_rect_roi(
        &mut self,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    ) -> Result<(), CoreError> {
        if width <= 0 || height < 0 {
            self.roi = None;
            return Ok(());
        }
        self.roi = Some(Roi::Rect(RectRoi {
            x,
            y,
            width,
            height: height.max(1),
        }));
        Ok(())
    }

    /// Port of `IJ.makeOval` plus `ImagePlus.setRoi` (IJ.java 1320-1335).
    /// Degenerate sizes delete the current selection; a zero height becomes one.
    pub fn set_oval_roi(
        &mut self,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    ) -> Result<(), CoreError> {
        if width <= 0 || height < 0 {
            self.roi = None;
            return Ok(());
        }
        self.roi = Some(Roi::Oval(RectRoi {
            x,
            y,
            width,
            height: height.max(1),
        }));
        Ok(())
    }

    pub fn set_polygon_roi(&mut self, points: &[(i32, i32)]) -> Result<(), CoreError> {
        if points.len() < 3 {
            return Err(CoreError::InvalidPolygon);
        }
        let xpoints: Vec<i32> = points.iter().map(|p| p.0).collect();
        let ypoints: Vec<i32> = points.iter().map(|p| p.1).collect();
        let (_, _, width, height) = polygon_bounds(&xpoints, &ypoints);
        if width == 0 && height == 0 {
            self.roi = None;
            return Ok(());
        }
        self.roi = Some(Roi::Polygon(PolygonRoi::new(points)?));
        Ok(())
    }

    pub fn set_line_roi(&mut self, x1: i32, y1: i32, x2: i32, y2: i32) {
        self.roi = Some(Roi::Line(LineRoi { x1, y1, x2, y2 }));
    }

    /// The irregular ROI mask, or `None` for the whole image, rectangular
    /// ROIs (Roi.getMask returns null for rectangles) and line ROIs (a line
    /// resets the processor ROI instead of installing a mask, ImagePlus.java
    /// 2080-2083).
    pub fn roi_mask(&self) -> Result<Option<RoiMask>, CoreError> {
        match &self.roi {
            Some(roi) => roi.raster_mask(self.width, self.height),
            None => Ok(None),
        }
    }

    pub fn clear_roi(&mut self) {
        self.roi = None;
    }

    /// Crops every stack plane to the area selection's clipped bounds
    /// (Resizer.java 25-195, StackProcessor.java 137-152). A rectangular
    /// selection is cleared; oval and polygon selections move to (0, 0).
    pub fn crop(&mut self) -> Result<(), CoreError> {
        let roi = self
            .roi
            .clone()
            .ok_or(CoreError::InvalidOperation("Area selection required"))?;
        if !roi.is_area() {
            return Err(CoreError::InvalidOperation("Area selection required"));
        }
        let (x, y, width, height) = roi
            .clipped_bounds(self.width, self.height)
            .map_err(|_| CoreError::InvalidOperation("Selection is outside image"))?;
        if width == 0 || height == 0 {
            return Err(CoreError::InvalidOperation("Selection is outside image"));
        }
        let (bx, by, bw, bh) = roi.bounds();
        let clipped_shape = (i64::from(bx), i64::from(by), i64::from(bw), i64::from(bh))
            != (i64::from(x), i64::from(y), i64::from(width), i64::from(height));
        if !matches!(roi, Roi::Rect(_)) && clipped_shape {
            // Resizer.java 51-60 turns this intersection into a ShapeRoi.
            return Err(CoreError::InvalidOperation(
                "Crop of clipped non-rectangular ROI requires ShapeRoi",
            ));
        }

        // Undo.setup(Undo.TRANSFORM, imp) runs before the resize and only for
        // single-slice images (Resizer.java 145-146). Whole-stack filters
        // clear ImageJ's undo state.
        if self.stack_size() == 1 {
            self.save_snapshot();
        } else {
            self.snapshot = None;
            self.snapshot_dimensions = None;
            self.snapshot_display_range = None;
        }

        let source_width = self.width;
        let source_slices = std::mem::take(&mut self.slices);
        self.slices = source_slices
            .iter()
            .map(|plane| crop_plane(plane, source_width, x, y, width, height))
            .collect();
        self.width = width;
        self.height = height;

        self.roi = match roi {
            Roi::Rect(_) => None,
            Roi::Oval(r) => Some(Roi::Oval(RectRoi {
                x: 0,
                y: 0,
                width: r.width,
                height: r.height,
            })),
            // roi.setLocation(0, 0) moves the polygon points relative to the
            // crop origin (Resizer.java 181-183).
            Roi::Polygon(p) => Some(Roi::Polygon(PolygonRoi {
                xpoints: p.xpoints.iter().map(|px| px - x as i32).collect(),
                ypoints: p.ypoints.iter().map(|py| py - y as i32).collect(),
            })),
            Roi::Line(_) => unreachable!("line ROIs are rejected above"),
        };
        Ok(())
    }

    /// Converts every plane to 8-bit grayscale, matching `Image > Type > 8-bit`
    /// with `doScaling = true` and `Prefs.calibrateConversions = false`.
    /// `Converter.java 38-39` sends single images to
    /// `ImageConverter.convertToGray8` (ImageConverter.java 24-55) and stacks
    /// to `StackConverter.convertToGray8` (StackConverter.java 26-122).
    pub fn convert_to_8bit(&mut self) {
        // Converter.java 55-56 resets the threshold before converting.
        self.threshold = None;
        if self.slices.len() > 1 {
            self.convert_stack_to_8bit();
            return;
        }
        // Undo.setup(Undo.TYPE_CONVERSION, imp) runs for single images only
        // (Converter.java 100-101).
        self.save_snapshot();
        let (converted, new_processor) = match self.slices[0].clone() {
            PixelPlane::Gray8(pixels) => {
                // ImageConverter.java 42-45: the default 8-bit LUT is
                // pseudo-color, so ImageJ only swaps in an identical grayscale
                // LUT and keeps the display range.
                (PixelPlane::Gray8(pixels), false)
            }
            PixelPlane::Gray16(pixels) => {
                // ShortProcessor.setMinAndMax rounds the display range
                // (ShortProcessor.java 272-273); convertShortToByte then
                // truncates the already integral bounds (TypeConverter.java 54).
                let min = java_round_to_i32(self.display_range.min);
                let max = java_round_to_i32(self.display_range.max);
                let scale = 256.0 / f64::from(max.wrapping_sub(min).wrapping_add(1));
                (
                    PixelPlane::Gray8(
                        pixels
                            .iter()
                            .map(|&px| short_to_byte(px, min, scale))
                            .collect(),
                    ),
                    true,
                )
            }
            PixelPlane::Gray32(pixels) => {
                // FloatProcessor.setMinAndMax stores the bounds as float
                // (FloatProcessor.java 128-129).
                let min = f64::from(self.display_range.min as f32);
                let max = f64::from(self.display_range.max as f32);
                let scale = 255.0 / (max - min);
                (
                    PixelPlane::Gray8(
                        pixels
                            .iter()
                            .map(|&px| float_to_byte(px, min, scale))
                            .collect(),
                    ),
                    true,
                )
            }
            PixelPlane::ColorRgb(pixels) => (
                PixelPlane::Gray8(pixels.iter().map(|&px| rgb_to_byte(px)).collect()),
                true,
            ),
            PixelPlane::Color256 { pixels, lut } => {
                if is_pseudo_color_lut(&lut) {
                    // ImageConverter.java 42-45 keeps the indices and swaps in
                    // a grayscale LUT.
                    (PixelPlane::Gray8(pixels), false)
                } else {
                    // ImageConverter.java 46-49 builds a ColorProcessor from
                    // the LUT and converts with the equal-weight RGB formula.
                    (
                        PixelPlane::Gray8(
                            pixels
                                .iter()
                                .map(|&index| rgb_to_byte(lut_rgb(&lut, index)))
                                .collect(),
                        ),
                        true,
                    )
                }
            }
        };
        self.slices[0] = converted;
        if new_processor {
            self.snapshot_display_range = Some(self.display_range);
            // A new ByteProcessor starts with the default 0..255 range.
            self.display_range = DisplayRange {
                min: 0.0,
                max: 255.0,
            };
        }
    }

    fn convert_stack_to_8bit(&mut self) {
        // A whole-stack type conversion does not create an Undo buffer, and
        // ImageJ's Undo is a no-op afterwards (measured with the 1.54u oracle).
        self.snapshot = None;
        self.snapshot_dimensions = None;
        self.snapshot_display_range = None;

        let current = self.current_slice_index();
        let color256 = matches!(self.slices[current], PixelPlane::Color256 { .. });
        if color256 {
            // StackConverter.java 40-44 sends COLOR_256 stacks to
            // convertRGBToGray8 (93-122), even when the LUT is pseudo-color.
            for plane in &mut self.slices {
                if let PixelPlane::Color256 { pixels, lut } = plane {
                    let converted = pixels
                        .iter()
                        .map(|&index| rgb_to_byte(lut_rgb(lut, index)))
                        .collect();
                    *plane = PixelPlane::Gray8(converted);
                }
            }
            self.display_range = DisplayRange {
                min: 0.0,
                max: 255.0,
            };
            return;
        }

        // StackConverter.java 55-56 reads min/max from the current processor
        // and applies them to every slice (line 70).
        let min16 = java_round_to_i32(self.display_range.min);
        let max16 = java_round_to_i32(self.display_range.max);
        let scale16 = 256.0 / f64::from(max16.wrapping_sub(min16).wrapping_add(1));
        let min32 = f64::from(self.display_range.min as f32);
        let max32 = f64::from(self.display_range.max as f32);
        let scale32 = 255.0 / (max32 - min32);

        let all_gray8 = self
            .slices
            .iter()
            .all(|plane| matches!(plane, PixelPlane::Gray8(_)));
        self.slices = self
            .slices
            .iter()
            .map(|plane| match plane {
                // A ByteProcessor converts to itself (TypeConverter.java 35-36).
                PixelPlane::Gray8(pixels) => PixelPlane::Gray8(pixels.clone()),
                PixelPlane::Gray16(pixels) => PixelPlane::Gray8(
                    pixels
                        .iter()
                        .map(|&px| short_to_byte(px, min16, scale16))
                        .collect(),
                ),
                PixelPlane::Gray32(pixels) => PixelPlane::Gray8(
                    pixels
                        .iter()
                        .map(|&px| float_to_byte(px, min32, scale32))
                        .collect(),
                ),
                PixelPlane::ColorRgb(pixels) => {
                    PixelPlane::Gray8(pixels.iter().map(|&px| rgb_to_byte(px)).collect())
                }
                PixelPlane::Color256 { .. } => {
                    unreachable!("COLOR_256 stacks are handled above")
                }
            })
            .collect();
        if !all_gray8 {
            // StackConverter.java 77-78 does not set a display range, so the
            // new ByteProcessors keep their default 0..255.
            self.display_range = DisplayRange {
                min: 0.0,
                max: 255.0,
            };
        }
    }

    /// Converts every plane to 16-bit grayscale, matching `Image > Type >
    /// 16-bit` with `doScaling = true` and `Prefs.calibrateConversions = false`.
    /// Single images go through `ImageConverter.convertToGray16`
    /// (ImageConverter.java 58-77); stacks go through
    /// `StackConverter.convertToGray16` (StackConverter.java 144-180).
    ///
    /// `COLOR_256` images throw `IllegalArgumentException` in Java
    /// (ImageConverter.java 59-62), and stacks of any type other than GRAY8 or
    /// GRAY32 throw from the stack converter (StackConverter.java 145-148).
    pub fn convert_to_16bit(&mut self) -> Result<(), CoreError> {
        // Converter.java 55-56 resets the threshold before the type checks, and
        // the catch block (Converter.java 136-143) does not restore it.
        self.threshold = None;
        if self.slices.len() > 1 {
            return self.convert_stack_to_16bit();
        }
        if matches!(self.slices[0], PixelPlane::Color256 { .. }) {
            // ImageConverter.java 59-62 rejects COLOR_256 even for a
            // pseudo-color LUT. Converter.java 136-143 only aborts the macro.
            return Err(CoreError::InvalidOperation("Unsupported conversion"));
        }
        // Undo.setup(Undo.TYPE_CONVERSION, imp) runs for single images only
        // (Converter.java 100-101); a GRAY16 no-op also replaces the buffer.
        self.save_snapshot();
        let converted = match self.slices[0].clone() {
            // ImageConverter.java 59-60 returns without touching anything.
            PixelPlane::Gray16(_) => return Ok(()),
            // convertByteToShort (TypeConverter.java 149-160).
            PixelPlane::Gray8(pixels) => {
                PixelPlane::Gray16(pixels.into_iter().map(u16::from).collect())
            }
            PixelPlane::Gray32(pixels) => {
                // FloatProcessor.setMinAndMax stores the bounds as float
                // (FloatProcessor.java 123-133). Zero/zero resets them from
                // the pixels before TypeConverter reads the range.
                let range = if self.display_range.min == 0.0 && self.display_range.max == 0.0 {
                    float_processor_range(&pixels)
                } else {
                    self.display_range
                };
                let min = f64::from(range.min as f32);
                let max = f64::from(range.max as f32);
                let scale = if (max - min) == 0.0 {
                    1.0
                } else {
                    65535.0 / (max - min)
                };
                PixelPlane::Gray16(
                    pixels
                        .iter()
                        .map(|&px| float_to_short(px, min, scale))
                        .collect(),
                )
            }
            // convertToShort sends RGB through convertRGBToByte
            // (TypeConverter.java 149-160); the ByteProcessor levels become
            // u16 values unchanged.
            PixelPlane::ColorRgb(pixels) => PixelPlane::Gray16(
                pixels.iter().map(|&px| u16::from(rgb_to_byte(px))).collect(),
            ),
            PixelPlane::Color256 { .. } => unreachable!("COLOR_256 was rejected above"),
        };
        self.slices[0] = converted;
        // `imp.setProcessor` installs a fresh ShortProcessor whose range is
        // computed lazily from the new pixels (ShortProcessor.java 70-87), so
        // any custom display range is replaced by the data range.
        self.snapshot_display_range = Some(self.display_range);
        let PixelPlane::Gray16(pixels) = &self.slices[0] else {
            unreachable!("conversion always produces Gray16")
        };
        self.display_range = gray16_display_range(pixels);
        Ok(())
    }

    fn convert_stack_to_16bit(&mut self) -> Result<(), CoreError> {
        let current = self.current_slice_index();
        // StackConverter.java 145-148 only accepts GRAY8 and GRAY32; GRAY16 is
        // a silent no-op on line 146.
        match self.slices[current] {
            PixelPlane::Gray16(_) => return Ok(()),
            PixelPlane::Gray8(_) | PixelPlane::Gray32(_) => {}
            _ => return Err(CoreError::InvalidOperation("Unsupported conversion")),
        }
        // ImageJ stacks share one type, so a mixed Rust stack has no Java
        // equivalent; reject it rather than convert some planes silently.
        if self.slices.iter().any(|plane| {
            matches!(
                plane,
                PixelPlane::ColorRgb(_) | PixelPlane::Color256 { .. }
            )
        }) {
            return Err(CoreError::InvalidOperation("Unsupported conversion"));
        }
        // A whole-stack type conversion does not create an Undo buffer.
        self.snapshot = None;
        self.snapshot_dimensions = None;
        self.snapshot_display_range = None;

        // StackConverter.java 167 only scales a GRAY32 stack; GRAY8 stacks
        // keep their 0..=255 values (convertByteToShort ignores the scale).
        let source_is_32 = matches!(self.slices[current], PixelPlane::Gray32(_));
        let range = if source_is_32 && self.display_range.min == 0.0 && self.display_range.max == 0.0 {
            let PixelPlane::Gray32(pixels) = &self.slices[current] else {
                unreachable!("source_is_32 requires a Gray32 plane")
            };
            float_processor_range(pixels)
        } else {
            self.display_range
        };
        let min = f64::from(range.min as f32);
        let max = f64::from(range.max as f32);
        let scale = if (max - min) == 0.0 {
            1.0
        } else {
            65535.0 / (max - min)
        };
        self.slices = self
            .slices
            .iter()
            .map(|plane| match plane {
                // convertByteToShort keeps the 8-bit value (TypeConverter.java
                // 149-160).
                PixelPlane::Gray8(pixels) => {
                    PixelPlane::Gray16(pixels.iter().map(|&px| u16::from(px)).collect())
                }
                // convertToShort returns the ShortProcessor itself
                // (TypeConverter.java 151).
                PixelPlane::Gray16(pixels) => PixelPlane::Gray16(pixels.clone()),
                // Every slice is converted with the current processor's range
                // (StackConverter.java 167-171).
                PixelPlane::Gray32(pixels) => PixelPlane::Gray16(
                    pixels
                        .iter()
                        .map(|&px| float_to_short(px, min, scale))
                        .collect(),
                ),
                PixelPlane::ColorRgb(_) | PixelPlane::Color256 { .. } => {
                    unreachable!("color stacks were rejected above")
                }
            })
            .collect();
        if source_is_32 {
            // StackConverter.java 178-179 resets the display range.
            self.display_range = DisplayRange {
                min: 0.0,
                max: 65535.0,
            };
        } else {
            // A GRAY8 stack leaves every new ShortProcessor with the range
            // ImageStack.addSlice captured from the first added slice
            // (ImageStack.java 116-119); getProcessor then applies that stack
            // range to every slice (ImageStack.java 330-331). This is the
            // first slice, not the current one.
            let PixelPlane::Gray16(pixels) = &self.slices[0] else {
                unreachable!("conversion always produces Gray16")
            };
            self.display_range = gray16_display_range(pixels);
        }
        Ok(())
    }

    /// Converts every plane to 32-bit float grayscale, matching `Image > Type >
    /// 32-bit` with `doScaling = true` and `Prefs.calibrateConversions = false`.
    /// Single images go through `ImageConverter.convertToGray32`
    /// (ImageConverter.java 171-190); stacks go through
    /// `StackConverter.convertToGray32` (StackConverter.java 181-223).
    ///
    /// `COLOR_256` images throw `IllegalArgumentException` in Java
    /// (ImageConverter.java 174-175, StackConverter.java 184-185).
    pub fn convert_to_32bit(&mut self) -> Result<(), CoreError> {
        // Converter.java 51-52 resets the threshold before the type checks, and
        // the catch block (Converter.java 132-140) does not restore it.
        self.threshold = None;
        if self.slices.len() > 1 {
            return self.convert_stack_to_32bit();
        }
        if matches!(self.slices[0], PixelPlane::Color256 { .. }) {
            // ImageConverter.java 174-175 rejects COLOR_256.
            return Err(CoreError::InvalidOperation("Unsupported conversion"));
        }
        // Undo.setup(Undo.TYPE_CONVERSION, imp) runs for single images only
        // (Converter.java 96-103); a GRAY32 no-op also replaces the buffer.
        self.save_snapshot();
        let converted = match self.slices[0].clone() {
            // ImageConverter.java 172-173 returns without touching anything.
            PixelPlane::Gray32(_) => return Ok(()),
            // convertByteToFloat without calibration table (TypeConverter.java 211-225).
            PixelPlane::Gray8(pixels) => {
                PixelPlane::Gray32(pixels.into_iter().map(f32::from).collect())
            }
            // convertShortToFloat without calibration table (TypeConverter.java 231-245).
            PixelPlane::Gray16(pixels) => {
                PixelPlane::Gray32(pixels.into_iter().map(f32::from).collect())
            }
            // convertRGBToFloat (TypeConverter.java 123-139).
            PixelPlane::ColorRgb(pixels) => {
                PixelPlane::Gray32(pixels.into_iter().map(rgb_to_float).collect())
            }
            PixelPlane::Color256 { .. } => unreachable!("COLOR_256 was rejected above"),
        };
        let converted_range =
            range_after_float_conversion(&self.slices[0], &converted, self.display_range);
        self.slices[0] = converted;
        // ImageConverter.java 188 restores the original display range via
        // IJ.setMinAndMax(imp, min, max).
        self.snapshot_display_range = Some(self.display_range);
        self.display_range = converted_range;
        Ok(())
    }

    fn convert_stack_to_32bit(&mut self) -> Result<(), CoreError> {
        let current = self.current_slice_index();
        // StackConverter.java 182-185 accepts GRAY8, GRAY16, and COLOR_RGB;
        // GRAY32 is a silent no-op on line 183.
        match self.slices[current] {
            PixelPlane::Gray32(_) => return Ok(()),
            PixelPlane::Gray8(_) | PixelPlane::Gray16(_) | PixelPlane::ColorRgb(_) => {}
            _ => return Err(CoreError::InvalidOperation("Unsupported conversion")),
        }
        // ImageJ stacks share one type, so a mixed Rust stack has no Java
        // equivalent; reject it rather than convert some planes silently.
        if self.slices.iter().any(|plane| {
            matches!(
                plane,
                PixelPlane::Color256 { .. }
            )
        }) {
            return Err(CoreError::InvalidOperation("Unsupported conversion"));
        }
        // A whole-stack type conversion does not create an Undo buffer.
        self.snapshot = None;
        self.snapshot_dimensions = None;
        self.snapshot_display_range = None;

        let source_plane = self.slices[current].clone();
        self.slices = self
            .slices
            .iter()
            .map(|plane| match plane {
                PixelPlane::Gray8(pixels) => {
                    PixelPlane::Gray32(pixels.iter().map(|&px| f32::from(px)).collect())
                }
                PixelPlane::Gray16(pixels) => {
                    PixelPlane::Gray32(pixels.iter().map(|&px| f32::from(px)).collect())
                }
                PixelPlane::Gray32(pixels) => PixelPlane::Gray32(pixels.clone()),
                PixelPlane::ColorRgb(pixels) => {
                    PixelPlane::Gray32(pixels.iter().map(|&px| rgb_to_float(px)).collect())
                }
                PixelPlane::Color256 { .. } => {
                    unreachable!("color256 stacks were rejected above")
                }
            })
            .collect();
        // ImageConverter.java 181 restores the source range via
        // IJ.setMinAndMax(imp, min, max), subject to processor rounding.
        self.display_range = range_after_float_conversion(
            &source_plane,
            &self.slices[current],
            self.display_range,
        );
        Ok(())
    }

    /// Converts every plane to RGB, matching `Image > Type > RGB Color` with
    /// `doScaling = true`. Single images go through
    /// `ImageConverter.convertToRGB` (ImageConverter.java 192-202); stacks go
    /// through `StackConverter.convertToRGB` (StackConverter.java 226-261).
    /// Both call `ImageProcessor.convertToRGB`, which renders the processor
    /// as its displayed 8-bit image (TypeConverter.java 248-255).
    ///
    /// Gray16/Gray32/Gray8 planes carry no LUT in this model, so ImageJ's
    /// default non-inverting grayscale LUT is the only one that can be
    /// reproduced; an inverting LUT on those types is not representable. An
    /// inverting LUT on a `Color256` plane is representable and honored.
    pub fn convert_to_rgb(&mut self) {
        // Converter.java 55-56 resets the threshold before converting.
        self.threshold = None;
        let is_stack = self.slices.len() > 1;
        if is_stack {
            // A whole-stack type conversion does not create an Undo buffer.
            self.snapshot = None;
            self.snapshot_dimensions = None;
            self.snapshot_display_range = None;
        } else {
            // Undo.setup(Undo.TYPE_CONVERSION, imp) runs for single images
            // only (Converter.java 100-101), even when the conversion is a
            // no-op.
            self.save_snapshot();
        }
        // ImageConverter.java 193-194 and ImageProcessor.convertToRGB leave
        // RGB planes alone.
        if self
            .slices
            .iter()
            .all(|plane| matches!(plane, PixelPlane::ColorRgb(_)))
        {
            return;
        }

        let current = self.current_slice_index();
        let mut range = self.display_range;
        if let PixelPlane::Gray32(pixels) = &self.slices[current] {
            // FloatProcessor.setMinAndMax treats 0/0 as resetMinAndMax
            // (FloatProcessor.java 124-129); the stack reads the range from the
            // current slice's processor (ImageStack.java 205-208).
            if range.min == 0.0 && range.max == 0.0 {
                range = float_processor_range(pixels);
            }
        }
        for plane in &mut self.slices {
            *plane = plane_to_displayed_rgb(plane, range, is_stack);
        }
        if !is_stack {
            self.snapshot_display_range = Some(self.display_range);
        }
        // A new ColorProcessor starts with the default 0..255 range.
        self.display_range = DisplayRange {
            min: 0.0,
            max: 255.0,
        };
    }

    pub fn save_snapshot(&mut self) {
        self.snapshot = Some(self.slices.clone());
        self.snapshot_dimensions = Some((self.width, self.height));
        self.snapshot_display_range = None;
    }

    pub fn undo(&mut self) -> bool {
        if let Some(prev) = self.snapshot.take() {
            let dimensions = self
                .snapshot_dimensions
                .take()
                .filter(|(width, height)| {
                    let expected = (*width as usize).saturating_mul(*height as usize);
                    prev.iter().all(|plane| plane.len() == expected)
                })
                .unwrap_or((self.width, self.height));
            let expected = (dimensions.0 as usize).saturating_mul(dimensions.1 as usize);
            if prev.iter().any(|plane| plane.len() != expected) {
                self.snapshot_display_range = None;
                return false;
            }
            self.slices = prev;
            self.width = dimensions.0;
            self.height = dimensions.1;
            if let Some(display_range) = self.snapshot_display_range.take() {
                self.display_range = display_range;
            }
            let size = self.slices.len().max(1) as u32;
            self.current_slice = self.current_slice.clamp(1, size);
            true
        } else {
            self.snapshot_display_range = None;
            false
        }
    }

    /// Inverts the current stack slice, matching `run("Invert", "slice")`.
    pub fn invert(&mut self) {
        self.save_snapshot();
        let index = self.current_slice_index();
        self.invert_plane_at(index);
    }

    /// Inverts every stack slice, matching `run("Invert", "stack")`. A
    /// complete-stack filter clears ImageJ's Undo state
    /// (PlugInFilterRunner.java 118-120 calls `Undo.reset()`), so only a
    /// one-plane image keeps a snapshot and remains undoable.
    pub fn invert_stack(&mut self) {
        if self.slices.len() > 1 {
            self.snapshot = None;
            self.snapshot_display_range = None;
        } else {
            self.save_snapshot();
        }
        for index in 0..self.slices.len() {
            self.invert_plane_at(index);
        }
    }

    fn invert_plane_at(&mut self, index: usize) {
        let width = self.width;
        let height = self.height;
        let display_range = self.display_range;
        let (rx, ry, rw, rh, mask) = match &self.roi {
            None => (0, 0, width, height, None),
            // Line ROIs are not areas, so ImagePlus resets the processor ROI
            // and Invert affects the whole image (ImagePlus.java lines 2080-2083).
            Some(Roi::Line(_)) => (0, 0, width, height, None),
            Some(roi) => {
                let (rx, ry, rw, rh) = roi.clipped_bounds(width, height).unwrap_or((0, 0, 0, 0));
                let mask = roi.raster_mask(width, height).ok().flatten();
                (rx, ry, rw, rh, mask)
            }
        };
        let plane = &mut self.slices[index];
        match plane {
            PixelPlane::Gray8(v) | PixelPlane::Color256 { pixels: v, .. } => {
                for y in ry..(ry + rh) {
                    for x in rx..(rx + rw) {
                        if mask.as_ref().is_some_and(|m| !m.contains(x, y)) {
                            continue;
                        }
                        let idx = (y * width + x) as usize;
                        v[idx] = 255 - v[idx];
                    }
                }
            }
            PixelPlane::Gray16(v) => {
                // ShortProcessor.invert() resets min/max to the image's own range
                // before process(INVERT) and then restores the display range
                // (original_imagej/ij/process/ShortProcessor.java lines 591-596),
                // so every pixel becomes imageMin + imageMax - v.
                let mut image_min = u16::MAX;
                let mut image_max = 0u16;
                for &px in v.iter() {
                    if px < image_min {
                        image_min = px;
                    }
                    if px > image_max {
                        image_max = px;
                    }
                }
                let min = image_min as i32;
                let max = image_max as i32;
                for y in ry..(ry + rh) {
                    for x in rx..(rx + rw) {
                        if mask.as_ref().is_some_and(|m| !m.contains(x, y)) {
                            continue;
                        }
                        let idx = (y * width + x) as usize;
                        let inverted = max - (v[idx] as i32 - min);
                        v[idx] = inverted as u16;
                    }
                }
            }
            PixelPlane::Gray32(v) => {
                let min = display_range.min as f32;
                let max = display_range.max as f32;
                for y in ry..(ry + rh) {
                    for x in rx..(rx + rw) {
                        if mask.as_ref().is_some_and(|m| !m.contains(x, y)) {
                            continue;
                        }
                        let idx = (y * width + x) as usize;
                        v[idx] = max - (v[idx] - min);
                    }
                }
            }
            PixelPlane::ColorRgb(v) => {
                for y in ry..(ry + rh) {
                    for x in rx..(rx + rw) {
                        if mask.as_ref().is_some_and(|m| !m.contains(x, y)) {
                            continue;
                        }
                        let idx = (y * width + x) as usize;
                        v[idx] = 0xff00_0000 | (!v[idx] & 0x00ff_ffff);
                    }
                }
            }
        }
    }

    pub fn measure(&self) -> Result<MeasurementSummary, CoreError> {
        self.measure_with_config(MeasurementConfig::default())
    }

    /// Measures the active ROI/slice with the `Analyze > Set Measurements...`
    /// options. Only `LIMIT` changes the numbers: when it is enabled and a
    /// threshold is active, each statistics class clips its histogram and
    /// moment scans to the threshold range (ByteStatistics.java 21-51,
    /// ShortStatistics.java 23-63, FloatStatistics.java 22-46). Column
    /// selection and precision belong to the ResultsTable layer.
    pub fn measure_with_config(
        &self,
        config: MeasurementConfig,
    ) -> Result<MeasurementSummary, CoreError> {
        // Analyzer.java 510-511 applies the processor threshold to the profile
        // FloatProcessor before measuring a line, so LIMIT also clips line stats.
        let limit = if config.contains(MeasurementFlags::LIMIT) {
            self.threshold
        } else {
            None
        };
        if let Some(Roi::Line(line)) = &self.roi {
            let mut summary = self.measure_line(*line, config.contains(MeasurementFlags::LIMIT), limit);
            self.populate_shape_descriptors(&mut summary, config);
            return Ok(summary);
        }
        let (rx, ry, rw, rh) = match &self.roi {
            // A selection entirely outside the image leaves a 0x0 processor
            // ROI (ImageProcessor.setRoi lines 860-864) but still produces a
            // measurement row. The per-type statistics classes then report
            // their empty result instead of raising.
            Some(roi) => roi
                .clipped_bounds(self.width, self.height)
                .unwrap_or((0, 0, 0, 0)),
            None => (0, 0, self.width, self.height),
        };
        let mask = if rw == 0 || rh == 0 {
            None
        } else {
            self.roi_mask()?
        };

        let raw = match self.plane() {
            PixelPlane::Gray8(pixels) => self.measure_byte_like(
                pixels,
                rx,
                ry,
                rw,
                rh,
                mask.as_ref(),
                self.threshold,
                limit,
            ),
            PixelPlane::Color256 { pixels, .. } => self.measure_byte_like(
                pixels,
                rx,
                ry,
                rw,
                rh,
                mask.as_ref(),
                self.threshold,
                limit,
            ),
            PixelPlane::Gray16(pixels) => self.measure_short(
                pixels,
                rx,
                ry,
                rw,
                rh,
                mask.as_ref(),
                self.threshold,
                limit,
            ),
            PixelPlane::Gray32(pixels) => self.measure_float(
                pixels,
                rx,
                ry,
                rw,
                rh,
                mask.as_ref(),
                self.threshold,
                limit,
            ),
            PixelPlane::ColorRgb(pixels) => self.measure_rgb(pixels, rx, ry, rw, rh, mask.as_ref()),
        };

        let mut summary = self.finish_measurement(rx, ry, rw, rh, raw);
        if config.contains(MeasurementFlags::ELLIPSE)
            || config.contains(MeasurementFlags::SHAPE_DESCRIPTORS)
        {
            let (major, minor, angle, centroid) =
                self.fit_ellipse(rx, ry, rw, rh, mask.as_ref(), limit);
            summary.major = major;
            summary.minor = minor;
            summary.ellipse_angle = angle;
            summary.x_centroid = self.calibration.transform_x(centroid.0);
            summary.y_centroid = self.calibration.transform_y(centroid.1, self.height);
        }
        self.populate_shape_descriptors(&mut summary, config);
        Ok(summary)
    }

    /// Port of `Analyzer.measureLength` for a 1-pixel-wide straight line.
    ///
    /// `ProfilePlot.getStraightLineProfile` samples the line with
    /// `ImageProcessor.getLine` (ImageProcessor.java 1116-1139) and wraps the
    /// values in a one-row FloatProcessor. Default measurements therefore
    /// report the profile statistics (`Area` is the number of samples), plus
    /// the Results-table-only `Length` and `Angle` columns.
    fn measure_line(
        &self,
        line: LineRoi,
        limit_enabled: bool,
        limit: Option<ThresholdLevels>,
    ) -> MeasurementSummary {
        let x1 = line.x1 as f64;
        let y1 = line.y1 as f64;
        let x2 = line.x2 as f64;
        let y2 = line.y2 as f64;
        let dx = x2 - x1;
        let dy = y2 - y1;
        // ImageProcessor.getLine: n = (int)Math.round(sqrt(dx*dx+dy*dy)).
        let n = ((dx * dx + dy * dy).sqrt() + 0.5).floor() as usize;
        let xinc = if n > 0 { dx / n as f64 } else { 0.0 };
        let yinc = if n > 0 { dy / n as f64 } else { 0.0 };

        // ProfilePlot leaves PlotWindow.interpolate true, so getLine uses the
        // bilinear path. FloatProcessor(double[]) then narrows the samples to
        // float (FloatProcessor.java 49-53) before FloatStatistics runs.
        let mut profile = Vec::with_capacity(n + 1);
        let mut rx = x1;
        let mut ry = y1;
        for _ in 0..=n {
            profile.push(self.interpolated_value(rx, ry) as f32);
            rx += xinc;
            ry += yinc;
        }

        let samples = (n + 1) as u32;
        // Analyzer.java 500-511 converts the float profile back to the source
        // depth whenever LIMIT is selected, even if no threshold is active.
        let raw = match (limit_enabled, self.plane()) {
            (true, PixelPlane::Gray8(_) | PixelPlane::Color256 { .. }) => {
                // ByteProcessor.setPixels(int, FloatProcessor) adds 0.5f,
                // clamps, then narrows to byte (ByteProcessor.java 1268-1278).
                let pixels: Vec<u8> = profile
                    .iter()
                    .map(|&value| (value + 0.5_f32).clamp(0.0, 255.0) as u8)
                    .collect();
                self.measure_byte_like(&pixels, 0, 0, samples, 1, None, None, limit)
            }
            (true, PixelPlane::Gray16(_)) => {
                let pixels: Vec<u16> = profile
                    .iter()
                    .map(|&value| float_to_short(value, 0.0, 1.0))
                    .collect();
                self.measure_short(&pixels, 0, 0, samples, 1, None, None, limit)
            }
            _ => self.measure_float(&profile, 0, 0, samples, 1, None, None, limit),
        };
        let mut summary = self.finish_measurement(0, 0, samples, 1, raw);

        // Line.getLength (Line.java 430-437) and Roi.getFloatAngle
        // (Roi.java 1960-1969) apply the spatial calibration; the angle's dy
        // is y1-y2 so ImageJ reports negative angles for downward lines.
        let length = (((x2 - x1) * self.calibration.pixel_width).powi(2)
            + ((y2 - y1) * self.calibration.pixel_height).powi(2))
        .sqrt();
        let angle = (((y1 - y2) * self.calibration.pixel_height)
            .atan2((x2 - x1) * self.calibration.pixel_width))
        .to_degrees();
        summary.line = Some(LineMeasurement { length, angle });
        summary
    }

    /// `ImageProcessor.getInterpolatedValue` (ImageProcessor.java 2005-2028).
    fn interpolated_value(&self, x: f64, y: f64) -> f64 {
        let width = self.width as f64;
        let height = self.height as f64;
        if x < 0.0 || x >= width - 1.0 || y < 0.0 || y >= height - 1.0 {
            if x < -1.0 || x >= width || y < -1.0 || y >= height {
                return 0.0;
            }
            return self.interpolated_edge_value(x, y);
        }
        let xbase = x as i32;
        let ybase = y as i32;
        let mut x_fraction = x - xbase as f64;
        let mut y_fraction = y - ybase as f64;
        if x_fraction < 0.0 {
            x_fraction = 0.0;
        }
        if y_fraction < 0.0 {
            y_fraction = 0.0;
        }
        let lower_left = self.pixel_value(xbase, ybase);
        let lower_right = self.pixel_value(xbase + 1, ybase);
        let upper_right = self.pixel_value(xbase + 1, ybase + 1);
        let upper_left = self.pixel_value(xbase, ybase + 1);
        let upper_average = upper_left + x_fraction * (upper_right - upper_left);
        let lower_average = lower_left + x_fraction * (lower_right - lower_left);
        lower_average + y_fraction * (upper_average - lower_average)
    }

    /// `ImageProcessor.getInterpolatedEdgeValue` (ImageProcessor.java
    /// 2083-2099), used for the one-pixel border outside the sample grid.
    fn interpolated_edge_value(&self, x: f64, y: f64) -> f64 {
        let xbase = x as i32;
        let ybase = y as i32;
        let mut x_fraction = x - xbase as f64;
        let mut y_fraction = y - ybase as f64;
        if x_fraction < 0.0 {
            x_fraction = 0.0;
        }
        if y_fraction < 0.0 {
            y_fraction = 0.0;
        }
        let lower_left = self.edge_value(xbase, ybase);
        let lower_right = self.edge_value(xbase + 1, ybase);
        let upper_right = self.edge_value(xbase + 1, ybase + 1);
        let upper_left = self.edge_value(xbase, ybase + 1);
        let upper_average = upper_left + x_fraction * (upper_right - upper_left);
        let lower_average = lower_left + x_fraction * (lower_right - lower_left);
        lower_average + y_fraction * (upper_average - lower_average)
    }

    fn edge_value(&self, x: i32, y: i32) -> f64 {
        let x = x.clamp(0, self.width as i32 - 1);
        let y = y.clamp(0, self.height as i32 - 1);
        self.pixel_value(x, y)
    }

    /// `ImageProcessor.getPixelValue` per plane type (ByteProcessor.java
    /// 313-319, ShortProcessor.java 396-402, FloatProcessor.java 395-400,
    /// ColorProcessor.java 402-414).
    fn pixel_value(&self, x: i32, y: i32) -> f64 {
        let index = (y as u32 * self.width + x as u32) as usize;
        match self.plane() {
            PixelPlane::ColorRgb(pixels) => {
                let c = pixels[index];
                let r = ((c >> 16) & 0xff) as f64;
                let g = ((c >> 8) & 0xff) as f64;
                let b = (c & 0xff) as f64;
                let weight = 1.0 / 3.0;
                ((r * weight + g * weight + b * weight) as f32) as f64
            }
            plane => plane.sample_f64(index),
        }
    }

    fn finish_measurement(
        &self,
        rx: u32,
        ry: u32,
        rw: u32,
        rh: u32,
        raw: RawMeasurement,
    ) -> MeasurementSummary {
        let n = raw.pixel_count as f64;
        let area = n * self.calibration.pixel_width * self.calibration.pixel_height;
        let mean = raw.sum / n;
        let std_dev = calculate_std_dev(n, raw.sum, raw.sum2);

        MeasurementSummary {
            pixel_count: raw.pixel_count,
            area,
            mean,
            std_dev,
            min: raw.min,
            max: raw.max,
            // ByteStatistics.java lines 115-116 and 153-154 add +0.5 for the pixel center.
            x_centroid: self.calibration.transform_x(raw.x_centroid),
            y_centroid: self.calibration.transform_y(raw.y_centroid, self.height),
            x_center_of_mass: self.calibration.transform_x(raw.x_center_of_mass),
            y_center_of_mass: self
                .calibration
                .transform_y(raw.y_center_of_mass, self.height),
            roi_x: self.calibration.transform_x(rx as f64),
            roi_y: self.calibration.transform_y(ry as f64, self.height),
            roi_width: (rw as f64) * self.calibration.pixel_width,
            roi_height: (rh as f64) * self.calibration.pixel_height,
            perimeter: self.roi_perimeter(),
            major: 0.0,
            minor: 0.0,
            ellipse_angle: 0.0,
            circularity: 0.0,
            aspect_ratio: 0.0,
            roundness: 0.0,
            solidity: 0.0,
            int_den: area * mean,
            raw_int_den: raw.sum,
            histogram: raw.histogram,
            mode: raw.mode,
            median: raw.median,
            skewness: raw.skewness,
            kurtosis: raw.kurtosis,
            area_fraction: raw.area_fraction,
            stack_position: self.stack_position(),
            line: None,
        }
    }

    fn fit_ellipse(
        &self,
        rx: u32,
        ry: u32,
        rw: u32,
        rh: u32,
        roi_mask: Option<&RoiMask>,
        limit: Option<ThresholdLevels>,
    ) -> (f64, f64, f64, (f64, f64)) {
        let psize = if ((self.calibration.pixel_width - self.calibration.pixel_height).abs()
            / self.calibration.pixel_width)
            < 0.01
        {
            self.calibration.pixel_width
        } else {
            0.0
        };
        if roi_mask.is_none() && limit.is_none() {
            let mut major = 2.0 * f64::from(rw) / 1.772453851;
            let mut minor = 2.0 * f64::from(rh) / 1.772453851;
            let mut angle = 0.0;
            if major < minor {
                std::mem::swap(&mut major, &mut minor);
                angle = 90.0;
            }
            return (
                major * psize,
                minor * psize,
                angle,
                (f64::from(rx) + f64::from(rw) / 2.0, f64::from(ry) + f64::from(rh) / 2.0),
            );
        }

        // EllipseFitter.java 190-238: sum the ROI mask in local coordinates,
        // including its 1/12 pixel-area correction to the second moments.
        let mut count = 0.0;
        let (mut sx, mut sy, mut sx2, mut sy2, mut sxy) = (0.0, 0.0, 0.0, 0.0, 0.0);
        for y in 0..rh {
            for x in 0..rw {
                let ix = rx + x;
                let iy = ry + y;
                if roi_mask.is_some_and(|m| !m.contains(ix, iy)) {
                    continue;
                }
                if limit.is_some_and(|t| {
                    let value = self.pixel_value(ix as i32, iy as i32);
                    value < t.lower || value > t.upper
                }) {
                    continue;
                }
                let x = f64::from(x);
                let y = f64::from(y);
                count += 1.0;
                sx += x;
                sy += y;
                sx2 += x * x;
                sy2 += y * y;
                sxy += x * y;
            }
        }
        if count == 0.0 {
            return (0.0, 0.0, 0.0, (f64::from(rx) + 0.5, f64::from(ry) + 0.5));
        }
        sx2 += 0.08333333 * count;
        sy2 += 0.08333333 * count;
        let xm = sx / count;
        let ym = sy / count;
        let u20 = sx2 / count - xm * xm;
        let u02 = sy2 / count - ym * ym;
        let u11 = sxy / count - xm * ym;
        let m4 = (4.0 * (u02 * u20 - u11 * u11).abs()).max(0.000001);
        let mut a11 = u02 / m4;
        let a12 = u11 / m4;
        let mut a22 = u20 / m4;
        let mut delta = a11 - a22;
        if delta == 0.0 {
            delta = 0.000001;
        }
        let mut theta = 0.5 * (2.0 * a12 / delta).atan();
        if theta < 0.0 {
            theta += 1.5707963267949;
        }
        if a12 > 0.0 {
            theta += 1.5707963267949;
        } else if a12 == 0.0 {
            if a22 > a11 {
                theta = 0.0;
                std::mem::swap(&mut a22, &mut a11);
            } else if a11 != a22 {
                theta = 1.5707963267949;
            }
        }
        let sin = if theta.sin() == 0.0 { 0.000001 } else { theta.sin() };
        let z = a12 * theta.cos() / sin;
        let mut major = (1.0 / (a22 + z).abs()).sqrt();
        let mut minor = (1.0 / (a11 - z).abs()).sqrt();
        let scale = (count / (std::f64::consts::PI * major * minor)).sqrt();
        major *= scale * 2.0;
        minor *= scale * 2.0;
        let mut angle = 180.0 * theta / std::f64::consts::PI;
        if angle == 180.0 {
            angle = 0.0;
        }
        if major < minor {
            std::mem::swap(&mut major, &mut minor);
        }
        (major * psize, minor * psize, angle, (f64::from(rx) + xm + 0.5, f64::from(ry) + ym + 0.5))
    }

    fn populate_shape_descriptors(&self, summary: &mut MeasurementSummary, config: MeasurementConfig) {
        if !config.contains(MeasurementFlags::SHAPE_DESCRIPTORS) {
            return;
        }
        let perimeter = summary.perimeter;
        summary.circularity = if perimeter == 0.0 {
            0.0
        } else {
            (4.0 * std::f64::consts::PI * summary.area / (perimeter * perimeter)).min(1.0)
        };
        if self.roi.as_ref().is_some_and(|roi| !roi.is_area()) {
            summary.solidity = f64::NAN;
            return;
        }
        summary.aspect_ratio = summary.major / summary.minor;
        summary.roundness = 4.0 * summary.area
            / (std::f64::consts::PI * summary.major * summary.major);
        let convex_area = match &self.roi {
            None => summary.pixel_count as f64,
            Some(Roi::Rect(rect)) => f64::from(rect.width) * f64::from(rect.height),
            Some(Roi::Polygon(poly)) => {
                convex_hull_area(&poly.xpoints.iter().copied().zip(poly.ypoints.iter().copied()).collect::<Vec<_>>())
            }
            Some(Roi::Oval(oval)) => oval_convex_area(*oval),
            Some(Roi::Line(_)) => unreachable!(),
        };
        summary.solidity = summary.pixel_count as f64 / convex_area;
    }

    /// `Analyzer.saveResults` uses `roi.getLength()` for selections and raw
    /// image dimensions when there is no ROI (Analyzer.java 556-562).
    fn roi_perimeter(&self) -> f64 {
        let pw = self.calibration.pixel_width;
        let ph = self.calibration.pixel_height;
        match &self.roi {
            None => 2.0 * f64::from(self.width) + 2.0 * f64::from(self.height),
            Some(Roi::Rect(rect)) => {
                2.0 * f64::from(rect.width) * pw + 2.0 * f64::from(rect.height) * ph
            }
            Some(Roi::Oval(oval)) => {
                // OvalRoi.getLength uses Ramanujan's approximation.
                let a = 0.5 * f64::from(oval.width) * pw;
                let b = 0.5 * f64::from(oval.height) * ph;
                std::f64::consts::PI
                    * (3.0 * (a + b) - ((3.0 * a + b) * (a + 3.0 * b)).sqrt())
            }
            Some(Roi::Polygon(poly)) => polygon_perimeter(poly, pw, ph),
            Some(Roi::Line(line)) => {
                let dx = f64::from(line.x2 - line.x1) * pw;
                let dy = f64::from(line.y2 - line.y1) * ph;
                dx.hypot(dy)
            }
        }
    }

    /// `Analyzer.saveResults` stack-position branch (Analyzer.java 660-686).
    fn stack_position(&self) -> StackPosition {
        if self.is_hyperstack() {
            let [channel, slice, frame] = self
                .convert_index_to_position(self.current_slice as i32)
                .unwrap_or([1, 1, 1]);
            StackPosition {
                channel: (self.n_channels > 1).then_some(channel),
                slice: (self.n_slices > 1).then_some(slice),
                frame: (self.n_frames > 1).then_some(frame),
            }
        } else {
            // A plain stack (or a single image) reports the raw slice index.
            StackPosition {
                channel: None,
                slice: Some(self.current_slice),
                frame: None,
            }
        }
    }

    /// ByteStatistics.java: `ip.getHistogram()` is the 256-bin ROI histogram,
    /// and ImageStatistics.getRawStatistics reads mean, std dev, mode and
    /// median back out of that histogram.
    fn measure_byte_like(
        &self,
        pixels: &[u8],
        rx: u32,
        ry: u32,
        rw: u32,
        rh: u32,
        mask: Option<&RoiMask>,
        threshold: Option<ThresholdLevels>,
        limit: Option<ThresholdLevels>,
    ) -> RawMeasurement {
        let mut histogram = vec![0u64; 256];
        for y in ry..(ry + rh) {
            let base = (y * self.width + rx) as usize;
            for offset in 0..rw as usize {
                let x = rx + offset as u32;
                if mask.is_some_and(|m| !m.contains(x, y)) {
                    continue;
                }
                histogram[pixels[base + offset] as usize] += 1;
            }
        }

        // ByteProcessor.setThreshold clamps the stored levels to 0..255
        // (ByteProcessor.java 480-486); ByteStatistics then truncates them
        // with `(int)` (ByteStatistics.java 28-29).
        let (min_bin, max_bin) = match limit {
            None => (0, 255),
            Some(levels) => (
                (levels.lower as i64).clamp(0, 255) as usize,
                (levels.upper as i64).clamp(0, 255) as usize,
            ),
        };
        let pixel_count: u64 = if min_bin <= max_bin {
            histogram[min_bin..=max_bin].iter().sum()
        } else {
            0
        };
        let (min, max) = if pixel_count == 0 {
            (f64::NAN, f64::NAN)
        } else {
            raw_min_max(&histogram, min_bin, max_bin)
        };
        let mode = mode_in_range(&histogram, min_bin, max_bin);
        let mut sum = 0.0;
        let mut sum2 = 0.0;
        if min_bin <= max_bin {
            for (i, &count) in histogram[min_bin..=max_bin].iter().enumerate() {
                let value = (min_bin + i) as f64;
                sum += value * count as f64;
                sum2 += (value * value) * count as f64;
            }
        }
        let mean = sum / pixel_count as f64;
        let median = median_from_hist(&histogram, min_bin, max_bin, pixel_count);
        let area_fraction = area_fraction_from_histogram(&histogram, threshold);
        let accept = |v: f64| v >= min_bin as f64 && v <= max_bin as f64;
        let (skewness, kurtosis, x_center_of_mass, y_center_of_mass) = self.raw_moments(
            rx,
            ry,
            rw,
            rh,
            mean,
            |idx| pixels[idx] as f64,
            true,
            |v| accept(v),
            mask,
        );
        let (x_centroid, y_centroid) =
            self.raw_centroid(rx, ry, rw, rh, |idx| pixels[idx] as f64, |v| accept(v), mask);

        RawMeasurement {
            histogram,
            pixel_count,
            sum,
            sum2,
            min,
            max,
            mode: mode as f64,
            median,
            skewness,
            kurtosis,
            x_centroid,
            y_centroid,
            x_center_of_mass,
            y_center_of_mass,
            area_fraction,
        }
    }

    /// ShortStatistics.java: min/max, median and the displayed mode come from
    /// the ROI histogram. For small selections ShortStatistics uses
    /// ShortProcessor.getHistogram2, whose length is max(256, bboxMax+1) and
    /// whose bbox scan ignores the mask (ShortProcessor.java lines 1069-1123).
    /// An all-false mask therefore reports min=length-1 rather than 65535.
    fn measure_short(
        &self,
        pixels: &[u16],
        rx: u32,
        ry: u32,
        rw: u32,
        rh: u32,
        mask: Option<&RoiMask>,
        threshold: Option<ThresholdLevels>,
        limit: Option<ThresholdLevels>,
    ) -> RawMeasurement {
        let mut histogram = vec![0u64; 65536];
        let mut bbox_max = 0u16;
        for y in ry..(ry + rh) {
            let base = (y * self.width + rx) as usize;
            for offset in 0..rw as usize {
                let x = rx + offset as u32;
                let value = pixels[base + offset];
                if value > bbox_max {
                    bbox_max = value;
                }
                if mask.is_some_and(|m| !m.contains(x, y)) {
                    continue;
                }
                histogram[value as usize] += 1;
            }
        }

        // ShortStatistics only takes the getHistogram2 shortcut for ROIs
        // smaller than 250,000 pixels (ShortStatistics.java lines 35-37).
        let hist_len = if (rw as u64) * (rh as u64) < 250_000 {
            (bbox_max as usize + 1).max(256)
        } else {
            65536
        };
        // ShortStatistics.java 30-39 truncates the threshold with `(int)` and
        // clips only the upper bound to the histogram length.
        let (min_threshold, max_threshold) = match limit {
            None => (0i64, hist_len as i64 - 1),
            Some(levels) => (
                levels.lower as i64,
                (levels.upper as i64).min(hist_len as i64 - 1),
            ),
        };
        let (min_bin, max_bin) = short_raw_min_max(
            &histogram,
            min_threshold,
            max_threshold,
            hist_len,
        );
        let in_range = min_bin <= max_bin;
        let pixel_count: u64 = if in_range {
            histogram[min_bin..=max_bin].iter().sum()
        } else {
            0
        };
        let mode = mode_in_range(&histogram, min_bin, max_bin);
        let mut sum = 0.0;
        let mut sum2 = 0.0;
        // ShortStatistics.getStatistics accumulates over the occupied raw range.
        if in_range {
            for (i, &count) in histogram[min_bin..=max_bin].iter().enumerate() {
                let value = (min_bin + i) as f64;
                sum += value * count as f64;
                sum2 += (value * value) * count as f64;
            }
        }
        let mean = sum / pixel_count as f64;
        let median = median_from_hist(
            &histogram,
            min_threshold.max(0) as usize,
            max_threshold.max(0) as usize,
            pixel_count,
        );
        let area_fraction = area_fraction_from_histogram(&histogram, threshold);
        let accept = |v: f64| {
            v >= min_threshold as f64 && v <= max_threshold as f64
        };
        let (skewness, kurtosis, x_center_of_mass, y_center_of_mass) = self.raw_moments(
            rx,
            ry,
            rw,
            rh,
            mean,
            |idx| pixels[idx] as f64,
            false,
            |v| accept(v),
            mask,
        );
        let (x_centroid, y_centroid) =
            self.raw_centroid(rx, ry, rw, rh, |idx| pixels[idx] as f64, |v| accept(v), mask);

        // ShortStatistics reports the raw bin indices from getRawMinAndMax even
        // when the threshold range is empty (min/max are not NaN there).
        let (min, max) = (min_bin as f64, max_bin as f64);
        RawMeasurement {
            histogram,
            pixel_count,
            sum,
            sum2,
            min,
            max,
            mode: mode as f64,
            median,
            skewness,
            kurtosis,
            x_centroid,
            y_centroid,
            x_center_of_mass,
            y_center_of_mass,
            area_fraction,
        }
    }

    /// FloatStatistics.java: 256 bins spanning the ROI min..max. Pixels outside
    /// that range (including NaN) do not contribute to pixelCount or the bins.
    fn measure_float(
        &self,
        pixels: &[f32],
        rx: u32,
        ry: u32,
        rw: u32,
        rh: u32,
        mask: Option<&RoiMask>,
        threshold: Option<ThresholdLevels>,
        limit: Option<ThresholdLevels>,
    ) -> RawMeasurement {
        // FloatStatistics.java 25-31: with LIMIT the stored double threshold is
        // used verbatim; otherwise the full ±Float.MAX_VALUE range applies.
        let (min_threshold, max_threshold) = limit.map_or(
            (-(f32::MAX as f64), f32::MAX as f64),
            |levels| (levels.lower, levels.upper),
        );

        let mut roi_min = f64::MAX;
        let mut roi_max = -f64::MAX;
        for y in ry..(ry + rh) {
            let base = (y * self.width + rx) as usize;
            for offset in 0..rw as usize {
                let x = rx + offset as u32;
                if mask.is_some_and(|m| !m.contains(x, y)) {
                    continue;
                }
                let v = pixels[base + offset] as f64;
                if v >= min_threshold && v <= max_threshold {
                    if v < roi_min {
                        roi_min = v;
                    }
                    if v > roi_max {
                        roi_max = v;
                    }
                }
            }
        }

        let min = roi_min;
        let max = roi_max;
        let hist_min = min;
        let hist_max = max;
        let bin_size = (hist_max - hist_min) / 256.0;
        let scale = 256.0 / (hist_max - hist_min);

        let mut histogram = vec![0u64; 256];
        let mut pixel_count = 0u64;
        let mut sum = 0.0;
        let mut sum2 = 0.0;
        for y in ry..(ry + rh) {
            let base = (y * self.width + rx) as usize;
            for offset in 0..rw as usize {
                let x = rx + offset as u32;
                if mask.is_some_and(|m| !m.contains(x, y)) {
                    continue;
                }
                let v = pixels[base + offset] as f64;
                if v >= min_threshold && v <= max_threshold && v >= hist_min && v <= hist_max {
                    pixel_count += 1;
                    sum += v;
                    sum2 += v * v;
                    let mut index = (scale * (v - hist_min)) as i32;
                    if index >= 256 {
                        index = 255;
                    }
                    histogram[index as usize] += 1;
                }
            }
        }

        let mean = sum / pixel_count as f64;
        let (mode_index, _) = mode_from_hist(&histogram);
        let mut mode = hist_min + mode_index as f64 * bin_size;
        // FloatStatistics.getMode adds half a bin when the bins are not unit-sized.
        if bin_size != 1.0 {
            mode += bin_size / 2.0;
        }

        let mut sorted = Vec::with_capacity(pixel_count as usize);
        for y in ry..(ry + rh) {
            let base = (y * self.width + rx) as usize;
            for offset in 0..rw as usize {
                let x = rx + offset as u32;
                if mask.is_some_and(|m| !m.contains(x, y)) {
                    continue;
                }
                let v = pixels[base + offset];
                let vf = v as f64;
                if vf >= min_threshold && vf <= max_threshold {
                    sorted.push(v);
                }
            }
        }
        sorted.sort_by(|a, b| a.total_cmp(b));
        let median = if pixel_count == 0 {
            f64::NAN
        } else if sorted.len() % 2 == 0 {
            let middle = sorted.len() / 2;
            // FloatStatistics.getMedian averages in float, not double arithmetic.
            ((sorted[middle - 1] + sorted[middle]) / 2.0_f32) as f64
        } else {
            sorted[sorted.len() / 2] as f64
        };

        let (skewness, kurtosis, x_center_of_mass, y_center_of_mass) = self.raw_moments(
            rx,
            ry,
            rw,
            rh,
            mean,
            |idx| pixels[idx] as f64,
            true,
            |v| v >= min_threshold && v <= max_threshold,
            mask,
        );
        // FloatStatistics.getCentroid also skips NaN and infinities.
        let (x_centroid, y_centroid) = self.raw_centroid(
            rx,
            ry,
            rw,
            rh,
            |idx| pixels[idx] as f64,
            |v| v >= min_threshold && v <= max_threshold,
            mask,
        );
        let area_fraction = self.area_fraction_float(pixels, rx, ry, rw, rh, mask, threshold);

        RawMeasurement {
            histogram,
            pixel_count,
            sum,
            sum2,
            min,
            max,
            mode,
            median,
            skewness,
            kurtosis,
            x_centroid,
            y_centroid,
            x_center_of_mass,
            y_center_of_mass,
            area_fraction,
        }
    }

    /// ColorStatistics.java: mean/min/max/mode/median come from the rounded
    /// luminance histogram, while moments use ColorProcessor.getPixelValue,
    /// which is an unrounded weighted average cast to float.
    fn measure_rgb(
        &self,
        pixels: &[u32],
        rx: u32,
        ry: u32,
        rw: u32,
        rh: u32,
        mask: Option<&RoiMask>,
    ) -> RawMeasurement {
        let mut histogram = vec![0u64; 256];
        for y in ry..(ry + rh) {
            let base = (y * self.width + rx) as usize;
            for offset in 0..rw as usize {
                let x = rx + offset as u32;
                if mask.is_some_and(|m| !m.contains(x, y)) {
                    continue;
                }
                histogram[luminance_bin(pixels[base + offset])] += 1;
            }
        }

        let pixel_count: u64 = histogram.iter().sum();
        let (min, max) = raw_min_max(&histogram, 0, 255);
        let (mode, _) = mode_from_hist(&histogram);
        let mut sum = 0.0;
        let mut sum2 = 0.0;
        for (i, &count) in histogram.iter().enumerate() {
            let value = i as f64;
            sum += value * count as f64;
            sum2 += (value * value) * count as f64;
        }
        let mean = sum / pixel_count as f64;
        let median = median_from_hist(&histogram, 0, 255, pixel_count);
        let (skewness, kurtosis, x_center_of_mass, y_center_of_mass) = self.raw_moments(
            rx,
            ry,
            rw,
            rh,
            mean,
            |idx| {
                let c = pixels[idx];
                let r = ((c >> 16) & 0xff) as f64;
                let g = ((c >> 8) & 0xff) as f64;
                let b = (c & 0xff) as f64;
                let weight = 1.0 / 3.0;
                ((r * weight + g * weight + b * weight) as f32) as f64
            },
            false,
            |_| true,
            mask,
        );
        let (x_centroid, y_centroid) =
            self.raw_centroid(rx, ry, rw, rh, |idx| pixels[idx] as f64, |_| true, mask);

        RawMeasurement {
            histogram,
            pixel_count,
            sum,
            sum2,
            min,
            max,
            mode: mode as f64,
            median,
            skewness,
            kurtosis,
            x_centroid,
            y_centroid,
            x_center_of_mass,
            y_center_of_mass,
            // ColorStatistics never calls calculateAreaFraction.
            area_fraction: 0.0,
        }
    }

    /// `FloatStatistics.calculateAreaFraction` (FloatStatistics.java 200-226).
    /// The threshold comparison is done in float, and NaN samples count as
    /// non-zero because `NaN != 0f` is true in Java.
    fn area_fraction_float(
        &self,
        pixels: &[f32],
        rx: u32,
        ry: u32,
        rw: u32,
        rh: u32,
        mask: Option<&RoiMask>,
        threshold: Option<ThresholdLevels>,
    ) -> f64 {
        let (lower, upper) = threshold.map_or((0.0, 0.0), |levels| {
            (levels.lower as f32, levels.upper as f32)
        });
        let mut sum = 0u64;
        let mut total = 0u64;
        for y in ry..(ry + rh) {
            let base = (y * self.width + rx) as usize;
            for offset in 0..rw as usize {
                let x = rx + offset as u32;
                if mask.is_some_and(|m| !m.contains(x, y)) {
                    continue;
                }
                let value = pixels[base + offset];
                total += 1;
                let counted = match threshold {
                    None => value != 0.0,
                    Some(_) => value >= lower && value <= upper,
                };
                if counted {
                    sum += 1;
                }
            }
        }
        sum as f64 * 100.0 / total as f64
    }

    fn raw_centroid<F, P>(
        &self,
        rx: u32,
        ry: u32,
        rw: u32,
        rh: u32,
        value_at: F,
        accept: P,
        mask: Option<&RoiMask>,
    ) -> (f64, f64)
    where
        F: Fn(usize) -> f64,
        P: Fn(f64) -> bool,
    {
        let mut count = 0u64;
        let mut xsum = 0.0;
        let mut ysum = 0.0;
        for y in ry..(ry + rh) {
            for x in rx..(rx + rw) {
                if mask.is_some_and(|m| !m.contains(x, y)) {
                    continue;
                }
                let idx = (y * self.width + x) as usize;
                if accept(value_at(idx)) {
                    count += 1;
                    xsum += x as f64;
                    ysum += y as f64;
                }
            }
        }
        (xsum / count as f64 + 0.5, ysum / count as f64 + 0.5)
    }

    fn raw_moments<F, P>(
        &self,
        rx: u32,
        ry: u32,
        rw: u32,
        rh: u32,
        mean: f64,
        value_at: F,
        add_java_min: bool,
        accept: P,
        mask: Option<&RoiMask>,
    ) -> (f64, f64, f64, f64)
    where
        F: Fn(usize) -> f64,
        P: Fn(f64) -> bool,
    {
        let mut sum1 = 0.0;
        let mut sum2 = 0.0;
        let mut sum3 = 0.0;
        let mut sum4 = 0.0;
        let mut xsum = 0.0;
        let mut ysum = 0.0;
        let mut count = 0u64;
        for y in ry..(ry + rh) {
            for x in rx..(rx + rw) {
                if mask.is_some_and(|m| !m.contains(x, y)) {
                    continue;
                }
                let idx = (y * self.width + x) as usize;
                let mut v = value_at(idx);
                if add_java_min {
                    // ByteStatistics.java line 135 / FloatStatistics.java line 145:
                    // Double.MIN_VALUE keeps a zero pixel's moment weight non-zero.
                    v += f64::from_bits(1);
                }
                if !accept(v) {
                    continue;
                }
                count += 1;
                let v2 = v * v;
                sum1 += v;
                sum2 += v2;
                sum3 += v * v2;
                sum4 += v2 * v2;
                xsum += x as f64 * v;
                ysum += y as f64 * v;
            }
        }
        finish_moments(count as f64, mean, sum1, sum2, sum3, sum4, xsum, ysum)
    }
}

fn crop_rows<T: Copy>(
    src: &[T],
    source_width: usize,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
) -> Vec<T> {
    let mut out = Vec::with_capacity(width * height);
    for row in 0..height {
        let start = (y + row) * source_width + x;
        out.extend_from_slice(&src[start..start + width]);
    }
    out
}

fn crop_plane(
    plane: &PixelPlane,
    source_width: u32,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> PixelPlane {
    let source_width = source_width as usize;
    let (x, y, width, height) = (x as usize, y as usize, width as usize, height as usize);
    match plane {
        PixelPlane::Gray8(pixels) => PixelPlane::Gray8(crop_rows(
            pixels,
            source_width,
            x,
            y,
            width,
            height,
        )),
        PixelPlane::Color256 { pixels, lut } => PixelPlane::Color256 {
            pixels: crop_rows(pixels, source_width, x, y, width, height),
            lut: lut.clone(),
        },
        PixelPlane::Gray16(pixels) => PixelPlane::Gray16(crop_rows(
            pixels,
            source_width,
            x,
            y,
            width,
            height,
        )),
        PixelPlane::Gray32(pixels) => PixelPlane::Gray32(crop_rows(
            pixels,
            source_width,
            x,
            y,
            width,
            height,
        )),
        PixelPlane::ColorRgb(pixels) => PixelPlane::ColorRgb(crop_rows(
            pixels,
            source_width,
            x,
            y,
            width,
            height,
        )),
    }
}

struct RawMeasurement {
    histogram: Vec<u64>,
    pixel_count: u64,
    sum: f64,
    sum2: f64,
    min: f64,
    max: f64,
    mode: f64,
    median: f64,
    skewness: f64,
    kurtosis: f64,
    x_centroid: f64,
    y_centroid: f64,
    x_center_of_mass: f64,
    y_center_of_mass: f64,
    area_fraction: f64,
}

fn luminance_bin(pixel: u32) -> usize {
    let r = ((pixel >> 16) & 0xff) as f64;
    let g = ((pixel >> 8) & 0xff) as f64;
    let b = (pixel & 0xff) as f64;
    let weight = 1.0 / 3.0;
    // ColorProcessor.getHistogram casts the rounded weighted average to int.
    (r * weight + g * weight + b * weight + 0.5) as usize
}

fn raw_min_max(histogram: &[u64], min_threshold: usize, max_threshold: usize) -> (f64, f64) {
    let mut min = min_threshold;
    if min < histogram.len() {
        // ImageStatistics/ShortStatistics stop at the last usable bin index,
        // which is maxThreshold for the truncated getHistogram2 histogram.
        while histogram[min] == 0 && min < max_threshold {
            min += 1;
        }
    }
    let mut max = max_threshold;
    while histogram[max] == 0 && max > 0 {
        max -= 1;
    }
    (min as f64, max as f64)
}

/// `ShortStatistics.getRawMinAndMax` (ShortStatistics.java 66-77). The scan
/// starts at the truncated threshold and, unlike `raw_min_max`, may walk the
/// lower bound past the histogram length when the threshold excludes every
/// pixel (the caller then reports an empty range).
fn short_raw_min_max(
    histogram: &[u64],
    min_threshold: i64,
    max_threshold: i64,
    hist_len: usize,
) -> (usize, usize) {
    let hist_len = hist_len as i64;
    let mut min = min_threshold;
    if min >= 0 && min < hist_len {
        while histogram[min as usize] == 0 && min < hist_len - 1 {
            min += 1;
        }
    }
    let mut max = max_threshold;
    while max > 0 && histogram[max as usize] == 0 {
        max -= 1;
    }
    (min.max(0) as usize, max.max(0) as usize)
}

/// `dmode` from the restricted `ImageStatistics` accumulation loop: the first
/// value with the highest count, or 0 when the range is empty
/// (ImageStatistics.java 129-132, ShortStatistics.java 101-106).
fn mode_in_range(histogram: &[u64], min: usize, max: usize) -> usize {
    let mut max_count = 0u64;
    let mut mode = 0usize;
    if min <= max && max < histogram.len() {
        for (i, &count) in histogram[min..=max].iter().enumerate() {
            if count > max_count {
                max_count = count;
                mode = min + i;
            }
        }
    }
    mode
}

fn mode_from_hist(histogram: &[u64]) -> (usize, u64) {
    let mut max_count = 0u64;
    let mut mode = 0usize;
    for (i, &count) in histogram.iter().enumerate() {
        if count > max_count {
            max_count = count;
            mode = i;
        }
    }
    (mode, max_count)
}

fn median_from_hist(histogram: &[u64], first: usize, last: usize, pixel_count: u64) -> f64 {
    if pixel_count == 0 || last > histogram.len() {
        return f64::NAN;
    }
    let half_count = pixel_count as f64 / 2.0;
    let mut sum = 0u64;
    let mut i = first;
    loop {
        sum += histogram[i];
        if !((sum as f64) <= half_count && i < last) {
            break;
        }
        i += 1;
    }
    i as f64
}

fn calculate_std_dev(n: f64, sum: f64, sum2: f64) -> f64 {
    if n > 0.0 {
        let mut std_dev = (n * sum2 - sum * sum) / n;
        if std_dev > 0.0 {
            std_dev = (std_dev / (n - 1.0)).sqrt();
        } else {
            std_dev = 0.0;
        }
        std_dev
    } else {
        0.0
    }
}

/// Java's `Math.round(double)` is `floor(x+0.5)`, unlike Rust's
/// round-half-away-from-zero.
fn java_round(value: f64) -> i64 {
    (value + 0.5).floor() as i64
}

/// `ImageStatistics.calculateAreaFraction` (ImageStatistics.java 276-293) for
/// the 8-bit and 16-bit histogram paths. Without a threshold the numerator is
/// every non-zero bin; with one it is the bins inside `[round(min), round(max)]`.
fn area_fraction_from_histogram(histogram: &[u64], threshold: Option<ThresholdLevels>) -> f64 {
    let total: u64 = histogram.iter().sum();
    let sum = match threshold {
        None => total.saturating_sub(histogram.first().copied().unwrap_or(0)),
        Some(levels) => {
            let lower = java_round(levels.lower);
            let upper = java_round(levels.upper);
            histogram
                .iter()
                .enumerate()
                .filter(|(index, _)| {
                    let value = *index as i64;
                    value >= lower && value <= upper
                })
                .map(|(_, count)| *count)
                .sum()
        }
    };
    sum as f64 * 100.0 / total as f64
}

fn finish_moments(
    n: f64,
    mean: f64,
    sum1: f64,
    sum2: f64,
    sum3: f64,
    sum4: f64,
    xsum: f64,
    ysum: f64,
) -> (f64, f64, f64, f64) {
    let mean2 = mean * mean;
    let variance = sum2 / n - mean2;
    let s_deviation = variance.sqrt();
    let skewness = ((sum3 - 3.0 * mean * sum2) / n + 2.0 * mean * mean2) / (variance * s_deviation);
    let kurtosis = (((sum4 - 4.0 * mean * sum3 + 6.0 * mean2 * sum2) / n - 3.0 * mean2 * mean2)
        / (variance * variance))
        - 3.0;
    (skewness, kurtosis, xsum / sum1 + 0.5, ysum / sum1 + 0.5)
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LineMeasurement {
    /// Calibrated endpoint distance (`Line.getLength`).
    pub length: f64,
    /// Degrees between the line and a horizontal axis (`Roi.getFloatAngle`).
    pub angle: f64,
}

/// Bit flags from `ij.measure.Measurements` (original_imagej/ij/measure/
/// Measurements.java lines 4-10). `SHAPE_DESCRIPTORS` and `CIRCULARITY`
/// intentionally share the same bit, exactly as in ImageJ 1.54u.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MeasurementFlags;

impl MeasurementFlags {
    pub const AREA: u32 = 1;
    pub const MEAN: u32 = 2;
    pub const STD_DEV: u32 = 4;
    pub const MODE: u32 = 8;
    pub const MIN_MAX: u32 = 16;
    pub const CENTROID: u32 = 32;
    pub const CENTER_OF_MASS: u32 = 64;
    pub const PERIMETER: u32 = 128;
    pub const LIMIT: u32 = 256;
    pub const RECT: u32 = 512;
    pub const LABELS: u32 = 1024;
    pub const ELLIPSE: u32 = 2048;
    pub const INVERT_Y: u32 = 4096;
    pub const CIRCULARITY: u32 = 8192;
    pub const SHAPE_DESCRIPTORS: u32 = 8192;
    pub const FERET: u32 = 16384;
    pub const INTEGRATED_DENSITY: u32 = 0x8000;
    pub const MEDIAN: u32 = 0x10000;
    pub const SKEWNESS: u32 = 0x20000;
    pub const KURTOSIS: u32 = 0x40000;
    pub const AREA_FRACTION: u32 = 0x80000;
    pub const SLICE: u32 = 0x100000;
    pub const STACK_POSITION: u32 = 0x100000;
    pub const SCIENTIFIC_NOTATION: u32 = 0x200000;
    pub const ADD_TO_OVERLAY: u32 = 0x400000;
    pub const NAN_EMPTY_CELLS: u32 = 0x800000;

    /// `Measurements.ALL_STATS` (Measurements.java lines 16-19).
    pub const ALL_STATS: u32 = Self::AREA
        + Self::MEAN
        + Self::STD_DEV
        + Self::MODE
        + Self::MIN_MAX
        + Self::CENTROID
        + Self::CENTER_OF_MASS
        + Self::PERIMETER
        + Self::RECT
        + Self::ELLIPSE
        + Self::SHAPE_DESCRIPTORS
        + Self::FERET
        + Self::INTEGRATED_DENSITY
        + Self::MEDIAN
        + Self::SKEWNESS
        + Self::KURTOSIS
        + Self::AREA_FRACTION;
}

/// State of `Analyze > Set Measurements...`: the `Measurements` bit flags and
/// the decimal-place count. The default matches ImageJ's startup state
/// (Analyzer.java line 42: Area, Mean and Min/Max at three decimals).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MeasurementConfig {
    pub flags: u32,
    pub precision: u8,
}

impl Default for MeasurementConfig {
    fn default() -> Self {
        Self {
            flags: MeasurementFlags::AREA | MeasurementFlags::MEAN | MeasurementFlags::MIN_MAX,
            precision: 3,
        }
    }
}

impl MeasurementConfig {
    pub fn contains(&self, flag: u32) -> bool {
        self.flags & flag != 0
    }
}

/// Values `Analyzer.saveResults` writes to the `Ch`, `Slice` and `Frame`
/// columns (Analyzer.java lines 660-686). `None` means ImageJ omits that
/// column for this row, either because the dimension is 1 or because the
/// image is not a hyperstack.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StackPosition {
    pub channel: Option<u32>,
    pub slice: Option<u32>,
    pub frame: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MeasurementSummary {
    pub pixel_count: u64,
    pub area: f64,
    pub mean: f64,
    pub std_dev: f64,
    pub min: f64,
    pub max: f64,
    pub x_centroid: f64,
    pub y_centroid: f64,
    pub x_center_of_mass: f64,
    pub y_center_of_mass: f64,
    pub roi_x: f64,
    pub roi_y: f64,
    pub roi_width: f64,
    pub roi_height: f64,
    /// `Roi.getLength()` as written to the `Perim.` column.
    #[serde(default)]
    pub perimeter: f64,
    #[serde(default)]
    pub major: f64,
    #[serde(default)]
    pub minor: f64,
    #[serde(default)]
    pub ellipse_angle: f64,
    #[serde(default)]
    pub circularity: f64,
    #[serde(default)]
    pub aspect_ratio: f64,
    #[serde(default)]
    pub roundness: f64,
    #[serde(default)]
    pub solidity: f64,
    pub int_den: f64,
    pub raw_int_den: f64,
    /// ROI histogram: 256 bins for 8-bit, RGB and 32-bit planes and the full
    /// 65,536 bins for 16-bit planes.
    pub histogram: Vec<u64>,
    /// Displayed modal gray value (`ImageStatistics.dmode`), not the bin index
    /// reported by `ImageStatistics.mode`.
    pub mode: f64,
    /// Median gray value from the 256-bin or 65,536-bin ROI histogram.
    pub median: f64,
    pub skewness: f64,
    pub kurtosis: f64,
    /// `%Area` (`ImageStatistics.areaFraction`): percentage of ROI pixels
    /// inside the active threshold, or non-zero pixels when no threshold is
    /// active. RGB images leave this at 0 because `ColorStatistics` never
    /// calls `calculateAreaFraction`.
    pub area_fraction: f64,
    /// `Ch`/`Slice`/`Frame` values for the `STACK_POSITION` columns.
    pub stack_position: StackPosition,
    /// Results-table-only `Length`/`Angle` values for line measurements.
    /// Area measurements leave these cells unset, exactly like ImageJ's
    /// default Results table.
    pub line: Option<LineMeasurement>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ResultsTable {
    pub rows: Vec<MeasurementSummary>,
    /// `Analyze > Set Measurements...` state applied to this table. Defaults
    /// to ImageJ's startup flags so `to_tsv()` stays byte-compatible with
    /// `to_tsv_basic()` (Analyzer.java line 40).
    #[serde(default)]
    pub config: MeasurementConfig,
}

/// `ResultsTable.defaultHeadings` (ResultsTable.java 42-45) plus the
/// dynamically allocated `Length` column at index 38. `Angle` reserves index
/// 17, so it precedes `Length` in every table that has both.
const RESULT_HEADINGS: [&str; 39] = [
    "Area",
    "Mean",
    "StdDev",
    "Mode",
    "Min",
    "Max",
    "X",
    "Y",
    "XM",
    "YM",
    "Perim.",
    "BX",
    "BY",
    "Width",
    "Height",
    "Major",
    "Minor",
    "Angle",
    "Circ.",
    "Feret",
    "IntDen",
    "Median",
    "Skew",
    "Kurt",
    "%Area",
    "RawIntDen",
    "Ch",
    "Slice",
    "Frame",
    "FeretX",
    "FeretY",
    "FeretAngle",
    "MinFeret",
    "AR",
    "Round",
    "Solidity",
    "MinThr",
    "MaxThr",
    "Length",
];

const COLUMN_ANGLE: usize = 17;
const COLUMN_CIRCULARITY: usize = 18;
const COLUMN_ASPECT_RATIO: usize = 33;
const COLUMN_ROUNDNESS: usize = 34;
const COLUMN_SOLIDITY: usize = 35;
const COLUMN_LENGTH: usize = 38;

/// Flags for which `Analyzer.measureLength` takes the full `saveResults` path.
/// When none is selected, only `Length` and `Angle` are written
/// (Analyzer.java 464-474).
const LINE_STAT_FLAGS: u32 = MeasurementFlags::AREA
    | MeasurementFlags::MEAN
    | MeasurementFlags::STD_DEV
    | MeasurementFlags::MODE
    | MeasurementFlags::MIN_MAX
    | MeasurementFlags::CENTROID
    | MeasurementFlags::MEDIAN;

impl ResultsTable {
    pub fn new() -> Self {
        Self {
            rows: Vec::new(),
            config: MeasurementConfig::default(),
        }
    }

    pub fn push(&mut self, row: MeasurementSummary) {
        self.rows.push(row);
    }

    pub fn clear(&mut self) {
        self.rows.clear();
    }

    /// Emits the selected columns in `ResultsTable.defaultHeadings` order with
    /// ImageJ's per-column formatting rules (ResultsTable.java 198-218,
    /// 644-667, 700-733 and 807-964; Analyzer.java 544-703).
    pub fn to_tsv(&self) -> String {
        let precision = i32::from(self.config.precision.min(9));
        // Analyzer.java 62 and 86 pass a negative precision to
        // ResultsTable.setPrecision when Scientific Notation is enabled.
        let table_precision = if self.config.contains(MeasurementFlags::SCIENTIFIC_NOTATION) {
            -precision
        } else {
            precision
        };

        let mut present = [false; RESULT_HEADINGS.len()];
        let mut fixed_places: [Option<i32>; RESULT_HEADINGS.len()] =
            [None; RESULT_HEADINGS.len()];

        if self.rows.is_empty() {
            // An empty table still has default headings in Rust's slice-1
            // output, so derive the header from the flags.
            for column in flag_columns(self.config) {
                present[column] = true;
            }
        }

        for (row_index, row) in self.rows.iter().enumerate() {
            let cells = row.table_cells(self.config);
            for (column, value) in cells.iter().enumerate() {
                let Some(value) = value else { continue };
                present[column] = true;
                // addValue only fixes a column's decimal places while the
                // counter is below 25 (ResultsTable.java 214-217).
                if row_index < 24 && !value.is_nan() && !is_java_int(*value) {
                    fixed_places[column] = Some(table_precision);
                }
            }
            // Shape descriptor columns get the positive precision on the
            // first row regardless of their value (Analyzer.java 590-595).
            // A first line row that measured only Length/Angle never reaches
            // that branch (Analyzer.java 464-474).
            if row_index == 0
                && self.config.contains(MeasurementFlags::SHAPE_DESCRIPTORS)
                && !(row.line.is_some() && !row.has_line_stat_columns(self.config))
            {
                for column in [
                    COLUMN_CIRCULARITY,
                    COLUMN_ASPECT_RATIO,
                    COLUMN_ROUNDNESS,
                    COLUMN_SOLIDITY,
                ] {
                    fixed_places[column] = Some(precision);
                }
            }
        }

        let mut out = String::from(" \t");
        let mut first = true;
        for (column, included) in present.iter().enumerate() {
            if !*included {
                continue;
            }
            if !first {
                out.push('\t');
            }
            first = false;
            out.push_str(RESULT_HEADINGS[column]);
        }
        out.push('\n');

        for (row_index, row) in self.rows.iter().enumerate() {
            let cells = row.table_cells(self.config);
            out.push_str(&(row_index + 1).to_string());
            for (column, included) in present.iter().enumerate() {
                if !*included {
                    continue;
                }
                out.push('\t');
                let value = cells[column].unwrap_or(0.0);
                out.push_str(&format_results_cell(
                    value,
                    fixed_places[column],
                    table_precision,
                ));
            }
            out.push('\n');
        }
        out
    }

    pub fn to_tsv_basic(&self) -> String {
        let has_line = self.rows.iter().any(|row| row.line.is_some());
        // ResultsTable's default headings reserve indexes 0..36, so "Angle"
        // (17) is emitted before the dynamically allocated "Length" column
        // (ResultsTable.java 34-42).
        let columns: usize = if has_line { 6 } else { 4 };
        let mut out = String::from(" \tArea\tMean\tMin\tMax");
        if has_line {
            out.push_str("\tAngle\tLength");
        }
        out.push('\n');

        // ResultsTable.addValue switches a whole column to three decimals as
        // soon as one of its first 24 values is fractional
        // (ResultsTable.java 214-217). Area-only tables keep the historical
        // per-value AUTO_FORMAT so existing area output is unchanged.
        let mut fixed_places = [false; 6];
        if has_line {
            for row in self.rows.iter().take(24) {
                for (index, value) in row.tsv_values().iter().enumerate() {
                    if (*value as i32) as f64 != *value && !value.is_nan() {
                        fixed_places[index] = true;
                    }
                }
            }
        }

        for (i, row) in self.rows.iter().enumerate() {
            out.push_str(&(i + 1).to_string());
            for (index, value) in row.tsv_values().iter().take(columns).enumerate() {
                out.push('\t');
                if fixed_places[index] {
                    out.push_str(&format_ij_num_fixed(*value, 3));
                } else {
                    out.push_str(&format_ij_num(*value));
                }
            }
            out.push('\n');
        }
        out
    }
}

impl MeasurementSummary {
    /// Values in ResultsTable default-heading order: Area, Mean, Min, Max,
    /// Angle, Length. Area rows leave the line-only cells unset (zero), which
    /// is how ImageJ renders a column that a row never wrote
    /// (ResultsTable.java 720-731).
    fn tsv_values(&self) -> [f64; 6] {
        let (angle, length) = self
            .line
            .map_or((0.0, 0.0), |line| (line.angle, line.length));
        [self.area, self.mean, self.min, self.max, angle, length]
    }

    fn has_line_stat_columns(&self, config: MeasurementConfig) -> bool {
        config.flags & LINE_STAT_FLAGS != 0
    }

    /// Cells this row actually writes, indexed by `ResultsTable`'s default
    /// headings. Unwritten cells stay `None` and render as the zero that
    /// `ResultsTable.addValue` would leave in an all-zero column
    /// (ResultsTable.java 205-213, 720-731).
    fn table_cells(&self, config: MeasurementConfig) -> [Option<f64>; 39] {
        let mut cells = [None; 39];
        if self.line.is_some() && !self.has_line_stat_columns(config) {
            // Analyzer.measureLength's early return writes only these two cells
            // (Analyzer.java 464-474).
            if let Some(line) = self.line {
                cells[COLUMN_ANGLE] = Some(line.angle);
                cells[COLUMN_LENGTH] = Some(line.length);
            }
            return cells;
        }

        if config.contains(MeasurementFlags::AREA) {
            cells[0] = Some(self.area);
        }
        if config.contains(MeasurementFlags::MEAN) {
            cells[1] = Some(self.mean);
        }
        if config.contains(MeasurementFlags::STD_DEV) {
            cells[2] = Some(self.std_dev);
        }
        if config.contains(MeasurementFlags::MODE) {
            cells[3] = Some(self.mode);
        }
        if config.contains(MeasurementFlags::MIN_MAX) {
            cells[4] = Some(self.min);
            cells[5] = Some(self.max);
        }
        if config.contains(MeasurementFlags::CENTROID) {
            cells[6] = Some(self.x_centroid);
            cells[7] = Some(self.y_centroid);
        }
        if config.contains(MeasurementFlags::CENTER_OF_MASS) {
            cells[8] = Some(self.x_center_of_mass);
            cells[9] = Some(self.y_center_of_mass);
        }
        if config.contains(MeasurementFlags::PERIMETER) {
            cells[10] = Some(self.perimeter);
        }
        if config.contains(MeasurementFlags::RECT) {
            cells[11] = Some(self.roi_x);
            cells[12] = Some(self.roi_y);
            cells[13] = Some(self.roi_width);
            cells[14] = Some(self.roi_height);
        }
        if config.contains(MeasurementFlags::ELLIPSE) {
            cells[15] = Some(self.major);
            cells[16] = Some(self.minor);
            cells[COLUMN_ANGLE] = Some(self.ellipse_angle);
        }
        if config.contains(MeasurementFlags::SHAPE_DESCRIPTORS) {
            cells[COLUMN_CIRCULARITY] = Some(self.circularity);
            cells[COLUMN_ASPECT_RATIO] = Some(self.aspect_ratio);
            cells[COLUMN_ROUNDNESS] = Some(self.roundness);
            cells[COLUMN_SOLIDITY] = Some(self.solidity);
        }
        if config.contains(MeasurementFlags::INTEGRATED_DENSITY) {
            cells[20] = Some(self.int_den);
            cells[25] = Some(self.raw_int_den);
        }
        if config.contains(MeasurementFlags::MEDIAN) {
            cells[21] = Some(self.median);
        }
        if config.contains(MeasurementFlags::SKEWNESS) {
            cells[22] = Some(self.skewness);
        }
        if config.contains(MeasurementFlags::KURTOSIS) {
            cells[23] = Some(self.kurtosis);
        }
        if config.contains(MeasurementFlags::AREA_FRACTION) {
            cells[24] = Some(self.area_fraction);
        }
        if config.contains(MeasurementFlags::STACK_POSITION) {
            // Ch/Slice/Frame are default headings 26-28 and only appear for
            // dimensions larger than one (Analyzer.java 660-686).
            if let Some(channel) = self.stack_position.channel {
                cells[26] = Some(f64::from(channel));
            }
            if let Some(slice) = self.stack_position.slice {
                cells[27] = Some(f64::from(slice));
            }
            if let Some(frame) = self.stack_position.frame {
                cells[28] = Some(f64::from(frame));
            }
        }
        if let Some(line) = self.line {
            // saveResults appends these after the statistics cells; the line
            // angle overwrites the ellipse angle at index 17
            // (Analyzer.java 687-697).
            cells[COLUMN_ANGLE] = Some(line.angle);
            cells[COLUMN_LENGTH] = Some(line.length);
        }
        cells
    }
}

fn flag_columns(config: MeasurementConfig) -> Vec<usize> {
    let mut columns = Vec::new();
    let mut add = |flag: u32, column: usize| {
        if config.contains(flag) {
            columns.push(column);
        }
    };
    add(MeasurementFlags::AREA, 0);
    add(MeasurementFlags::MEAN, 1);
    add(MeasurementFlags::STD_DEV, 2);
    add(MeasurementFlags::MODE, 3);
    add(MeasurementFlags::MIN_MAX, 4);
    add(MeasurementFlags::MIN_MAX, 5);
    add(MeasurementFlags::CENTROID, 6);
    add(MeasurementFlags::CENTROID, 7);
    add(MeasurementFlags::CENTER_OF_MASS, 8);
    add(MeasurementFlags::CENTER_OF_MASS, 9);
    add(MeasurementFlags::PERIMETER, 10);
    add(MeasurementFlags::RECT, 11);
    add(MeasurementFlags::RECT, 12);
    add(MeasurementFlags::RECT, 13);
    add(MeasurementFlags::RECT, 14);
    add(MeasurementFlags::ELLIPSE, 15);
    add(MeasurementFlags::ELLIPSE, 16);
    add(MeasurementFlags::ELLIPSE, COLUMN_ANGLE);
    add(MeasurementFlags::SHAPE_DESCRIPTORS, COLUMN_CIRCULARITY);
    add(MeasurementFlags::INTEGRATED_DENSITY, 20);
    add(MeasurementFlags::MEDIAN, 21);
    add(MeasurementFlags::SKEWNESS, 22);
    add(MeasurementFlags::KURTOSIS, 23);
    add(MeasurementFlags::AREA_FRACTION, 24);
    add(MeasurementFlags::INTEGRATED_DENSITY, 25);
    add(MeasurementFlags::SHAPE_DESCRIPTORS, COLUMN_ASPECT_RATIO);
    add(MeasurementFlags::SHAPE_DESCRIPTORS, COLUMN_ROUNDNESS);
    add(MeasurementFlags::SHAPE_DESCRIPTORS, COLUMN_SOLIDITY);
    columns
}

/// Java's `(int)value` cast, including saturation at the int range, which is
/// what ResultsTable.addValue compares against the original double.
fn is_java_int(value: f64) -> bool {
    let cast = if value.is_nan() {
        0
    } else if value >= f64::from(i32::MAX) {
        i32::MAX
    } else if value <= f64::from(i32::MIN) {
        i32::MIN
    } else {
        value as i32
    };
    f64::from(cast) == value
}

fn format_results_cell(value: f64, fixed_places: Option<i32>, table_precision: i32) -> String {
    let places = match fixed_places {
        Some(places) => places,
        // ResultsTable.n(): integers use d2s(n, 0) while the precision is
        // non-negative; everything else uses the table precision
        // (ResultsTable.java 832-839).
        None => {
            if table_precision >= 0 && is_java_int(value) {
                0
            } else {
                table_precision
            }
        }
    };
    format_results_d2s(value, places)
}

/// Port of `ResultsTable.d2s` (ResultsTable.java 917-964), including the
/// `DecimalFormat` patterns: the zero-decimal pattern is HALF_UP, while the
/// fixed and scientific patterns keep `DecimalFormat`'s default HALF_EVEN.
fn format_results_d2s(value: f64, mut places: i32) -> String {
    if value.is_nan() {
        return "NaN".to_string();
    }
    if value.is_infinite() {
        return if value > 0.0 {
            "Infinity".to_string()
        } else {
            "-Infinity".to_string()
        };
    }
    if value == f32::MAX as f64 {
        return "3.4e38".to_string();
    }

    let magnitude = value.abs();
    if (magnitude != 0.0 && magnitude < 1.0 / 10f64.powi(places))
        || magnitude > 999_999_999_999.0
        || places < 0
    {
        if places < 0 {
            places = (-places).min(9);
        } else {
            places = 3;
        }
        return format!("{:.*e}", places as usize, value).replace('e', "E");
    }
    if places < 0 {
        places = 0;
    }
    if places > 9 {
        places = 9;
    }
    if places == 0 {
        // DecimalFormat("0") is the only pattern with HALF_UP
        // (ResultsTable.java 961).
        format!("{:.0}", value.round())
    } else {
        format!("{:.*}", places as usize, value)
    }
}

/// Formats one cell like `ResultsTable.d2s` at the default precision of 3.
///
/// Integer-valued columns keep ImageJ's `AUTO_FORMAT` behaviour and print
/// without decimals; fractional values print with exactly three decimals.
/// Values whose magnitude is in `(0, 1e-3)` or above `999999999999` switch to
/// the `0.000E0` pattern (ResultsTable.java lines 915-960).
pub fn format_ij_num(val: f64) -> String {
    if val.is_nan() {
        return "NaN".to_string();
    }
    if val.is_infinite() {
        return if val > 0.0 {
            "Infinity".to_string()
        } else {
            "-Infinity".to_string()
        };
    }
    if val == f32::MAX as f64 {
        return "3.4e38".to_string();
    }

    let magnitude = val.abs();
    if (magnitude != 0.0 && magnitude < 1.0 / 1000.0) || magnitude > 999_999_999_999.0 {
        return format!("{:.3e}", val).replace('e', "E");
    }

    // ResultsTable.addValue only assigns the precision to columns whose first
    // value is fractional; the integer cast also saturates at the int range.
    if (val as i32) as f64 == val {
        format!("{:.0}", val)
    } else {
        format!("{:.3}", val)
    }
}

/// Formats one cell like `ResultsTable.d2s` with an explicit fixed precision.
fn format_ij_num_fixed(val: f64, places: usize) -> String {
    if val.is_nan() {
        return "NaN".to_string();
    }
    if val.is_infinite() {
        return if val > 0.0 {
            "Infinity".to_string()
        } else {
            "-Infinity".to_string()
        };
    }
    if val == f32::MAX as f64 {
        return "3.4e38".to_string();
    }

    let magnitude = val.abs();
    let threshold = 1.0 / 10f64.powi(places as i32);
    if (magnitude != 0.0 && magnitude < threshold) || magnitude > 999_999_999_999.0 {
        return format!("{:.*e}", places, val).replace('e', "E");
    }
    format!("{:.*}", places, val)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn results_table_d2s_matches_java_decimal_format_rounding() {
        // Expected strings from ImageJ 1.54u's ResultsTable.d2s
        // (ResultsTable.java 917-964). "0" is the only pattern set to
        // HALF_UP; the fixed and scientific patterns use HALF_EVEN.
        for (value, places, expected) in [
            (2.5, 0, "3"),
            (-2.5, 0, "-3"),
            (0.125, 2, "0.12"),
            (0.375, 2, "0.38"),
            (0.005, 2, "5.000E-3"),
            (1.005, 2, "1.00"),
            (0.000_999_999_9, 3, "1.000E-3"),
            (1e-12, 3, "1.000E-12"),
            (999_999_999_999.0, 0, "999999999999"),
            (1e12, 0, "1.000E12"),
            (f64::NAN, 3, "NaN"),
            (f64::INFINITY, 3, "Infinity"),
            (f64::NEG_INFINITY, 3, "-Infinity"),
            (f32::MAX as f64, 3, "3.4e38"),
        ] {
            assert_eq!(format_results_d2s(value, places), expected);
        }
    }

    #[test]
    fn ramp_16x16_rect_measurement_matches_imagej_154u_oracle() {
        let mut doc = ImageDocument::new_ramp_8bit("test", 16, 16).unwrap();
        doc.set_rect_roi(2, 3, 8, 6).unwrap();
        let stats = doc.measure().unwrap();
        assert_eq!(stats.pixel_count, 48);
        assert_eq!(stats.area, 48.0);
        assert_eq!(stats.mean, 88.0);
        assert_eq!(stats.min, 32.0);
        assert_eq!(stats.max, 144.0);
        assert_eq!(stats.x_centroid, 6.0);
        assert_eq!(stats.y_centroid, 6.0);
    }

    #[test]
    fn undo_restores_previous_pixels_after_invert() {
        let mut doc = ImageDocument::new_ramp_8bit("test", 8, 8).unwrap();
        let original = doc.plane().clone();
        doc.invert();
        assert_ne!(doc.plane(), &original);
        assert!(doc.undo());
        assert_eq!(doc.plane(), &original);
    }

    #[test]
    fn convert_to_8bit_float_uses_create8bitimage_edge_cases() {
        // FloatProcessor.create8BitImage (FloatProcessor.java 190-208) clamps
        // negative values and relies on Java's (int) cast, which maps NaN to 0
        // and +Infinity to Integer.MAX_VALUE.
        let mut doc = ImageDocument::new(
            "float-edge",
            4,
            1,
            PixelPlane::Gray32(vec![f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 0.5]),
        )
        .unwrap();
        doc.display_range = DisplayRange {
            min: 0.0,
            max: 1.0,
        };
        doc.convert_to_8bit();
        assert_eq!(doc.plane(), &PixelPlane::Gray8(vec![0, 255, 0, 128]));

        // max == min makes scale infinite: pixels equal to min become
        // 0*Infinity = NaN (0), pixels above min become 255.
        let mut degenerate = ImageDocument::new(
            "float-degenerate",
            3,
            1,
            PixelPlane::Gray32(vec![4.0, 5.0, 6.0]),
        )
        .unwrap();
        degenerate.display_range = DisplayRange {
            min: 5.0,
            max: 5.0,
        };
        degenerate.convert_to_8bit();
        assert_eq!(degenerate.plane(), &PixelPlane::Gray8(vec![0, 0, 255]));
    }

    #[test]
    fn undo_after_8bit_conversion_restores_original_display_range() {
        // TYPE_CONVERSION saves the original processor, including its min/max
        // (Undo.java 149-160).
        let mut doc = ImageDocument::new(
            "conversion-undo",
            3,
            1,
            PixelPlane::Gray16(vec![100, 250, 500]),
        )
        .unwrap();
        let original = doc.plane().clone();
        doc.display_range = DisplayRange { min: 100.0, max: 500.0 };
        doc.convert_to_8bit();
        assert_eq!(doc.display_range, DisplayRange { min: 0.0, max: 255.0 });
        assert!(doc.undo());
        assert_eq!(doc.plane(), &original);
        assert_eq!(doc.display_range, DisplayRange { min: 100.0, max: 500.0 });
    }

    #[test]
    fn later_filter_replaces_type_conversion_undo_range() {
        let mut doc = ImageDocument::new(
            "conversion-then-filter",
            2,
            1,
            PixelPlane::Gray16(vec![100, 500]),
        )
        .unwrap();
        doc.display_range = DisplayRange { min: 100.0, max: 500.0 };
        doc.convert_to_8bit();
        let converted = doc.plane().clone();
        doc.invert();
        assert!(doc.undo());
        assert_eq!(doc.plane(), &converted);
        assert_eq!(doc.display_range, DisplayRange { min: 0.0, max: 255.0 });
    }

    #[test]
    fn undo_after_16bit_conversion_restores_original_display_range() {
        // Undo.TYPE_CONVERSION snapshots the processor before the swap
        // (Converter.java 100-101, Undo.java 149-160).
        let mut doc = ImageDocument::new(
            "conversion-16-undo",
            3,
            1,
            PixelPlane::Gray32(vec![0.25, 0.5, 0.75]),
        )
        .unwrap();
        let original = doc.plane().clone();
        doc.display_range = DisplayRange {
            min: 0.0,
            max: 1.0,
        };
        doc.convert_to_16bit().unwrap();
        assert_eq!(doc.plane(), &PixelPlane::Gray16(vec![16384, 32768, 49151]));
        assert_eq!(
            doc.display_range,
            DisplayRange {
                min: 16384.0,
                max: 49151.0
            }
        );
        assert!(doc.undo());
        assert_eq!(doc.plane(), &original);
        assert_eq!(
            doc.display_range,
            DisplayRange {
                min: 0.0,
                max: 1.0
            }
        );
    }

    #[test]
    fn undo_after_32bit_conversion_restores_original_display_range() {
        // TYPE_CONVERSION saves the original processor, including its min/max
        // (Undo.java 149-160, Converter.java 96-103).
        let mut doc8 = ImageDocument::new(
            "conversion-32-undo-8",
            3,
            1,
            PixelPlane::Gray8(vec![10, 50, 200]),
        )
        .unwrap();
        let original8 = doc8.plane().clone();
        doc8.display_range = DisplayRange { min: 10.0, max: 200.0 };
        doc8.convert_to_32bit().unwrap();
        assert_eq!(doc8.plane(), &PixelPlane::Gray32(vec![10.0, 50.0, 200.0]));
        assert_eq!(doc8.display_range, DisplayRange { min: 10.0, max: 200.0 });
        assert!(doc8.undo());
        assert_eq!(doc8.plane(), &original8);
        assert_eq!(doc8.display_range, DisplayRange { min: 10.0, max: 200.0 });

        let mut doc16 = ImageDocument::new(
            "conversion-32-undo-16",
            3,
            1,
            PixelPlane::Gray16(vec![100, 250, 500]),
        )
        .unwrap();
        let original16 = doc16.plane().clone();
        doc16.display_range = DisplayRange { min: 100.0, max: 500.0 };
        doc16.convert_to_32bit().unwrap();
        assert_eq!(doc16.plane(), &PixelPlane::Gray32(vec![100.0, 250.0, 500.0]));
        assert_eq!(doc16.display_range, DisplayRange { min: 100.0, max: 500.0 });
        assert!(doc16.undo());
        assert_eq!(doc16.plane(), &original16);
        assert_eq!(doc16.display_range, DisplayRange { min: 100.0, max: 500.0 });

        let mut doc_rgb = ImageDocument::new(
            "conversion-32-undo-rgb",
            2,
            1,
            PixelPlane::ColorRgb(vec![0x00ff_0000, 0x0000_ff00]),
        )
        .unwrap();
        let original_rgb = doc_rgb.plane().clone();
        doc_rgb.convert_to_32bit().unwrap();
        assert_eq!(doc_rgb.display_range, DisplayRange { min: 0.0, max: 255.0 });
        assert!(doc_rgb.undo());
        assert_eq!(doc_rgb.plane(), &original_rgb);
        assert_eq!(doc_rgb.display_range, DisplayRange { min: 0.0, max: 255.0 });

        let mut stack = ImageDocument::new_stack(
            "conversion-32-stack",
            2,
            1,
            vec![
                PixelPlane::Gray8(vec![10, 20]),
                PixelPlane::Gray8(vec![30, 40]),
            ],
        )
        .unwrap();
        stack.convert_to_32bit().unwrap();
        assert!(!stack.undo(), "stack type conversions have no undo");
    }

    #[test]
    fn undo_after_rgb_conversion_restores_original_display_range() {
        // TYPE_CONVERSION saves the original processor, including its min/max
        // (Undo.java 149-160, Converter.java 100-101).
        let mut doc = ImageDocument::new(
            "conversion-rgb-undo",
            3,
            1,
            PixelPlane::Gray16(vec![100, 250, 500]),
        )
        .unwrap();
        let original = doc.plane().clone();
        doc.display_range = DisplayRange { min: 100.0, max: 500.0 };
        doc.convert_to_rgb();
        assert!(matches!(doc.plane(), PixelPlane::ColorRgb(_)));
        assert_eq!(doc.display_range, DisplayRange { min: 0.0, max: 255.0 });
        assert!(doc.undo());
        assert_eq!(doc.plane(), &original);
        assert_eq!(doc.display_range, DisplayRange { min: 100.0, max: 500.0 });

        let mut stack = ImageDocument::new_stack(
            "conversion-rgb-stack",
            2,
            1,
            vec![
                PixelPlane::Gray8(vec![10, 20]),
                PixelPlane::Gray8(vec![30, 40]),
            ],
        )
        .unwrap();
        stack.convert_to_rgb();
        assert!(!stack.undo(), "stack type conversions have no undo");
    }

    #[test]
    fn rgb_conversion_expands_gray_and_indexed_planes_through_their_luts() {
        let mut gray = ImageDocument::new("rgb-gray", 2, 1, PixelPlane::Gray8(vec![0, 255])).unwrap();
        gray.convert_to_rgb();
        assert_eq!(
            gray.plane(),
            &PixelPlane::ColorRgb(vec![0xff00_0000, 0xffff_ffff])
        );

        let mut lut = Lut256::default();
        lut.r.reverse();
        lut.g.reverse();
        lut.b.reverse();
        let mut inverted = ImageDocument::new(
            "rgb-inverted",
            2,
            1,
            PixelPlane::Color256 {
                pixels: vec![0, 255],
                lut: Box::new(lut),
            },
        )
        .unwrap();
        inverted.convert_to_rgb();
        assert_eq!(
            inverted.plane(),
            &PixelPlane::ColorRgb(vec![0xffff_ffff, 0xff00_0000])
        );
    }

    #[test]
    fn undo_after_crop_restores_image_dimensions_with_pixels() {
        // Undo.TRANSFORM restores the duplicate image's stack, including its
        // dimensions (Undo.java 64-68, 161-163).
        let mut doc = ImageDocument::new_ramp_8bit("crop", 8, 6).unwrap();
        let original = doc.slices.clone();
        doc.set_rect_roi(2, 1, 4, 3).unwrap();
        doc.crop().unwrap();
        assert_eq!((doc.width, doc.height), (4, 3));
        assert!(doc.undo());
        assert_eq!((doc.width, doc.height), (8, 6));
        assert_eq!(doc.slices, original);
        assert_eq!(doc.plane().len(), (doc.width * doc.height) as usize);
    }

    #[test]
    fn a_later_pixel_edit_replaces_the_crop_undo_dimensions() {
        let mut doc = ImageDocument::new_ramp_8bit("crop", 8, 6).unwrap();
        doc.set_rect_roi(2, 1, 4, 3).unwrap();
        doc.crop().unwrap();
        let cropped = doc.slices.clone();
        doc.display_range = DisplayRange { min: 20.0, max: 200.0 };
        doc.apply_display_lut().unwrap();
        assert!(doc.undo());
        assert_eq!((doc.width, doc.height), (4, 3));
        assert_eq!(doc.slices, cropped);
    }

    #[test]
    fn crop_rejects_clipped_oval_until_shape_roi_is_representable() {
        // Resizer.java 51-60 intersects an out-of-bounds oval with the image,
        // producing ShapeRoi (oracle selectionType()==9). Our ROI model has
        // no ShapeRoi, so preserving the original oval would be misleading.
        let mut doc = ImageDocument::new_ramp_8bit("crop", 10, 8).unwrap();
        doc.set_oval_roi(-2, 1, 6, 4).unwrap();
        let before = doc.clone();
        assert_eq!(
            doc.crop(),
            Err(CoreError::InvalidOperation(
                "Crop of clipped non-rectangular ROI requires ShapeRoi"
            ))
        );
        assert_eq!(doc, before);
    }

    #[test]
    fn hyperstack_index_and_position_follow_imageplus_czt_order() {
        let slices = vec![PixelPlane::Gray8(vec![0u8; 6]); 12];
        let mut doc = ImageDocument::new_stack("hyper", 3, 2, slices).unwrap();
        // A plain 12-slice stack has (C,Z,T) = (1,12,1) (ImagePlus.java 1351-1364).
        assert_eq!(doc.get_dimensions(), [3, 2, 1, 12, 1]);

        doc.set_dimensions(2, 3, 2).unwrap();
        assert_eq!(doc.get_dimensions(), [3, 2, 2, 3, 2]);
        for t in 1..=2u32 {
            for z in 1..=3u32 {
                for c in 1..=2u32 {
                    doc.set_position(c as i32, z as i32, t as i32);
                    // ImagePlus.getStackIndex: channel varies fastest, then Z, then T.
                    let index = (t - 1) * 2 * 3 + (z - 1) * 2 + c;
                    assert_eq!(doc.get_slice_number(), index);
                    assert_eq!(doc.get_position(), (c, z, t));
                    assert_eq!(
                        doc.convert_index_to_position(index as i32).unwrap(),
                        [c, z, t]
                    );
                    assert_eq!(doc.get_stack_index(c as i32, z as i32, t as i32), index);
                }
            }
        }
    }

    #[test]
    fn set_position_zero_keeps_current_and_clamps_to_dimensions() {
        let slices = vec![PixelPlane::Gray8(vec![0u8; 6]); 6];
        let mut doc = ImageDocument::new_stack("hyper", 3, 2, slices).unwrap();
        doc.set_dimensions(2, 3, 1).unwrap();
        doc.set_position(2, 2, 1);
        assert_eq!(doc.get_position(), (2, 2, 1));

        // ImagePlus.setPosition treats zero as "keep current" and clamps to max
        // (ImagePlus.java 1850-1875).
        doc.set_position(0, 0, 0);
        assert_eq!(doc.get_position(), (2, 2, 1));
        doc.set_position(9, 9, 9);
        assert_eq!(doc.get_position(), (2, 3, 1));
    }

    #[test]
    fn set_slice_updates_position_for_one_dimensional_stacks_only() {
        let slices = vec![PixelPlane::Gray8(vec![0u8; 6]); 12];
        let mut doc = ImageDocument::new_stack("stack", 3, 2, slices.clone()).unwrap();
        doc.set_slice(4).unwrap();
        assert_eq!(doc.get_slice_number(), 4);
        assert_eq!(doc.get_position(), (1, 4, 1));

        // ImagePlus.setCurrentSlice only calls updatePosition when one dimension
        // equals the stack size (ImagePlus.java 1823-1829).
        let mut hyper = ImageDocument::new_stack("hyper", 3, 2, slices).unwrap();
        hyper.set_dimensions(2, 3, 2).unwrap();
        hyper.set_position(1, 2, 1);
        hyper.set_slice(9).unwrap();
        assert_eq!(hyper.get_slice_number(), 9);
        assert_eq!(hyper.get_position(), (1, 2, 1));
    }

    #[test]
    fn invert_stack_only_changes_the_requested_slices() {
        let mut slices = Vec::new();
        for s in 0..3u8 {
            slices.push(PixelPlane::Gray8(
                (0..12).map(|i| i + s * 20).collect::<Vec<_>>(),
            ));
        }
        let mut doc = ImageDocument::new_stack("stack", 4, 3, slices.clone()).unwrap();
        doc.set_slice(2).unwrap();
        doc.invert();
        assert_eq!(doc.slices[0], slices[0]);
        assert_ne!(doc.slices[1], slices[1]);
        assert_eq!(doc.slices[2], slices[2]);

        doc.invert_stack();
        // A stack invert after a per-slice invert leaves slice 2 at its original
        // values (double invert) while the other slices are inverted once.
        assert_ne!(doc.slices[0], slices[0]);
        assert_eq!(doc.slices[1], slices[1]);
        assert_ne!(doc.slices[2], slices[2]);
    }

    #[test]
    fn set_dimensions_leaves_position_and_current_slice_unchanged() {
        let slices = vec![PixelPlane::Gray8(vec![0u8; 12]); 12];
        let mut doc = ImageDocument::new_stack("stack", 4, 3, slices).unwrap();
        doc.set_slice(9).unwrap();
        assert_eq!(doc.get_position(), (1, 9, 1));
        assert_eq!(doc.get_slice_number(), 9);

        // ImagePlus.setDimensions without a StackWindow only replaces C/Z/T
        // (ImagePlus.java 1281-1288); currentSlice and position stay stale.
        doc.set_dimensions(2, 3, 2).unwrap();
        assert_eq!(doc.get_dimensions(), [4, 3, 2, 3, 2]);
        assert_eq!(doc.get_position(), (1, 9, 1));
        assert_eq!(doc.get_slice_number(), 9);

        // A mismatched product falls back to a plain Z-stack instead of
        // raising (ImagePlus.java 1266-1278).
        doc.set_dimensions(2, 2, 2).unwrap();
        assert_eq!(doc.get_dimensions(), [4, 3, 1, 12, 1]);
        assert_eq!(doc.get_position(), (1, 9, 1));
        assert_eq!(doc.get_slice_number(), 9);

        doc.set_dimensions(0, 0, 0).unwrap();
        assert_eq!(doc.get_dimensions(), [4, 3, 1, 12, 1]);
    }

    #[test]
    fn macro_set_slice_uses_set_position_for_hyperstacks() {
        let slices = vec![PixelPlane::Gray8(vec![0u8; 6]); 12];
        let mut doc = ImageDocument::new_stack("hyper", 3, 2, slices).unwrap();
        doc.set_dimensions(2, 3, 2).unwrap();
        doc.set_position(1, 2, 1);

        // ImagePlus.setSlice does not move C/Z/T when no dimension equals the
        // stack size (ImagePlus.java 1823-1829).
        doc.set_slice(9).unwrap();
        assert_eq!(doc.get_slice_number(), 9);
        assert_eq!(doc.get_position(), (1, 2, 1));

        // Functions.setSlice calls setPosition(n) for hyperstacks
        // (Functions.java 3550-3556).
        doc.set_slice_macro(9).unwrap();
        assert_eq!(doc.get_slice_number(), 9);
        assert_eq!(doc.get_position(), (1, 2, 2));
    }

    #[test]
    fn stack_nd_processing_clears_undo_but_single_image_keeps_it() {
        let mut doc =
            ImageDocument::new_stack("stack", 2, 2, vec![PixelPlane::Gray8(vec![0, 1, 2, 3]); 3])
                .unwrap();
        let original = doc.slices.clone();
        doc.invert_stack();
        // PlugInFilterRunner calls Undo.reset() for complete stacks
        // (PlugInFilterRunner.java 118-120).
        assert!(!doc.undo());
        assert_ne!(doc.slices, original);

        let mut single =
            ImageDocument::new("single", 2, 2, PixelPlane::Gray8(vec![0, 1, 2, 3])).unwrap();
        single.invert_stack();
        // With one slice the stack option still runs the single-image branch.
        assert!(single.undo());
        assert_eq!(single.slices[0], PixelPlane::Gray8(vec![0, 1, 2, 3]));
    }

    #[test]
    fn changing_stack_slices_discards_the_filter_snapshot() {
        let mut doc = ImageDocument::new_stack(
            "stack",
            2,
            2,
            vec![
                PixelPlane::Gray8(vec![0, 1, 2, 3]),
                PixelPlane::Gray8(vec![10, 11, 12, 13]),
                PixelPlane::Gray8(vec![20, 21, 22, 23]),
            ],
        )
        .unwrap();
        doc.set_slice(2).unwrap();
        doc.invert();
        assert!(doc.snapshot.is_some());

        // ImagePlus.setSlice calls ip.setSnapshotPixels(null) when it swaps
        // processors (ImagePlus.java 1997-1999).
        doc.set_slice(3).unwrap();
        assert!(doc.snapshot.is_none());
        assert!(!doc.undo());
        doc.set_slice(2).unwrap();
        assert_eq!(doc.plane(), &PixelPlane::Gray8(vec![245, 244, 243, 242]));
    }
}
