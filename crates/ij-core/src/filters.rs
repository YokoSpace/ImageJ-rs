//! Pure ports of ImageJ 1.54u filters.
//!
//! Everything here works on owned `ImageDocument` state: no I/O, no UI and no
//! JVM. The reference implementations live in `original_imagej/ij/process/`.

use crate::{CoreError, ImageDocument, PixelPlane, RoiMask};

/// The rectangle an `ImageProcessor` filter processes plus its optional
/// irregular mask, mirroring `ImageProcessor.roiX/roiY/roiWidth/roiHeight` and
/// `ImageProcessor.mask` after `PlugInFilterRunner.prepareProcessor`.
struct FilterRegion {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    mask: Option<RoiMask>,
}

/// The `RankFilters` filter types this increment ports
/// (original_imagej/ij/plugin/filter/RankFilters.java lines 25-26). MEAN,
/// VARIANCE and the multi-step OPEN/CLOSE/TOP_HAT filters, as well as
/// OUTLIERS, REMOVE_NAN and DESPECKLE's own constant, are not part of it.
/// DESPECKLE is reachable through [`ImageDocument::despeckle`], which is the
/// median filter with radius 1 (RankFilters.java lines 117-119).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RankFilterKind {
    Median,
    Minimum,
    Maximum,
}

impl RankFilterKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Median => "Median",
            Self::Minimum => "Minimum",
            Self::Maximum => "Maximum",
        }
    }

    /// Dialog limits: `maxRadius` is 100 for median and 1000 for the other
    /// rank filters (RankFilters.java line 188). Larger radii are outside the
    /// verified parameter range of this increment.
    fn max_radius(self) -> f64 {
        match self {
            Self::Median => 100.0,
            Self::Minimum | Self::Maximum => 1000.0,
        }
    }
}

impl ImageDocument {
    /// Port of `run("Smooth")` / `run("Smooth", "slice")`.
    ///
    /// `Filters.run` calls `ImageProcessor.smooth()`, which runs the 3x3
    /// `BLUR_MORE` filter and does nothing when the image is one pixel wide
    /// (original_imagej/ij/process/ImageProcessor.java lines 1634-1636).
    ///
    /// Only the current stack slice is processed. A stack-wide Smooth needs the
    /// `stack` answer to "Process Stack?" and is not implemented yet.
    pub fn smooth(&mut self) -> Result<(), CoreError> {
        // `Filters.setup` requests DOES_ALL-DOES_8C (Filters.java line 26), so
        // indexed 8-bit color images are not processable by ImageJ either. RGB
        // has a ColorProcessor BLUR_MORE path that this increment does not port.
        reject_unsupported_gray_type(self.plane(), "Smooth")?;

        // PlugInFilterRunner snapshots a single image before any filter that
        // changes pixels and installs Undo afterwards (PlugInFilterRunner.java
        // lines 107-121).
        self.save_snapshot();
        if self.width <= 1 {
            return Ok(());
        }

        let region = self.filter_region()?;
        if region.width == 0 || region.height == 0 {
            return Ok(());
        }
        let width = self.width;
        let height = self.height;

        match self.plane_mut() {
            PixelPlane::Gray8(pixels) => {
                *pixels = smooth_byte(pixels, width, height, &region);
            }
            PixelPlane::Gray16(pixels) => {
                *pixels = smooth_short(pixels, width, height, &region);
            }
            PixelPlane::Gray32(pixels) => {
                *pixels = smooth_float(pixels, width, height, &region);
            }
            // Rejected above; kept explicit instead of silently ignoring them.
            PixelPlane::Color256 { .. } => {
                return Err(CoreError::UnsupportedFilter {
                    filter: "Smooth",
                    pixel_type: "8-bit color (indexed)",
                });
            }
            PixelPlane::ColorRgb(_) => {
                return Err(CoreError::UnsupportedFilter {
                    filter: "Smooth",
                    pixel_type: "RGB",
                });
            }
        }
        Ok(())
    }

    /// Port of `run("Sharpen")` / `run("Sharpen", "slice")`.
    ///
    /// `Filters.run` calls `ImageProcessor.sharpen()`, which runs the 3x3
    /// convolution `{-1,-1,-1,-1,12,-1,-1,-1,-1}` and does nothing when the
    /// image is one pixel wide (original_imagej/ij/process/ImageProcessor.java
    /// lines 1640-1646). Unlike `filter(BLUR_MORE)` the convolve paths have no
    /// separate edge passes, so only the ROI rectangle is written.
    ///
    /// Only the current stack slice is processed. A stack-wide Sharpen needs
    /// the `stack` answer to "Process Stack?" and is not implemented yet.
    pub fn sharpen(&mut self) -> Result<(), CoreError> {
        reject_unsupported_gray_type(self.plane(), "Sharpen")?;

        self.save_snapshot();
        if self.width <= 1 {
            return Ok(());
        }

        let region = self.filter_region()?;
        if region.width == 0 || region.height == 0 {
            return Ok(());
        }
        let width = self.width;
        let height = self.height;

        match self.plane_mut() {
            PixelPlane::Gray8(pixels) => {
                *pixels = convolve_3x3_byte(pixels, width, height, &region, SHARPEN_KERNEL);
            }
            PixelPlane::Gray16(pixels) => {
                *pixels = convolve_3x3_short(pixels, width, height, &region, SHARPEN_KERNEL);
            }
            PixelPlane::Gray32(pixels) => {
                *pixels = convolve_3x3_float(pixels, width, height, &region, SHARPEN_KERNEL);
            }
            // Rejected above; kept explicit instead of silently ignoring them.
            PixelPlane::Color256 { .. } => {
                return Err(CoreError::UnsupportedFilter {
                    filter: "Sharpen",
                    pixel_type: "8-bit color (indexed)",
                });
            }
            PixelPlane::ColorRgb(_) => {
                return Err(CoreError::UnsupportedFilter {
                    filter: "Sharpen",
                    pixel_type: "RGB",
                });
            }
        }
        Ok(())
    }

    /// Port of `run("Find Edges")` / `run("Find Edges", "slice")`.
    ///
    /// `Filters.run` calls `ImageProcessor.findEdges()`, which runs the 3x3
    /// Sobel filter only when the image is wider than one pixel
    /// (original_imagej/ij/process/ImageProcessor.java lines 1649-1653).
    ///
    /// Only the current stack slice is processed. A stack-wide Find Edges
    /// needs the `stack` answer to "Process Stack?" and is not implemented yet.
    pub fn find_edges(&mut self) -> Result<(), CoreError> {
        reject_unsupported_gray_type(self.plane(), "Find Edges")?;

        self.save_snapshot();
        if self.width <= 1 {
            return Ok(());
        }

        let region = self.filter_region()?;
        if region.width == 0 || region.height == 0 {
            return Ok(());
        }
        let width = self.width;
        let height = self.height;

        match self.plane_mut() {
            PixelPlane::Gray8(pixels) => {
                *pixels = find_edges_byte(pixels, width, height, &region);
            }
            PixelPlane::Gray16(pixels) => {
                *pixels = find_edges_short(pixels, width, height, &region);
            }
            PixelPlane::Gray32(pixels) => {
                *pixels = find_edges_float(pixels, width, height, &region);
            }
            // Rejected above; kept explicit instead of silently ignoring them.
            PixelPlane::Color256 { .. } => {
                return Err(CoreError::UnsupportedFilter {
                    filter: "Find Edges",
                    pixel_type: "8-bit color (indexed)",
                });
            }
            PixelPlane::ColorRgb(_) => {
                return Err(CoreError::UnsupportedFilter {
                    filter: "Find Edges",
                    pixel_type: "RGB",
                });
            }
        }
        Ok(())
    }

    /// Port of `run("Gaussian Blur...", "sigma=...")` for grayscale images.
    ///
    /// `ij.plugin.filter.GaussianBlur` converts the processor to float, blurs
    /// one direction at a time, and converts back (GaussianBlur.java 176-212).
    /// ByteProcessor/ShortProcessor add `0.5f` and truncate on the way back
    /// (ByteProcessor.java 1268-1279, ShortProcessor.java 1241-1252);
    /// FloatProcessor blurs in place. Only the current stack slice is
    /// processed, matching the `slice` answer to "Process Stack?".
    ///
    /// Indexed 8-bit color and RGB are rejected explicitly: this increment
    /// covers the three grayscale representations only.
    pub fn gaussian_blur(&mut self, sigma: f64) -> Result<(), CoreError> {
        reject_unsupported_gray_type(self.plane(), "Gaussian Blur")?;

        // ImageJ 1.54u computes the kernel radius as
        // `(int)Math.ceil(sigma*sqrt(-2*ln(accuracy)))+1`; for sigma above
        // roughly 5e8 that int overflows and ImageJ aborts with
        // NegativeArraySizeException. Reject the range explicitly instead of
        // silently producing different pixels.
        if sigma > 5.0e8 {
            return Err(CoreError::InvalidFilterParameter {
                filter: "Gaussian Blur",
                parameter: "sigma",
                value: format!("{sigma}"),
            });
        }

        // PlugInFilterRunner snapshots the processor before a filter that
        // modifies pixels and installs Undo afterwards (lines 107-121), even
        // when sigma is zero and the filter is a no-op.
        self.save_snapshot();

        let region = self.filter_region()?;
        let width = self.width;
        let height = self.height;

        match self.plane_mut() {
            PixelPlane::Gray8(pixels) => {
                // GaussianBlur.run picks accuracy 0.002 for ByteProcessor
                // (GaussianBlur.java 135-136).
                let original = pixels.clone();
                let mut float: Vec<f32> = pixels.iter().map(|&p| p as f32).collect();
                blur_gaussian_float(&mut float, width, height, &region, sigma, 0.002);
                // ByteProcessor.setPixels converts the whole image, not just
                // the ROI (ByteProcessor.java 1268-1279).
                for (dst, &src) in pixels.iter_mut().zip(float.iter()) {
                    *dst = rounded_byte(src);
                }
                restore_outside_roi(pixels, &original, width, height, &region, sigma);
            }
            PixelPlane::Gray16(pixels) => {
                let original = pixels.clone();
                let mut float: Vec<f32> = pixels.iter().map(|&p| p as f32).collect();
                blur_gaussian_float(&mut float, width, height, &region, sigma, 0.0002);
                for (dst, &src) in pixels.iter_mut().zip(float.iter()) {
                    *dst = rounded_short(src);
                }
                restore_outside_roi(pixels, &original, width, height, &region, sigma);
            }
            PixelPlane::Gray32(pixels) => {
                let original = pixels.clone();
                blur_gaussian_float(pixels, width, height, &region, sigma, 0.0002);
                restore_outside_roi(pixels, &original, width, height, &region, sigma);
            }
            // Rejected above; kept explicit instead of silently ignoring them.
            PixelPlane::Color256 { .. } => {
                return Err(CoreError::UnsupportedFilter {
                    filter: "Gaussian Blur",
                    pixel_type: "8-bit color (indexed)",
                });
            }
            PixelPlane::ColorRgb(_) => {
                return Err(CoreError::UnsupportedFilter {
                    filter: "Gaussian Blur",
                    pixel_type: "RGB",
                });
            }
        }
        Ok(())
    }

    /// Port of `RankFilters.rank` for the MEDIAN, MIN and MAX filter types of
    /// 8-bit, 16-bit and 32-bit float grayscale images
    /// (original_imagej/ij/plugin/filter/RankFilters.java lines 242-312 and
    /// 385-597).
    ///
    /// Only the current stack slice is processed, matching the `slice` answer
    /// to "Process Stack?"; stack-wide processing is not implemented yet.
    ///
    /// Indexed 8-bit color and RGB are rejected explicitly. RankFilters runs
    /// on RGB through a different per-channel path that this increment does
    /// not port.
    ///
    /// The threshold state is left untouched. RankFilters does not request
    /// KEEP_THRESHOLD, so PlugInFilterRunner calls `resetBinaryThreshold`
    /// afterwards (PlugInFilterRunner.java lines 122-123), which only clears
    /// thresholds stored with `NO_LUT_UPDATE` (ImageProcessor.java lines
    /// 835-838). `setAutoThreshold` stores its levels with RED_LUT
    /// (ImageProcessor.java lines 601-611), so those survive; `ImageDocument`
    /// does not model `lutUpdateMode` and therefore cannot represent the
    /// NO_LUT_UPDATE case (documented limitation, not a silently different
    /// behaviour for any threshold this API can create).
    pub fn rank_filter(&mut self, kind: RankFilterKind, radius: f64) -> Result<(), CoreError> {
        self.rank_filter_named(kind, radius, kind.as_str())
    }

