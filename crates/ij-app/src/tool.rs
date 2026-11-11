/// ImageJ classic tools matching original_imagej/ij/gui/Toolbar.java
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Rectangle = 0,
    Oval = 1,
    Polygon = 2,
    Freehand = 3,
    Line = 4,
    Polyline = 5,
    Freeline = 6,
    Point = 7,
    Wand = 8,
    Text = 9,
    Magnifier = 11,
    Hand = 12,
    Dropper = 13,
    Angle = 14,
}

impl Tool {
    pub const ALL: &'static [Tool] = &[
        Tool::Rectangle,
        Tool::Oval,
        Tool::Polygon,
        Tool::Freehand,
        Tool::Line,
        Tool::Polyline,
        Tool::Freeline,
        Tool::Point,
        Tool::Wand,
        Tool::Text,
        Tool::Magnifier,
        Tool::Hand,
        Tool::Dropper,
        Tool::Angle,
    ];

    pub fn id(&self) -> usize {
        *self as usize
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Rectangle => "Rectangle",
            Self::Oval => "Oval",
            Self::Polygon => "Polygon",
            Self::Freehand => "Freehand",
            Self::Line => "Straight Line",
            Self::Polyline => "Segmented Line",
            Self::Freeline => "Freehand Line",
            Self::Point => "Point",
            Self::Wand => "Wand (tracing)",
            Self::Text => "Text",
            Self::Magnifier => "Magnifying Glass",
            Self::Hand => "Scrolling Tool (Hand)",
            Self::Dropper => "Color Picker",
            Self::Angle => "Angle",
        }
    }

    pub fn symbol(&self) -> &'static str {
        match self {
            Self::Rectangle => "▭",
            Self::Oval => "⬭",
            Self::Polygon => "⬡",
            Self::Freehand => "✎",
            Self::Line => "╱",
            Self::Polyline => "☡",
            Self::Freeline => "〰",
            Self::Point => "┼",
            Self::Wand => "🪄",
            Self::Text => "A",
            Self::Magnifier => "🔍",
            Self::Hand => "✋",
            Self::Dropper => "💧",
            Self::Angle => "∠",
        }
    }

    pub fn is_supported(&self) -> bool {
        matches!(
            self,
            Self::Rectangle
                | Self::Oval
                | Self::Polygon
                | Self::Freehand
                | Self::Line
                | Self::Point
                | Self::Polyline
                | Self::Freeline
                | Self::Wand
                | Self::Magnifier
                | Self::Hand
                | Self::Angle
        )
    }
}

/// Appends one freehand vertex using ImageJ's integer construction rules.
///
/// `FreehandRoi.grow` clamps the offscreen point to `[0, width]`/`[0, height]`
/// (`Roi.setImage` 322-323) and appends it only when it differs from the
/// previous vertex (FreehandRoi.java 24-33), so consecutive duplicate pixels
/// never enter the polygon.
pub fn freehand_push(points: &mut Vec<(i32, i32)>, point: (i32, i32), img_w: u32, img_h: u32) {
    let ox = point.0.clamp(0, img_w as i32);
    let oy = point.1.clamp(0, img_h as i32);
    if points.last().copied() != Some((ox, oy)) {
        points.push((ox, oy));
    }
}

/// True when `finishPolygon` would keep a FreehandRoi.
///
/// `PolygonRoi.finishPolygon` deletes the selection when `nPoints<3` or when
/// the polygon bounds have zero width/height (PolygonRoi.java 591-593).
pub fn freehand_is_valid(points: &[(i32, i32)]) -> bool {
    if points.len() < 3 {
        return false;
    }
    let (mut min_x, mut max_x) = (i32::MAX, i32::MIN);
    let (mut min_y, mut max_y) = (i32::MAX, i32::MIN);
    for &(x, y) in points {
        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_y = min_y.min(y);
        max_y = max_y.max(y);
    }
    max_x > min_x && max_y > min_y
}

/// Open polyline segments for a polygon that is still being constructed.
///
/// ImageJ paints the in-progress polygon as an open polyline through the
/// committed vertices plus the cursor position (PolygonRoi.paint, which uses
/// `g.drawPolyline` while `state==CONSTRUCTING`, PolygonRoi.java 228).
pub fn polygon_preview_segments(
    vertices: &[(i32, i32)],
    cursor: Option<(i32, i32)>,
) -> Vec<((i32, i32), (i32, i32))> {
    let mut segments: Vec<((i32, i32), (i32, i32))> = vertices
        .windows(2)
        .map(|pair| (pair[0], pair[1]))
        .collect();
    if let (Some(&last), Some(current)) = (vertices.last(), cursor) {
        if current != last {
            segments.push((last, current));
        }
    }
    segments
}

/// True when `finishPolygon` would keep a POLYLINE or FREELINE.
///
/// Open line selections are deleted when `nPoints < 2` only
/// (PolygonRoi.java 591-593); unlike freehand areas they survive a zero
/// width or height because both bounds axes are inflated by one
/// (PolygonRoi.java 571-573).
pub fn polyline_is_valid(points: &[(i32, i32)]) -> bool {
    points.len() >= 2
}

/// True when a click closes a polygon by landing in the start box.
///
/// `handleMouseUp` accepts a 10x10 *screen* pixel box centred on the start
/// point (`new Rectangle(screenXD(startXD)-5, screenYD(startYD)-5, 10, 10)`,
/// PolygonRoi.java 1100) and `Rectangle.contains` is half-open, so the box
/// covers `[-5, 5)` screen pixels around the start *regardless of zoom*.
pub fn polygon_closes_at_start(
    start_pixel: (i32, i32),
    pointer_rel: (f32, f32),
    zoom: f32,
) -> bool {
    if zoom <= 0.0 {
        return false;
    }
    let dx = pointer_rel.0 - start_pixel.0 as f32 * zoom;
    let dy = pointer_rel.1 - start_pixel.1 as f32 * zoom;
    dx >= -5.0 && dx < 5.0 && dy >= -5.0 && dy < 5.0
}
