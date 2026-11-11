//! Pure port of `ij.plugin.filter.Binary`'s Erode / Dilate / Open / Close
//! commands (original_imagej/ij/plugin/filter/Binary.java lines 124-158)
//! driving `ByteProcessor.filter(ERODE/DILATE)`
//! (original_imagej/ij/process/ByteProcessor.java lines 575-680).
//!
//! `PlugInFilterRunner` snapshots the processor before `run(ip)`
//! (PlugInFilterRunner.java lines 107-121), so every pass reads a frozen
//! copy of the plane and writes the live pixels in place. Out-of-image
//! neighbours are padded with the background value (`getEdgePixel0`,
//! ByteProcessor.java line 797). `Prefs.padEdges` keeps its `false` default
//! (IJ_Props.txt) so Java never takes the `getEdgePixel1` pad branch.

use crate::filters::FilterRegion;
use crate::{CoreError, ImageDocument, PixelPlane};

/// The four `Process > Binary` morphology operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum BinaryMorphology {
    Erode,
    Dilate,
    Open,
    Close,
}

/// `Binary.doIterations` limit (Binary.java line 19).
pub const MAX_ITERATIONS: u32 = 100;

impl ImageDocument {
    /// Port of one `Binary` plugin invocation over the current slice.
    ///
    /// * `iterations` mirrors the Binary Options "Iterations (1-100)" field
    ///   (Binary.java lines 102-103): every iteration is one full
    ///   `filter(ERODE)`/`filter(DILATE)` pass reading the previous pass'
    ///   output.
    /// * `count` mirrors the "Count (1-8)" field, the neighbour threshold
    ///   (`binaryCount` in the Java switch).
    /// * `black_background` mirrors `Prefs.blackBackground` (Binary.java
    ///   lines 122-124): `false` keeps foreground 0 / background 255, `true`
    ///   swaps the pair.
    ///
    /// Only 8-bit planes whose pixels are all 0 or 255 are accepted, matching
    /// the `Binary.showDialog` gate (Binary.java lines 76-79). Only the
    /// current slice is processed, like every other `Process` menu filter.
    pub fn binary_morphology(
        &mut self,
        operation: BinaryMorphology,
        iterations: u32,
        count: u32,
        black_background: bool,
    ) -> Result<(), CoreError> {
        if iterations == 0 || iterations > MAX_ITERATIONS {
            return Err(CoreError::InvalidFilterParameter {
                filter: "binary morphology",
                parameter: "iterations",
                value: format!("{iterations}"),
            });
        }
        if count == 0 || count > 8 {
            return Err(CoreError::InvalidFilterParameter {
                filter: "binary morphology",
                parameter: "count",
                value: format!("{count}"),
            });
        }
        // Binary.showDialog's isBinary gate (Binary.java lines 76-79).
        match self.plane() {
            PixelPlane::Gray8(pixels) => {
                if pixels.iter().any(|&p| p != 0 && p != 255) {
                    return Err(CoreError::InvalidOperation(
                        "8-bit binary (0 and 255 only) image required.",
                    ));
                }
            }
            _ => {
                return Err(CoreError::InvalidOperation(
                    "8-bit binary (0 and 255 only) image required.",
                ));
            }
        }

        self.save_snapshot();

        let width = self.width;
        let height = self.height;
        let region = self.filter_region()?;
        let plane = match self.plane_mut() {
            PixelPlane::Gray8(pixels) => pixels,
            _ => unreachable!("checked above"),
        };

        // Binary.run lines 133-140: Erode/Dilate repeat their own pass
        // `iterations` times; Open runs erode then dilate and Close runs
        // dilate then erode, each half `iterations` single passes.
        let mut passes = Vec::new();
        match operation {
            BinaryMorphology::Erode => {
                for _ in 0..iterations {
                    passes.push(true);
                }
            }
            BinaryMorphology::Dilate => {
                for _ in 0..iterations {
                    passes.push(false);
                }
            }
            BinaryMorphology::Open => {
                for _ in 0..iterations {
                    passes.push(true);
                }
                for _ in 0..iterations {
                    passes.push(false);
                }
            }
            BinaryMorphology::Close => {
                for _ in 0..iterations {
                    passes.push(false);
                }
                for _ in 0..iterations {
                    passes.push(true);
                }
            }
        }

        // Binary.run lines 122-124: foreground = Prefs.blackBackground ? 255
        // : 0 and background = 255 - foreground.
        let (foreground, background) = if black_background { (255u8, 0u8) } else { (0u8, 255u8) };

        // PlugInFilterRunner's masking step (lines 268-272) restores pixels
        // outside an irregular mask from the pre-command snapshot; the mask
        // never changes during the passes.
        let original = plane.clone();
        for erode_pass in passes {
            let snapshot = plane.clone();
            filter_pass(
                &snapshot,
                plane,
                width,
                height,
                &region,
                erode_pass,
                foreground,
                background,
                count,
            );
        }
        if let Some(mask) = &region.mask {
            for y in region.y..region.y + region.height {
                for x in region.x..region.x + region.width {
                    if !mask.contains(x, y) {
                        let index = (y * width + x) as usize;
                        plane[index] = original[index];
                    }
                }
            }
        }
        Ok(())
    }

