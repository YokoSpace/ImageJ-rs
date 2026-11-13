//! `Image > Stacks > Add Slice` and `Delete Slice` for plain Z-stacks.
//!
//! Both commands use StackEditor.java 30-71. Surviving pixels, calibration,
//! ROI and title are retained. setStack creates a fresh processor, clearing
//! its threshold and resetting RGB display limits. Grayscale display limits
//! are copied from the stack. The document Undo snapshot is cleared.

use crate::{CoreError, DisplayRange, ImageDocument, PixelPlane};

impl ImageDocument {
    fn require_plain_z_stack(&self) -> Result<(), CoreError> {
        if self.supports_stack_axis_edit() {
            Ok(())
        } else {
            Err(CoreError::UnsupportedStackAxisEdit {
                channels: self.n_channels,
                slices: self.n_slices,
                frames: self.n_frames,
            })
        }
    }

    /// `run("Add Slice")` (StackEditor.java 30-49).
    ///
    /// `insert_before_current` is `IJ.altKeyDown()`: the blank slice goes in
    /// front of the current slice instead of after it. The blank slice is
    /// `ip.createProcessor(width, height)` of the current slice, i.e. an
    /// all-zero plane of the same type and colour model
    /// (ByteProcessor.java 136-143, ColorProcessor.java 98-102,
    /// ShortProcessor.java 183-188, FloatProcessor.java 236-241).
    pub fn add_slice(&mut self, insert_before_current: bool) -> Result<(), CoreError> {
        self.require_plain_z_stack()?;
        let len = self.width as usize * self.height as usize;
        let blank = self.slices[self.current_slice_index()].empty_like(len);
        // StackEditor.java 41-43: `n` is the one-based current slice, and
        // ImageStack.addSlice(label, ip, n) inserts at the zero-based index
        // `n`, so the new slice lands at one-based index `n + 1`.
        let mut n = self.current_slice as i32;
        if insert_before_current {
            n -= 1;
        }
        let index = n.max(0) as usize;
        self.slices.insert(index, blank);
        self.slice_labels.insert(index, None);
        self.n_slices = self.slices.len() as u32;
        // StackEditor.java 44-45: setStack(null, stack) then setSlice(n+1).
        self.set_current_slice((n + 1) as u32);
        self.reset_stack_processor();
        Ok(())
    }

    /// `run("Delete Slice")` (StackEditor.java 51-71).
    ///
    /// The last remaining slice cannot be deleted: ImageJ reports
    /// `"Delete Slice" requires a stack` before locking the image.
    pub fn delete_slice(&mut self) -> Result<(), CoreError> {
        if self.slices.len() < 2 {
            return Err(CoreError::DeleteSliceRequiresStack);
        }
        self.require_plain_z_stack()?;
        let n = self.current_slice as usize;
        self.slices.remove(n - 1);
        self.slice_labels.remove(n - 1);
        self.n_slices = self.slices.len() as u32;
        if self.slices.len() == 1 {
            if let Some(label) = &self.slice_labels[0] {
                self.slice_label_property = Some(label.clone());
            }
        }
        // StackEditor.java 67-68: `if (n--<1) n = 1;` leaves 0 for n == 1, and
        // ImagePlus.setSlice ignores an index below 1 (ImagePlus.java 1972),
        // so the current slice stays 1 in that case.
        let next = if n < 2 { 1 } else { n - 1 };
        self.set_current_slice(next as u32);
        self.reset_stack_processor();
        Ok(())
    }

    fn reset_stack_processor(&mut self) {
        // setStack replaces the processor (ImagePlus.java 848-852).
        // ImageStack.getProcessor carries min/max only for non-RGB types
        // (ImageStack.java 308-336); thresholds are never copied.
        self.threshold = None;
        if matches!(self.plane(), PixelPlane::ColorRgb(_)) {
            self.display_range = DisplayRange {
                min: 0.0,
                max: 255.0,
            };
        }
        self.snapshot = None;
        self.snapshot_dimensions = None;
        self.snapshot_display_range = None;
    }
}
