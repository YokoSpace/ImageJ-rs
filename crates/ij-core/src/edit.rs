//! Pure `Edit > Clear / Fill / Draw` rasterization: Filler.java 18-175
//! (`clear`, `fill`, `draw`) plus the per-plane fill/draw value rules of
//! ImageProcessor.setColor for the default gray color model
//! (ByteProcessor.java 324-327, ShortProcessor.java 987-999,
//! FloatProcessor.java 1018-1029) and for palette color models
//! (ImageProcessor.java 273: the weighting is gated on the default model).
//!
//! Clear, Fill and Draw all fail without a selection because Filler.setup
//! keeps ROI_REQUIRED and PlugInFilterRunner.checkImagePlus rejects a
//! missing ROI before Filler.run is reached (PlugInFilterRunner.java
//! 409-411).

use crate::{
    CoreError, DisplayRange, ImageDocument, Lut256, PixelPlane, Roi, Wand, WandMode,
    java_round, java_round_to_i32,
};

/// ImageProcessor.getBestIndex (ImageProcessor.java 248-285) for the default
/// gray color model: the index of the minimum squared distance to the
/// (r, g, b) color. Non-gray colors first map to luminance-weighted primaries
/// `3*r*w[0]` etc. with the default 1/3 weights
/// (ColorProcessor.getWeightingFactors); gray colors are used as-is.
pub fn best_gray_index(color: [u8; 3]) -> u8 {
    let (mut r1, mut g1, mut b1) = (
        i64::from(color[0]),
        i64::from(color[1]),
        i64::from(color[2]),
    );
    if !(r1 == g1 && g1 == b1) {
        let w = [1.0f64 / 3.0, 1.0f64 / 3.0, 1.0f64 / 3.0];
        r1 = java_round(3.0 * (r1) as f64 * w[0]);
        g1 = java_round(3.0 * (g1) as f64 * w[1]);
        b1 = java_round(3.0 * (b1) as f64 * w[2]);
    }
    let mut min_distance = i64::MAX;
    let mut best = 0u8;
    for i in 0..256u32 {
        // The default gray LUT maps index i to (i, i, i).
        let gray = i64::from(i as u8);
        let distance =
            (gray - r1) * (gray - r1) + (gray - g1) * (gray - g1) + (gray - b1) * (gray - b1);
        // The first strictly smaller distance wins (ImageProcessor.java
        // 279-285); equal distances keep the lower index.
        if distance < min_distance {
            min_distance = distance;
            best = i as u8;
        }
    }
    best
}

/// ImageProcessor.getBestIndex for a palette (non-default) color model: the
/// raw RGB distance against every LUT entry, with no weighting
/// (ImageProcessor.java 273 gates the weighting on `icm==defaultColorModel`).
pub fn best_lut_index(color: [u8; 3], lut: &Lut256) -> u8 {
    let (r1, g1, b1) = (
        i64::from(color[0]),
        i64::from(color[1]),
        i64::from(color[2]),
    );
    let mut min_distance = i64::MAX;
    let mut best = 0u8;
    for i in 0..256u32 {
        let (r2, g2, b2) = (
            i64::from(lut.r[i as usize]),
            i64::from(lut.g[i as usize]),
            i64::from(lut.b[i as usize]),
        );
        let distance =
            (r2 - r1) * (r2 - r1) + (g2 - g1) * (g2 - g1) + (b2 - b1) * (b2 - b1);
        if distance < min_distance {
            min_distance = distance;
            best = i as u8;
        }
    }
    best
}