    /// Port of `run("Despeckle")`: the median filter with radius 1
    /// (RankFilters.java lines 117-119).
    pub fn despeckle(&mut self) -> Result<(), CoreError> {
        self.rank_filter_named(RankFilterKind::Median, 1.0, "Despeckle")
    }

    fn rank_filter_named(
        &mut self,
        kind: RankFilterKind,
        radius: f64,
        filter: &'static str,
    ) -> Result<(), CoreError> {
        reject_unsupported_rank_type(self.plane(), filter)?;

        // RankFilters.showDialog validates `radius<0 || radius>maxRadius ||
        // gd.invalidNumber()` (RankFilters.java lines 188-190). A macro that
        // passes such a value bypasses the dialog; reproducing that accident
        // (e.g. radius=-1 behaves as radius=1 because of `radius*radius`) or
        // letting `(int)(radius*radius)` overflow would be unverifiable, so
        // unsupported parameters are rejected instead.
        if !radius.is_finite() || radius < 0.0 || radius > kind.max_radius() {
            return Err(CoreError::InvalidFilterParameter {
                filter,
                parameter: "radius",
                value: format!("{radius}"),
            });
        }

        // PlugInFilterRunner installs an Undo snapshot before running any
        // DOES_ALL filter (PlugInFilterRunner.java lines 107-118).
        self.save_snapshot();

        let region = self.filter_region()?;
        if region.width == 0 || region.height == 0 {
            return Ok(());
        }
        let kernel = RankKernel::new(radius);
        let width = self.width;
        let height = self.height;

        match self.plane_mut() {
            PixelPlane::Gray8(pixels) => {
                *pixels = rank_filter_cached(pixels, width, height, &region, kind, &kernel);
            }
            PixelPlane::Gray16(pixels) => {
                *pixels = rank_filter_cached(pixels, width, height, &region, kind, &kernel);
            }
            PixelPlane::Gray32(pixels) => {
                *pixels = rank_filter_float(pixels, width, height, &region, kind, &kernel);
            }
            // Rejected above; kept explicit instead of silently ignoring them.
            PixelPlane::Color256 { .. } => {
                return Err(CoreError::UnsupportedFilter {
                    filter,
                    pixel_type: "8-bit color (indexed)",
                });
            }
            PixelPlane::ColorRgb(_) => {
                return Err(CoreError::UnsupportedFilter {
                    filter,
                    pixel_type: "RGB",
                });
            }
        }
        Ok(())
    }

    /// `PlugInFilterRunner.prepareProcessor` sets the processor ROI for area
    /// selections and resets it for non-area selections (lines 207-213);
    /// `Filters.run` then calls `ip.resetRoi()` for a line selection
    /// (Filters.java lines 32-33), so line ROIs filter the whole image.
    fn filter_region(&self) -> Result<FilterRegion, CoreError> {
        let (x, y, width, height) = match &self.roi {
            Some(roi) if roi.is_area() => roi
                .clipped_bounds(self.width, self.height)
                .unwrap_or((0, 0, 0, 0)),
            Some(_) => (0, 0, self.width, self.height),
            None => (0, 0, self.width, self.height),
        };
        let mask = if width == 0 || height == 0 {
            None
        } else {
            self.roi_mask()?
        };
        Ok(FilterRegion {
            x,
            y,
            width,
            height,
            mask,
        })
    }
}

fn reject_unsupported_gray_type(plane: &PixelPlane, filter: &'static str) -> Result<(), CoreError> {
    let pixel_type = match plane {
        PixelPlane::Gray8(_) | PixelPlane::Gray16(_) | PixelPlane::Gray32(_) => return Ok(()),
        PixelPlane::Color256 { .. } => "8-bit color (indexed)",
        PixelPlane::ColorRgb(_) => "RGB",
    };
    Err(CoreError::UnsupportedFilter { filter, pixel_type })
}

/// RankFilters runs on every ImageJ type; this increment ports the three
/// grayscale representations. Indexed color and RGB are rejected.
fn reject_unsupported_rank_type(plane: &PixelPlane, filter: &'static str) -> Result<(), CoreError> {
    let pixel_type = match plane {
        PixelPlane::Gray8(_) | PixelPlane::Gray16(_) | PixelPlane::Gray32(_) => return Ok(()),
        PixelPlane::Color256 { .. } => "8-bit color (indexed)",
        PixelPlane::ColorRgb(_) => "RGB",
    };
    Err(CoreError::UnsupportedFilter { filter, pixel_type })
}

/// Circular structuring element from `RankFilters.makeLineRadii`
/// (RankFilters.java lines 866-890).
struct RankKernel {
    /// Two entries (`left`, `right`, both relative to the center column) per
    /// kernel line, then `nPoints` and `kRadius`.
    line_radii: Vec<i32>,
    k_height: i32,
    k_radius: i32,
    n_points: usize,
}

impl RankKernel {
    fn new(radius: f64) -> Self {
        // Legacy-compatible size classes (RankFilters.java lines 866-871).
        let radius = if (1.5..1.75).contains(&radius) {
            1.75
        } else if (2.5..2.85).contains(&radius) {
            2.85
        } else {
            radius
        };
        // `(int)(radius*radius) + 1` and `(int)Math.sqrt(r2 + 1e-10)`
        // (RankFilters.java lines 871-872). `radius` is capped by the dialog
        // limits above, so the i32 cast cannot overflow here.
        let r2 = (radius * radius) as i32 + 1;
        let k_radius = ((r2 as f64) + 1e-10).sqrt() as i32;
        let k_height = 2 * k_radius + 1;
        let mut line_radii = vec![0i32; (2 * k_height + 2) as usize];
        line_radii[(2 * k_radius) as usize] = -k_radius;
        line_radii[(2 * k_radius + 1) as usize] = k_radius;
        let mut n_points = 2 * k_radius + 1;
        for y in 1..=k_radius {
            // `(int)Math.sqrt(r2-y*y+1e-10)`: the integer subtraction happens
            // before promotion to double (RankFilters.java line 879).
            let dx = (((r2 - y * y) as f64) + 1e-10).sqrt() as i32;
            line_radii[(2 * (k_radius - y)) as usize] = -dx;
            line_radii[(2 * (k_radius - y) + 1) as usize] = dx;
            line_radii[(2 * (k_radius + y)) as usize] = -dx;
            line_radii[(2 * (k_radius + y) + 1) as usize] = dx;
            n_points += 4 * dx + 2;
        }
        let len = line_radii.len();
        line_radii[len - 2] = n_points;
        line_radii[len - 1] = k_radius;
        Self {
            line_radii,
            k_height,
            k_radius,
            n_points: n_points as usize,
        }
    }
}

/// Integral `ImageProcessor` storage used by the shared rank cache.
///
/// Java caches 8-bit pixels as `bPixels[pp]&0xff` and 16-bit pixels as
/// `sPixels[pp]&0xffff` (RankFilters.java lines 622-629), then writes back with
/// the matching `&0xff`/`&0xffff` truncation (lines 648-656). Both conversions
/// are exact in `f32`, so one cache engine serves both depths.
trait RankSample: Copy {
    fn to_cache(self) -> f32;
    fn from_cache(value: f32) -> Self;
}

impl RankSample for u8 {
    fn to_cache(self) -> f32 {
        self as f32
    }

    fn from_cache(value: f32) -> Self {
        // `(byte)(((int)(v+0.5f))&0xff)` (RankFilters.java line 652).
        (((value + 0.5f32) as i32) & 0xff) as u8
    }
}

impl RankSample for u16 {
    fn to_cache(self) -> f32 {
        self as f32
    }

    fn from_cache(value: f32) -> Self {
        // `(short)(((int)(v+0.5f))&0xffff)` (RankFilters.java line 656).
        (((value + 0.5f32) as i32) & 0xffff) as u16
    }
}

/// `makeCachePointers` (RankFilters.java lines 907-917): two cache index bases
/// per kernel line, already offset by `kRadius`, so adding the ROI-relative `x`
/// addresses `roi.x + lineRadii`.
fn make_cache_pointers(kernel: &RankKernel, cache_width: usize) -> Vec<usize> {
    let mut pointers = vec![0usize; 2 * kernel.k_height as usize];
    for line in 0..kernel.k_height as usize {
        let row_base = (line * cache_width) as i64;
        let left = row_base + (kernel.k_radius + kernel.line_radii[2 * line]) as i64;
        let right = row_base + (kernel.k_radius + kernel.line_radii[2 * line + 1]) as i64;
        debug_assert!(left >= 0 && right >= 0);
        pointers[2 * line] = left as usize;
        pointers[2 * line + 1] = right as usize;
    }
    pointers
}

/// `readLineToCache` (RankFilters.java lines 620-643): copy the inside part of
/// a row as an exact integer and pad both sides with the nearest inside column.
#[allow(clippy::too_many_arguments)]
fn read_line_to_cache<T: RankSample>(
    src: &[T],
    pixel_line: usize,
    xmin_inside: usize,
    width_inside: usize,
    cache: &mut [f32],
    cache_line: usize,
    pad_left: usize,
    pad_right: usize,
) {
    debug_assert!(width_inside > 0);
    for i in 0..width_inside {
        cache[cache_line + pad_left + i] = src[pixel_line + xmin_inside + i].to_cache();
    }
    let first = cache[cache_line + pad_left];
    for i in 0..pad_left {
        cache[cache_line + i] = first;
    }
    let last = cache[cache_line + pad_left + width_inside - 1];
    for i in 0..pad_right {
        cache[cache_line + pad_left + width_inside + i] = last;
    }
}

/// `readLineToCacheOrPad` (RankFilters.java lines 604-617): out-of-image rows
/// duplicate the nearest image row, which makes the cache equal to sampling
/// with `clamp(y+dy, 0, height-1)`. Keeping the ring buffer makes the min/max
/// side-column scans contiguous instead of recomputing clamps per sample.
#[allow(clippy::too_many_arguments)]
fn read_line_to_cache_or_pad<T: RankSample>(
    src: &[T],
    width: usize,
    height: usize,
    roi_y: i32,
    xmin_inside: usize,
    width_inside: usize,
    cache: &mut [f32],
    cache_width: usize,
    cache_height: usize,
    pad_left: usize,
    pad_right: usize,
    k_height: i32,
    y: i32,
) {
    let line_in_cache = y.rem_euclid(cache_height as i32) as usize;
    if y < height as i32 {
        read_line_to_cache(
            src,
            y as usize * width,
            xmin_inside,
            width_inside,
            cache,
            line_in_cache * cache_width,
            pad_left,
            pad_right,
        );
        if y == 0 {
            // Rows above the image duplicate the first row; they occupy the
            // last slots of the ring buffer because `prevY` is negative
            // (RankFilters.java lines 611-614).
            for prev_y in (roi_y - k_height / 2)..0 {
                let prev_line_in_cache = (cache_height as i32 + prev_y) as usize;
                cache.copy_within(0..cache_width, prev_line_in_cache * cache_width);
            }
        }
    } else {
        let from = (height - 1).rem_euclid(cache_height) * cache_width;
        cache.copy_within(from..from + cache_width, line_in_cache * cache_width);
    }
}

/// `getAreaMax` (RankFilters.java lines 670-678). `ignore_right` leaves out the
/// entering kernel column so an incremental rescan does not double-count it.
fn cache_area_max(
    cache: &[f32],
    pointers: &[usize],
    x: i32,
    ignore_right: i32,
    mut max: f32,
    sign: f32,
    k_height: i32,
) -> f32 {
    for line in 0..k_height as usize {
        let start = pointers[2 * line] as isize + x as isize;
        let end = pointers[2 * line + 1] as isize + x as isize - ignore_right as isize;
        if start <= end {
            for &raw in &cache[start as usize..=end as usize] {
                let value = raw * sign;
                if max < value {
                    max = value;
                }
            }
        }
    }
    max
}

/// `getSideMax` (RankFilters.java lines 682-690): max of the column entering
/// the window (`is_right`) or leaving it. The leaving side is only asked for
/// `x >= 1`, so `x - 1` never underflows the left padding.
fn cache_side_max(
    cache: &[f32],
    pointers: &[usize],
    x: i32,
    is_right: bool,
    sign: f32,
    k_height: i32,
) -> f32 {
    let mut max = -f32::MAX;
    let base = if is_right { x as isize } else { x as isize - 1 };
    for line in 0..k_height as usize {
        let edge = if is_right { 1 } else { 0 };
        let index = pointers[2 * line + edge] as isize + base;
        debug_assert!(index >= 0);
        let value = cache[index as usize] * sign;
        if max < value {
            max = value;
        }
    }
    max
}

