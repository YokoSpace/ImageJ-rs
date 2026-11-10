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

    pub fn save_snapshot(&mut self) {
        self.snapshot = Some(self.slices.clone());
    }

    pub fn undo(&mut self) -> bool {
        if let Some(prev) = self.snapshot.take() {
            self.slices = prev;
            let size = self.slices.len().max(1) as u32;
            self.current_slice = self.current_slice.clamp(1, size);
            true
        } else {
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
        if let Some(Roi::Line(line)) = &self.roi {
            return Ok(self.measure_line(*line));
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
            PixelPlane::Gray8(pixels) => {
                self.measure_byte_like(pixels, rx, ry, rw, rh, mask.as_ref())
            }
            PixelPlane::Color256 { pixels, .. } => {
                self.measure_byte_like(pixels, rx, ry, rw, rh, mask.as_ref())
            }
            PixelPlane::Gray16(pixels) => self.measure_short(pixels, rx, ry, rw, rh, mask.as_ref()),
            PixelPlane::Gray32(pixels) => self.measure_float(pixels, rx, ry, rw, rh, mask.as_ref()),
            PixelPlane::ColorRgb(pixels) => self.measure_rgb(pixels, rx, ry, rw, rh, mask.as_ref()),
        };

        Ok(self.finish_measurement(rx, ry, rw, rh, raw))
    }

    /// Port of `Analyzer.measureLength` for a 1-pixel-wide straight line.
    ///
    /// `ProfilePlot.getStraightLineProfile` samples the line with
    /// `ImageProcessor.getLine` (ImageProcessor.java 1116-1139) and wraps the
    /// values in a one-row FloatProcessor. Default measurements therefore
    /// report the profile statistics (`Area` is the number of samples), plus
    /// the Results-table-only `Length` and `Angle` columns.
    fn measure_line(&self, line: LineRoi) -> MeasurementSummary {
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
        let raw = self.measure_float(&profile, 0, 0, samples, 1, None);
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
            int_den: area * mean,
            raw_int_den: raw.sum,
            histogram: raw.histogram,
            mode: raw.mode,
            median: raw.median,
            skewness: raw.skewness,
            kurtosis: raw.kurtosis,
            line: None,
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

        let pixel_count: u64 = histogram.iter().sum();
        let (min, max) = if pixel_count == 0 {
            (f64::NAN, f64::NAN)
        } else {
            raw_min_max(&histogram, 0, 255)
        };
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
            |idx| pixels[idx] as f64,
            true,
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
        let pixel_count: u64 = histogram.iter().sum();
        let (min, max) = raw_min_max(&histogram, 0, hist_len - 1);
        let (mode, _) = mode_from_hist(&histogram);
        let mut sum = 0.0;
        let mut sum2 = 0.0;
        // ShortStatistics.getStatistics accumulates over the occupied raw range.
        for i in min as usize..=max as usize {
            let value = i as f64;
            let count = histogram[i] as f64;
            sum += value * count;
            sum2 += (value * value) * count;
        }
        let mean = sum / pixel_count as f64;
        let median = median_from_hist(&histogram, 0, 65535, pixel_count);
        let (skewness, kurtosis, x_center_of_mass, y_center_of_mass) = self.raw_moments(
            rx,
            ry,
            rw,
            rh,
            mean,
            |idx| pixels[idx] as f64,
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
    ) -> RawMeasurement {
        let min_threshold = -(f32::MAX as f64);
        let max_threshold = f32::MAX as f64;

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
        }
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
    /// Results-table-only `Length`/`Angle` values for line measurements.
    /// Area measurements leave these cells unset, exactly like ImageJ's
    /// default Results table.
    pub line: Option<LineMeasurement>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ResultsTable {
    pub rows: Vec<MeasurementSummary>,
}

impl ResultsTable {
    pub fn new() -> Self {
        Self { rows: Vec::new() }
    }

    pub fn push(&mut self, row: MeasurementSummary) {
        self.rows.push(row);
    }

    pub fn clear(&mut self) {
        self.rows.clear();
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