/// ShortProcessor.setColor value (ShortProcessor.java 987-999): the gray
/// index scaled by the current display min/max, with the all-equal-pixels
/// special cases. The second element is the display range that the
/// `setMinAndMax(0, 255)` branch installs for an all-zero image.
fn gray16_value(color: [u8; 3], display_range: DisplayRange) -> (i64, Option<DisplayRange>) {
    let best = i64::from(best_gray_index(color));
    let min = display_range.min;
    let max = display_range.max;
    if best > 0 && min == 0.0 && max == 0.0 {
        // setValue(bestIndex); setMinAndMax(0.0, 255.0);
        return (best, Some(DisplayRange { min: 0.0, max: 255.0 }));
    }
    if best == 0 && min > 0.0 && color == [0, 0, 0] {
        // setValue(0.0) for unsigned 16-bit (ShortProcessor.java 993-997).
        return (0, None);
    }
    ((min + (max - min) * (best as f64 / 255.0)) as i64, None)
}

/// FloatProcessor.setColor value (FloatProcessor.java 1018-1029): the gray
/// index scaled by the current display min/max as a float, with the same
/// all-equal-pixels special cases as the 16-bit processor.
fn gray32_value(color: [u8; 3], display_range: DisplayRange) -> (f32, Option<DisplayRange>) {
    let best = i64::from(best_gray_index(color));
    let min = display_range.min;
    let max = display_range.max;
    if best > 0 && min == 0.0 && max == 0.0 {
        return (
            best as f32,
            Some(DisplayRange { min: 0.0, max: 255.0 }),
        );
    }
    if best == 0 && min > 0.0 && color == [0, 0, 0] {
        return (0.0, None);
    }
    (
        (min + (max - min) * (best as f64 / 255.0)) as f32,
        None,
    )
}

/// ColorProcessor.setColor / ColorRgb value: an opaque ARGB int
/// (ColorProcessor.java 113-115; Toolbar colors have full alpha).
fn rgb_value(color: [u8; 3]) -> u32 {
    0xff00_0000
        | (u32::from(color[0]) << 16)
        | (u32::from(color[1]) << 8)
        | u32::from(color[2])
}

/// The typed fill/draw value of one plane.
#[derive(Debug, Clone, Copy)]
enum PlaneFillValue {
    /// ByteProcessor/Color256: the LUT index from getBestIndex.
    Index(u8),
    /// ShortProcessor: the scaled fgColor.
    Int16(i64),
    /// FloatProcessor: the scaled fillColor.
    Float32(f32),
    /// ColorProcessor: the opaque ARGB int.
    Argb(u32),
}

impl PlaneFillValue {
    fn for_plane(
        plane: &PixelPlane,
        color: [u8; 3],
        display_range: DisplayRange,
    ) -> (Self, Option<DisplayRange>) {
        match plane {
            PixelPlane::Gray8(_) => (Self::Index(best_gray_index(color)), None),
            PixelPlane::Color256 { lut, .. } => {
                (Self::Index(best_lut_index(color, lut)), None)
            }
            PixelPlane::Gray16(_) => {
                let (value, range) = gray16_value(color, display_range);
                (Self::Int16(value), range)
            }
            PixelPlane::Gray32(_) => {
                let (value, range) = gray32_value(color, display_range);
                (Self::Float32(value), range)
            }
            PixelPlane::ColorRgb(_) => (Self::Argb(rgb_value(color)), None),
        }
    }

    fn write_at(&self, plane: &mut PixelPlane, index: usize) {
        match (plane, self) {
            (PixelPlane::Gray8(pixels), Self::Index(value))
            | (PixelPlane::Color256 { pixels, .. }, Self::Index(value)) => {
                if let Some(pixel) = pixels.get_mut(index) {
                    *pixel = *value;
                }
            }
            (PixelPlane::Gray16(pixels), Self::Int16(value)) => {
                if let Some(pixel) = pixels.get_mut(index) {
                    // ShortProcessor.process(FILL) clamps to 0..65535 before
                    // the (short) cast (ShortProcessor.java 557-566); the
                    // drawPixel path uses the same fgColor value.
                    *pixel = (*value).clamp(0, 65535) as u16;
                }
            }
            (PixelPlane::Gray32(pixels), Self::Float32(value)) => {
                if let Some(pixel) = pixels.get_mut(index) {
                    *pixel = *value;
                }
            }
            (PixelPlane::ColorRgb(pixels), Self::Argb(value)) => {
                if let Some(pixel) = pixels.get_mut(index) {
                    *pixel = *value;
                }
            }
            _ => {}
        }
    }
}