/// `getMedian` (RankFilters.java lines 729-752): counts the values above and
/// below the guess, then selects the `kNPoints/2`-th lowest on whichever side
/// holds it. Returning `guess` when neither side is large enough is exact for
/// odd `kNPoints`, so no sort is needed. Short values are exact integers in
/// `f32`, so ties are value-identical to ImageJ's selection.
#[allow(clippy::too_many_arguments)]
fn cache_median(
    cache: &[f32],
    pointers: &[usize],
    x: i32,
    above: &mut [f32],
    below: &mut [f32],
    n_points: usize,
    guess: f32,
    k_height: i32,
) -> f32 {
    let mut n_above = 0usize;
    let mut n_below = 0usize;
    for line in 0..k_height as usize {
        let start = (pointers[2 * line] as isize + x as isize) as usize;
        let end = (pointers[2 * line + 1] as isize + x as isize) as usize;
        for &value in &cache[start..=end] {
            if value > guess {
                above[n_above] = value;
                n_above += 1;
            } else if value < guess {
                below[n_below] = value;
                n_below += 1;
            }
        }
    }
    let half = n_points / 2;
    if n_above > half {
        find_nth_lowest(above, n_above, n_above - half - 1)
    } else if n_below > half {
        find_nth_lowest(below, n_below, half)
    } else {
        guess
    }
}

/// Integral (8-bit and 16-bit) path of `RankFilters.doFiltering`/`filterLine`
/// (RankFilters.java lines 385-597).
///
/// The cache is the padded ring buffer of lines 328-334 and 489-514: one line
/// per kernel row, with out-of-image x and y samples replicated from the
/// nearest edge pixel, so sampling equals `clamp(x+dx)`/`clamp(y+dy)`. Byte
/// pixels enter as `pixel & 0xff` (line 625) and short pixels as
/// `pixel & 0xffff` (line 629), both exact in `f32`.
///
/// Min/max use the side-column cache of lines 546-560: only the entering and
/// leaving kernel columns are scanned, with a full-window rescan only when the
/// leaving column contained the current extremum. The median uses the
/// `kNPoints/2`-th order statistic of `getMedian`, not a sort. Writing back
/// applies `(byte)(((int)(v+0.5f))&0xff)` / `(short)(((int)(v+0.5f))&0xffff)`
/// (lines 652, 656); the integers selected here make `+0.5f` and truncation the
/// identity, and the mask is the unsigned reinterpretation.
///
/// RankFilters itself ignores the ROI mask: every pixel of the ROI rectangle is
/// filtered, and PlugInFilterRunner restores masked-out pixels afterwards
/// (lines 271-275). The incremental state therefore keeps advancing where the
/// result is discarded.
fn rank_filter_cached<T: RankSample>(
    src: &[T],
    width: u32,
    height: u32,
    region: &FilterRegion,
    kind: RankFilterKind,
    kernel: &RankKernel,
) -> Vec<T> {
    let mut out = src.to_vec();
    let width_i = width as i32;
    let roi_x = region.x as i32;
    let roi_y = region.y as i32;
    let k_radius = kernel.k_radius;
    let k_height = kernel.k_height;
    let n_points = kernel.n_points;

    let xmin = roi_x - k_radius;
    let xmax = roi_x + region.width as i32 + k_radius;
    let pad_left = if xmin < 0 { -xmin } else { 0 } as usize;
    let pad_right = if xmax > width_i { xmax - width_i } else { 0 } as usize;
    let xmin_inside = xmin.max(0) as usize;
    let xmax_inside = xmax.min(width_i) as usize;
    let width_inside = xmax_inside - xmin_inside;

    let cache_width = region.width as usize + 2 * k_radius as usize;
    let cache_height = k_height as usize;
    let mut cache = vec![0f32; cache_width * cache_height];
    let mut pointers = make_cache_pointers(kernel, cache_width);
    let mut previous_y = k_height / 2 - cache_height as i32;

    let sign = match kind {
        RankFilterKind::Minimum => -1.0f32,
        _ => 1.0f32,
    };
    let small_kernel = k_radius < 2;
    let (mut above, mut below) = if matches!(kind, RankFilterKind::Median) {
        (vec![0f32; n_points], vec![0f32; n_points])
    } else {
        (Vec::new(), Vec::new())
    };

    for y in region.y..region.y + region.height {
        let y_i = y as i32;
        let shift = cache_width as i64 * (y_i - previous_y) as i64;
        let cache_len = cache.len() as i64;
        for pointer in pointers.iter_mut() {
            *pointer = (*pointer as i64 + shift).rem_euclid(cache_len) as usize;
        }
        previous_y = y_i;

        // The first processed line reads the whole kernel height, later lines
        // only the row entering the window (RankFilters.java lines 489-494).
        let y_start_reading = if y_i == roi_y {
            (roi_y - k_height / 2).max(0)
        } else {
            y_i + k_height / 2
        };
        for y_new in y_start_reading..=y_i + k_height / 2 {
            read_line_to_cache_or_pad(
                src,
                width as usize,
                height as usize,
                roi_y,
                xmin_inside,
                width_inside,
                &mut cache,
                cache_width,
                cache_height,
                pad_left,
                pad_right,
                k_height,
                y_new,
            );
        }

        let cache_line_p = cache_width * (y_i as usize % cache_height) + k_radius as usize;
        let mut line_max = 0f32;
        let mut full_calculation = true;
        // The first median guess is the current pixel (RankFilters.java
        // line 533); later pixels keep the previous median (line 580).
        let mut median = cache[cache_line_p];

        for x in 0..region.width as i32 {
            let value = match kind {
                RankFilterKind::Minimum | RankFilterKind::Maximum => {
                    if full_calculation {
                        full_calculation = small_kernel;
                        line_max =
                            cache_area_max(&cache, &pointers, x, 0, -f32::MAX, sign, k_height);
                    } else {
                        let new_points_max =
                            cache_side_max(&cache, &pointers, x, true, sign, k_height);
                        if new_points_max >= line_max {
                            line_max = new_points_max;
                        } else {
                            let removed_points_max =
                                cache_side_max(&cache, &pointers, x, false, sign, k_height);
                            if removed_points_max >= line_max {
                                line_max = cache_area_max(
                                    &cache,
                                    &pointers,
                                    x,
                                    1,
                                    new_points_max,
                                    sign,
                                    k_height,
                                );
                            }
                        }
                    }
                    line_max * sign
                }
                RankFilterKind::Median => {
                    median = cache_median(
                        &cache, &pointers, x, &mut above, &mut below, n_points, median, k_height,
                    );
                    median
                }
            };
            if mask_allows(region.mask.as_ref(), region.x + x as u32, y) {
                out[(y * width + region.x + x as u32) as usize] = T::from_cache(value);
            }
        }
    }
    out
}

/// The padded input stripe on one output line: `readLineToCache` and
/// `readLineToCacheOrPad` replicate the nearest image pixel in x and y
/// (RankFilters.java lines 599-643). `offset` is relative to `roi_x`.
struct FloatCache<'a> {
    src: &'a [f32],
    width: i32,
    height: i32,
    roi_x: i32,
    y: i32,
    half_height: i32,
}

impl FloatCache<'_> {
    fn sample(&self, line: i32, offset: i32) -> f32 {
        let sy = (self.y + line - self.half_height).clamp(0, self.height - 1);
        let sx = (self.roi_x + offset).clamp(0, self.width - 1);
        self.src[(sy * self.width + sx) as usize]
    }
}

/// `getAreaMax` (RankFilters.java lines 670-678): strict `<` comparisons from
/// `initial`, so for equal maxima (notably `-0.0`/`0.0`) the first value in
/// kernel scan order wins and the original sign of zero survives.
fn get_area_max(
    cache: &FloatCache,
    kernel: &RankKernel,
    x: i32,
    ignore_right: i32,
    mut max: f32,
    sign: f32,
) -> f32 {
    for line in 0..kernel.k_height {
        let left = kernel.line_radii[(2 * line) as usize];
        let right = kernel.line_radii[(2 * line + 1) as usize] - ignore_right;
        for offset in left..=right {
            let value = cache.sample(line, x + offset) * sign;
            if max < value {
                max = value;
            }
        }
    }
    max
}

/// `getSideMax` (RankFilters.java lines 682-690): the border column entering
/// (`is_right`) or leaving the window.
fn get_side_max(cache: &FloatCache, kernel: &RankKernel, x: i32, is_right: bool, sign: f32) -> f32 {
    let mut max = -f32::MAX;
    let base = if is_right { x } else { x - 1 };
    for line in 0..kernel.k_height {
        let edge = if is_right {
            kernel.line_radii[(2 * line + 1) as usize]
        } else {
            kernel.line_radii[(2 * line) as usize]
        };
        let value = cache.sample(line, base + edge) * sign;
        if max < value {
            max = value;
        }
    }
    max
}

/// `getNaNAwareMedian` (RankFilters.java lines 757-783). NaNs are ignored, the
/// result is NaN only when the window has nothing else, and for an even number
/// of remaining values the guess decides which side of the centre is used
/// (lines 745-751), which is why the counts are reproduced instead of a plain
/// sorted-index lookup.
fn nan_aware_median(
    cache: &FloatCache,
    kernel: &RankKernel,
    x: i32,
    buf1: &mut [f32],
    buf2: &mut [f32],
    guess: f32,
) -> f32 {
    let mut n_above = 0usize;
    let mut n_below = 0usize;
    let mut points = kernel.n_points as i32;
    for line in 0..kernel.k_height {
        let left = kernel.line_radii[(2 * line) as usize];
        let right = kernel.line_radii[(2 * line + 1) as usize];
        for offset in left..=right {
            let value = cache.sample(line, x + offset);
            if value.is_nan() {
                points -= 1;
            } else if value > guess {
                buf1[n_above] = value;
                n_above += 1;
            } else if value < guess {
                buf2[n_below] = value;
                n_below += 1;
            }
        }
    }
    if points == 0 {
        return f32::NAN;
    }
    let half = points / 2;
    if n_above as i32 > half {
        find_nth_lowest(buf1, n_above, (n_above as i32 - half - 1) as usize)
    } else if n_below as i32 > half {
        find_nth_lowest(buf2, n_below, half as usize)
    } else {
        guess
    }
}

/// Port of `findNthLowestNumber` (RankFilters.java lines 790-815). The exact
/// Hoare partition is required for float parity: its `<`/`>` comparisons treat
/// `-0.0` and `0.0` as equal, so the returned bit pattern among equal values
/// depends on the swap sequence rather than on a sorted order.
fn find_nth_lowest(buf: &mut [f32], buf_length: usize, n: usize) -> f32 {
    // Java's `int` indices can go to -1 in `j`; keep signed arithmetic.
    let n = n as i32;
    let mut l = 0i32;
    let mut m = buf_length as i32 - 1;
    let mut med = buf[n as usize];
    while l < m {
        let mut i = l;
        let mut j = m;
        loop {
            while buf[i as usize] < med {
                i += 1;
            }
            while med < buf[j as usize] {
                j -= 1;
            }
            buf.swap(i as usize, j as usize);
            i += 1;
            j -= 1;
            if !(j >= n && i <= n) {
                break;
            }
        }
        if j < n {
            l = i;
        }
        if n < i {
            m = j;
        }
        med = buf[n as usize];
    }
    med
}

