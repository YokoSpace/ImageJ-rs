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
            Self::Rectangle | Self::Oval | Self::Polygon | Self::Line | Self::Magnifier | Self::Hand
        )
    }
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
