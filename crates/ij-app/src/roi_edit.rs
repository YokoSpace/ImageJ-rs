use ij_core::{AngleRoi, LineRoi, PointRoi, PolygonRoi, PolylineRoi, RectRoi, Roi};

/// Result of hit testing a ROI with screen coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoiHit {
    /// Over a handle (0..=7 for Rect/Oval, 0..=2 for Line).
    Handle(u8),
    /// Inside the selection area.
    Inside,
    /// Outside the selection.
    Outside,
}

/// Arrow keys for nudging ROIs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arrow {
    Left,
    Right,
    Up,
    Down,
}

/// Hit-tests a ROI given screen coordinates, magnification, and viewport source origin.
///
/// Follows ImageCanvas.java:1375-1432, Roi.java:1567-1591, OvalRoi.java:377-401,
/// and Line.java:585-598.
pub fn hit_test(roi: &Roi, screen: (f64, f64), mag: f64, src_origin: (f64, f64)) -> RoiHit {
    let sx = screen.0.round() as i32;
    let sy = screen.1.round() as i32;

    match roi {
        Roi::Rect(r) => {
            // Roi.java lines 1567-1591:
            // margin = 5 for screen width > 1280 (standard desktop), getHandleSize() = 7, size = 12, halfSize = 6
            let margin = 5;
            let size = 7 + margin;
            let half_size = size / 2;

            let sx1 = screen_x(r.x as f64, mag, src_origin.0) - half_size;
            let sy1 = screen_y(r.y as f64, mag, src_origin.1) - half_size;
            let sx3 = screen_x((r.x + r.width) as f64, mag, src_origin.0) - half_size;
            let sy3 = screen_y((r.y + r.height) as f64, mag, src_origin.1) - half_size;
            let sx2 = sx1 + (sx3 - sx1) / 2;
            let sy2 = sy1 + (sy3 - sy1) / 2;

            if sx >= sx1 && sx <= sx1 + size && sy >= sy1 && sy <= sy1 + size {
                return RoiHit::Handle(0);
            }
            if sx >= sx2 && sx <= sx2 + size && sy >= sy1 && sy <= sy1 + size {
                return RoiHit::Handle(1);
            }
            if sx >= sx3 && sx <= sx3 + size && sy >= sy1 && sy <= sy1 + size {
                return RoiHit::Handle(2);
            }
            if sx >= sx3 && sx <= sx3 + size && sy >= sy2 && sy <= sy2 + size {
                return RoiHit::Handle(3);
            }
            if sx >= sx3 && sx <= sx3 + size && sy >= sy3 && sy <= sy3 + size {
                return RoiHit::Handle(4);
            }
            if sx >= sx2 && sx <= sx2 + size && sy >= sy3 && sy <= sy3 + size {
                return RoiHit::Handle(5);
            }
            if sx >= sx1 && sx <= sx1 + size && sy >= sy3 && sy <= sy3 + size {
                return RoiHit::Handle(6);
            }
            if sx >= sx1 && sx <= sx1 + size && sy >= sy2 && sy <= sy2 + size {
                return RoiHit::Handle(7);
            }

            // Inside test (Roi.java:1532):
            let ox = off_screen_x(sx, mag, src_origin.0);
            let oy = off_screen_y(sy, mag, src_origin.1);
            if ox >= r.x && ox < r.x + r.width && oy >= r.y && oy < r.y + r.height {
                RoiHit::Inside
            } else {
                RoiHit::Outside
            }
        }
        Roi::Oval(r) => {
            // OvalRoi.java lines 377-401:
            let size = 7 + 3; // getHandleSize() + 3 = 10
            let half_size = size / 2;

            let sx1 = screen_x(r.x as f64, mag, src_origin.0) - half_size;
            let sy1 = screen_y(r.y as f64, mag, src_origin.1) - half_size;
            let sx3 = screen_x((r.x + r.width) as f64, mag, src_origin.0) - half_size;
            let sy3 = screen_y((r.y + r.height) as f64, mag, src_origin.1) - half_size;
            let sx2 = sx1 + (sx3 - sx1) / 2;
            let sy2 = sy1 + (sy3 - sy1) / 2;

            let sw2 = (0.14645 * (sx3 - sx1) as f64) as i32;
            let sh2 = (0.14645 * (sy3 - sy1) as f64) as i32;

            if sx >= sx1 + sw2 && sx <= sx1 + sw2 + size && sy >= sy1 + sh2 && sy <= sy1 + sh2 + size {
                return RoiHit::Handle(0);
            }
            if sx >= sx2 && sx <= sx2 + size && sy >= sy1 && sy <= sy1 + size {
                return RoiHit::Handle(1);
            }
            if sx >= sx3 - sw2 && sx <= sx3 - sw2 + size && sy >= sy1 + sh2 && sy <= sy1 + sh2 + size {
                return RoiHit::Handle(2);
            }
            if sx >= sx3 && sx <= sx3 + size && sy >= sy2 && sy <= sy2 + size {
                return RoiHit::Handle(3);
            }
            if sx >= sx3 - sw2 && sx <= sx3 - sw2 + size && sy >= sy3 - sh2 && sy <= sy3 - sh2 + size {
                return RoiHit::Handle(4);
            }
            if sx >= sx2 && sx <= sx2 + size && sy >= sy3 && sy <= sy3 + size {
                return RoiHit::Handle(5);
            }
            if sx >= sx1 + sw2 && sx <= sx1 + sw2 + size && sy >= sy3 - sh2 && sy <= sy3 - sh2 + size {
                return RoiHit::Handle(6);
            }
            if sx >= sx1 && sx <= sx1 + size && sy >= sy2 && sy <= sy2 + size {
                return RoiHit::Handle(7);
            }

            // Inside test (OvalRoi.java:369 - Ellipse2D.contains):
            let ox = off_screen_x(sx, mag, src_origin.0);
            let oy = off_screen_y(sy, mag, src_origin.1);
            if r.width > 0 && r.height > 0 {
                // OvalRoi.contains tests pixel centers against a center
                // shifted by half a pixel (OvalRoi.java:355-364).
                let norm_x = (ox as f64 - r.x as f64 + 0.5) / r.width as f64 - 0.5;
                let norm_y = (oy as f64 - r.y as f64 + 0.5) / r.height as f64 - 0.5;
                if norm_x * norm_x + norm_y * norm_y <= 0.25 {
                    return RoiHit::Inside;
                }
            }
            RoiHit::Outside
        }
        Roi::Line(l) => {
            // Line.java lines 585-598:
            let size = 5 + 5; // HANDLE_SIZE + 5 = 10
            let half_size = size / 2;

            // Roi.screenXD/YD adds 0.5 for line coordinates before the
            // ImageCanvas transform (Roi.java:2519-2530).
            let sx1 = screen_x(l.x1 as f64 + 0.5, mag, src_origin.0) - half_size;
            let sy1 = screen_y(l.y1 as f64 + 0.5, mag, src_origin.1) - half_size;
            let sx2 = screen_x(l.x2 as f64 + 0.5, mag, src_origin.0) - half_size;
            let sy2 = screen_y(l.y2 as f64 + 0.5, mag, src_origin.1) - half_size;
            let sx3 = sx1 + (sx2 - sx1) / 2 - 1;
            let sy3 = sy1 + (sy2 - sy1) / 2 - 1;

            if sx >= sx1 && sx <= sx1 + size && sy >= sy1 && sy <= sy1 + size {
                return RoiHit::Handle(0);
            }
            if sx >= sx2 && sx <= sx2 + size && sy >= sy2 && sy <= sy2 + size {
                return RoiHit::Handle(1);
            }
            if sx >= sx3 && sx <= sx3 + size + 2 && sy >= sy3 && sy <= sy3 + size + 2 {
                return RoiHit::Handle(2);
            }

            // Line contains is false for 1-pixel line (Line.java:567)
            RoiHit::Outside
        }
        Roi::Polygon(p) => {
            let ox = off_screen_x(sx, mag, src_origin.0);
            let oy = off_screen_y(sy, mag, src_origin.1);
            let (bx, by, bw, bh) = p.bounds();
            if ox >= bx && ox < bx + bw && oy >= by && oy < by + bh {
                if point_in_polygon(ox, oy, &p.xpoints, &p.ypoints) {
                    return RoiHit::Inside;
                }
            }
            RoiHit::Outside
        }
        Roi::Point(p) => p.points.iter().enumerate().find_map(|(index, &(x, y))| {
            let px = screen_x(x as f64, mag, src_origin.0);
            let py = screen_y(y as f64, mag, src_origin.1);
            (index < u8::MAX as usize && (sx - px).abs() <= 5 && (sy - py).abs() <= 5)
                .then_some(RoiHit::Handle(index as u8))
        }).unwrap_or(RoiHit::Outside),
        Roi::Angle(a) => {
            for (index, &(x, y)) in a.points.iter().enumerate() {
                let px = screen_x(x as f64, mag, src_origin.0);
                let py = screen_y(y as f64, mag, src_origin.1);
                if (sx - px).abs() <= 5 && (sy - py).abs() <= 5 {
                    return RoiHit::Handle(index as u8);
                }
            }
            RoiHit::Outside
        }
        // Line selections contain nothing (Roi.contains returns false for
        // open-line types), and the Rust UI has no vertex-handle dragging
        // for polylines yet (PolygonRoi.isHandle/moveHandle).
        Roi::Polyline(_) => RoiHit::Outside,
    }
}