/// FloatProcessor path of `RankFilters.doFiltering`/`filterLine`
/// (RankFilters.java lines 385-597). Float pixels are copied bit-for-bit into
/// the cache (line 631) and written back unrounded (lines 512-513), so NaN
/// payloads and the sign of zero are observable:
///
/// * the median's first guess is the current pixel, replaced by `Float.NaN`
///   when that pixel is NaN (line 577), so a NaN centre stays NaN;
/// * min/max start at `-Float.MAX_VALUE`, so an all-NaN window saturates to
///   `+Float.MAX_VALUE` for Minimum and `-Float.MAX_VALUE` for Maximum;
/// * for `kRadius >= 2` min/max advance incrementally with `>=` comparisons
///   (lines 546-560), which can select a different signed zero than a full
///   rescan. The incremental state must therefore be reproduced exactly.
fn rank_filter_float(
    src: &[f32],
    width: u32,
    height: u32,
    region: &FilterRegion,
    kind: RankFilterKind,
    kernel: &RankKernel,
) -> Vec<f32> {
    let mut out = src.to_vec();
    let width_i = width as i32;
    let height_i = height as i32;
    let roi_x = region.x as i32;
    let sign = match kind {
        RankFilterKind::Minimum => -1.0f32,
        _ => 1.0f32,
    };
    let small_kernel = kernel.k_radius < 2;
    let (mut buf1, mut buf2) = if matches!(kind, RankFilterKind::Median) {
        (vec![0f32; kernel.n_points], vec![0f32; kernel.n_points])
    } else {
        (Vec::new(), Vec::new())
    };

    for y in region.y..region.y + region.height {
        let y_i = y as i32;
        let cache = FloatCache {
            src,
            width: width_i,
            height: height_i,
            roi_x,
            y: y_i,
            half_height: kernel.k_height / 2,
        };
        let mut line_max = 0f32;
        let mut full_calculation = true;

        for x in 0..region.width as i32 {
            let px = roi_x + x;
            // RankFilters knows nothing about the mask: it filters every pixel
            // of the ROI rectangle and PlugInFilterRunner restores masked-out
            // pixels afterwards, so the incremental min/max state must keep
            // advancing even where the result is discarded.
            let keep = mask_allows(region.mask.as_ref(), px as u32, y);
            let value = match kind {
                RankFilterKind::Minimum | RankFilterKind::Maximum => {
                    if full_calculation {
                        full_calculation = small_kernel;
                        line_max = get_area_max(&cache, kernel, x, 0, -f32::MAX, sign);
                    } else {
                        let new_points_max = get_side_max(&cache, kernel, x, true, sign);
                        if new_points_max >= line_max {
                            line_max = new_points_max;
                        } else {
                            let removed_points_max = get_side_max(&cache, kernel, x, false, sign);
                            if removed_points_max >= line_max {
                                line_max = get_area_max(&cache, kernel, x, 1, new_points_max, sign);
                            }
                        }
                    }
                    line_max * sign
                }
                RankFilterKind::Median => {
                    if full_calculation {
                        full_calculation = small_kernel;
                    }
                    let guess = src[(y_i * width_i + px) as usize];
                    let guess = if guess.is_nan() { f32::NAN } else { guess };
                    nan_aware_median(&cache, kernel, x, &mut buf1, &mut buf2, guess)
                }
            };
            if keep {
                out[(y_i * width_i + px) as usize] = value;
            }
        }
    }
    out
}

/// ByteProcessor.filter(BLUR_MORE): `(p1+..+p9+4)/9` for every pixel of the ROI
/// rectangle (ByteProcessor.java lines 599-600).
///
/// The 8-bit path is the only one with separate edge passes. When the ROI's
/// last column/row lands on `width-2`/`height-2`, `xMax==width-2` /
/// `yMax==height-2` also rewrite the image's own last column/row
/// (ByteProcessor.java lines 678-681). Those pixels sit outside the ROI
/// rectangle, so `reset(mask)` never restores them.
fn smooth_byte(src: &[u8], width: u32, height: u32, region: &FilterRegion) -> Vec<u8> {
    let mut out = src.to_vec();
    for y in region.y..region.y + region.height {
        for x in region.x..region.x + region.width {
            if !mask_allows(region.mask.as_ref(), x, y) {
                continue;
            }
            out[(y * width + x) as usize] = blur_byte(src, width, height, x as i32, y as i32);
        }
    }

    let x_max = ((region.x + region.width) as i32 - 1).min(width as i32 - 2);
    if x_max == width as i32 - 2 && region.x + region.width < width {
        let last = width - 1;
        for y in region.y..region.y + region.height {
            out[(y * width + last) as usize] = blur_byte(src, width, height, last as i32, y as i32);
        }
    }
    let y_max = ((region.y + region.height) as i32 - 1).min(height as i32 - 2);
    if y_max == height as i32 - 2 && region.y + region.height < height {
        let last = height - 1;
        for x in region.x..region.x + region.width {
            out[(last * width + x) as usize] = blur_byte(src, width, height, x as i32, last as i32);
        }
    }
    out
}

fn blur_byte(src: &[u8], width: u32, height: u32, x: i32, y: i32) -> u8 {
    let mut sum = 0u32;
    for dy in -1..=1 {
        for dx in -1..=1 {
            sum += sample(src, width, height, x + dx, y + dy) as u32;
        }
    }
    // The +4 is ImageJ's nearest-integer rounding before the integer divide.
    ((sum + 4) / 9) as u8
}

/// ShortProcessor.filter3x3(BLUR_MORE): `(v1+..+v9+4)/9` over the ROI
/// rectangle only (ShortProcessor.java lines 688-697).
fn smooth_short(src: &[u16], width: u32, height: u32, region: &FilterRegion) -> Vec<u16> {
    let mut out = src.to_vec();
    for y in region.y..region.y + region.height {
        for x in region.x..region.x + region.width {
            if !mask_allows(region.mask.as_ref(), x, y) {
                continue;
            }
            let mut sum = 0u32;
            for dy in -1..=1 {
                for dx in -1..=1 {
                    sum += sample(src, width, height, x as i32 + dx, y as i32 + dy) as u32;
                }
            }
            out[(y * width + x) as usize] = ((sum + 4) / 9) as u16;
        }
    }
    out
}

/// FloatProcessor.filter3x3(BLUR_MORE): the nine neighbours are summed in float
/// from top-left to bottom-right and multiplied by `0.11111111f`
/// (FloatProcessor.java lines 617-626). Reproducing the single-precision sum
/// order is required for bit-exact parity.
fn smooth_float(src: &[f32], width: u32, height: u32, region: &FilterRegion) -> Vec<f32> {
    let mut out = src.to_vec();
    for y in region.y..region.y + region.height {
        for x in region.x..region.x + region.width {
            if !mask_allows(region.mask.as_ref(), x, y) {
                continue;
            }
            let (px, py) = (x as i32, y as i32);
            let v1 = sample(src, width, height, px - 1, py - 1);
            let v2 = sample(src, width, height, px, py - 1);
            let v3 = sample(src, width, height, px + 1, py - 1);
            let v4 = sample(src, width, height, px - 1, py);
            let v5 = sample(src, width, height, px, py);
            let v6 = sample(src, width, height, px + 1, py);
            let v7 = sample(src, width, height, px - 1, py + 1);
            let v8 = sample(src, width, height, px, py + 1);
            let v9 = sample(src, width, height, px + 1, py + 1);
            // Left-associative like Java's `v1+v2+...+v9`; starting from the
            // first term (not 0.0) also preserves a negative zero sum.
            out[(y * width + x) as usize] =
                (v1 + v2 + v3 + v4 + v5 + v6 + v7 + v8 + v9) * 0.11111111f32;
        }
    }
    out
}

/// `ImageProcessor.sharpen`'s kernel (ImageProcessor.java lines 1642-1644).
/// Its coefficients sum to 4, which is the `scale` divisor below.
const SHARPEN_KERNEL: [i32; 9] = [-1, -1, -1, -1, 12, -1, -1, -1, -1];

/// `ByteProcessor.convolve3x3` (lines 514-557). The sum is int math and
/// `(sum+scale/2)/scale` relies on Java's truncation toward zero, which Rust's
/// integer `/` also guarantees.
fn convolve_3x3_byte(
    src: &[u8],
    width: u32,
    height: u32,
    region: &FilterRegion,
    kernel: [i32; 9],
) -> Vec<u8> {
    let scale = kernel_scale(&kernel);
    let mut out = src.to_vec();
    for y in region.y..region.y + region.height {
        for x in region.x..region.x + region.width {
            if !mask_allows(region.mask.as_ref(), x, y) {
                continue;
            }
            let (px, py) = (x as i32, y as i32);
            let v = [
                sample(src, width, height, px - 1, py - 1) as i32,
                sample(src, width, height, px, py - 1) as i32,
                sample(src, width, height, px + 1, py - 1) as i32,
                sample(src, width, height, px - 1, py) as i32,
                sample(src, width, height, px, py) as i32,
                sample(src, width, height, px + 1, py) as i32,
                sample(src, width, height, px - 1, py + 1) as i32,
                sample(src, width, height, px, py + 1) as i32,
                sample(src, width, height, px + 1, py + 1) as i32,
            ];
            let mut sum = 0i32;
            for i in 0..9 {
                sum += kernel[i] * v[i];
            }
            let sum = (sum + scale / 2) / scale;
            out[(y * width + x) as usize] = sum.clamp(0, 255) as u8;
        }
    }
    out
}

/// `ShortProcessor.convolve3x3` (lines 643-733): int math, same truncating
/// rounding, saturation at 0 and 65535.
fn convolve_3x3_short(
    src: &[u16],
    width: u32,
    height: u32,
    region: &FilterRegion,
    kernel: [i32; 9],
) -> Vec<u16> {
    let scale = kernel_scale(&kernel);
    let mut out = src.to_vec();
    for y in region.y..region.y + region.height {
        for x in region.x..region.x + region.width {
            if !mask_allows(region.mask.as_ref(), x, y) {
                continue;
            }
            let (px, py) = (x as i32, y as i32);
            let v = [
                sample(src, width, height, px - 1, py - 1) as i32,
                sample(src, width, height, px, py - 1) as i32,
                sample(src, width, height, px + 1, py - 1) as i32,
                sample(src, width, height, px - 1, py) as i32,
                sample(src, width, height, px, py) as i32,
                sample(src, width, height, px + 1, py) as i32,
                sample(src, width, height, px - 1, py + 1) as i32,
                sample(src, width, height, px, py + 1) as i32,
                sample(src, width, height, px + 1, py + 1) as i32,
            ];
            let mut sum = 0i32;
            for i in 0..9 {
                sum += kernel[i] * v[i];
            }
            let sum = (sum + scale / 2) / scale;
            out[(y * width + x) as usize] = sum.clamp(0, 65_535) as u16;
        }
    }
    out
}

/// `FloatProcessor.filter3x3(CONVOLVE)` (lines 581-659): the products are
/// accumulated left-to-right in `f32`, then multiplied by `1f/scale` (0.25 for
/// the Sharpen kernel) and never clamped. Reproducing the single-precision
/// order is required for bit-exact parity.
fn convolve_3x3_float(
    src: &[f32],
    width: u32,
    height: u32,
    region: &FilterRegion,
    kernel: [i32; 9],
) -> Vec<f32> {
    let mut scale = 0f32;
    for k in kernel {
        scale += k as f32;
    }
    if scale == 0.0 {
        scale = 1.0;
    }
    let scale = 1f32 / scale;

    let mut out = src.to_vec();
    for y in region.y..region.y + region.height {
        for x in region.x..region.x + region.width {
            if !mask_allows(region.mask.as_ref(), x, y) {
                continue;
            }
            let (px, py) = (x as i32, y as i32);
            let k = kernel.map(|v| v as f32);
            let v1 = sample(src, width, height, px - 1, py - 1);
            let v2 = sample(src, width, height, px, py - 1);
            let v3 = sample(src, width, height, px + 1, py - 1);
            let v4 = sample(src, width, height, px - 1, py);
            let v5 = sample(src, width, height, px, py);
            let v6 = sample(src, width, height, px + 1, py);
            let v7 = sample(src, width, height, px - 1, py + 1);
            let v8 = sample(src, width, height, px, py + 1);
            let v9 = sample(src, width, height, px + 1, py + 1);
            let sum = k[0] * v1
                + k[1] * v2
                + k[2] * v3
                + k[3] * v4
                + k[4] * v5
                + k[5] * v6
                + k[6] * v7
                + k[7] * v8
                + k[8] * v9;
            out[(y * width + x) as usize] = sum * scale;
        }
    }
    out
}

/// `ByteProcessor.filter(FIND_EDGES)` (lines 568-681). The interior loop runs
/// only over `xMin..xMax`/`yMin..yMax`, where `xMin=max(roiX,1)` and
/// `xMax=min(roiX+roiWidth-1,width-2)` (ImageProcessor.java 879-882), and the
/// four `filterEdge` passes clamp at the image border. The right/bottom passes
/// also run when the ROI ends on `width-2`/`height-2`, rewriting the image's
/// last column/row even though a rectangular ROI has no mask to restore them
/// (ByteProcessor.java 678-681).
fn find_edges_byte(src: &[u8], width: u32, height: u32, region: &FilterRegion) -> Vec<u8> {
    let mut out = src.to_vec();
    let x_min = region.x.max(1) as i64;
    let x_max = (region.x as i64 + region.width as i64 - 1).min(width as i64 - 2);
    let y_min = region.y.max(1) as i64;
    let y_max = (region.y as i64 + region.height as i64 - 1).min(height as i64 - 2);

    for y in y_min..=y_max {
        for x in x_min..=x_max {
            out[(y as u32 * width + x as u32) as usize] =
                sobel_byte(src, width, height, x as i32, y as i32);
        }
    }

    if x_min == 1 {
        find_edges_byte_edge(
            src,
            &mut out,
            width,
            height,
            region.height,
            region.x as i32,
            region.y as i32,
            0,
            1,
        );
    }
    if y_min == 1 {
        find_edges_byte_edge(
            src,
            &mut out,
            width,
            height,
            region.width,
            region.x as i32,
            region.y as i32,
            1,
            0,
        );
    }
    if x_max == width as i64 - 2 {
        find_edges_byte_edge(
            src,
            &mut out,
            width,
            height,
            region.height,
            width as i32 - 1,
            region.y as i32,
            0,
            1,
        );
    }
    if y_max == height as i64 - 2 {
        find_edges_byte_edge(
            src,
            &mut out,
            width,
            height,
            region.width,
            region.x as i32,
            height as i32 - 1,
            1,
            0,
        );
    }
    // ImageProcessor.reset(mask) only restores non-mask pixels inside the ROI
    // rectangle (ByteProcessor.java 192-204), so a right/bottom edge pass that
    // lands outside the clipped ROI keeps its filtered values.
    if let Some(mask) = &region.mask {
        for y in region.y..region.y + region.height {
            for x in region.x..region.x + region.width {
                if !mask.contains(x, y) {
                    let i = (y * width + x) as usize;
                    out[i] = src[i];
                }
            }
        }
    }
    out
}

