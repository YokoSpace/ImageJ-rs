use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

use crate::{
    CoreError, ImageDocument, MeasurementConfig, MeasurementSummary, PixelPlane, PolygonRoi, Roi,
    RoiMask, ThresholdLevels, Wand, WandMode,
};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ParticleAnalysisOptions {
    pub min_size: f64,
    pub max_size: f64,
    pub min_circularity: f64,
    pub max_circularity: f64,
    pub exclude_on_edges: bool,
    pub include_holes: bool,
    pub pixel_units: bool,
    pub black_background: bool,
}

impl Default for ParticleAnalysisOptions {
    fn default() -> Self {
        Self {
            min_size: 0.0,
            max_size: f64::INFINITY,
            min_circularity: 0.0,
            max_circularity: 1.0,
            exclude_on_edges: false,
            include_holes: false,
            pixel_units: false,
            black_background: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AnalyzedParticle {
    pub roi: Roi,
    pub measurement: MeasurementSummary,
    pub mask: RoiMask,
}

impl ImageDocument {
    /// Scans a cloned processor in ImageJ's row-major order. The original
    /// plane, threshold and ROI are never changed by the analysis.
    pub fn analyze_particles(
        &self,
        options: ParticleAnalysisOptions,
        config: MeasurementConfig,
    ) -> Result<Vec<AnalyzedParticle>, CoreError> {
        if !options.min_size.is_finite()
            || options.min_size < 0.0
            || options.max_size < options.min_size
            || !(0.0..=1.0).contains(&options.min_circularity)
            || !(options.min_circularity..=1.0).contains(&options.max_circularity)
        {
            return Err(CoreError::InvalidOperation(
                "invalid particle analysis range",
            ));
        }
        if matches!(self.plane(), PixelPlane::ColorRgb(_)) {
            return Err(CoreError::InvalidOperation(
                "RGB particle analysis requires a threshold mask",
            ));
        }
        let levels = match self.threshold {
            Some(levels) => levels,
            None => {
                let PixelPlane::Gray8(values) = self.plane() else {
                    return Err(CoreError::InvalidOperation("A threshold has not been set"));
                };
                if values.iter().any(|&v| v != 0 && v != 255) {
                    return Err(CoreError::InvalidOperation("A threshold has not been set"));
                }
                let foreground = if options.black_background { 255.0 } else { 0.0 };
                ThresholdLevels {
                    lower: foreground,
                    upper: foreground,
                }
            }
        };
        let full_byte_range = levels.lower == 0.0
            && levels.upper == 255.0
            && matches!(
                self.plane(),
                PixelPlane::Gray8(_) | PixelPlane::Color256 { .. }
            );
        let mut work = self.clone();
        let source_roi = self.roi.as_ref().filter(|roi| roi.is_area());
        let (left, top, width, height) = source_roi
            .map(|roi| roi.clipped_bounds(self.width, self.height))
            .transpose()?
            .unwrap_or((0, 0, self.width, self.height));
        let source_mask = source_roi
            .map(|roi| roi.raster_mask(self.width, self.height))
            .transpose()?
            .flatten();
        let max_x = left + width;
        let max_y = top + height;
        if source_roi.is_some() {
            // ParticleAnalyzer.java 590-603 erases the processor outside the
            // source selection before tracing. Filtering seeds alone lets a
            // connected particle extend past the selected area.
            for y in 0..self.height {
                for x in 0..self.width {
                    let in_bounds = x >= left && x < max_x && y >= top && y < max_y;
                    let in_mask = source_mask.as_ref().is_none_or(|mask| mask.contains(x, y));
                    if !in_bounds || !in_mask {
                        erase(work.plane_mut(), self.width, x, y, levels);
                    }
                }
            }
        }
        let unit_squared = if options.pixel_units {
            1.0
        } else {
            self.calibration.pixel_width * self.calibration.pixel_height
        };
        let min_size = options.min_size / unit_squared;
        let max_size = options.max_size / unit_squared;
        let mut found = Vec::new();
        for y in top..max_y {
            for x in left..max_x {
                if source_mask
                    .as_ref()
                    .is_some_and(|mask| !mask.contains(x, y))
                    || !inside(work.plane(), self.width, x, y, levels)
                {
                    continue;
                }
                let points = Wand::auto_outline(&work, x as i32, y as i32, 0.0, WandMode::Legacy);
                if points.len() < 3 {
                    // An invalid outline must not leave a seed eligible forever.
                    erase(work.plane_mut(), self.width, x, y, levels);
                    continue;
                }
                let roi = Roi::Polygon(PolygonRoi::new(&points)?);
                let (rx, ry, rw, rh) = roi.bounds();
                let polygon_mask = roi
                    .raster_mask(self.width, self.height)?
                    .ok_or(CoreError::EmptySelection)?;
                let mask = if options.include_holes {
                    polygon_mask
                } else {
                    component_mask(
                        work.plane(),
                        self.width,
                        self.height,
                        x,
                        y,
                        levels,
                        &polygon_mask,
                    )
                };
                let mut measured = self.clone();
                measured.roi = Some(roi.clone());
                let measurement = measured.measure_with_mask(config, Some(&mask))?;
                // ParticleAnalyzer.java 925-934 filters before attaching the
                // ImagePlus to the traced ROI, so this perimeter is in pixels.
                // The Results `Perim.` column remains calibrated.
                let perimeter = crate::polygon_perimeter(
                    match &roi {
                        Roi::Polygon(poly) => poly,
                        _ => unreachable!(),
                    },
                    1.0,
                    1.0,
                );
                let circularity = if perimeter == 0.0 {
                    0.0
                } else {
                    (4.0 * std::f64::consts::PI * measurement.pixel_count as f64
                        / (perimeter * perimeter))
                        .min(1.0)
                };
                let on_edge = rx <= left as i32
                    || ry <= top as i32
                    || rx + rw >= max_x as i32
                    || ry + rh >= max_y as i32;
                if measurement.pixel_count as f64 >= min_size
                    && (measurement.pixel_count as f64) <= max_size
                    && circularity >= options.min_circularity
                    && circularity <= options.max_circularity
                    && (!options.exclude_on_edges || !on_edge)
                {
                    found.push(AnalyzedParticle {
                        roi,
                        measurement,
                        mask: mask.clone(),
                    });
                }
                for yy in mask.y..mask.y + mask.height {
                    for xx in mask.x..mask.x + mask.width {
                        if mask.contains(xx, yy) {
                            erase(work.plane_mut(), self.width, xx, yy, levels);
                        }
                    }
                }
                // ParticleAnalyzer.java 612-618 sets `done` after the first
                // 8-bit particle when the threshold covers every byte value.
                if full_byte_range {
                    return Ok(found);
                }
            }
        }
        Ok(found)
    }
}

fn inside(plane: &PixelPlane, width: u32, x: u32, y: u32, levels: ThresholdLevels) -> bool {
    let index = (y * width + x) as usize;
    let value = match plane {
        PixelPlane::Gray8(p) => p[index] as f64,
        PixelPlane::Color256 { pixels, .. } => pixels[index] as f64,
        PixelPlane::Gray16(p) => p[index] as f64,
        PixelPlane::Gray32(p) => p[index] as f64,
        PixelPlane::ColorRgb(_) => return false,
    };
    value >= levels.lower && value <= levels.upper
}

fn erase(plane: &mut PixelPlane, width: u32, x: u32, y: u32, levels: ThresholdLevels) {
    let index = (y * width + x) as usize;
    match plane {
        PixelPlane::Gray8(p) => p[index] = if levels.lower > 0.0 { 0 } else { 255 },
        PixelPlane::Color256 { pixels, .. } => {
            pixels[index] = if levels.lower > 0.0 { 0 } else { 255 }
        }
        PixelPlane::Gray16(p) => p[index] = if levels.lower > 0.0 { 0 } else { 65535 },
        PixelPlane::Gray32(p) => p[index] = -f32::MAX,
        PixelPlane::ColorRgb(_) => {}
    }
}

fn component_mask(
    plane: &PixelPlane,
    width: u32,
    height: u32,
    seed_x: u32,
    seed_y: u32,
    levels: ThresholdLevels,
    polygon: &RoiMask,
) -> RoiMask {
    let mut mask = RoiMask {
        x: polygon.x,
        y: polygon.y,
        width: polygon.width,
        height: polygon.height,
        pixels: vec![false; polygon.pixels.len()],
    };
    let mut queue = VecDeque::from([(seed_x, seed_y)]);
    while let Some((x, y)) = queue.pop_front() {
        if x >= width
            || y >= height
            || !polygon.contains(x, y)
            || !inside(plane, width, x, y, levels)
            || mask.contains(x, y)
        {
            continue;
        }
        mask.pixels[((y - mask.y) * mask.width + x - mask.x) as usize] = true;
        if x > 0 {
            queue.push_back((x - 1, y));
        }
        if y > 0 {
            queue.push_back((x, y - 1));
        }
        if x + 1 < width {
            queue.push_back((x + 1, y));
        }
        if y + 1 < height {
            queue.push_back((x, y + 1));
        }
    }
    mask
}
