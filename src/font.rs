//! The bundled DejaVu Sans font, used both on screen and inside exported PDFs,
//! so typed text measures and looks the same in both places.

use std::sync::OnceLock;

pub static FONT_BYTES: &[u8] = include_bytes!("../assets/DejaVuSans.ttf");
pub const FAMILY: &str = "PDFMark";

pub struct FontInfo {
    face: ttf_parser::Face<'static>,
    pub units_per_em: f64,
    pub ascender: f64,
    pub descender: f64,
}

pub fn font() -> &'static FontInfo {
    static FONT: OnceLock<FontInfo> = OnceLock::new();
    FONT.get_or_init(|| {
        let face = ttf_parser::Face::parse(FONT_BYTES, 0).expect("bundled font is valid");
        FontInfo { units_per_em: face.units_per_em() as f64, ascender: face.ascender() as f64, descender: face.descender() as f64, face }
    })
}

impl FontInfo {
    pub fn glyph(&self, c: char) -> Option<u16> {
        self.face.glyph_index(c).map(|g| g.0)
    }

    /// Advance width in font units.
    pub fn advance(&self, gid: u16) -> f64 {
        self.face.glyph_hor_advance(ttf_parser::GlyphId(gid)).unwrap_or(0) as f64
    }

    /// Width of a line of text at `size` points (no kerning, same as the export).
    pub fn measure(&self, text: &str, size: f64) -> f64 {
        text.chars().map(|c| self.glyph(c).map(|g| self.advance(g)).unwrap_or(0.0)).sum::<f64>() * size / self.units_per_em
    }

    pub fn bbox(&self) -> [f64; 4] {
        let b = self.face.global_bounding_box();
        [b.x_min as f64, b.y_min as f64, b.x_max as f64, b.y_max as f64]
    }
}

/// Characters (other than line breaks/tabs) the bundled font cannot draw.
pub fn unsupported_chars(text: &str) -> Vec<char> {
    let f = font();
    let mut out: Vec<char> = Vec::new();
    for c in text.chars() {
        if matches!(c, '\n' | '\r' | '\t') {
            continue;
        }
        if f.glyph(c).is_none() && !out.contains(&c) {
            out.push(c);
        }
    }
    out
}

pub fn unsupported_message(missing: &[char]) -> String {
    let list: Vec<String> = missing.iter().take(10).map(|c| c.to_string()).collect();
    format!("The included font cannot display: {}. Please use supported characters.", list.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage_and_measurement() {
        assert!(unsupported_chars("Zażółć gęślą jaźń\nПривет Ελληνικά").is_empty());
        assert_eq!(unsupported_chars("a龘b龘"), vec!['龘']);
        let w = font().measure("Hello", 16.0);
        assert!(w > 30.0 && w < 50.0, "{w}");
        assert_eq!(font().measure("", 16.0), 0.0);
    }
}
