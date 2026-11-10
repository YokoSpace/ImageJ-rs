use crate::{ImageDocument, PixelPlane};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WandMode {
    Legacy,
    FourConnected,
    EightConnected,
}

pub struct Wand;

impl Wand {
    /// Returns ImageJ's ordered boundary vertices, or an empty outline when
    /// the click is outside the image or lands on a NaN float pixel.
    pub fn auto_outline(
        doc: &ImageDocument,
        start_x: i32,
        start_y: i32,
        tolerance: f64,
        mode: WandMode,
    ) -> Vec<(i32, i32)> {
        let width = doc.width as i32;
        let height = doc.height as i32;
        if start_x < 0 || start_x >= width || start_y < 0 || start_y >= height {
            return Vec::new();
        }
        let plane = doc.plane();
        let exact_pixel_value = tolerance == 0.0;
        let threshold_mode = doc.threshold.is_some();
        let start_value = pixel_value(plane, width, start_x, start_y, exact_pixel_value);
        if start_value.is_nan() {
            return Vec::new();
        }
        let (lower, upper) = match doc.threshold {
            Some(levels) => (levels.lower as f32, levels.upper as f32),
            None => (
                (start_value as f64 - tolerance) as f32,
                (start_value as f64 + tolerance) as f32,
            ),
        };
        let state = WandState {
            plane,
            width,
            height,
            lower,
            upper,
            exact_pixel_value,
        };
        let mut x = start_x;
        let y = start_y;
        let seed_x;
        if state.inside(x, y) {
            seed_x = x;
            loop {
                x += 1;
                if !state.inside(x, y) {
                    break;
                }
            }
        } else {
            loop {
                x += 1;
                if x >= width {
                    return Vec::new();
                }
                if state.inside(x, y) {
                    break;
                }
            }
            seed_x = x;
        }

        let legacy = mode == WandMode::Legacy && tolerance == 0.0;
        let four_connected = if legacy {
            !threshold_mode && !state.is_line(x, y)
        } else {
            mode == WandMode::FourConnected
        };
        let mut first = true;
        loop {
            let (points, inside_selected) = state.trace_edge(x, y, four_connected);
            if legacy
                || (inside_selected
                    && (first
                        || (points.iter().map(|p| p.0).min().unwrap_or(width) <= seed_x
                            && polygon_contains(&points, seed_x, start_y))))
            {
                return points;
            }
            first = false;
            if !state.inside(x, y) {
                loop {
                    x += 1;
                    if x > width {
                        return Vec::new();
                    }
                    if state.inside(x, y) {
                        break;
                    }
                }
            }
            loop {
                x += 1;
                if x > width {
                    return Vec::new();
                }
                if !state.inside(x, y) {
                    break;
                }
            }
        }
    }
}

struct WandState<'a> {
    plane: &'a PixelPlane,
    width: i32,
    height: i32,
    lower: f32,
    upper: f32,
    exact_pixel_value: bool,
}

impl WandState<'_> {
    fn inside(&self, x: i32, y: i32) -> bool {
        if x < 0 || x >= self.width || y < 0 || y >= self.height {
            return false;
        }
        let value = pixel_value(self.plane, self.width, x, y, self.exact_pixel_value);
        value >= self.lower && value <= self.upper
    }

    fn inside_direction(&self, x: i32, y: i32, direction: i32) -> bool {
        match direction & 3 {
            0 => self.inside(x, y),
            1 => self.inside(x, y - 1),
            2 => self.inside(x - 1, y - 1),
            _ => self.inside(x - 1, y),
        }
    }

    fn is_line(&self, xs: i32, ys: i32) -> bool {
        let xmin = xs;
        let xmax = (xs + 10).min(self.width - 1);
        let ymin = (ys - 5).max(0);
        let ymax = (ys + 5).min(self.height - 1);
        let mut area = 0;
        let mut inside_count = 0;
        for x in xmin..=xmax {
            for y in ymin..=ymax {
                area += 1;
                if self.inside(x, y) {
                    inside_count += 1;
                }
            }
        }
        area > 0 && (inside_count as f64 / area as f64) < 0.25
    }

    fn trace_edge(
        &self,
        start_x: i32,
        start_y: i32,
        four_connected: bool,
    ) -> (Vec<(i32, i32)>, bool) {
        let mut points = Vec::new();
        let start_direction = if self.inside(start_x, start_y) { 1 } else { 3 };
        let mut x = start_x;
        let mut y = start_y + if start_direction == 3 { 1 } else { 0 };
        let start_y = y;
        let mut direction = start_direction;
        let max_steps = (self.width as usize)
            .saturating_mul(self.height as usize)
            .saturating_mul(16)
            .saturating_add(64);
        for _ in 0..max_steps {
            let new_direction = if four_connected {
                let mut next = direction;
                loop {
                    if !self.inside_direction(x, y, next) {
                        break;
                    }
                    next += 1;
                    if next >= direction + 2 {
                        break;
                    }
                }
                next - 1
            } else {
                let mut next = direction + 1;
                loop {
                    if self.inside_direction(x, y, next) {
                        break;
                    }
                    next -= 1;
                    if next < direction {
                        break;
                    }
                }
                next
            };
            if new_direction != direction {
                points.push((x, y));
            }
            match new_direction & 3 {
                0 => x += 1,
                1 => y -= 1,
                2 => x -= 1,
                _ => y += 1,
            }
            direction = new_direction;
            if x == start_x && y == start_y && (direction & 3) == start_direction {
                if points.first().is_some_and(|point| point.0 != x) {
                    points.push((x, y));
                }
                return (points, direction <= 0);
            }
        }
        (Vec::new(), false)
    }
}

fn pixel_value(plane: &PixelPlane, width: i32, x: i32, y: i32, exact_pixel_value: bool) -> f32 {
    let index = (y * width + x) as usize;
    match plane {
        PixelPlane::Gray8(pixels) | PixelPlane::Color256 { pixels, .. } => pixels[index] as f32,
        PixelPlane::Gray16(pixels) => pixels[index] as f32,
        PixelPlane::Gray32(pixels) => pixels[index],
        PixelPlane::ColorRgb(pixels) => {
            let color = pixels[index] & 0x00ff_ffff;
            if exact_pixel_value {
                color as f32
            } else {
                let r = ((color >> 16) & 255) as f64;
                let g = ((color >> 8) & 255) as f64;
                let b = (color & 255) as f64;
                ((r + g + b) / 3.0) as f32
            }
        }
    }
}

fn polygon_contains(points: &[(i32, i32)], x: i32, y: i32) -> bool {
    if points.len() < 3 {
        return false;
    }
    let mut inside = false;
    let mut previous = points.len() - 1;
    for current in 0..points.len() {
        let (xi, yi) = points[current];
        let (xj, yj) = points[previous];
        if (yi > y) != (yj > y)
            && (x as f64) < (xj - xi) as f64 * (y - yi) as f64 / (yj - yi) as f64 + xi as f64
        {
            inside = !inside;
        }
        previous = current;
    }
    inside
}
