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
                // Polygon is nonfunctional until click/drag vertex creation is implemented
                | Self::Line
                | Self::Magnifier
                | Self::Hand
        )
    }
}