/// Translates a ROI by integer displacement (dx, dy).
pub fn move_by(roi: &Roi, dx: i32, dy: i32) -> Roi {
    match roi {
        Roi::Rect(r) => Roi::Rect(RectRoi::new(r.x + dx, r.y + dy, r.width, r.height).unwrap()),
        Roi::Oval(r) => Roi::Oval(RectRoi::new(r.x + dx, r.y + dy, r.width, r.height).unwrap()),
        Roi::Line(l) => Roi::Line(LineRoi {
            x1: l.x1 + dx,
            y1: l.y1 + dy,
            x2: l.x2 + dx,
            y2: l.y2 + dy,
        }),
        Roi::Polygon(p) => Roi::Polygon(PolygonRoi {
            xpoints: p.xpoints.iter().map(|&x| x + dx).collect(),
            ypoints: p.ypoints.iter().map(|&y| y + dy).collect(),
        }),
        Roi::Point(p) => Roi::Point(PointRoi {
            points: p.points.iter().map(|&(x, y)| (x + dx, y + dy)).collect(),
        }),
        Roi::Angle(a) => Roi::Angle(AngleRoi {
            points: a.points.map(|(x, y)| (x + dx, y + dy)),
        }),
        Roi::Polyline(p) => Roi::Polyline(PolylineRoi {
            points: p
                .points
                .iter()
                .map(|&(x, y)| (x + dx, y + dy))
                .collect(),
            freehand: p.freehand,
        }),
    }
}