    /// The whole-stack answer (`run("Options...", "... stack do=Erode")`),
    /// reached when `IJ.setupDialog` adds DOES_STACKS: every slice runs the
    /// same morphology with the reused ROI rectangle. `PlugInFilterRunner`
    /// drops the undo snapshot for whole-stack runs (PlugInFilterRunner.java
    /// line 126, `Undo.reset()`), which matches the other whole-stack
    /// filters' "no undo" contract.
    pub fn binary_morphology_stack(
        &mut self,
        operation: BinaryMorphology,
        iterations: u32,
        count: u32,
        black_background: bool,
    ) -> Result<(), CoreError> {
        for index in 0..self.slices.len() {
            self.current_slice = (index + 1) as u32;
            self.binary_morphology(operation, iterations, count, black_background)?;
        }
        self.current_slice = (self.slices.len()) as u32;
        // PlugInFilterRunner's whole-stack path calls Undo.reset()
        // (PlugInFilterRunner.java line 126): no undo for whole-stack runs.
        self.snapshot = None;
        self.snapshot_dimensions = None;
        self.snapshot_display_range = None;
        Ok(())
    }
}

/// One `ByteProcessor.filter(ERODE)` (`erode=true`) or `filter(DILATE)` pass
/// (ByteProcessor.java lines 575-680) written in place.
///
/// * The main loop covers `[max(x,1) .. min(x+w-1, width-2)]` by
///   `[max(y,1) .. min(y+h-1, height-2)]`, the `ImageProcessor.setRoi`
///   clamps (ImageProcessor.java lines 881-884), where all eight neighbours
///   are in bounds and read from the snapshot.
/// * The four `filterEdge` calls (ByteProcessor.java lines 677-680) rewrite
///   exactly the strips the main loop skips because a neighbour would fall
///   outside the image; their out-of-image neighbours read as the background
///   value (`getEdgePixel0`, line 797).
/// * Every written pixel runs the same switch the Java code uses
///   (lines 635-661 for ERODE, mirrored for DILATE): `p5 == target -> status
///   quo`, else a neighbour-count vote towards `target`, else the opposite
///   value.
#[allow(clippy::too_many_arguments)]
fn filter_pass(
    snapshot: &[u8],
    plane: &mut [u8],
    width: u32,
    height: u32,
    region: &FilterRegion,
    erode: bool,
    foreground: u8,
    background: u8,
    count: u32,
) {
    // The pass votes towards `target`: erode counts background-valued
    // neighbours, dilate counts foreground-valued ones (the value that wins
    // the vote and the value the centre keeps when it already matches).
    let target = if erode { background } else { foreground };

    let x_min = (region.x as i64).max(1);
    let x_max = ((region.x + region.width) as i64 - 1).min(width as i64 - 2);
    let y_min = (region.y as i64).max(1);
    let y_max = ((region.y + region.height) as i64 - 1).min(height as i64 - 2);

    // Main loop (ByteProcessor.java lines 583-675): all neighbours are in
    // bounds thanks to the setRoi clamps above.
    for y in y_min..=y_max {
        for x in x_min..=x_max {
            let index = (y * width as i64 + x) as usize;
            let centre = snapshot[index];
            if centre == target {
                plane[index] = target;
                continue;
            }
            let mut matching = 0u32;
            for ny in (y - 1)..=(y + 1) {
                for nx in (x - 1)..=(x + 1) {
                    if nx == x && ny == y {
                        continue;
                    }
                    if snapshot[(ny * width as i64 + nx) as usize] == target {
                        matching += 1;
                    }
                }
            }
            if matching >= count {
                plane[index] = target;
            } else {
                plane[index] = snapshot[index];
            }
        }
    }

    // filterEdge strips (ByteProcessor.java lines 677-680): out-of-image
    // neighbours pad with the background value (getEdgePixel0, line 797).
    let edge_pixel = |snapshot: &[u8], x: i64, y: i64| -> u8 {
        if x < 0 || x > width as i64 - 1 || y < 0 || y > height as i64 - 1 {
            background
        } else {
            snapshot[(y * width as i64 + x) as usize]
        }
    };
    let strip_value = |snapshot: &[u8], x: i64, y: i64| -> u8 {
        let centre = edge_pixel(snapshot, x, y);
        if centre == target {
            return target;
        }
        let mut matching = 0u32;
        for ny in (y - 1)..=(y + 1) {
            for nx in (x - 1)..=(x + 1) {
                if nx == x && ny == y {
                    continue;
                }
                if edge_pixel(snapshot, nx, ny) == target {
                    matching += 1;
                }
            }
        }
        if matching >= count {
            target
        } else {
            255 - target
        }
    };

    if x_min == 1 {
        for y in (region.y as i64)..((region.y + region.height) as i64) {
            let x = region.x as i64;
            let index = (y * width as i64 + x) as usize;
            plane[index] = strip_value(snapshot, x, y);
        }
    }
    if y_min == 1 {
        for x in (region.x as i64)..((region.x + region.width) as i64) {
            let y = region.y as i64;
            let index = (y * width as i64 + x) as usize;
            plane[index] = strip_value(snapshot, x, y);
        }
    }
    if width >= 2 && x_max == width as i64 - 2 {
        for y in (region.y as i64)..((region.y + region.height) as i64) {
            let x = width as i64 - 1;
            let index = (y * width as i64 + x) as usize;
            plane[index] = strip_value(snapshot, x, y);
        }
    }
    if height >= 2 && y_max == height as i64 - 2 {
        for x in (region.x as i64)..((region.x + region.width) as i64) {
            let y = height as i64 - 1;
            let index = (y * width as i64 + x) as usize;
            plane[index] = strip_value(snapshot, x, y);
        }
    }
}