/// `ByteProcessor.findEdges`' `filterEdge` pass (lines 682-718) with
/// `getEdgePixel` border replication (lines 770-776).
#[allow(clippy::too_many_arguments)]
fn find_edges_byte_edge(
    src: &[u8],
    out: &mut [u8],
    width: u32,
    height: u32,
    n: u32,
    mut x: i32,
    mut y: i32,
    xinc: i32,
    yinc: i32,
) {
    for _ in 0..n {
        out[(y as u32 * width + x as u32) as usize] = sobel_byte(src, width, height, x, y);
        x += xinc;
        y += yinc;
    }
}

/// `FloatProcessor.filter3x3(FIND_EDGES)` (lines 629-644): the whole ROI
/// rectangle is written, sampling clamps at the image border, and both Sobel
/// sums plus the squared magnitude stay in single precision before the
/// `double` square root. Reproducing the `f32` order is required for bit-exact
/// parity.
fn find_edges_float(src: &[f32], width: u32, height: u32, region: &FilterRegion) -> Vec<f32> {
    let mut out = src.to_vec();
    for y in region.y..region.y + region.height {
        for x in region.x..region.x + region.width {
            if !mask_allows(region.mask.as_ref(), x, y) {
                continue;
            }
            let (px, py) = (x as i32, y as i32);
            let v1 = sample(src, width, height, px - 1, py - 1);
            let v2 = sample(src, width, height, px, py - 1);
            let v3 = sample(src, width, height, px + 1, py - 1);
            let v4 = sample(src, width, height, px - 1, py);
            let v6 = sample(src, width, height, px + 1, py);
            let v7 = sample(src, width, height, px - 1, py + 1);
            let v8 = sample(src, width, height, px, py + 1);
            let v9 = sample(src, width, height, px + 1, py + 1);
            let sum1 = v1 + 2f32 * v2 + v3 - v7 - 2f32 * v8 - v9;
            let sum2 = v1 + 2f32 * v4 + v7 - v3 - 2f32 * v6 - v9;
            out[(y * width + x) as usize] = ((sum1 * sum1 + sum2 * sum2) as f64).sqrt() as f32;
        }
    }
    out
}

/// `ShortProcessor.filter3x3(FIND_EDGES)` (lines 700-716): the whole ROI
/// rectangle is written with clamped sampling, the magnitude is clamped to
/// 65535 and then truncated by the `(short)` cast.
fn find_edges_short(src: &[u16], width: u32, height: u32, region: &FilterRegion) -> Vec<u16> {
    let mut out = src.to_vec();
    for y in region.y..region.y + region.height {
        for x in region.x..region.x + region.width {
            if !mask_allows(region.mask.as_ref(), x, y) {
                continue;
            }
            let (px, py) = (x as i32, y as i32);
            let p1 = sample(src, width, height, px - 1, py - 1) as i32;
            let p2 = sample(src, width, height, px, py - 1) as i32;
            let p3 = sample(src, width, height, px + 1, py - 1) as i32;
            let p4 = sample(src, width, height, px - 1, py) as i32;
            let p6 = sample(src, width, height, px + 1, py) as i32;
            let p7 = sample(src, width, height, px - 1, py + 1) as i32;
            let p8 = sample(src, width, height, px, py + 1) as i32;
            let p9 = sample(src, width, height, px + 1, py + 1) as i32;
            let sum1 = p1 + 2 * p2 + p3 - p7 - 2 * p8 - p9;
            let sum2 = p1 + 2 * p4 + p7 - p3 - 2 * p6 - p9;
            let result = ((sum1 as f64 * sum1 as f64) + (sum2 as f64 * sum2 as f64)).sqrt();
            out[(y * width + x) as usize] = result.min(65_535.0) as u16;
        }
    }
    out
}

/// Shared Sobel magnitude for `ByteProcessor.filter`'s interior and edge
/// passes: `(int)Math.sqrt(sum1*sum1 + sum2*sum2)` with 8-bit saturation
/// (ByteProcessor.java 604-608).
fn sobel_byte(src: &[u8], width: u32, height: u32, x: i32, y: i32) -> u8 {
    let p1 = sample(src, width, height, x - 1, y - 1) as i32;
    let p2 = sample(src, width, height, x, y - 1) as i32;
    let p3 = sample(src, width, height, x + 1, y - 1) as i32;
    let p4 = sample(src, width, height, x - 1, y) as i32;
    let p6 = sample(src, width, height, x + 1, y) as i32;
    let p7 = sample(src, width, height, x - 1, y + 1) as i32;
    let p8 = sample(src, width, height, x, y + 1) as i32;
    let p9 = sample(src, width, height, x + 1, y + 1) as i32;
    let sum1 = p1 + 2 * p2 + p3 - p7 - 2 * p8 - p9;
    let sum2 = p1 + 2 * p4 + p7 - p3 - 2 * p6 - p9;
    let sum = ((sum1 * sum1 + sum2 * sum2) as f64).sqrt() as i32;
    sum.min(255) as u8
}

/// Java's `scale` is the integer kernel sum, replaced by 1 when it is zero
/// (ByteProcessor.java 522-524, ShortProcessor.java 666-668).
fn kernel_scale(kernel: &[i32; 9]) -> i32 {
    let sum: i32 = kernel.iter().sum();
    if sum == 0 { 1 } else { sum }
}

/// Neighbour sampling clamps to the image bounds: `ByteProcessor.getEdgePixel`
/// (lines 782-789) and the index arithmetic in `ShortProcessor`/`FloatProcessor`
/// `filter3x3` both replicate the border rows and columns.
fn sample<T: Copy>(pixels: &[T], width: u32, height: u32, x: i32, y: i32) -> T {
    let cx = x.clamp(0, width as i32 - 1) as u32;
    let cy = y.clamp(0, height as i32 - 1) as u32;
    pixels[(cy * width + cx) as usize]
}

/// `PlugInFilterRunner` restores pixels whose mask byte is zero after the
/// filter returns (ByteProcessor.reset(ImageProcessor) lines 192-204).
fn mask_allows(mask: Option<&RoiMask>, x: u32, y: u32) -> bool {
    mask.map_or(true, |mask| mask.contains(x, y))
}

/// `ByteProcessor.setPixels(int, FloatProcessor)`: `value = fPixels[i] + 0.5f`,
/// clamp to 0..255, then Java's truncating `(byte)` cast
/// (ByteProcessor.java 1268-1279).
fn rounded_byte(value: f32) -> u8 {
    let value = (value + 0.5f32).clamp(0.0, 255.0);
    value as u8
}

/// `ShortProcessor.setPixels(int, FloatProcessor)` (ShortProcessor.java
/// 1241-1252): identical `+0.5f` rounding with a 0..65535 clamp.
fn rounded_short(value: f32) -> u16 {
    let value = (value + 0.5f32).clamp(0.0, 65_535.0);
    value as u16
}

/// Restore the pixels that ImageJ puts back after the blur:
///
/// * `resetOutOfRoi` copies the rows above and below the ROI rectangle within
///   `ceil(5*sigmaY)` of it, for the ROI's columns only
///   (GaussianBlur.java 583-597).
/// * `PlugInFilterRunner`'s masking step restores pixels outside an irregular
///   mask inside the ROI rectangle (PlugInFilterRunner.java 268-272).
///
/// Pixels that `convolveLine` writes outside the ROI rectangle because its
/// first loop runs to `min(kRadius, length)` rather than stopping at
/// `writeTo` are *not* restored, which is why the blurred values survive
/// there.
fn restore_outside_roi<T: Copy>(
    dst: &mut [T],
    original: &[T],
    width: u32,
    height: u32,
    region: &FilterRegion,
    sigma: f64,
) {
    let has_roi = region.height != height && sigma > 0.0;
    if has_roi {
        let radius = (5.0 * sigma).ceil() as i32;
        let roi_y = region.y as i32;
        let y0 = (roi_y - radius).max(0) as u32;
        for y in y0..region.y {
            for x in region.x..region.x + region.width {
                let i = (y * width + x) as usize;
                dst[i] = original[i];
            }
        }
        let y_end = (roi_y + region.height as i32)
            .saturating_add(radius)
            .clamp(0, height as i32) as u32;
        for y in (region.y + region.height)..y_end {
            for x in region.x..region.x + region.width {
                let i = (y * width + x) as usize;
                dst[i] = original[i];
            }
        }
    }
    if let Some(mask) = &region.mask {
        for y in region.y..region.y + region.height {
            for x in region.x..region.x + region.width {
                if !mask.contains(x, y) {
                    let i = (y * width + x) as usize;
                    dst[i] = original[i];
                }
            }
        }
    }
}

/// `GaussianBlur.blurFloat` (GaussianBlur.java 205-212). The x pass processes
/// `ceil(5*sigmaY)` extra lines above and below the ROI because the y pass
/// reads filtered neighbours; `resetOutOfRoi` later restores those lines
/// (GaussianBlur.java 190-192, 583-597). The caller commits the whole buffer
/// and then applies that restore, because `convolveLine` can also write
/// outside the ROI rectangle (see [`restore_outside_roi`]).
fn blur_gaussian_float(
    pixels: &mut [f32],
    width: u32,
    height: u32,
    region: &FilterRegion,
    sigma: f64,
    accuracy: f64,
) {
    // blurFloat checks `sigmaX > 0` and `sigmaY > 0`, so zero and negative
    // sigma (and NaN) leave the pixels untouched.
    if sigma.is_nan() || sigma <= 0.0 {
        return;
    }
    let extra_lines = (5.0 * sigma).ceil() as i32;
    blur_one_direction(
        pixels,
        width,
        height,
        region,
        sigma,
        accuracy,
        true,
        extra_lines,
    );
    blur_one_direction(pixels, width, height, region, sigma, accuracy, false, 0);
}

