//! Tool icons drawn with vector shapes (original artwork, no icon font needed).

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Paint,
    Eraser,
    Projection,
    PolygonFill,
    Smudge,
    Clone,
    ColorPicker,
}

impl Tool {
    pub const ALL: [Tool; 7] = [
        Tool::Paint,
        Tool::Eraser,
        Tool::Projection,
        Tool::PolygonFill,
        Tool::Smudge,
        Tool::Clone,
        Tool::ColorPicker,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Tool::Paint => "Paint",
            Tool::Eraser => "Eraser",
            Tool::Projection => "Projection",
            Tool::PolygonFill => "Polygon fill",
            Tool::Smudge => "Smudge",
            Tool::Clone => "Clone",
            Tool::ColorPicker => "Color picker",
        }
    }

    pub fn shortcut(self) -> &'static str {
        match self {
            Tool::Paint => "1",
            Tool::Eraser => "2",
            Tool::Projection => "3",
            Tool::PolygonFill => "4",
            Tool::Smudge => "5",
            Tool::Clone => "6",
            Tool::ColorPicker => "P",
        }
    }

    /// Milestone that delivers the tool.
    pub fn milestone(self) -> &'static str {
        match self {
            Tool::Smudge | Tool::Clone => "M8",
            _ => "M4",
        }
    }
}

/// Draws `tool` centered in `rect` (expects roughly 20–30 px squares).
pub fn paint(painter: &Painter, rect: Rect, tool: Tool, color: Color32) {
    let c = rect.center();
    let s = rect.width().min(rect.height()) / 24.0;
    // Points in a 24x24 design grid centered on the rect.
    let p = |x: f32, y: f32| -> Pos2 { c + vec2((x - 12.0) * s, (y - 12.0) * s) };
    let stroke = Stroke::new(1.6 * s.max(0.8), color);

    match tool {
        Tool::Paint => {
            // Handle, ferrule and a round bristle tip.
            painter.line_segment([p(19.0, 5.0), p(11.0, 13.0)], Stroke::new(2.2 * s, color));
            painter.add(Shape::convex_polygon(
                vec![p(11.5, 11.0), p(13.0, 12.5), p(9.5, 16.5), p(7.5, 14.5)],
                color,
                Stroke::NONE,
            ));
            painter.circle_filled(p(6.5, 17.5), 2.6 * s, color);
        }
        Tool::Eraser => {
            let body = [p(4.0, 15.0), p(13.0, 6.0), p(20.0, 13.0), p(11.0, 22.0)];
            painter.add(Shape::closed_line(body.to_vec(), stroke));
            painter.add(Shape::convex_polygon(
                vec![p(4.0, 15.0), p(8.5, 10.5), p(15.5, 17.5), p(11.0, 22.0)],
                color,
                Stroke::NONE,
            ));
            painter.line_segment([p(11.0, 22.0), p(20.0, 22.0)], stroke);
        }
        Tool::Projection => {
            // A frame projected from a point.
            let frame = Rect::from_min_max(p(10.0, 5.0), p(21.0, 19.0));
            painter.rect_stroke(frame, 1.0, stroke, egui::StrokeKind::Middle);
            let eye = p(3.5, 12.0);
            for corner in [frame.left_top(), frame.left_bottom()] {
                painter.line_segment([eye, corner], stroke);
            }
            painter.circle_filled(eye, 1.8 * s, color);
            painter.circle_filled(frame.center(), 2.2 * s, color);
        }
        Tool::PolygonFill => {
            let tri = vec![p(12.0, 4.0), p(21.0, 19.0), p(3.0, 19.0)];
            painter.add(Shape::convex_polygon(
                tri,
                color.gamma_multiply(0.45),
                stroke,
            ));
            for v in [p(12.0, 4.0), p(21.0, 19.0), p(3.0, 19.0)] {
                painter.circle_filled(v, 1.8 * s, color);
            }
        }
        Tool::Smudge => {
            // Finger-like stroke: a wave that thins out.
            let pts: Vec<Pos2> = (0..=16)
                .map(|i| {
                    let t = i as f32 / 16.0;
                    p(
                        3.0 + t * 18.0,
                        13.0 + (t * std::f32::consts::TAU).sin() * 4.0 * (1.0 - t * 0.6),
                    )
                })
                .collect();
            painter.add(Shape::line(pts, Stroke::new(2.6 * s, color)));
            painter.circle_filled(p(3.0, 13.0), 2.4 * s, color);
        }
        Tool::Clone => {
            painter.circle_stroke(p(9.0, 10.0), 5.5 * s, stroke);
            painter.circle_filled(p(15.0, 15.0), 5.5 * s, color);
            painter.circle_stroke(p(15.0, 15.0), 5.5 * s, stroke);
        }
        Tool::ColorPicker => {
            // Pipette with a drop.
            painter.line_segment([p(17.0, 7.0), p(7.0, 17.0)], Stroke::new(2.2 * s, color));
            painter.circle_filled(p(18.0, 6.0), 3.0 * s, color);
            painter.add(Shape::convex_polygon(
                vec![p(5.0, 17.5), p(6.5, 19.0), p(4.5, 21.5), p(3.0, 20.0)],
                color,
                Stroke::NONE,
            ));
        }
    }
}

/// Size of a tool button in the tools bar.
pub const BUTTON: egui::Vec2 = vec2(30.0, 30.0);

/// Allocates a tool button. Tools are inert until the brush engine lands.
pub fn tool_button(ui: &mut egui::Ui, tool: Tool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(BUTTON, egui::Sense::hover());
    let visuals = ui.visuals();
    let (bg, fg) = if response.hovered() {
        (
            visuals.widgets.hovered.weak_bg_fill,
            Color32::from_gray(170),
        )
    } else {
        (Color32::TRANSPARENT, Color32::from_gray(120))
    };
    ui.painter().rect_filled(rect, 4.0, bg);
    paint(ui.painter(), rect.shrink(4.0), tool, fg);
    response.on_hover_text(format!(
        "{} ({}) — arrives in {}",
        tool.label(),
        tool.shortcut(),
        tool.milestone()
    ))
}