/// ImageProcessor.drawPixel (ByteProcessor.java 384-387): writes only inside
/// the image; the clip rect spans the whole image for the default settings
/// (ImageProcessor.java 956).
fn draw_pixel_at(
    plane: &mut PixelPlane,
    stride: u32,
    height: u32,
    x: i32,
    y: i32,
    value: &PlaneFillValue,
) {
    if x < 0 || y < 0 || x >= stride as i32 || y >= height as i32 {
        return;
    }
    let index = (y as u32) * stride + x as u32;
    value.write_at(plane, index as usize);
}

/// Port of ImageProcessor.lineTo (ImageProcessor.java 1221-1267) with the
/// default line width 1: the clip rectangle spans one pixel beyond the image
/// edges (`clipXMin-lineWidth/2-1` with clipXMin=0, ImageProcessor.java
/// 1222-1225 and 956), so out-of-image dots are dropped by drawPixel's bounds
/// check. `plot` receives every stepped pixel rounded like
/// `(int)Math.round(x)`; the Java `(int)` casts truncate toward zero.
fn raster_line(
    clip: (i64, i64, i64, i64),
    x1: i64,
    y1: i64,
    x2: i64,
    y2: i64,
    plot: &mut impl FnMut(i32, i32),
) {
    let (clip_x_min, clip_x_max, clip_y_min, clip_y_max) = clip;
    let dx = x2 - x1;
    let dy = y2 - y1;
    let n = dx.unsigned_abs().max(dy.unsigned_abs()) as i64;
    let step = n.max(1) as f64;
    let xinc = if dx != 0 { (dx) as f64 / step } else { 0.0 };
    let yinc = if dy != 0 { (dy) as f64 / step } else { 0.0 };
    let (mut x, mut y) = ((x1) as f64, (y1) as f64);
    let mut i1 = 0i64;
    if dx > 0 {
        i1 = i1.max(java_trunc_to_i32(((clip_x_min) as f64 - x) / xinc));
    } else if dx < 0 {
        i1 = i1.max(java_trunc_to_i32(((clip_x_max) as f64 - x) / xinc));
    } else if x < (clip_x_min) as f64 || x > (clip_x_max) as f64 {
        return; // vertical line outside x range
    }
    if dy > 0 {
        i1 = i1.max(java_trunc_to_i32(((clip_y_min) as f64 - y) / yinc));
    } else if dy < 0 {
        i1 = i1.max(java_trunc_to_i32(((clip_y_max) as f64 - y) / yinc));
    } else if y < (clip_y_min) as f64 || y > (clip_y_max) as f64 {
        return; // horizontal line outside y range
    }
    let mut i2 = n;
    if dx > 0 {
        i2 = i2.min(java_trunc_to_i32(((clip_x_max) as f64 - x) / xinc));
    } else if dx < 0 {
        i2 = i2.min(java_trunc_to_i32(((clip_x_min) as f64 - x) / xinc));
    }
    if dy > 0 {
        i2 = i2.min(java_trunc_to_i32(((clip_y_max) as f64 - y) / yinc));
    } else if dy < 0 {
        i2 = i2.min(java_trunc_to_i32(((clip_y_min) as f64 - y) / yinc));
    }
    x += (i1) as f64 * xinc;
    y += (i1) as f64 * yinc;
    let mut i = i1;
    while i <= i2 {
        plot(java_round_to_i32(x), java_round_to_i32(y));
        x += xinc;
        y += yinc;
        i += 1;
    }
}

/// Java's `(int)` cast on a double: truncate toward zero, saturating at the
/// int range like the JVM cast (NaN maps to 0).
fn java_trunc_to_i32(value: f64) -> i64 {
    if value.is_nan() {
        return 0;
    }
    let clamped = value.clamp(i64::from(i32::MIN) as f64, i64::from(i32::MAX) as f64);
    clamped as i64
}