/// Port of `GaussianBlur.blur1Direction` (GaussianBlur.java 222-319) without
/// the progress bar and worker threads. Each line is independent, so running
/// the lines sequentially produces the same pixels as ImageJ's thread pool.
#[allow(clippy::too_many_arguments)]
fn blur_one_direction(
    pixels: &mut [f32],
    width: u32,
    height: u32,
    region: &FilterRegion,
    sigma: f64,
    accuracy: f64,
    x_direction: bool,
    extra_lines: i32,
) {
    const UPSCALE_K_RADIUS: i32 = 2;
    const MIN_DOWNSCALED_SIGMA: f64 = 4.0;

    let width_i = width as i32;
    let height_i = height as i32;
    let length = if x_direction { width_i } else { height_i };
    let point_inc = if x_direction { 1 } else { width_i };
    let line_inc = if x_direction { width_i } else { 1 };
    let roi_x = region.x as i32;
    let roi_y = region.y as i32;
    let roi_w = region.width as i32;
    let roi_h = region.height as i32;

    let line_from = ((if x_direction { roi_y } else { roi_x }) - extra_lines).max(0);
    let line_to = ((if x_direction {
        roi_y + roi_h
    } else {
        roi_x + roi_w
    })
    .saturating_add(extra_lines))
    .min(if x_direction { height_i } else { width_i });
    let write_from = if x_direction { roi_x } else { roi_y };
    let write_to = if x_direction {
        roi_x + roi_w
    } else {
        roi_y + roi_h
    };

    // Large sigma: downscale the line, convolve, then upscale
    // (GaussianBlur.java 245-271).
    let do_downscaling = sigma > 2.0 * MIN_DOWNSCALED_SIGMA + 0.5;
    let reduce_by = if do_downscaling {
        ((sigma / MIN_DOWNSCALED_SIGMA).floor() as i32).min(length)
    } else {
        1
    };
    // Downscaling adds sigma 1/sqrt(3) and upscaling adds 1/2 (in downscaled
    // pixels); subtract those variances so the total stays sigma^2.
    let sigma_gauss = if do_downscaling {
        (sigma * sigma / (reduce_by * reduce_by) as f64 - 1.0 / 3.0 - 1.0 / 4.0).sqrt()
    } else {
        sigma
    };
    let max_length = if do_downscaling {
        (length + reduce_by - 1) / reduce_by + 2 * (UPSCALE_K_RADIUS + 1)
    } else {
        length
    };
    let gauss_kernel = make_gaussian_kernel(sigma_gauss, accuracy, max_length);
    let k_radius = (gauss_kernel[0].len() as i32).saturating_mul(reduce_by);
    let read_from = (write_from - k_radius).max(0);
    let read_to = write_to.saturating_add(k_radius).min(length);
    let new_length = if do_downscaling {
        (read_to - read_from + reduce_by - 1) / reduce_by + 2 * (UPSCALE_K_RADIUS + 1)
    } else {
        length
    };
    // Input point corresponding to cache index 0; may be negative.
    let unscaled0 = read_from - (UPSCALE_K_RADIUS + 1) * reduce_by;

    let downscale_kernel = if do_downscaling {
        Some(make_downscale_kernel(reduce_by))
    } else {
        None
    };
    let upscale_kernel = if do_downscaling {
        Some(make_upscale_kernel(reduce_by))
    } else {
        None
    };

    let mut cache1 = vec![0f32; new_length as usize];
    let mut cache2 = if do_downscaling {
        Some(vec![0f32; new_length as usize])
    } else {
        None
    };

    for line in line_from..line_to {
        let pixel0 = line * line_inc;
        if let (Some(downscale), Some(upscale), Some(cache2)) = (
            downscale_kernel.as_ref(),
            upscale_kernel.as_ref(),
            cache2.as_mut(),
        ) {
            downscale_line(
                pixels,
                &mut cache1,
                downscale,
                reduce_by,
                pixel0,
                unscaled0,
                length,
                point_inc,
                new_length,
            );
            convolve_line(
                &cache1,
                cache2,
                &gauss_kernel,
                0,
                new_length,
                1,
                new_length - 1,
                0,
                1,
            );
            upscale_line(
                cache2, pixels, upscale, reduce_by, pixel0, unscaled0, write_from, write_to,
                point_inc,
            );
        } else {
            let mut p = pixel0 + read_from * point_inc;
            for i in read_from..read_to {
                cache1[i as usize] = pixels[p as usize];
                p += point_inc;
            }
            convolve_line(
                &cache1,
                pixels,
                &gauss_kernel,
                read_from,
                read_to,
                write_from,
                write_to,
                pixel0,
                point_inc,
            );
        }
    }
}

/// `GaussianBlur.makeGaussianKernel` (GaussianBlur.java 536-574): a one-sided
/// normalized kernel plus the running sum used to replicate out-of-line edge
/// pixels. The float casts and double-precision normalisation must stay in
/// this exact order for bit-exact parity.
fn make_gaussian_kernel(sigma: f64, accuracy: f64, mut max_radius: i32) -> [Vec<f32>; 2] {
    // A too-small maxRadius would result in an inaccurate sum.
    if max_radius < 50 {
        max_radius = 50;
    }
    let mut k_radius = {
        let radius = (sigma * (-2.0 * accuracy.ln()).sqrt()).ceil();
        // `(int)Math.ceil(...)+1` overflows for extreme sigma, which ImageJ
        // aborts on; clamp so the pure port cannot panic. The public
        // gaussian_blur rejects sigma above 5e8 first.
        if !radius.is_finite() || radius >= i32::MAX as f64 {
            max_radius
        } else {
            (radius as i32).saturating_add(1)
        }
    };
    if k_radius > max_radius {
        k_radius = max_radius;
    }
    let mut kernel = vec![0f32; k_radius as usize];
    for i in 0..k_radius {
        let x = i as f64;
        kernel[i as usize] = (-0.5 * x * x / sigma / sigma).exp() as f32;
    }
    if k_radius < max_radius && k_radius > 3 {
        // Replace the near-edge values with a quadratic that reaches zero at
        // the first out-of-kernel pixel (GaussianBlur.java 543-556).
        let mut sqrt_slope = f64::MAX;
        let mut r = k_radius;
        while r > k_radius / 2 {
            r -= 1;
            let a = (kernel[r as usize] as f64).sqrt() / (k_radius - r) as f64;
            if a < sqrt_slope {
                sqrt_slope = a;
            } else {
                break;
            }
        }
        for r1 in (r + 2)..k_radius {
            let d = (k_radius - r1) as f64;
            kernel[r1 as usize] = (d * d * sqrt_slope * sqrt_slope) as f32;
        }
    }
    let sum = if k_radius < max_radius {
        let mut sum = kernel[0] as f64;
        for i in 1..k_radius {
            // Java computes `2*kernel[0][i]` in float before adding to double.
            sum += (2.0f32 * kernel[i as usize]) as f64;
        }
        sum
    } else {
        sigma * (2.0 * std::f64::consts::PI).sqrt()
    };

    let mut rsum = 0.5 + 0.5 * (kernel[0] as f64) / sum;
    let mut kernel_sum = vec![0f32; k_radius as usize];
    for i in 0..k_radius {
        let v = (kernel[i as usize] as f64) / sum;
        kernel[i as usize] = v as f32;
        rsum -= v;
        kernel_sum[i as usize] = rsum as f32;
    }
    [kernel, kernel_sum]
}

/// `GaussianBlur.makeDownscaleKernel` (GaussianBlur.java 383-399).
fn make_downscale_kernel(unit_length: i32) -> Vec<f32> {
    let mid = unit_length * 3 / 2;
    let mut kernel = vec![0f32; (3 * unit_length) as usize];
    let mut i = 0;
    while i <= unit_length / 2 {
        let x = i as f64 / unit_length as f64;
        let v = ((0.75 - x * x) / unit_length as f64) as f32;
        kernel[(mid - i) as usize] = v;
        kernel[(mid + i) as usize] = v;
        i += 1;
    }
    let mut i = unit_length / 2 + 1;
    while i < (unit_length * 3 + 1) / 2 {
        let x = i as f64 / unit_length as f64;
        let v = ((0.125 + 0.5 * (x - 1.0) * (x - 2.0)) / unit_length as f64) as f32;
        kernel[(mid - i) as usize] = v;
        kernel[(mid + i) as usize] = v;
        i += 1;
    }
    kernel
}

/// `GaussianBlur.makeUpscaleKernel` (GaussianBlur.java 425-442).
fn make_upscale_kernel(unit_length: i32) -> Vec<f32> {
    let mut kernel = vec![0f32; (4 * unit_length) as usize];
    let mid = 2 * unit_length;
    kernel[0] = 0.0;
    for i in 0..unit_length {
        let x = i as f64 / unit_length as f64;
        let v = (2.0 / 3.0 - x * x * (1.0 - 0.5 * x)) as f32;
        kernel[(mid + i) as usize] = v;
        kernel[(mid - i) as usize] = v;
    }
    for i in unit_length..2 * unit_length {
        let x = i as f64 / unit_length as f64;
        let v = ((2.0 - x) * (2.0 - x) * (2.0 - x) / 6.0) as f32;
        kernel[(mid + i) as usize] = v;
        kernel[(mid - i) as usize] = v;
    }
    kernel
}

/// `GaussianBlur.downscaleLine` (GaussianBlur.java 337-353). The output cache
/// is written by three staggered accumulations per input run, exactly as in
/// ImageJ, including the edge-pixel replication.
#[allow(clippy::too_many_arguments)]
fn downscale_line(
    pixels: &[f32],
    cache: &mut [f32],
    kernel: &[f32],
    reduce_by: i32,
    pixel0: i32,
    unscaled0: i32,
    length: i32,
    point_inc: i32,
    new_length: i32,
) {
    let mut p = pixel0 + point_inc * (unscaled0 - reduce_by * 3 / 2);
    let p_last = pixel0 + point_inc * (length - 1);
    let mut xout = -1;
    while xout <= new_length {
        let mut sum0 = 0f32;
        let mut sum1 = 0f32;
        let mut sum2 = 0f32;
        for x in 0..reduce_by {
            let index = if p < pixel0 {
                pixel0
            } else if p > p_last {
                p_last
            } else {
                p
            };
            let v = pixels[index as usize];
            sum0 += v * kernel[(x + 2 * reduce_by) as usize];
            sum1 += v * kernel[(x + reduce_by) as usize];
            sum2 += v * kernel[x as usize];
            p += point_inc;
        }
        if xout > 0 {
            cache[(xout - 1) as usize] += sum0;
        }
        if xout >= 0 && xout < new_length {
            cache[xout as usize] += sum1;
        }
        if xout + 1 < new_length {
            cache[(xout + 1) as usize] = sum2;
        }
        xout += 1;
    }
}

/// `GaussianBlur.upscaleLine` (GaussianBlur.java 404-415).
#[allow(clippy::too_many_arguments)]
fn upscale_line(
    cache: &[f32],
    pixels: &mut [f32],
    kernel: &[f32],
    reduce_by: i32,
    pixel0: i32,
    unscaled0: i32,
    write_from: i32,
    write_to: i32,
    point_inc: i32,
) {
    let mut p = pixel0 + point_inc * write_from;
    let mut xout = write_from;
    while xout < write_to {
        let numerator = xout - unscaled0 + reduce_by - 1;
        let xin = numerator / reduce_by;
        let x = reduce_by - 1 - numerator % reduce_by;
        pixels[p as usize] = cache[(xin - 2) as usize] * kernel[x as usize]
            + cache[(xin - 1) as usize] * kernel[(x + reduce_by) as usize]
            + cache[xin as usize] * kernel[(x + 2 * reduce_by) as usize]
            + cache[(xin + 1) as usize] * kernel[(x + 3 * reduce_by) as usize];
        p += point_inc;
        xout += 1;
    }
}

/// `GaussianBlur.convolveLine` (GaussianBlur.java 466-509). Every product and
/// accumulation stays in `f32` in this order; the running `kernel[1]` sum
/// folds the replicated edge pixels into the result.
#[allow(clippy::too_many_arguments)]
fn convolve_line(
    input: &[f32],
    pixels: &mut [f32],
    kernel: &[Vec<f32>; 2],
    _read_from: i32,
    _read_to: i32,
    write_from: i32,
    write_to: i32,
    point0: i32,
    point_inc: i32,
) {
    let length = input.len() as i32;
    // Out-of-edge pixels are replaced by the nearest edge pixels.
    let first = input[0];
    let last = input[(length - 1) as usize];
    let kern = &kernel[0];
    let kern0 = kern[0];
    let kern_sum = &kernel[1];
    let k_radius = kern.len() as i32;
    let first_part = k_radius.min(length);
    let mut p = point0 + write_from * point_inc;
    let mut i = write_from;

    // While the sum would include pixels < 0.
    while i < first_part {
        let mut result = input[i as usize] * kern0;
        result += kern_sum[i as usize] * first;
        if i + k_radius > length {
            result += kern_sum[(length - i - 1) as usize] * last;
        }
        for k in 1..k_radius {
            let mut v = 0f32;
            if i - k >= 0 {
                v += input[(i - k) as usize];
            }
            if i + k < length {
                v += input[(i + k) as usize];
            }
            result += kern[k as usize] * v;
        }
        pixels[p as usize] = result;
        i += 1;
        p += point_inc;
    }

    // Only pixels within the line are addressed (the easy case).
    let i_end_inside = (length - k_radius).min(write_to);
    while i < i_end_inside {
        let mut result = input[i as usize] * kern0;
        for k in 1..k_radius {
            result += kern[k as usize] * (input[(i - k) as usize] + input[(i + k) as usize]);
        }
        pixels[p as usize] = result;
        i += 1;
        p += point_inc;
    }

    // While the sum would include pixels >= length.
    while i < write_to {
        let mut result = input[i as usize] * kern0;
        if i < k_radius {
            result += kern_sum[i as usize] * first;
        }
        if i + k_radius >= length {
            result += kern_sum[(length - i - 1) as usize] * last;
        }
        for k in 1..k_radius {
            let mut v = 0f32;
            if i - k >= 0 {
                v += input[(i - k) as usize];
            }
            if i + k < length {
                v += input[(i + k) as usize];
            }
            result += kern[k as usize] * v;
        }
        pixels[p as usize] = result;
        i += 1;
        p += point_inc;
    }
}