/// Drags a handle to a new off-screen image coordinate (ox, oy).
///
/// Follows Roi.java:950-998 (Rect), OvalRoi.java:70-103 (Oval), and Line.java:177-235 (Line).
pub fn drag_handle(roi: &Roi, handle: u8, image_xy: (i32, i32)) -> Roi {
    let (ox, oy) = image_xy;
    match roi {
        Roi::Rect(r) => {
            let mut x = r.x;
            let mut y = r.y;
            let mut x2 = r.x + r.width;
            let mut y2 = r.y + r.height;

            match handle {
                0 => {
                    x = ox;
                    y = oy;
                }
                1 => {
                    y = oy;
                }
                2 => {
                    x2 = ox;
                    y = oy;
                }
                3 => {
                    x2 = ox;
                }
                4 => {
                    x2 = ox;
                    y2 = oy;
                }
                5 => {
                    y2 = oy;
                }
                6 => {
                    x = ox;
                    y2 = oy;
                }
                7 => {
                    x = ox;
                }
                _ => return roi.clone(),
            }

            // Roi.java lines 990-997:
            // if (x<x2) width=x2-x else {width=1; x=x2;}
            // if (y<y2) height=y2-y else {height=1; y=y2;}
            let (final_x, final_w) = if x < x2 { (x, x2 - x) } else { (x2, 1) };
            let (final_y, final_h) = if y < y2 { (y, y2 - y) } else { (y2, 1) };
            Roi::Rect(RectRoi::new(final_x, final_y, final_w, final_h).unwrap())
        }
        Roi::Oval(r) => {
            let mut x = r.x;
            let mut y = r.y;
            let mut x2 = r.x + r.width;
            let mut y2 = r.y + r.height;

            // OvalRoi.java lines 77-78:
            let w2 = (0.14645 * r.width as f64) as i32;
            let h2 = (0.14645 * r.height as f64) as i32;

            match handle {
                0 => {
                    x = ox - w2;
                    y = oy - h2;
                }
                1 => {
                    y = oy;
                }
                2 => {
                    x2 = ox + w2;
                    y = oy - h2;
                }
                3 => {
                    x2 = ox;
                }
                4 => {
                    x2 = ox + w2;
                    y2 = oy + h2;
                }
                5 => {
                    y2 = oy;
                }
                6 => {
                    x = ox - w2;
                    y2 = oy + h2;
                }
                7 => {
                    x = ox;
                }
                _ => return roi.clone(),
            }

            // OvalRoi.java lines 95-102:
            let (final_x, final_w) = if x < x2 { (x, x2 - x) } else { (x2, 1) };
            let (final_y, final_h) = if y < y2 { (y, y2 - y) } else { (y2, 1) };
            Roi::Oval(RectRoi::new(final_x, final_y, final_w, final_h).unwrap())
        }
        Roi::Line(l) => match handle {
            0 => Roi::Line(LineRoi {
                x1: ox,
                y1: oy,
                x2: l.x2,
                y2: l.y2,
            }),
            1 => Roi::Line(LineRoi {
                x1: l.x1,
                y1: l.y1,
                x2: ox,
                y2: oy,
            }),
            2 => {
                // Line.java lines 230-234:
                // dx = ox - (x1d + (x2d - x1d)/2);
                // dy = oy - (y1d + (y2d - y1d)/2);
                let center_x = l.x1 + (l.x2 - l.x1) / 2;
                let center_y = l.y1 + (l.y2 - l.y1) / 2;
                let dx = ox - center_x;
                let dy = oy - center_y;
                Roi::Line(LineRoi {
                    x1: l.x1 + dx,
                    y1: l.y1 + dy,
                    x2: l.x2 + dx,
                    y2: l.y2 + dy,
                })
            }
            _ => roi.clone(),
        },
        Roi::Polygon(_) => roi.clone(),
        Roi::Point(p) => {
            let mut next = p.clone();
            if let Some(point) = next.points.get_mut(handle as usize) {
                *point = (ox, oy);
            }
            Roi::Point(next)
        }
        Roi::Angle(a) => {
            let mut next = *a;
            if let Some(point) = next.points.get_mut(handle as usize) {
                *point = (ox, oy);
            }
            Roi::Angle(next)
        }
        // PolygonRoi.moveHandle drags individual vertices; not ported yet.
        Roi::Polyline(_) => roi.clone(),
    }
}