/// The image `lineTo` clip rectangle for the default line width
/// (ImageProcessor.java 1222-1225 with clipXMin=0, clipXMax=width-1,
/// ImageProcessor.java 956).
fn image_line_clip(width: u32, height: u32) -> (i64, i64, i64, i64) {
    (-1, i64::from(width), -1, i64::from(height))
}

/// Roi.drawPixels(ImageProcessor) dispatch at the default stroke width 1 for
/// the supported ROI types:
/// - Rect (Roi.java 1497-1513): `ip.drawRect(x, y, width+1, height+1)`, whose
///   lineWidth==1 branch (ImageProcessor.java 1307-1312) outlines
///   `(x, y)`..`(x+width, y+height)`.
/// - Oval (OvalRoi.java 303-317 with getPolygon at 329-348): the wand outline
///   of the oval mask, started at `(width/2, height/2)` with `255`/`255`
///   thresholds and shifted by the ROI origin.
/// - Polygon (PolygonRoi.java 308-337): the closed vertex polyline.
/// - Line (Line.java 548-570): the straight line through the endpoints.
fn raster_roi_outline(
    roi: &Roi,
    clip: (i64, i64, i64, i64),
    plot: &mut impl FnMut(i32, i32),
) {
    match roi {
        Roi::Rect(rect) => {
            // Roi.drawPixels (Roi.java 1506-1508) with the default 1-pixel
            // line width calls ip.drawRect(x, y, width+1, height+1), whose
            // lineWidth==1 branch (ImageProcessor.java 1309-1314) outlines
            // (x, y)..(x+width, y+height) inclusive.
            let (x, y) = (i64::from(rect.x), i64::from(rect.y));
            let x2 = i64::from(rect.x + rect.width);
            let y2 = i64::from(rect.y + rect.height);
            raster_line(clip, x, y, x2, y, plot);
            raster_line(clip, x2, y, x2, y2, plot);
            raster_line(clip, x2, y2, x, y2, plot);
            raster_line(clip, x, y2, x, y, plot);
        }
        Roi::Oval(rect) => {
            let mask = crate::oval_mask(rect.width, rect.height);
            let pixels: Vec<u8> = mask
                .iter()
                .map(|&on| if on { 255 } else { 0 })
                .collect();
            let Ok(mask_doc) = ImageDocument::new(
                "oval-mask",
                rect.width as u32,
                rect.height as u32,
                PixelPlane::Gray8(pixels),
            ) else {
                return;
            };
            let outline = Wand::auto_outline(
                &mask_doc,
                (rect.width / 2) as i32,
                (rect.height / 2) as i32,
                // Wand.autoOutline(width/2, height/2, 255, 255) passes lower
                // and upper bounds, not a tolerance: trace exactly the 255
                // pixels of the mask (Wand.java autoOutline 4-arg overload).
                0.0,
                WandMode::Legacy,
            );
            // OvalRoi.drawPixels strokes ip.drawPolygon(getPolygon()) with the
            // default 1-pixel line width, so the wand-traced vertices are
            // connected with lineTo segments closed back onto the first
            // vertex — not drawn as loose dots. The wand trace is relative to
            // the mask, so translate every vertex by the ROI origin.
            let count = outline.len();
            let offset_x = i64::from(rect.x);
            let offset_y = i64::from(rect.y);
            for step in 0..count {
                let (x1, y1) = outline[step];
                let (x2, y2) = outline[(step + 1) % count];
                raster_line(
                    clip,
                    i64::from(x1) + offset_x,
                    i64::from(y1) + offset_y,
                    i64::from(x2) + offset_x,
                    i64::from(y2) + offset_y,
                    plot,
                );
            }
        }
        Roi::Polygon(polygon) => {
            let points: Vec<(i32, i32)> = polygon
                .xpoints
                .iter()
                .zip(polygon.ypoints.iter())
                .map(|(&x, &y)| (x, y))
                .collect();
            raster_closed_polygon(&points, clip, plot);
        }
        Roi::Line(line) => {
            raster_line(
                clip,
                i64::from(line.x1),
                i64::from(line.y1),
                i64::from(line.x2),
                i64::from(line.y2),
                plot,
            );
        }
        Roi::Point(point) => {
            // PointRoi.drawPixels calls moveTo/lineTo at each marker. The
            // default Analyzer.markWidth is zero, which setLineWidth clamps
            // to one (PointRoi.java 334-341, ImageProcessor.java 1210-1212).
            for &(x, y) in &point.points {
                raster_line(clip, i64::from(x), i64::from(y), i64::from(x), i64::from(y), plot);
            }
        }
        Roi::Angle(angle) => raster_open_polygon(&angle.points, clip, plot),
        Roi::Polyline(line) => raster_open_polygon(&line.points, clip, plot),
    }
}