// ---------------------------------------------------------------------------
// Auto-thresholding (Unit 5): `AutoThresholder.Otsu` and `AutoThresholder.IsoData`.
// ---------------------------------------------------------------------------

/// The subset of `AutoThresholder.Method` ported by this increment
/// (original_imagej/ij/process/AutoThresholder.java lines 11-29). Every other
/// method name is rejected by `parse` instead of silently falling back to a
/// different algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AutoThresholdMethod {
    IsoData,
    Otsu,
}

impl AutoThresholdMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::IsoData => "IsoData",
            Self::Otsu => "Otsu",
        }
    }

    /// Parses the `AutoThresholder.Method` name. `Method.valueOf` is case
    /// sensitive (AutoThresholder.java lines 90-97). String options such as
    /// "dark"/"16" are not accepted here; the Session command carries the
    /// dark-background and 16-bit histogram flags explicitly, and unsupported
    /// options must be rejected rather than silently dropped.
    pub fn parse(method: &str) -> Result<Self, CoreError> {
        match method {
            "IsoData" => Ok(Self::IsoData),
            "Otsu" => Ok(Self::Otsu),
            other => Err(CoreError::UnsupportedThresholdMethod(other.to_string())),
        }
    }
}

impl ImageDocument {
    /// Port of `ImageProcessor.setAutoThreshold(String)`'s default path for
    /// 8-bit and 16-bit grayscale images (original_imagej/ij/process/
    /// ImageProcessor.java lines 594-675). This is the method string without
    /// the "16" keyword, i.e. ImageJ's ThresholdAdjuster with the "16-bit
    /// histogram" checkbox off.
    ///
    /// The histogram is the ROI (and ROI-mask) histogram that the statistics
    /// class reads. Only the threshold state changes; pixels are never
    /// rewritten. 16-bit default mode additionally resets the display range
    /// to the image min/max, exactly like `resetMinAndMax()`.
    pub fn set_auto_threshold(
        &mut self,
        method: AutoThresholdMethod,
        dark_background: bool,
    ) -> Result<crate::ThresholdLevels, CoreError> {
        self.set_auto_threshold_with_options(method, dark_background, false)
    }

    /// Port of `ImageProcessor.setAutoThreshold(String)` including the
    /// `histogram16` option. `histogram_16` corresponds to the "16" keyword
    /// in the method string (ImageProcessor.java line 606), which ImageJ
    /// only exposes for 16-bit images (ThresholdAdjuster.java line 503).
    /// Requesting it for 8-bit data is rejected instead of silently changing
    /// the bilevel `addOne` rule.
    pub fn set_auto_threshold_with_options(
        &mut self,
        method: AutoThresholdMethod,
        dark_background: bool,
        histogram_16: bool,
    ) -> Result<crate::ThresholdLevels, CoreError> {
        // Threshold state lives on each ImageProcessor, so a stack needs one
        // level pair per slice. This increment only models a single value.
        if self.slices.len() > 1 {
            return Err(CoreError::UnsupportedThresholdStack {
                stack_size: self.slices.len(),
            });
        }
        let (levels, reset_display_range) = match self.plane() {
            PixelPlane::Gray8(pixels) => {
                if histogram_16 {
                    return Err(CoreError::UnsupportedThresholdOption {
                        option: "16-bit histogram",
                        pixel_type: "8-bit grayscale",
                    });
                }
                (
                    self.auto_threshold_gray8(pixels, method, dark_background)?,
                    None,
                )
            }
            PixelPlane::Gray16(pixels) => {
                if histogram_16 {
                    (
                        self.auto_threshold_gray16_histogram(pixels, method, dark_background)?,
                        None,
                    )
                } else {
                    let (levels, range) =
                        self.auto_threshold_gray16_scaled(pixels, method, dark_background)?;
                    (levels, Some(range))
                }
            }
            PixelPlane::Color256 { .. } => {
                return Err(CoreError::UnsupportedThreshold {
                    pixel_type: "8-bit color (indexed)",
                });
            }
            PixelPlane::Gray32(_) => {
                return Err(CoreError::UnsupportedThreshold {
                    pixel_type: "32-bit float",
                });
            }
            PixelPlane::ColorRgb(_) => {
                return Err(CoreError::UnsupportedThreshold { pixel_type: "RGB" });
            }
        };
        if let Some(range) = reset_display_range {
            self.display_range = range;
        }
        self.threshold = Some(levels);
        Ok(levels)
    }

    /// 8-bit `ImageProcessor.setAutoThreshold` path. `addOne` is true because
    /// the method string never carries the "16" keyword here
    /// (ImageProcessor.java line 606), and a Gray8 LUT is non-inverting.
    fn auto_threshold_gray8(
        &self,
        pixels: &[u8],
        method: AutoThresholdMethod,
        dark_background: bool,
    ) -> Result<crate::ThresholdLevels, CoreError> {
        let histogram = self.threshold_histogram_gray8(pixels);
        let threshold = auto_threshold_histogram(method, &histogram)?;
        let (mut lower, mut upper) = if dark_background {
            (threshold as f64 + 1.0, 255.0)
        } else {
            (0.0, threshold as f64)
        };
        // ByteProcessor.setThreshold lines 480-486 clamps the stored levels.
        if lower < 0.0 {
            lower = 0.0;
        }
        if upper > 255.0 {
            upper = 255.0;
        }
        Ok(crate::ThresholdLevels { lower, upper })
    }

    /// 16-bit default path: `resetMinAndMax()`, `convertToByte(true)`, then
    /// `scaleAndSetThreshold()` (ImageProcessor.java lines 636-675,
    /// TypeConverter.java lines 49-69).
    fn auto_threshold_gray16_scaled(
        &self,
        pixels: &[u16],
        method: AutoThresholdMethod,
        dark_background: bool,
    ) -> Result<(crate::ThresholdLevels, crate::DisplayRange), CoreError> {
        // ShortProcessor.findMinAndMax scans the whole image, not the ROI
        // (ShortProcessor.java lines 70-87).
        let (min, max) = gray16_min_max(pixels);
        let min_f = min as f64;
        let max_f = max as f64;
        // TypeConverter.convertShortToByte: 256.0/(max-min+1), then
        // (int)((pixel-min)*scale+0.5) clamped to 255.
        let scale = 256.0 / ((max as f64 - min as f64) + 1.0);
        let histogram = self.threshold_histogram_gray16_as_byte(pixels, min, scale);
        let threshold = auto_threshold_histogram(method, &histogram)?;
        let (mut lower, mut upper) = if dark_background {
            (threshold as f64 + 1.0, 255.0)
        } else {
            (0.0, threshold as f64)
        };
        // ImageProcessor.scaleAndSetThreshold lines 752-778.
        if max > min {
            if lower != 0.0 {
                lower = min_f + (lower / 255.0) * (max_f - min_f);
            }
            if upper == 255.0 {
                upper = 65535.0;
            } else {
                upper = min_f + (upper / 255.0) * (max_f - min_f);
            }
        } else {
            lower = min_f;
            upper = min_f;
        }
        // ShortProcessor.setThreshold lines 1130-1166 clamps and rounds.
        if lower < 0.0 {
            lower = 0.0;
        }
        if upper > 65535.0 {
            upper = 65535.0;
        }
        Ok((
            crate::ThresholdLevels {
                lower: java_round(lower),
                upper: java_round(upper),
            },
            crate::DisplayRange {
                min: min_f,
                max: max_f,
            },
        ))
    }

    /// 16-bit histogram path: ShortStatistics uses `getHistogram2()` for ROI
    /// rectangles under 250,000 pixels and `getHistogram()` (65536 bins)
    /// otherwise (ShortStatistics.java lines 35-40). `setThreshold` receives
    /// the raw levels, so no 0-255 rescaling happens.
    fn auto_threshold_gray16_histogram(
        &self,
        pixels: &[u16],
        method: AutoThresholdMethod,
        dark_background: bool,
    ) -> Result<crate::ThresholdLevels, CoreError> {
        let histogram = self.threshold_histogram_gray16(pixels);
        let threshold = auto_threshold_histogram_16bit(method, &histogram, false)?;
        // ImageProcessor.java lines 651-654: only histograms longer than 256
        // bins make tmax 65535.
        let tmax = if histogram.len() > 256 {
            65535.0
        } else {
            255.0
        };
        let (mut lower, mut upper) = if dark_background {
            (threshold as f64, tmax)
        } else {
            (0.0, threshold as f64)
        };
        // ShortProcessor.setThreshold lines 1130-1166 clamps and rounds.
        if lower < 0.0 {
            lower = 0.0;
        }
        if upper > 65535.0 {
            upper = 65535.0;
        }
        Ok(crate::ThresholdLevels {
            lower: java_round(lower),
            upper: java_round(upper),
        })
    }

    /// `ByteProcessor.getHistogram()` over the processor's ROI and mask
    /// (ByteProcessor.java lines 1136-1168). An empty or out-of-image ROI
    /// leaves a 0x0 processor ROI, whose histogram is all zeros.
    fn threshold_histogram_gray8(&self, pixels: &[u8]) -> Vec<i64> {
        let mut histogram = vec![0i64; 256];
        let region = self.threshold_region();
        if region.width == 0 || region.height == 0 {
            return histogram;
        }
        for y in region.y..(region.y + region.height) {
            let base = (y * self.width + region.x) as usize;
            for offset in 0..region.width as usize {
                let x = region.x + offset as u32;
                if region.mask.as_ref().is_some_and(|m| !m.contains(x, y)) {
                    continue;
                }
                histogram[pixels[base + offset] as usize] += 1;
            }
        }
        histogram
    }

    /// The 256-bin histogram of `convertToByte(true)` output over the ROI
    /// (TypeConverter.java lines 49-69).
    fn threshold_histogram_gray16_as_byte(&self, pixels: &[u16], min: u16, scale: f64) -> Vec<i64> {
        let mut histogram = vec![0i64; 256];
        let region = self.threshold_region();
        if region.width == 0 || region.height == 0 {
            return histogram;
        }
        for y in region.y..(region.y + region.height) {
            let base = (y * self.width + region.x) as usize;
            for offset in 0..region.width as usize {
                let x = region.x + offset as u32;
                if region.mask.as_ref().is_some_and(|m| !m.contains(x, y)) {
                    continue;
                }
                let value = (pixels[base + offset] as i32 - min as i32) as f64;
                let mut scaled = (value * scale + 0.5) as i32;
                if scaled > 255 {
                    scaled = 255;
                }
                histogram[scaled as usize] += 1;
            }
        }
        histogram
    }

    /// `ShortProcessor.getHistogram2()` or `getHistogram()`, whichever
    /// ShortStatistics selects for the current ROI rectangle.
    fn threshold_histogram_gray16(&self, pixels: &[u16]) -> Vec<i64> {
        let region = self.threshold_region();
        let small = (region.width as u64) * (region.height as u64) < 250_000;
        if !small {
            let mut histogram = vec![0i64; 65536];
            self.count_gray16_histogram(pixels, &region, &mut histogram);
            return histogram;
        }
        // getHistogram2() sizes the histogram from the largest value in the
        // ROI rectangle, including pixels excluded by the ROI mask
        // (ShortProcessor.java lines 1069-1127).
        let mut max_value = 0u16;
        if region.width > 0 && region.height > 0 {
            for y in region.y..(region.y + region.height) {
                let base = (y * self.width + region.x) as usize;
                for offset in 0..region.width as usize {
                    max_value = max_value.max(pixels[base + offset]);
                }
            }
        }
        let mut histogram = vec![0i64; (max_value as usize + 1).max(256)];
        self.count_gray16_histogram(pixels, &region, &mut histogram);
        histogram
    }

    fn count_gray16_histogram(&self, pixels: &[u16], region: &FilterRegion, histogram: &mut [i64]) {
        if region.width == 0 || region.height == 0 {
            return;
        }
        for y in region.y..(region.y + region.height) {
            let base = (y * self.width + region.x) as usize;
            for offset in 0..region.width as usize {
                let x = region.x + offset as u32;
                if region.mask.as_ref().is_some_and(|m| !m.contains(x, y)) {
                    continue;
                }
                histogram[pixels[base + offset] as usize] += 1;
            }
        }
    }

