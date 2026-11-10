use ij_core::ImageDocument;

pub const ZOOM_LEVELS: &[f64] = &[
    0.25, 0.333, 0.5, 0.75, 1.0, 1.5, 2.0, 3.0, 4.0, 6.0, 8.0, 12.0, 16.0,
];

#[derive(Debug, Clone)]
pub struct ImageView {
    pub width: u32,
    pub height: u32,
    pub zoom: f64,
    pub pan: (f32, f32),
    pub is_dragging: bool,
    pub drag_start_pixel: Option<(i32, i32)>,
    pub drag_current_pixel: Option<(i32, i32)>,
    pub polygon_points: Vec<(i32, i32)>,
}

impl ImageView {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            zoom: 1.0,
            pan: (0.0, 0.0),
            is_dragging: false,
            drag_start_pixel: None,
            drag_current_pixel: None,
            polygon_points: Vec::new(),
        }
    }

    pub fn zoom_in(&mut self) {
        for &lvl in ZOOM_LEVELS {
            if lvl > self.zoom + 1e-4 {
                self.zoom = lvl;
                return;
            }
        }
    }

    pub fn zoom_out(&mut self) {
        for &lvl in ZOOM_LEVELS.iter().rev() {
            if lvl < self.zoom - 1e-4 {
                self.zoom = lvl;
                return;
            }
        }
    }

    pub fn set_zoom(&mut self, zoom: f64) {
        self.zoom = zoom.clamp(0.1, 32.0);
    }

    /// Maps coordinates relative to the image canvas origin to integer pixel coordinates.
    pub fn screen_to_pixel(&self, sx: f32, sy: f32) -> (i32, i32) {
        let z = self.zoom as f32;
        let px = ((sx - self.pan.0) / z).floor() as i32;
        let py = ((sy - self.pan.1) / z).floor() as i32;
        (px, py)
    }

    /// Maps integer pixel coordinates to screen coordinates relative to image canvas origin.
    pub fn pixel_to_screen(&self, px: f32, py: f32) -> (f32, f32) {
        let z = self.zoom as f32;
        (px * z + self.pan.0, py * z + self.pan.1)
    }

    /// Creates subtitle string matching original ImageWindow.java lines 310-385.
    pub fn create_subtitle(doc: &ImageDocument) -> String {
        let mut s = String::new();
        let stack_size = doc.stack_size();
        if stack_size > 1 {
            s.push_str(&format!("{}/{}; ", doc.current_slice, stack_size));
        }

        s.push_str(&format!("{}x{} pixels; ", doc.width, doc.height));

        match doc.plane().bit_depth() {
            8 => s.push_str("8-bit; "),
            16 => s.push_str("16-bit; "),
            24 => s.push_str("RGB; "),
            32 => s.push_str("32-bit; "),
            _ => s.push_str("8-bit; "),
        }

        let bytes_per_pixel = match doc.plane().bit_depth() {
            8 => 1,
            16 => 2,
            24 => 3,
            32 => 4,
            _ => 1,
        };
        let total_bytes =
            (doc.width as u64) * (doc.height as u64) * bytes_per_pixel * (stack_size as u64);
        let size_kb = (total_bytes as f64) / 1024.0;
        if size_kb < 1024.0 {
            s.push_str(&format!("{}K", size_kb.round() as u64));
        } else {
            s.push_str(&format!("{:.1}MB", size_kb / 1024.0));
        }

        s
    }
}