/// PolygonRoi.drawPixels leaves ANGLE, POLYLINE and FREELINE open
/// (PolygonRoi.java 308-337).
fn raster_open_polygon(
    points: &[(i32, i32)],
    clip: (i64, i64, i64, i64),
    plot: &mut impl FnMut(i32, i32),
) {
    for pair in points.windows(2) {
        raster_line(
            clip,
            i64::from(pair[0].0),
            i64::from(pair[0].1),
            i64::from(pair[1].0),
            i64::from(pair[1].1),
            plot,
        );
    }
}

/// Port of ImageProcessor.drawPolygon (ImageProcessor.java 1346-1353) with
/// the default line width 1: `moveTo(p0)` then `lineTo` over the remaining
/// vertices, closed at the first vertex.
fn raster_closed_polygon(
    points: &[(i32, i32)],
    clip: (i64, i64, i64, i64),
    plot: &mut impl FnMut(i32, i32),
) {
    let n = points.len();
    if n == 0 {
        return;
    }
    let mut prev = (i64::from(points[0].0), i64::from(points[0].1));
    for point in &points[1..] {
        let next = (i64::from(point.0), i64::from(point.1));
        raster_line(clip, prev.0, prev.1, next.0, next.1, plot);
        prev = next;
    }
    // PolygonRoi closes the shape back onto the first vertex.
    raster_line(
        clip,
        prev.0,
        prev.1,
        i64::from(points[0].0),
        i64::from(points[0].1),
        plot,
    );
}

/// ImageProcessor.fill() over the clipped rectangle (the ROI rectangle for
/// rectangle selections): ByteProcessor.applyTable writes
/// `lut[p] = fgColor`, ShortProcessor process(FILL) writes the clamped
/// fgColor, FloatProcessor process(FILL) writes fillColor and
/// ColorProcessor.fill() writes the ARGB int.
fn fill_rect_value(
    plane: &mut PixelPlane,
    stride: u32,
    clip: (u32, u32, u32, u32),
    value: PlaneFillValue,
) {
    let (x0, y0, width, height) = clip;
    for y in y0..y0 + height {
        let row = (y * stride) as usize;
        for x in x0..x0 + width {
            value.write_at(plane, row + x as usize);
        }
    }
}

/// ImageProcessor.fill(ImageProcessor mask) (ByteProcessor.java 221-241):
/// `pixel = value` only where the mask byte is nonzero. The mask is already
/// clipped to the image (Roi.raster_mask).
fn fill_mask_value(
    plane: &mut PixelPlane,
    stride: u32,
    mask: &crate::RoiMask,
    value: PlaneFillValue,
) {
    for ly in 0..mask.height {
        let row = ((mask.y + ly) * stride) as usize;
        for lx in 0..mask.width {
            if mask.pixels[(ly * mask.width + lx) as usize] {
                value.write_at(plane, row + (mask.x + lx) as usize);
            }
        }
    }
}