    /// The rectangle and mask that `ImageProcessor.setRoi(Roi)` installs
    /// (ImageProcessor.java lines 900-913). Line selections have no mask, but
    /// their integer bounds enclose the drawn 1-pixel line (Roi.setIntBounds
    /// lines 538-551), so `makeLine(1,2,9,7)` scans 9x6 pixels, not 8x5.
    fn threshold_region(&self) -> FilterRegion {
        let empty = FilterRegion {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
            mask: None,
        };
        let (x, y, width, height) = match &self.roi {
            None => (0, 0, self.width, self.height),
            Some(crate::Roi::Line(line)) => {
                let rect = crate::RectRoi {
                    x: line.x1.min(line.x2),
                    y: line.y1.min(line.y2),
                    width: (line.x2 - line.x1).abs() + 1,
                    height: (line.y2 - line.y1).abs() + 1,
                };
                match rect.clamp_to(self.width, self.height) {
                    Ok(bounds) => bounds,
                    Err(_) => return empty,
                }
            }
            Some(roi) => match roi.clipped_bounds(self.width, self.height) {
                Ok(bounds) => bounds,
                Err(_) => return empty,
            },
        };
        let mask = self.roi_mask().ok().flatten();
        FilterRegion {
            x,
            y,
            width,
            height,
            mask,
        }
    }
}

/// Port of `AutoThresholder.getThreshold(Method, int[])`
/// (original_imagej/ij/process/AutoThresholder.java lines 43-88) for the
/// 256-bin histograms used by 8-bit images. `setAutoThreshold` requests the
/// subtract-one bilevel shortcut for method strings without "16"
/// (AutoThresholder.java lines 166-187, ImageProcessor.java line 606).
pub fn auto_threshold_histogram(
    method: AutoThresholdMethod,
    histogram: &[i64],
) -> Result<i64, CoreError> {
    if histogram.len() != 256 {
        return Err(CoreError::UnsupportedThresholdHistogram {
            bins: histogram.len(),
        });
    }
    auto_threshold_histogram_bins(method, histogram, true)
}

/// Port of `AutoThresholder.getThreshold(Method, int[])` for 16-bit
/// histograms. ImageJ's 16-bit histogram mode passes `bilevelSubractOne`
/// false (method string contains "16"); histograms longer than 256 bins are
/// trimmed to their nonzero range and `minbin` is added back
/// (AutoThresholder.java lines 49-62 and 85).
pub fn auto_threshold_histogram_16bit(
    method: AutoThresholdMethod,
    histogram: &[i64],
    subtract_one: bool,
) -> Result<i64, CoreError> {
    if histogram.len() < 256 || histogram.len() > 65536 {
        return Err(CoreError::UnsupportedThresholdHistogram16 {
            bins: histogram.len(),
        });
    }
    auto_threshold_histogram_bins(method, histogram, subtract_one)
}

fn auto_threshold_histogram_bins(
    method: AutoThresholdMethod,
    histogram: &[i64],
    subtract_one: bool,
) -> Result<i64, CoreError> {
    // The bilevel shortcut runs before the trimming (lines 46-48). `-1`
    // means "not a bilevel histogram, run the method".
    let bilevel = bilevel_threshold(histogram, subtract_one);
    if bilevel >= 0 {
        return Ok(bilevel);
    }
    if histogram.len() > 256 {
        let mut minbin = -1i64;
        let mut maxbin = -1i64;
        for (i, &count) in histogram.iter().enumerate() {
            if count > 0 {
                maxbin = i as i64;
            }
        }
        for (i, &count) in histogram.iter().enumerate().rev() {
            if count > 0 {
                minbin = i as i64;
            }
        }
        if minbin < 0 || maxbin < 0 {
            // ImageJ evaluates histogram[-1] here and throws
            // ArrayIndexOutOfBoundsException (AutoThresholder.java lines
            // 56-62). Reject the impossible histogram instead of guessing.
            return Err(CoreError::EmptyThresholdHistogram {
                bins: histogram.len(),
            });
        }
        let data = &histogram[minbin as usize..=maxbin as usize];
        return Ok(method_threshold(method, data) + minbin);
    }
    Ok(method_threshold(method, histogram))
}

/// Runs the method and maps its failure value (`-1`) to 0
/// (AutoThresholder.java lines 83-84).
fn method_threshold(method: AutoThresholdMethod, data: &[i64]) -> i64 {
    let mut threshold = match method {
        AutoThresholdMethod::Otsu => otsu(data),
        AutoThresholdMethod::IsoData => iso_data(data),
    };
    if threshold == -1 {
        threshold = 0;
    }
    threshold
}

/// `AutoThresholder.bilevel` (AutoThresholder.java lines 166-187).
/// `subtract_one` is `bilevelSubractOne`: true for method strings without
/// "16", false for ImageJ's 16-bit histogram mode.
fn bilevel_threshold(data: &[i64], subtract_one: bool) -> i64 {
    let mut first_non_zero = -1i64;
    let mut second_non_zero = -1i64;
    let mut non_zero_count = 0;
    for (i, &count) in data.iter().enumerate() {
        if count > 0 {
            non_zero_count += 1;
            if non_zero_count > 2 {
                return -1;
            }
            if first_non_zero == -1 {
                first_non_zero = i as i64;
            } else {
                second_non_zero = i as i64;
            }
        }
    }
    let subtract = if subtract_one { 1 } else { 0 };
    if non_zero_count == 1 {
        first_non_zero - subtract
    } else if non_zero_count == 2 {
        second_non_zero - subtract
    } else {
        -1
    }
}

/// Java `Math.round(double)` is `floor(x+0.5)`, unlike Rust's
/// round-half-away-from-zero (java.lang.Math.round).
fn java_round(value: f64) -> f64 {
    (value + 0.5).floor()
}

/// `ShortProcessor.findMinAndMax` (ShortProcessor.java lines 70-87) scans
/// every pixel, not just the ROI.
fn gray16_min_max(pixels: &[u16]) -> (u16, u16) {
    let mut min = u16::MAX;
    let mut max = 0u16;
    for &value in pixels {
        min = min.min(value);
        max = max.max(value);
    }
    (min, max)
}

/// `AutoThresholder.Otsu` (AutoThresholder.java lines 850-904). `max_bcv < bcv`
/// is strict, so the lowest threshold wins ties. An all-zero histogram leaves
/// `threshold` at `Integer.MIN_VALUE`, which is what ImageJ stores too.
fn otsu(data: &[i64]) -> i64 {
    let mut num_pixels = 0i64;
    for &count in data {
        num_pixels += count;
    }
    // 1.0/0.0 is Infinity in Java and Rust; Infinity*0 is NaN in both.
    let term = 1.0 / num_pixels as f64;
    let histo: Vec<f64> = data.iter().map(|&count| term * count as f64).collect();
    let mut cnh = vec![0.0f64; data.len()];
    cnh[0] = histo[0];
    for ih in 1..data.len() {
        cnh[ih] = cnh[ih - 1] + histo[ih];
    }
    let mut mean = vec![0.0f64; data.len()];
    for ih in 1..data.len() {
        mean[ih] = mean[ih - 1] + ih as f64 * histo[ih];
    }
    let total_mean = mean[data.len() - 1];

    let mut threshold = i32::MIN as i64;
    let mut max_bcv = 0.0f64;
    for ih in 0..data.len() {
        let mut bcv = total_mean * cnh[ih] - mean[ih];
        bcv *= bcv / (cnh[ih] * (1.0 - cnh[ih]));
        if max_bcv < bcv {
            max_bcv = bcv;
            threshold = ih as i64;
        }
    }
    threshold
}

/// `AutoThresholder.IsoData` (AutoThresholder.java lines 434-494). `l`/`h` and
/// the sums are Java `int`s; truncating division is the same in Rust. The
/// method aborts with `-1` when no fixed point is reached, which
/// `auto_threshold_histogram` maps to 0.
fn iso_data(data: &[i64]) -> i64 {
    let mut g = 0i64;
    for i in 1..data.len() {
        if data[i] > 0 {
            g = i as i64 + 1;
            break;
        }
    }
    loop {
        let mut l = 0i64;
        let mut totl = 0i64;
        for i in 0..g as usize {
            totl += data[i];
            l += data[i] * i as i64;
        }
        let mut h = 0i64;
        let mut toth = 0i64;
        for i in (g as usize + 1)..data.len() {
            toth += data[i];
            h += data[i] * i as i64;
        }
        if totl > 0 && toth > 0 {
            l /= totl;
            h /= toth;
            // Math.round((l+h)/2.0); both means are non-negative.
            if g == ((l + h) as f64 / 2.0).round() as i64 {
                return g;
            }
        }
        g += 1;
        if g > data.len() as i64 - 2 {
            return -1;
        }
    }
}

#[cfg(test)]
mod threshold_tests {
    use super::*;
    use crate::CoreError;

    #[test]
    fn method_parse_accepts_only_ported_names() {
        assert_eq!(
            AutoThresholdMethod::parse("Otsu").unwrap(),
            AutoThresholdMethod::Otsu
        );
        assert_eq!(
            AutoThresholdMethod::parse("IsoData").unwrap(),
            AutoThresholdMethod::IsoData
        );
        for rejected in ["otsu", "Otsu dark", "Otsu 16", "Huang", ""] {
            assert!(matches!(
                AutoThresholdMethod::parse(rejected),
                Err(CoreError::UnsupportedThresholdMethod(_))
            ));
        }
    }

    #[test]
    fn histogram_must_have_256_bins() {
        let error = auto_threshold_histogram(AutoThresholdMethod::Otsu, &[1i64, 2, 3]).unwrap_err();
        assert!(matches!(
            error,
            CoreError::UnsupportedThresholdHistogram { bins: 3 }
        ));
    }

    #[test]
    fn bilevel_shortcut_subtracts_one() {
        let mut histogram = vec![0i64; 256];
        histogram[7] = 5;
        assert_eq!(
            auto_threshold_histogram(AutoThresholdMethod::IsoData, &histogram).unwrap(),
            6
        );
        histogram[9] = 2;
        assert_eq!(
            auto_threshold_histogram(AutoThresholdMethod::IsoData, &histogram).unwrap(),
            8
        );
        histogram[20] = 1;
        assert_ne!(
            auto_threshold_histogram(AutoThresholdMethod::IsoData, &histogram).unwrap(),
            8
        );
    }

    #[test]
    fn empty_histogram_follows_otsu_and_isodata_edge_behavior() {
        let histogram = vec![0i64; 256];
        assert_eq!(
            auto_threshold_histogram(AutoThresholdMethod::Otsu, &histogram).unwrap(),
            i32::MIN as i64
        );
        // IsoData never converges and returns -1, which becomes 0.
        assert_eq!(
            auto_threshold_histogram(AutoThresholdMethod::IsoData, &histogram).unwrap(),
            0
        );
    }

    #[test]
    fn histogram16_accepts_long_histograms_and_adds_minbin_back() {
        let mut histogram = vec![0i64; 501];
        histogram[300] = 1;
        histogram[400] = 1;
        histogram[500] = 1;
        assert_eq!(
            auto_threshold_histogram_16bit(AutoThresholdMethod::Otsu, &histogram, false).unwrap(),
            300
        );
        assert_eq!(
            auto_threshold_histogram_16bit(AutoThresholdMethod::IsoData, &histogram, false)
                .unwrap(),
            425
        );
    }

    #[test]
    fn histogram16_bilevel_shortcut_does_not_subtract_one() {
        let mut histogram = vec![0i64; 256];
        histogram[7] = 5;
        assert_eq!(
            auto_threshold_histogram_16bit(AutoThresholdMethod::Otsu, &histogram, false).unwrap(),
            7
        );
        assert_eq!(
            auto_threshold_histogram_16bit(AutoThresholdMethod::Otsu, &histogram, true).unwrap(),
            6
        );
    }

    #[test]
    fn histogram16_rejects_an_all_zero_long_histogram() {
        // AutoThresholder.java lines 56-62 index histogram[-1] when a
        // >256-bin histogram has no nonzero bins; ImageJ throws there.
        let histogram = vec![0i64; 65536];
        assert!(matches!(
            auto_threshold_histogram_16bit(AutoThresholdMethod::Otsu, &histogram, false),
            Err(CoreError::EmptyThresholdHistogram { bins: 65536 })
        ));
    }

    #[test]
    fn histogram16_rejects_out_of_range_bin_counts() {
        assert!(matches!(
            auto_threshold_histogram_16bit(AutoThresholdMethod::Otsu, &[0i64; 255], false),
            Err(CoreError::UnsupportedThresholdHistogram16 { bins: 255 })
        ));
        assert!(matches!(
            auto_threshold_histogram_16bit(AutoThresholdMethod::Otsu, &[0i64; 65537], false),
            Err(CoreError::UnsupportedThresholdHistogram16 { bins: 65537 })
        ));
    }

    #[test]
    fn java_round_matches_math_round_for_negative_halves() {
        assert_eq!(java_round(-1.5), -1.0);
        assert_eq!(java_round(-2.5), -2.0);
        assert_eq!(java_round(0.5), 1.0);
    }
}
