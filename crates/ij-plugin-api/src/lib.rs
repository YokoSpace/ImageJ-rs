//! Typed, in-process ImageJ-rs plugin contract.
//!
//! A plugin receives image metadata, owned pixel planes and an append-only
//! Results buffer. The session commits those buffers only after `run` succeeds.

use ij_core::{Calibration, DisplayRange, ImageDocument, MeasurementSummary, PixelPlane, Roi};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelKind {
    Gray8,
    Color256,
    Gray16,
    Gray32,
    ColorRgb,
}

impl PixelKind {
    pub fn of(plane: &PixelPlane) -> Self {
        match plane {
            PixelPlane::Gray8(_) => Self::Gray8,
            PixelPlane::Color256 { .. } => Self::Color256,
            PixelPlane::Gray16(_) => Self::Gray16,
            PixelPlane::Gray32(_) => Self::Gray32,
            PixelPlane::ColorRgb(_) => Self::ColorRgb,
        }
    }
}

fn validate_plane(plane: &PixelPlane, index: usize, expected: usize) -> Result<(), String> {
    if plane.len() != expected {
        return Err(format!(
            "slice {} pixel count does not match image dimensions",
            index + 1
        ));
    }
    if let PixelPlane::Color256 { lut, .. } = plane {
        if lut.r.len() != 256 || lut.g.len() != 256 || lut.b.len() != 256 {
            return Err(format!(
                "slice {} palette must have 256 entries per channel",
                index + 1
            ));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImageMetadata {
    pub width: u32,
    pub height: u32,
    pub stack_size: usize,
    pub current_slice: u32,
    pub pixel_kind: PixelKind,
    pub slice_kinds: Vec<PixelKind>,
    pub roi: Option<Roi>,
    pub calibration: Calibration,
    pub display_range: DisplayRange,
}

impl ImageMetadata {
    pub fn from_document(doc: &ImageDocument) -> Result<Self, String> {
        let current = doc
            .current_slice
            .checked_sub(1)
            .and_then(|index| doc.slices.get(index as usize))
            .ok_or_else(|| "current slice is outside the image stack".to_string())?;
        let expected = (doc.width as usize)
            .checked_mul(doc.height as usize)
            .ok_or_else(|| "image dimensions overflow".to_string())?;
        if expected == 0 {
            return Err("image dimensions must be positive".into());
        }
        for (index, plane) in doc.slices.iter().enumerate() {
            validate_plane(plane, index, expected)?;
        }
        Ok(Self {
            width: doc.width,
            height: doc.height,
            stack_size: doc.slices.len(),
            current_slice: doc.current_slice,
            pixel_kind: PixelKind::of(current),
            slice_kinds: doc.slices.iter().map(PixelKind::of).collect(),
            roi: doc.roi.clone(),
            calibration: doc.calibration.clone(),
            display_range: doc.display_range,
        })
    }
}

pub struct PluginContext {
    metadata: ImageMetadata,
    slices: Vec<PixelPlane>,
    appended_results: Vec<MeasurementSummary>,
}

impl PluginContext {
    pub fn from_document(doc: &ImageDocument) -> Result<Self, String> {
        Ok(Self {
            metadata: ImageMetadata::from_document(doc)?,
            slices: doc.slices.clone(),
            appended_results: Vec::new(),
        })
    }

    pub fn metadata(&self) -> &ImageMetadata {
        &self.metadata
    }

    pub fn slices(&self) -> &[PixelPlane] {
        &self.slices
    }

    pub fn slices_mut(&mut self) -> &mut [PixelPlane] {
        &mut self.slices
    }

    pub fn current_plane_mut(&mut self) -> &mut PixelPlane {
        &mut self.slices[self.metadata.current_slice as usize - 1]
    }

    pub fn append_result(&mut self, row: MeasurementSummary) {
        self.appended_results.push(row);
    }

    pub fn pending_results(&self) -> &[MeasurementSummary] {
        &self.appended_results
    }

    pub fn validate_output(&self) -> Result<(), String> {
        if self.slices.len() != self.metadata.stack_size {
            return Err("stack size changed".into());
        }
        let expected = (self.metadata.width as usize)
            .checked_mul(self.metadata.height as usize)
            .ok_or_else(|| "image dimensions overflow".to_string())?;
        for (index, plane) in self.slices.iter().enumerate() {
            validate_plane(plane, index, expected)?;
            if PixelKind::of(plane) != self.metadata.slice_kinds[index] {
                return Err(format!("slice {} pixel type changed", index + 1));
            }
        }
        Ok(())
    }

    pub fn into_output(self) -> (Vec<PixelPlane>, Vec<MeasurementSummary>) {
        (self.slices, self.appended_results)
    }
}

pub trait RustImagePlugin {
    fn name(&self) -> &str;
    fn supports(&self, meta: &ImageMetadata) -> Result<(), String>;
    fn run(&self, ctx: &mut PluginContext) -> Result<(), String>;
}
