use crate::Tool;
use egui::{Color32, Painter, Rect, Shape, Stroke, StrokeKind, Vec2};

/// Default tool outlines from ImageJ 1.54u Toolbar.drawButton (Toolbar.java 356-474).
/// Geometry avoids missing-glyph boxes and platform-specific emoji fonts.
pub(crate) fn paint_tool_icon(
    painter: &Painter,
    rect: Rect,
    tool: Tool,
    ink: Color32,
    foreground: [u8; 3],
    background: [u8; 3],
) {
    let origin = rect.center() - Vec2::splat(9.0);
    let point = |x, y| origin + Vec2::new(x, y);
    let stroke = Stroke::new(1.0_f32, ink);
    let path = |points: &[(f32, f32)]| {
        painter.add(Shape::line(
            points.iter().map(|&(x, y)| point(x, y)).collect(),
            stroke,
        ));
    };
    let line = |a: (f32, f32), b: (f32, f32)| path(&[a, b]);
    let dot = |x, y| {
        painter.rect_filled(Rect::from_min_size(point(x, y), Vec2::splat(2.0)), 0.0, ink);
    };
    match tool {
        Tool::Rectangle => {
            painter.rect_stroke(
                Rect::from_min_size(point(-1.0, 1.0), egui::vec2(17.0, 13.0)),
                0.0,
                stroke,
                StrokeKind::Middle,
            );
        }
        Tool::Oval => {
            painter.add(Shape::ellipse_stroke(
                point(8.5, 7.5),
                egui::vec2(8.5, 6.5),
                stroke,
            ));
        }
        Tool::Polygon => path(&[
            (5., 2.),
            (16., 2.),
            (16., 3.),
            (12., 7.),
            (12., 8.),
            (15., 13.),
            (15., 14.),
            (1., 14.),
            (1., 6.),
            (5., 2.),
        ]),
        Tool::Freehand => path(&[
            (2., 3.),
            (5., 3.),
            (7., 6.),
            (10., 6.),
            (12., 3.),
            (15., 3.),
            (17., 5.),
            (17., 8.),
            (16., 11.),
            (13., 13.),
            (11., 14.),
            (6., 14.),
            (4., 13.),
            (1., 11.),
            (0., 9.),
            (0., 5.),
            (2., 3.),
        ]),
        Tool::Line => {
            line((0., 16.), (17., -1.));
            dot(-1., 15.);
            dot(16., -1.);
        }
        Tool::Polyline => path(&[
            (15., 6.),
            (11., 2.),
            (1., 2.),
            (1., 3.),
            (7., 9.),
            (2., 14.),
        ]),
        Tool::Freeline => path(&[
            (16., 4.),
            (14., 6.),
            (12., 6.),
            (9., 3.),
            (8., 3.),
            (6., 7.),
            (2., 11.),
            (1., 11.),
        ]),
        Tool::Point => {
            path(&[
                (1., 8.),
                (6., 8.),
                (6., 6.),
                (10., 6.),
                (10., 10.),
                (6., 10.),
                (6., 9.),
            ]);
            line((8., 1.), (8., 5.));
            line((11., 8.), (15., 8.));
            line((8., 11.), (8., 15.));
            painter.rect_filled(
                Rect::from_min_size(point(7., 7.), Vec2::splat(3.0)),
                0.0,
                Color32::YELLOW,
            );
        }
        Tool::Wand => {
            line((4., 1.), (6., 3.));
            line((2., 1.), (3., 2.));
            line((2., 3.), (4., 5.));
            line((5., 4.), (17., 16.));
            dot(6., 1.);
            dot(2., 5.);
            painter.line_segment(
                [point(3., 3.), point(5., 3.)],
                Stroke::new(1.0_f32, Color32::YELLOW),
            );
            painter.line_segment(
                [point(4., 2.), point(4., 4.)],
                Stroke::new(1.0_f32, Color32::YELLOW),
            );
        }
        Tool::Text => {
            path(&[(1., 16.), (9., 0.), (16., 16.)]);
            line((0., 16.), (2., 16.));
            line((15., 16.), (17., 16.));
            line((4., 10.), (13., 10.));
        }
        Tool::Magnifier => {
            painter.circle_stroke(point(9.5, 6.5), 6.5, stroke);
            line((5., 12.), (-1., 18.));
        }
        Tool::Hand => path(&[
            (5., 17.),
            (5., 16.),
            (0., 11.),
            (0., 8.),
            (1., 8.),
            (5., 11.),
            (5., 2.),
            (8., 2.),
            (8., 8.),
            (8., 0.),
            (11., 0.),
            (11., 8.),
            (11., 1.),
            (14., 1.),
            (14., 9.),
            (14., 3.),
            (17., 3.),
            (17., 12.),
            (16., 13.),
            (16., 17.),
        ]),
        Tool::Dropper => {
            let swatch = |x, y, w, h, rgb: [u8; 3]| {
                let r = Rect::from_min_size(point(x, y), egui::vec2(w, h));
                painter.rect_filled(r, 0.0, Color32::from_rgb(rgb[0], rgb[1], rgb[2]));
                painter.rect_stroke(r, 0.0, stroke, StrokeKind::Inside);
            };
            swatch(2., 3., 15., 16., background);
            swatch(0., 2., 13., 13., foreground);
            // The eyedropper sits over the foreground/background swatches.
            let tip = [(12., 2.), (5., 9.), (5., 11.), (7., 11.), (14., 4.)];
            painter.add(Shape::convex_polygon(
                tip.iter().map(|&(x, y)| point(x, y)).collect(),
                Color32::GRAY,
                stroke,
            ));
            path(&[(13., 3.), (15., 3.), (15., 5.)]);
            for (a, b) in [
                ((15., -2.), (17., -2.)),
                ((14., -1.), (18., -1.)),
                ((14., 0.), (18., 0.)),
                ((11., 1.), (18., 1.)),
                ((12., 2.), (17., 2.)),
            ] {
                line(a, b);
            }
        }
        Tool::Angle => {
            line((0., 14.), (13., 2.));
            line((0., 14.), (16., 14.));
            path(&[(10., 14.), (10., 11.), (9., 10.), (9., 9.), (8., 8.)]);
            dot(13., 1.);
            dot(16., 13.);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_has_visible_geometry_inside_its_button() {
        let rect = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(28.0, 24.0));
        for &tool in Tool::ALL {
            let ctx = egui::Context::default();
            let output = ctx.run(egui::RawInput::default(), |ctx| {
                let painter = ctx.layer_painter(egui::LayerId::background());
                paint_tool_icon(&painter, rect, tool, Color32::BLACK, [0; 3], [255; 3]);
            });
            assert!(!output.shapes.is_empty(), "{} has no icon", tool.name());
            for shape in output.shapes {
                assert!(
                    rect.contains_rect(shape.shape.visual_bounding_rect()),
                    "{} extends outside its button",
                    tool.name()
                );
                assert!(
                    !matches!(shape.shape, Shape::Text(_)),
                    "{} needs a font",
                    tool.name()
                );
            }
        }
    }
}