/// Nudges the whole ROI one pixel in direction `a`, clamped to the image boundary.
///
/// Follows Roi.java:1191-1217.
pub fn nudge(roi: &Roi, a: Arrow, img_w: u32, img_h: u32) -> Roi {
    let x_max = img_w as i32;
    let y_max = img_h as i32;

    match roi {
        Roi::Rect(r) => {
            let (mut x, mut y) = (r.x, r.y);
            let (w, h) = (r.width, r.height);
            match a {
                Arrow::Up => {
                    y -= 1;
                    if y < 0 {
                        y = 0;
                    }
                }
                Arrow::Down => {
                    y += 1;
                    if (y + h) >= y_max {
                        y = y_max - h;
                    }
                }
                Arrow::Left => {
                    x -= 1;
                    if x < 0 {
                        x = 0;
                    }
                }
                Arrow::Right => {
                    x += 1;
                    if (x + w) >= x_max {
                        x = x_max - w;
                    }
                }
            }
            Roi::Rect(RectRoi::new(x, y, w, h).unwrap())
        }
        Roi::Oval(r) => {
            let (mut x, mut y) = (r.x, r.y);
            let (w, h) = (r.width, r.height);
            match a {
                Arrow::Up => {
                    y -= 1;
                    if y < 0 {
                        y = 0;
                    }
                }
                Arrow::Down => {
                    y += 1;
                    if (y + h) >= y_max {
                        y = y_max - h;
                    }
                }
                Arrow::Left => {
                    x -= 1;
                    if x < 0 {
                        x = 0;
                    }
                }
                Arrow::Right => {
                    x += 1;
                    if (x + w) >= x_max {
                        x = x_max - w;
                    }
                }
            }
            Roi::Oval(RectRoi::new(x, y, w, h).unwrap())
        }
        Roi::Line(l) => {
            let (min_x, min_y, w, h) = l.bounds();
            let mut bx = min_x;
            let mut by = min_y;
            match a {
                Arrow::Up => {
                    by -= 1;
                    if by < 0 {
                        by = 0;
                    }
                }
                Arrow::Down => {
                    by += 1;
                    if (by + h) >= y_max {
                        by = y_max - h;
                    }
                }
                Arrow::Left => {
                    bx -= 1;
                    if bx < 0 {
                        bx = 0;
                    }
                }
                Arrow::Right => {
                    bx += 1;
                    if (bx + w) >= x_max {
                        bx = x_max - w;
                    }
                }
            }
            let dx = bx - min_x;
            let dy = by - min_y;
            Roi::Line(LineRoi {
                x1: l.x1 + dx,
                y1: l.y1 + dy,
                x2: l.x2 + dx,
                y2: l.y2 + dy,
            })
        }
        Roi::Polygon(p) => {
            let (min_x, min_y, w, h) = p.bounds();
            let mut bx = min_x;
            let mut by = min_y;
            match a {
                Arrow::Up => {
                    by -= 1;
                    if by < 0 {
                        by = 0;
                    }
                }
                Arrow::Down => {
                    by += 1;
                    if (by + h) >= y_max {
                        by = y_max - h;
                    }
                }
                Arrow::Left => {
                    bx -= 1;
                    if bx < 0 {
                        bx = 0;
                    }
                }
                Arrow::Right => {
                    bx += 1;
                    if (bx + w) >= x_max {
                        bx = x_max - w;
                    }
                }
            }
            let dx = bx - min_x;
            let dy = by - min_y;
            Roi::Polygon(PolygonRoi {
                xpoints: p.xpoints.iter().map(|&x| x + dx).collect(),
                ypoints: p.ypoints.iter().map(|&y| y + dy).collect(),
            })
        }
        Roi::Point(_) | Roi::Angle(_) => {
            let (x, y, w, h) = roi.bounds();
            let (dx, dy) = match a {
                Arrow::Up => (0, -1),
                Arrow::Down => (0, 1),
                Arrow::Left => (-1, 0),
                Arrow::Right => (1, 0),
            };
            let next_x = (x + dx).clamp(0, (x_max - w).max(0));
            let next_y = (y + dy).clamp(0, (y_max - h).max(0));
            let mut next = roi.clone();
            next.set_location(next_x, next_y);
            next
        }
        Roi::Polyline(p) => {
            let (min_x, min_y, w, h) = p.bounds();
            let mut bx = min_x;
            let mut by = min_y;
            match a {
                Arrow::Up => {
                    by -= 1;
                    if by < 0 {
                        by = 0;
                    }
                }
                Arrow::Down => {
                    by += 1;
                    if (by + h) >= y_max {
                        by = y_max - h;
                    }
                }
                Arrow::Left => {
                    bx -= 1;
                    if bx < 0 {
                        bx = 0;
                    }
                }
                Arrow::Right => {
                    bx += 1;
                    if (bx + w) >= x_max {
                        bx = x_max - w;
                    }
                }
            }
            let dx = bx - min_x;
            let dy = by - min_y;
            Roi::Polyline(PolylineRoi {
                points: p
                    .points
                    .iter()
                    .map(|&(x, y)| (x + dx, y + dy))
                    .collect(),
                freehand: p.freehand,
            })
        }
    }
}

