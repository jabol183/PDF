//! Annotation model, geometry helpers, undo history and project validation.
//!
//! All annotation coordinates are "viewport units": the page as displayed at
//! 100% zoom, origin top-left, y pointing down, with the page's crop box,
//! rotation and UserUnit already applied. This matches the coordinates used by
//! the PDFMark browser extension, so `.pdfmark` projects work in both.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const MAX_PDF_BYTES: usize = 100 * 1024 * 1024;
pub const MAX_PROJECT_BYTES: usize = 150 * 1024 * 1024;
pub const MAX_PAGES: usize = 3000;
pub const MAX_MARKS: usize = 20_000;
pub const HISTORY_LIMIT: usize = 80;
pub const LINE_HEIGHT: f64 = 1.3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tool {
    Read,
    Select,
    Highlight,
    Pen,
    Text,
    Rectangle,
    Ellipse,
    Arrow,
    Eraser,
}

impl Tool {
    pub const ALL: [Tool; 9] =
        [Tool::Read, Tool::Select, Tool::Highlight, Tool::Pen, Tool::Text, Tool::Rectangle, Tool::Ellipse, Tool::Arrow, Tool::Eraser];

    pub fn label(self) -> &'static str {
        match self {
            Tool::Read => "Read",
            Tool::Select => "Move",
            Tool::Highlight => "Highlight",
            Tool::Pen => "Pen",
            Tool::Text => "Text",
            Tool::Rectangle => "Box",
            Tool::Ellipse => "Circle",
            Tool::Arrow => "Arrow",
            Tool::Eraser => "Eraser",
        }
    }

    pub fn shortcut(self) -> char {
        match self {
            Tool::Read => 'V',
            Tool::Select => 'M',
            Tool::Highlight => 'H',
            Tool::Pen => 'P',
            Tool::Text => 'T',
            Tool::Rectangle => 'R',
            Tool::Ellipse => 'O',
            Tool::Arrow => 'A',
            Tool::Eraser => 'E',
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Tool::Read => "Select original text to copy or highlight. Fill form fields directly.",
            Tool::Select => "Drag an added mark to move it. Double-click text to edit. Delete to remove.",
            Tool::Highlight => "Drag over an area to highlight it.",
            Tool::Pen => "Draw with your mouse, pen, or finger.",
            Tool::Text => "Click the page to add typed text.",
            Tool::Rectangle => "Drag to outline an area.",
            Tool::Ellipse => "Drag to circle an area.",
            Tool::Arrow => "Drag from the tail to the tip of your arrow.",
            Tool::Eraser => "Click an added annotation to remove the whole mark.",
        }
    }

    pub fn default_color(self) -> Option<[u8; 3]> {
        match self {
            Tool::Highlight => Some([0xed, 0xc9, 0x49]),
            Tool::Pen => Some([0x2f, 0x61, 0x9c]),
            Tool::Text => Some([0x25, 0x33, 0x4b]),
            Tool::Rectangle | Tool::Ellipse | Tool::Arrow => Some([0xc9, 0x5b, 0x50]),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pt {
    pub x: f64,
    pub y: f64,
}

impl Pt {
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn between(a: Pt, b: Pt) -> Self {
        Rect { x: a.x.min(b.x), y: a.y.min(b.y), w: (a.x - b.x).abs(), h: (a.y - b.y).abs() }
    }
    pub fn right(&self) -> f64 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f64 {
        self.y + self.h
    }
    pub fn union(&self, o: &Rect) -> Rect {
        let x = self.x.min(o.x);
        let y = self.y.min(o.y);
        Rect { x, y, w: self.right().max(o.right()) - x, h: self.bottom().max(o.bottom()) - y }
    }
}

fn one() -> f64 {
    1.0
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Shape {
    Pen {
        points: Vec<Pt>,
        width: f64,
    },
    Highlight {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        #[serde(default = "one")]
        width: f64,
    },
    Rectangle {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        width: f64,
    },
    Ellipse {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        width: f64,
    },
    Arrow {
        x: f64,
        y: f64,
        x2: f64,
        y2: f64,
        width: f64,
    },
    Text {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        size: f64,
        text: String,
        lines: Vec<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Mark {
    pub id: String,
    pub page: u32,
    pub color: String,
    #[serde(flatten)]
    pub shape: Shape,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FieldValue {
    Bool(bool),
    Text(String),
    List(Vec<String>),
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Edits {
    pub marks: Vec<Mark>,
    pub fields: BTreeMap<String, FieldValue>,
}

pub fn new_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{:x}-{:x}-{:x}", nanos, std::process::id(), n)
}

pub fn parse_color(c: &str) -> Option<[u8; 3]> {
    let b = c.as_bytes();
    if b.len() != 7 || b[0] != b'#' || !b[1..].iter().all(|x| x.is_ascii_hexdigit()) {
        return None;
    }
    let p = |i: usize| u8::from_str_radix(&c[i..i + 2], 16).ok();
    Some([p(1)?, p(3)?, p(5)?])
}

pub fn color_hex(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

impl Mark {
    pub fn rgb(&self) -> [u8; 3] {
        parse_color(&self.color).unwrap_or([0, 0, 0])
    }

    pub fn kind_label(&self) -> &'static str {
        match self.shape {
            Shape::Pen { .. } => "Drawing",
            Shape::Highlight { .. } => "Highlight",
            Shape::Rectangle { .. } => "Rectangle",
            Shape::Ellipse { .. } => "Ellipse",
            Shape::Arrow { .. } => "Arrow",
            Shape::Text { .. } => "Text",
        }
    }

    pub fn stroke_width(&self) -> f64 {
        match &self.shape {
            Shape::Pen { width, .. }
            | Shape::Highlight { width, .. }
            | Shape::Rectangle { width, .. }
            | Shape::Ellipse { width, .. }
            | Shape::Arrow { width, .. } => *width,
            Shape::Text { .. } => 0.0,
        }
    }

    pub fn bounds(&self) -> Rect {
        match &self.shape {
            Shape::Pen { points, .. } => {
                let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
                for p in points {
                    x0 = x0.min(p.x);
                    y0 = y0.min(p.y);
                    x1 = x1.max(p.x);
                    y1 = y1.max(p.y);
                }
                if points.is_empty() {
                    return Rect { x: 0.0, y: 0.0, w: 0.0, h: 0.0 };
                }
                Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }
            }
            Shape::Arrow { x, y, x2, y2, .. } => Rect::between(Pt::new(*x, *y), Pt::new(*x2, *y2)),
            Shape::Highlight { x, y, w, h, .. }
            | Shape::Rectangle { x, y, w, h, .. }
            | Shape::Ellipse { x, y, w, h, .. }
            | Shape::Text { x, y, w, h, .. } => Rect { x: *x, y: *y, w: *w, h: *h },
        }
    }

    pub fn translated(&self, dx: f64, dy: f64) -> Mark {
        let mut m = self.clone();
        match &mut m.shape {
            Shape::Pen { points, .. } => {
                for p in points.iter_mut() {
                    p.x += dx;
                    p.y += dy;
                }
            }
            Shape::Arrow { x, y, x2, y2, .. } => {
                *x += dx;
                *y += dy;
                *x2 += dx;
                *y2 += dy;
            }
            Shape::Highlight { x, y, .. } | Shape::Rectangle { x, y, .. } | Shape::Ellipse { x, y, .. } | Shape::Text { x, y, .. } => {
                *x += dx;
                *y += dy;
            }
        }
        m
    }

    pub fn hit_test(&self, p: Pt, tolerance: f64) -> bool {
        match &self.shape {
            Shape::Pen { points, width } => points.iter().enumerate().any(|(i, a)| {
                let b = points.get(i + 1).unwrap_or(a);
                distance_to_segment(p, *a, *b) <= tolerance + width / 2.0
            }),
            Shape::Arrow { x, y, x2, y2, width } => distance_to_segment(p, Pt::new(*x, *y), Pt::new(*x2, *y2)) <= tolerance + width / 2.0,
            _ => {
                let b = self.bounds();
                p.x >= b.x - tolerance && p.x <= b.right() + tolerance && p.y >= b.y - tolerance && p.y <= b.bottom() + tolerance
            }
        }
    }
}

pub fn distance_to_segment(p: Pt, a: Pt, b: Pt) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let d = dx * dx + dy * dy;
    let t = if d > 0.0 { (((p.x - a.x) * dx + (p.y - a.y) * dy) / d).clamp(0.0, 1.0) } else { 0.0 };
    (p.x - a.x - t * dx).hypot(p.y - a.y - t * dy)
}

/// The two barb end points of an arrow head.
pub fn arrow_heads(x: f64, y: f64, x2: f64, y2: f64, width: f64) -> [Pt; 2] {
    let a = (y2 - y).atan2(x2 - x);
    let len = (width * 4.0).max(10.0);
    [-0.48f64, 0.48].map(|d| Pt::new(x2 - len * (a + d).cos(), y2 - len * (a + d).sin()))
}

/// Pen strokes with a single point are drawn as a tiny segment so they show up as a dot.
pub fn pen_points(points: &[Pt]) -> Vec<Pt> {
    if points.len() > 1 {
        points.to_vec()
    } else if let Some(p) = points.first() {
        vec![*p, Pt::new(p.x + 0.01, p.y + 0.01)]
    } else {
        vec![]
    }
}

/// Affine transform `[a b c d e f]` as used by PDF: x' = a*x + c*y + e, y' = b*x + d*y + f.
pub type Matrix = [f64; 6];

pub fn apply(m: &Matrix, x: f64, y: f64) -> (f64, f64) {
    (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
}

pub fn invert(m: &Matrix) -> Result<Matrix, String> {
    let [a, b, c, d, e, f] = *m;
    let det = a * d - b * c;
    if !det.is_finite() || det == 0.0 {
        return Err("Invalid page transform.".into());
    }
    Ok([d / det, -b / det, -c / det, a / det, (c * f - d * e) / det, (b * e - a * f) / det])
}

/// Undo/redo snapshots of all marks and form values. A new edit discards redo states.
#[derive(Clone, Debug)]
pub struct History {
    items: Vec<Edits>,
    index: usize,
}

impl History {
    pub fn new(initial: Edits) -> Self {
        Self { items: vec![initial], index: 0 }
    }
    pub fn current(&self) -> Edits {
        self.items[self.index].clone()
    }
    pub fn can_undo(&self) -> bool {
        self.index > 0
    }
    pub fn can_redo(&self) -> bool {
        self.index + 1 < self.items.len()
    }
    pub fn push(&mut self, value: Edits) {
        self.items.truncate(self.index + 1);
        self.items.push(value);
        if self.items.len() > HISTORY_LIMIT {
            self.items.remove(0);
        }
        self.index = self.items.len() - 1;
    }
    /// Replace the current state without adding an undo step (used while typing in form fields).
    pub fn replace_current(&mut self, value: Edits) {
        self.items[self.index] = value;
    }
    pub fn undo(&mut self) -> Edits {
        if self.can_undo() {
            self.index -= 1;
        }
        self.current()
    }
    pub fn redo(&mut self) -> Edits {
        if self.can_redo() {
            self.index += 1;
        }
        self.current()
    }
}

fn finite(x: f64, min: f64, max: f64) -> bool {
    x.is_finite() && x >= min && x <= max
}

const COORD: f64 = 100_000.0;

/// Validates untrusted edits (from a project file or a recovery draft).
pub fn validate_edits(input: Edits, page_count: usize) -> Result<Edits, String> {
    if input.marks.len() > MAX_MARKS {
        return Err("Invalid or oversized annotation list.".into());
    }
    let mut ids = std::collections::HashSet::new();
    for m in &input.marks {
        if m.id.is_empty()
            || m.id.len() > 100
            || !ids.insert(m.id.as_str())
            || m.page < 1
            || m.page as usize > page_count
            || parse_color(&m.color).is_none()
        {
            return Err("Invalid annotation in project.".into());
        }
        let c = |v: f64| finite(v, -COORD, COORD);
        let size = |v: f64| finite(v, 0.0, COORD);
        let stroke = |v: f64| finite(v, 0.1, 50.0);
        let ok = match &m.shape {
            Shape::Pen { points, width } => {
                !points.is_empty() && points.len() <= 10_000 && points.iter().all(|p| c(p.x) && c(p.y)) && stroke(*width)
            }
            Shape::Highlight { x, y, w, h, width } | Shape::Rectangle { x, y, w, h, width } | Shape::Ellipse { x, y, w, h, width } => {
                c(*x) && c(*y) && size(*w) && size(*h) && stroke(*width)
            }
            Shape::Arrow { x, y, x2, y2, width } => c(*x) && c(*y) && c(*x2) && c(*y2) && stroke(*width),
            Shape::Text { x, y, w, h, size: s, text, lines } => {
                c(*x)
                    && c(*y)
                    && size(*w)
                    && size(*h)
                    && finite(*s, 5.0, 144.0)
                    && !text.trim().is_empty()
                    && text.chars().count() <= 10_000
                    && lines.len() <= 2000
                    && lines.join("\n") == *text
            }
        };
        if !ok {
            return Err(format!("Invalid {} annotation in project.", m.kind_label().to_lowercase()));
        }
    }
    if input.fields.len() > 20_000 {
        return Err("Invalid form values.".into());
    }
    for (name, value) in &input.fields {
        let ok = name.len() <= 1000
            && match value {
                FieldValue::Bool(_) => true,
                FieldValue::Text(s) => s.chars().count() <= 10_000,
                FieldValue::List(v) => v.len() <= 1000 && v.iter().all(|s| s.chars().count() <= 10_000),
            };
        if !ok {
            return Err("Invalid form value.".into());
        }
    }
    Ok(input)
}

pub fn is_pdf(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(1024)];
    head.windows(5).any(|w| w == b"%PDF-")
}

pub fn safe_filename(name: &str) -> String {
    let cleaned: String = name.chars().map(|c| if "<>:\"/\\|?*".contains(c) || (c as u32) < 0x20 { '_' } else { c }).take(180).collect();
    if cleaned.trim().is_empty() { "document.pdf".into() } else { cleaned }
}

/// "report.pdf" -> "report"
pub fn stem(name: &str) -> String {
    let lower = name.to_lowercase();
    if lower.ends_with(".pdf") { name[..name.len() - 4].to_string() } else { name.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect_mark(id: &str) -> Mark {
        Mark { id: id.into(), page: 1, color: "#ff0000".into(), shape: Shape::Rectangle { x: 1.0, y: 2.0, w: 3.0, h: 4.0, width: 2.0 } }
    }

    #[test]
    fn history_undo_redo_and_truncation() {
        let mut h = History::new(Edits::default());
        let mut e = Edits::default();
        e.marks.push(rect_mark("1"));
        h.push(e);
        assert!(h.can_undo());
        assert_eq!(h.undo().marks.len(), 0);
        assert!(h.can_redo());
        h.push(Edits::default());
        assert!(!h.can_redo());
        for _ in 0..200 {
            h.push(Edits::default());
        }
        let mut n = 0;
        while h.can_undo() {
            h.undo();
            n += 1;
        }
        assert_eq!(n, HISTORY_LIMIT - 1);
    }

    #[test]
    fn translate_and_hit_test() {
        let pen = Mark {
            id: "p".into(),
            page: 1,
            color: "#000000".into(),
            shape: Shape::Pen { points: vec![Pt::new(1.0, 2.0), Pt::new(3.0, 4.0)], width: 2.0 },
        };
        let moved = pen.translated(2.0, 3.0);
        match &moved.shape {
            Shape::Pen { points, .. } => assert_eq!(points[0], Pt::new(3.0, 5.0)),
            _ => unreachable!(),
        }
        assert!(moved.hit_test(Pt::new(3.0, 5.0), 5.0));
        assert!(!moved.hit_test(Pt::new(30.0, 50.0), 5.0));
        assert!(rect_mark("r").hit_test(Pt::new(2.0, 3.0), 0.0));
    }

    #[test]
    fn inverse_transform() {
        let inv = invert(&[1.0, 0.0, 0.0, -1.0, 0.0, 800.0]).unwrap();
        assert_eq!(inv.map(|n| if n == 0.0 { 0.0 } else { n }), [1.0, 0.0, 0.0, -1.0, 0.0, 800.0]);
        assert!(invert(&[0.0; 6]).is_err());
    }

    #[test]
    fn project_json_is_compatible_with_extension_format() {
        let json = r##"{"marks":[
            {"id":"a","type":"highlight","page":1,"x":1,"y":2,"w":3,"h":4,"color":"#edc949","width":1},
            {"id":"b","type":"text","page":1,"x":1,"y":2,"w":30,"h":20,"size":16,"color":"#25334b","text":"Zażółć\nx","lines":["Zażółć","x"]},
            {"id":"c","type":"pen","page":2,"points":[{"x":1,"y":2}],"width":2.5,"color":"#2f619c"},
            {"id":"d","type":"arrow","page":2,"x":1,"y":2,"x2":5,"y2":6,"width":2.5,"color":"#c95b50"}],
            "fields":{"name":"Ł","ok":true,"multi":["a","b"]}}"##;
        let edits: Edits = serde_json::from_str(json).unwrap();
        let edits = validate_edits(edits, 2).unwrap();
        assert_eq!(edits.marks.len(), 4);
        assert_eq!(edits.fields["ok"], FieldValue::Bool(true));
        let again: Edits = serde_json::from_str(&serde_json::to_string(&edits).unwrap()).unwrap();
        assert_eq!(again, edits);
        let out = serde_json::to_value(&edits.marks[0]).unwrap();
        assert_eq!(out["type"], "highlight");
        assert_eq!(out["x"], 1.0);
    }

    #[test]
    fn rejects_malformed_edits() {
        assert!(serde_json::from_str::<Edits>(r#"{"marks":[{"type":"bogus"}],"fields":{}}"#).is_err());
        let mut e = Edits::default();
        e.marks.push(rect_mark("x"));
        assert!(validate_edits(e.clone(), 0).is_err(), "page out of range");
        e.marks.push(rect_mark("x"));
        assert!(validate_edits(e, 1).is_err(), "duplicate id");
        let mut bad = Edits::default();
        let mut m = rect_mark("y");
        m.color = "red".into();
        bad.marks.push(m);
        assert!(validate_edits(bad, 1).is_err());
        let mut text = Edits::default();
        text.marks.push(Mark {
            id: "t".into(),
            page: 1,
            color: "#000000".into(),
            shape: Shape::Text { x: 0.0, y: 0.0, w: 1.0, h: 1.0, size: 12.0, text: "a\nb".into(), lines: vec!["a".into()] },
        });
        assert!(validate_edits(text, 1).is_err());
    }

    #[test]
    fn misc_helpers() {
        assert!(is_pdf(b"\n%PDF-1.7"));
        assert!(!is_pdf(b"hello"));
        assert_eq!(safe_filename("a/b:c?.pdf"), "a_b_c_.pdf");
        assert_eq!(safe_filename(""), "document.pdf");
        assert_eq!(stem("Report.PDF"), "Report");
        assert_eq!(parse_color("#Ab0010"), Some([0xab, 0, 0x10]));
        assert_eq!(color_hex([1, 2, 255]), "#0102ff");
        assert_ne!(new_id(), new_id());
    }
}