/// Filler.clear/fill/draw on the current slice (Filler.java 56-121): the
/// fill applies to the ROI mask interior (or the whole clipped ROI rectangle
/// for rectangle selections); line selections are re-drawn in the fill color
/// (Filler.java 81-88 and 100-108).
fn fill_editable_plane(doc: &mut ImageDocument, color: [u8; 3]) -> Result<(), CoreError> {
    let (value, new_range) = {
        let index = doc.current_slice_index();
        PlaneFillValue::for_plane(&doc.slices[index], color, doc.display_range)
    };
    if let Some(range) = new_range {
        doc.display_range = range;
    }
    let (width, height) = (doc.width, doc.height);
    let roi = doc.roi.clone();
    let index = doc.current_slice_index();
    match roi {
        // Filler.java 81-88 and 100-108: line selections draw their outline
        // instead of filling.
        Some(roi) if roi.is_line() => {
            let plane = &mut doc.slices[index];
            let mut plot = |x: i32, y: i32| {
                draw_pixel_at(plane, width, height, x, y, &value);
            };
            raster_roi_outline(&roi, image_line_clip(width, height), &mut plot);
        }
        // Filler.java 88-89 and 110: `ip.fill()` fills the ROI rectangle,
        // clipped to the image by ImageProcessor.setRoi (ImageProcessor.java
        // 855-877).
        Some(Roi::Rect(rect)) => {
            let clip = rect.clamp_to(width, height)?;
            let plane = &mut doc.slices[index];
            fill_rect_value(plane, width, clip, value);
        }
        // Filler.java 110 plus PlugInFilterRunner.java 283-290: the
        // SUPPORTS_MASKING reset restores every pixel outside the ROI mask,
        // leaving exactly the mask interior filled.
        Some(roi) => {
            let mask = roi
                .raster_mask(width, height)?
                .ok_or(CoreError::InvalidOperation(
                    "This command requires a selection",
                ))?;
            let plane = &mut doc.slices[index];
            fill_mask_value(plane, width, &mask, value);
        }
        // PlugInFilterRunner.java 409-411 rejects a missing ROI before
        // Filler.run runs.
        None => {
            return Err(CoreError::InvalidOperation(
                "This command requires a selection",
            ));
        }
    }
    Ok(())
}

impl ImageDocument {
    /// `Edit > Clear` (`Filler.run("clear")`) on the current slice
    /// (Filler.java 79-95).
    pub fn clear_roi_or_plane(&mut self, background_color: [u8; 3]) -> Result<(), CoreError> {
        if matches!(self.roi, Some(Roi::Point(_))) {
            return Err(CoreError::InvalidOperation("Area selection required"));
        }
        self.save_snapshot();
        fill_editable_plane(self, background_color)
    }

    /// `Edit > Fill` (`Filler.run("fill")`) on the current slice
    /// (Filler.java 97-113).
    pub fn fill_roi_or_plane(&mut self, foreground_color: [u8; 3]) -> Result<(), CoreError> {
        if matches!(self.roi, Some(Roi::Point(_))) {
            return Err(CoreError::InvalidOperation("Area selection required"));
        }
        self.save_snapshot();
        fill_editable_plane(self, foreground_color)
    }

    /// `Edit > Draw` (`Filler.run("draw")`, Filler.java 116-121): draws the
    /// ROI outline with the foreground color; requires a selection
    /// (PlugInFilterRunner.java 409-411).
    pub fn draw_roi(&mut self, foreground_color: [u8; 3]) -> Result<(), CoreError> {
        self.save_snapshot();
        let Some(roi) = self.roi.clone() else {
            return Err(CoreError::InvalidOperation(
                "This command requires a selection",
            ));
        };
        let (value, new_range) = {
            let index = self.current_slice_index();
            PlaneFillValue::for_plane(&self.slices[index], foreground_color, self.display_range)
        };
        if let Some(range) = new_range {
            self.display_range = range;
        }
        let (width, height) = (self.width, self.height);
        let index = self.current_slice_index();
        let plane = &mut self.slices[index];
        let mut plot = |x: i32, y: i32| {
            draw_pixel_at(plane, width, height, x, y, &value);
        };
        raster_roi_outline(&roi, image_line_clip(width, height), &mut plot);
        Ok(())
    }
}