/// Nudges the lower-right corner (or endpoint 2 of a line) by one pixel.
///
/// Follows Roi.java:1232-1256 (Rect/Oval) and Line.java:640-652.
pub fn nudge_corner(roi: &Roi, a: Arrow, img_w: u32, img_h: u32) -> Roi {
    let x_max = img_w as i32;
    let y_max = img_h as i32;

    match roi {
        Roi::Rect(r) => {
            let mut w = r.width;
            let mut h = r.height;
            match a {
                Arrow::Up => {
                    h -= 1;
                    if h < 1 {
                        h = 1;
                    }
                }
                Arrow::Down => {
                    h += 1;
                    if (r.y + h) > y_max {
                        h = y_max - r.y;
                    }
                }
                Arrow::Left => {
                    w -= 1;
                    if w < 1 {
                        w = 1;
                    }
                }
                Arrow::Right => {
                    w += 1;
                    if (r.x + w) > x_max {
                        w = x_max - r.x;
                    }
                }
            }
            Roi::Rect(RectRoi::new(r.x, r.y, w, h).unwrap())
        }
        Roi::Oval(r) => {
            let mut w = r.width;
            let mut h = r.height;
            match a {
                Arrow::Up => {
                    h -= 1;
                    if h < 1 {
                        h = 1;
                    }
                }
                Arrow::Down => {
                    h += 1;
                    if (r.y + h) > y_max {
                        h = y_max - r.y;
                    }
                }
                Arrow::Left => {
                    w -= 1;
                    if w < 1 {
                        w = 1;
                    }
                }
                Arrow::Right => {
                    w += 1;
                    if (r.x + w) > x_max {
                        w = x_max - r.x;
                    }
                }
            }
            Roi::Oval(RectRoi::new(r.x, r.y, w, h).unwrap())
        }
        Roi::Line(l) => {
            // Line.java lines 640-652 and 104-105:
            // Moves endpoint 2 (x2, y2) by one pixel, clamped to [0, xMax-1] / [0, yMax-1]
            let x2_max = img_w.saturating_sub(1) as i32;
            let y2_max = img_h.saturating_sub(1) as i32;
            let mut x2 = l.x2;
            let mut y2 = l.y2;
            match a {
                Arrow::Up => {
                    y2 -= 1;
                    if y2 < 0 {
                        y2 = 0;
                    }
                }
                Arrow::Down => {
                    y2 += 1;
                    if y2 > y2_max {
                        y2 = y2_max;
                    }
                }
                Arrow::Left => {
                    x2 -= 1;
                    if x2 < 0 {
                        x2 = 0;
                    }
                }
                Arrow::Right => {
                    x2 += 1;
                    if x2 > x2_max {
                        x2 = x2_max;
                    }
                }
            }
            Roi::Line(LineRoi {
                x1: l.x1,
                y1: l.y1,
                x2,
                y2,
            })
        }
        Roi::Polygon(_) | Roi::Point(_) | Roi::Angle(_) => {
            // PolygonRoi is type > OVAL, so Roi.java line 1233 returns early without changes.
            roi.clone()
        }
        Roi::Polyline(_) => {
            // Open lines are PolygonRoi types too: the same Roi.java line
            // 1233 early return leaves them unchanged.
            roi.clone()
        }
    }
}

#[inline]
fn screen_x(ox: f64, mag: f64, src_x: f64) -> i32 {
    ((ox - src_x) * mag) as i32
}

#[inline]
fn screen_y(oy: f64, mag: f64, src_y: f64) -> i32 {
    ((oy - src_y) * mag) as i32
}

#[inline]
fn off_screen_x(sx: i32, mag: f64, src_x: f64) -> i32 {
    src_x as i32 + (sx as f64 / mag) as i32
}

#[inline]
fn off_screen_y(sy: i32, mag: f64, src_y: f64) -> i32 {
    src_y as i32 + (sy as f64 / mag) as i32
}

fn point_in_polygon(x: i32, y: i32, xp: &[i32], yp: &[i32]) -> bool {
    let mut inside = false;
    let n = xp.len();
    if n < 3 || yp.len() != n {
        return false;
    }
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = (xp[i], yp[i]);
        let (xj, yj) = (xp[j], yp[j]);
        if ((yi > y) != (yj > y)) && (x < (xj - xi) * (y - yi) / (yj - yi) + xi) {
            inside = !inside;
        }
        j = i;
    }
    inside
}
