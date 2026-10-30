//! Small vector icons drawn with the egui painter (no icon font needed).

use crate::model::Tool;
use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, pos2, vec2};

fn p(r: Rect, x: f32, y: f32) -> Pos2 {
    pos2(r.left() + x * r.width(), r.top() + y * r.height())
}

pub fn tool(painter: &Painter, r: Rect, tool: Tool, color: Color32) {
    let s = Stroke::new(1.6, color);
    match tool {
        Tool::Read => {
            // Mouse pointer.
            let pts = vec![
                p(r, 0.3, 0.12),
                p(r, 0.3, 0.82),
                p(r, 0.47, 0.66),
                p(r, 0.6, 0.92),
                p(r, 0.7, 0.87),
                p(r, 0.57, 0.62),
                p(r, 0.78, 0.6),
            ];
            painter.add(Shape::closed_line(pts, s));
        }
        Tool::Select => {
            // Four-way move arrows.
            let c = r.center();
            let d = r.width() * 0.38;
            for (dx, dy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
                let tip = c + vec2(dx, dy) * d;
                painter.line_segment([c, tip], s);
                let back = c + vec2(dx, dy) * (d - 4.0);
                let side = vec2(-dy, dx) * 3.5;
                painter.line_segment([tip, back + side], s);
                painter.line_segment([tip, back - side], s);
            }
        }
        Tool::Highlight => {
            painter.rect_filled(Rect::from_min_max(p(r, 0.12, 0.62), p(r, 0.88, 0.86)), 2.0, Color32::from_rgb(0xf9, 0xdf, 0x81));
            let pts = vec![p(r, 0.35, 0.62), p(r, 0.66, 0.16), p(r, 0.82, 0.27), p(r, 0.52, 0.72)];
            painter.add(Shape::closed_line(pts, s));
        }
        Tool::Pen => {
            painter.line_segment([p(r, 0.2, 0.8), p(r, 0.72, 0.28)], Stroke::new(2.2, color));
            painter.line_segment([p(r, 0.72, 0.28), p(r, 0.8, 0.2)], Stroke::new(3.0, color));
            painter.line_segment([p(r, 0.14, 0.86), p(r, 0.2, 0.8)], s);
        }
        Tool::Text => {
            painter.line_segment([p(r, 0.2, 0.2), p(r, 0.8, 0.2)], Stroke::new(2.0, color));
            painter.line_segment([p(r, 0.5, 0.2), p(r, 0.5, 0.84)], Stroke::new(2.0, color));
        }
        Tool::Rectangle => {
            painter.rect_stroke(Rect::from_min_max(p(r, 0.16, 0.24), p(r, 0.84, 0.76)), 2.0, s, egui::StrokeKind::Middle);
        }
        Tool::Ellipse => {
            let c = r.center();
            let pts: Vec<Pos2> = (0..40)
                .map(|i| {
                    let a = i as f32 / 40.0 * std::f32::consts::TAU;
                    c + vec2(a.cos() * r.width() * 0.36, a.sin() * r.height() * 0.28)
                })
                .collect();
            painter.add(Shape::closed_line(pts, s));
        }
        Tool::Arrow => {
            let (a, b) = (p(r, 0.2, 0.8), p(r, 0.8, 0.2));
            painter.line_segment([a, b], s);
            painter.line_segment([b, p(r, 0.48, 0.24)], s);
            painter.line_segment([b, p(r, 0.76, 0.52)], s);
        }
        Tool::Eraser => {
            let pts = vec![p(r, 0.12, 0.6), p(r, 0.48, 0.24), p(r, 0.86, 0.6), p(r, 0.62, 0.84), p(r, 0.36, 0.84)];
            painter.add(Shape::closed_line(pts, s));
            painter.line_segment([p(r, 0.3, 0.42), p(r, 0.68, 0.8)], s);
        }
    }
}

pub fn undo(painter: &Painter, r: Rect, color: Color32, redo: bool) {
    let s = Stroke::new(1.7, color);
    let c = r.center();
    let rad = r.width() * 0.28;
    let (from, to) = (200.0f32.to_radians(), 380.0f32.to_radians());
    let pts: Vec<Pos2> = (0..=20)
        .map(|i| {
            let a = from + (to - from) * i as f32 / 20.0;
            let x = if redo { -a.cos() } else { a.cos() };
            c + vec2(x * rad, -a.sin() * rad + 2.0)
        })
        .collect();
    let tip = pts[0];
    painter.add(Shape::line(pts, s));
    let dir = if redo { 1.0 } else { -1.0 };
    painter.line_segment([tip, tip + vec2(dir * -4.0, -4.0)], s);
    painter.line_segment([tip, tip + vec2(dir * 4.5, -1.5)], s);
}

pub fn trash(painter: &Painter, r: Rect, color: Color32) {
    let s = Stroke::new(1.5, color);
    painter.line_segment([p(r, 0.2, 0.28), p(r, 0.8, 0.28)], s);
    painter.line_segment([p(r, 0.4, 0.28), p(r, 0.42, 0.18)], s);
    painter.line_segment([p(r, 0.42, 0.18), p(r, 0.58, 0.18)], s);
    painter.line_segment([p(r, 0.58, 0.18), p(r, 0.6, 0.28)], s);
    let body = vec![p(r, 0.27, 0.3), p(r, 0.32, 0.84), p(r, 0.68, 0.84), p(r, 0.73, 0.3)];
    painter.add(Shape::line(body, s));
}
