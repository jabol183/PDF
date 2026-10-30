//! Writes annotations into a copy of the PDF as vector page content, and
//! applies edited form values with regenerated appearances.
//!
//! Original page content streams are kept untouched: they are wrapped in
//! `q ... Q` and a new content stream with the marks is appended.

use crate::font::{self, FAMILY};
use crate::forms::{self, FieldEntry};
use crate::model::{Edits, FieldValue, LINE_HEIGHT, Mark, Matrix, Shape, arrow_heads, invert, pen_points};
use lopdf::{Dictionary, Document, Object, ObjectId, Stream, StringFormat};
use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;

const FONT_RES: &str = "PDFMarkF";
const GS_RES: &str = "PDFMarkHL";
const HIGHLIGHT_OPACITY: f64 = 0.32;

pub fn export_pdf(bytes: &[u8], edits: &Edits, transforms: &HashMap<u32, Matrix>) -> Result<Vec<u8>, String> {
    let mut doc = forms::load(bytes, None).map_err(|e| format!("This PDF could not be prepared for export: {e}"))?;
    if doc.is_encrypted() {
        return Err("Encrypted PDF export is not supported. Save an editable project, or open an unencrypted PDF.".into());
    }

    // Every string that will be drawn with the bundled font.
    let mut all_text = String::new();
    for m in &edits.marks {
        if let Shape::Text { text, .. } = &m.shape {
            all_text.push_str(text);
            all_text.push('\n');
        }
    }
    let fields = forms::collect_fields(&doc);
    let by_name: HashMap<&str, &FieldEntry> = fields.iter().map(|f| (f.name.as_str(), f)).collect();
    for (name, value) in &edits.fields {
        let entry = by_name.get(name.as_str()).ok_or_else(|| format!("The form field “{name}” no longer exists."))?;
        match value {
            FieldValue::Text(s) => {
                all_text.push_str(s);
                // Choice fields display their option text.
                for (export, display) in forms::choice_options(&doc, entry.id) {
                    if &export == s {
                        all_text.push_str(&display);
                    }
                }
            }
            FieldValue::List(v) => {
                for (_, display) in forms::choice_options(&doc, entry.id) {
                    all_text.push_str(&display);
                }
                all_text.push_str(&v.join(""));
            }
            FieldValue::Bool(_) => {}
        }
        all_text.push('\n');
    }
    let missing = font::unsupported_chars(&all_text);
    if !missing.is_empty() {
        let list: Vec<String> = missing.iter().take(10).map(|c| c.to_string()).collect();
        return Err(format!("The included font cannot display: {}. Use supported characters before exporting.", list.join(" ")));
    }
    let mut embed = FontEmbed::new();
    for c in all_text.chars() {
        embed.add(c);
    }
    let font_id = if all_text.trim().is_empty() { None } else { Some(doc.new_object_id()) };

    // Annotation marks, page by page.
    let pages = doc.get_pages();
    let mut by_page: BTreeMap<u32, Vec<&Mark>> = BTreeMap::new();
    for m in &edits.marks {
        by_page.entry(m.page).or_default().push(m);
    }
    let gs_id = if edits.marks.iter().any(|m| matches!(m.shape, Shape::Highlight { .. })) {
        let mut gs = Dictionary::new();
        gs.set("Type", Object::Name(b"ExtGState".to_vec()));
        gs.set("ca", Object::Real(HIGHLIGHT_OPACITY as f32));
        gs.set("CA", Object::Real(HIGHLIGHT_OPACITY as f32));
        gs.set("BM", Object::Name(b"Multiply".to_vec()));
        Some(doc.add_object(gs))
    } else {
        None
    };
    for (page_no, marks) in &by_page {
        let page_id = *pages.get(page_no).ok_or_else(|| format!("Page {page_no} does not exist in this PDF."))?;
        let transform = transforms.get(page_no).ok_or_else(|| format!("Missing page geometry for page {page_no}."))?;
        let content = page_content(marks, transform, &mut embed)?;
        let uses_text = marks.iter().any(|m| matches!(m.shape, Shape::Text { .. }));
        let uses_gs = marks.iter().any(|m| matches!(m.shape, Shape::Highlight { .. }));
        add_resources(&mut doc, page_id, if uses_text { font_id } else { None }, if uses_gs { gs_id } else { None })?;
        wrap_contents(&mut doc, page_id, content)?;
    }

    // Form values.
    if !edits.fields.is_empty() {
        apply_fields(&mut doc, edits, &by_name, font_id, &mut embed)?;
    }

    if let Some(fid) = font_id {
        embed.write(&mut doc, fid)?;
    }

    let mut out = Vec::new();
    doc.save_to(&mut out).map_err(|e| format!("The edited PDF could not be written: {e}"))?;
    Ok(out)
}

// ---------------------------------------------------------------- content

fn n(v: f64) -> String {
    if !v.is_finite() {
        return "0".into();
    }
    let s = format!("{:.4}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" || s.is_empty() { "0".into() } else { s.to_string() }
}

fn rgb_ops(c: [u8; 3], op: &str) -> String {
    format!("{} {} {} {op}", n(c[0] as f64 / 255.0), n(c[1] as f64 / 255.0), n(c[2] as f64 / 255.0))
}

fn page_content(marks: &[&Mark], transform: &Matrix, embed: &mut FontEmbed) -> Result<Vec<u8>, String> {
    let inv = invert(transform)?;
    let mut s = String::from("Q\nq\n");
    let _ = writeln!(s, "{} {} {} {} {} {} cm", n(inv[0]), n(inv[1]), n(inv[2]), n(inv[3]), n(inv[4]), n(inv[5]));
    for m in marks {
        let c = m.rgb();
        match &m.shape {
            Shape::Highlight { x, y, w, h, .. } => {
                let _ = writeln!(s, "q /{GS_RES} gs {} {} {} {} {} re f Q", rgb_ops(c, "rg"), n(*x), n(*y), n(*w), n(*h));
            }
            Shape::Rectangle { x, y, w, h, width } => {
                let _ = writeln!(s, "q {} {} w {} {} {} {} re S Q", rgb_ops(c, "RG"), n(*width), n(*x), n(*y), n(*w), n(*h));
            }
            Shape::Ellipse { x, y, w, h, width } => {
                let _ = writeln!(s, "q {} {} w", rgb_ops(c, "RG"), n(*width));
                ellipse_path(&mut s, x + w / 2.0, y + h / 2.0, w / 2.0, h / 2.0);
                s.push_str("S Q\n");
            }
            Shape::Pen { points, width } => {
                let pts = pen_points(points);
                let _ = write!(s, "q {} {} w 1 J 1 j", rgb_ops(c, "RG"), n(*width));
                for (i, p) in pts.iter().enumerate() {
                    let _ = write!(s, " {} {} {}", n(p.x), n(p.y), if i == 0 { "m" } else { "l" });
                }
                s.push_str(" S Q\n");
            }
            Shape::Arrow { x, y, x2, y2, width } => {
                let heads = arrow_heads(*x, *y, *x2, *y2, *width);
                let _ = writeln!(
                    s,
                    "q {} {} w 1 J 1 j {} {} m {} {} l {} {} m {} {} l {} {} l S Q",
                    rgb_ops(c, "RG"),
                    n(*width),
                    n(*x),
                    n(*y),
                    n(*x2),
                    n(*y2),
                    n(heads[0].x),
                    n(heads[0].y),
                    n(*x2),
                    n(*y2),
                    n(heads[1].x),
                    n(heads[1].y)
                );
            }
            Shape::Text { x, y, size, lines, .. } => {
                // Undo the viewport's flipped y axis so glyphs stand upright.
                let _ = writeln!(s, "q 1 0 0 -1 {} {} cm BT /{FONT_RES} {} Tf {}", n(*x), n(y + size), n(*size), rgb_ops(c, "rg"));
                for (i, line) in lines.iter().enumerate() {
                    let _ = writeln!(s, "1 0 0 1 0 {} Tm <{}> Tj", n(-(i as f64) * size * LINE_HEIGHT), embed.hex(line));
                }
                s.push_str("ET Q\n");
            }
        }
    }
    s.push_str("Q\n");
    Ok(s.into_bytes())
}

fn ellipse_path(s: &mut String, cx: f64, cy: f64, rx: f64, ry: f64) {
    const K: f64 = 0.552_284_749_8;
    let (ox, oy) = (rx * K, ry * K);
    let _ = write!(s, "{} {} m ", n(cx - rx), n(cy));
    let _ = write!(s, "{} {} {} {} {} {} c ", n(cx - rx), n(cy - oy), n(cx - ox), n(cy - ry), n(cx), n(cy - ry));
    let _ = write!(s, "{} {} {} {} {} {} c ", n(cx + ox), n(cy - ry), n(cx + rx), n(cy - oy), n(cx + rx), n(cy));
    let _ = write!(s, "{} {} {} {} {} {} c ", n(cx + rx), n(cy + oy), n(cx + ox), n(cy + ry), n(cx), n(cy + ry));
    let _ = write!(s, "{} {} {} {} {} {} c h ", n(cx - ox), n(cy + ry), n(cx - rx), n(cy + oy), n(cx - rx), n(cy));
}

fn deflate(data: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    let _ = e.write_all(data);
    e.finish().unwrap_or_default()
}

fn flate_stream(mut dict: Dictionary, data: &[u8]) -> Stream {
    dict.set("Filter", Object::Name(b"FlateDecode".to_vec()));
    let mut s = Stream::new(dict, deflate(data));
    s.allows_compression = false;
    s
}

/// Wraps existing page content in q/Q and appends our content stream.
fn wrap_contents(doc: &mut Document, page_id: ObjectId, content: Vec<u8>) -> Result<(), String> {
    let existing: Vec<Object> = {
        let page = doc.get_dictionary(page_id).map_err(|e| e.to_string())?;
        match page.get(b"Contents") {
            Ok(Object::Reference(id)) => match doc.get_object(*id) {
                Ok(Object::Array(a)) => a.clone(),
                _ => vec![Object::Reference(*id)],
            },
            Ok(Object::Array(a)) => a.clone(),
            _ => vec![],
        }
    };
    let open = doc.add_object(Stream::new(Dictionary::new(), b"q\n".to_vec()));
    let ours = doc.add_object(flate_stream(Dictionary::new(), &content));
    let mut contents = vec![Object::Reference(open)];
    contents.extend(existing);
    contents.push(Object::Reference(ours));
    let page = doc.get_dictionary_mut(page_id).map_err(|e| e.to_string())?;
    page.set("Contents", Object::Array(contents));
    Ok(())
}

/// Resolves the page's (possibly inherited or referenced) resource dictionary
/// into a direct dictionary on the page and adds our font / graphics state.
fn add_resources(doc: &mut Document, page_id: ObjectId, font: Option<ObjectId>, gs: Option<ObjectId>) -> Result<(), String> {
    let mut res: Dictionary = {
        let mut found: Option<Dictionary> = None;
        let mut cur = Some(page_id);
        for _ in 0..32 {
            let Some(id) = cur else { break };
            let d = doc.get_dictionary(id).map_err(|e| e.to_string())?;
            if let Ok(r) = d.get(b"Resources") {
                found = forms::dict(doc, r).cloned();
                break;
            }
            cur = d.get(b"Parent").ok().and_then(|p| p.as_reference().ok());
        }
        found.unwrap_or_default()
    };
    let add = |res: &mut Dictionary, category: &str, key: &str, id: ObjectId| {
        let mut sub = res.get(category.as_bytes()).ok().and_then(|o| forms::dict(doc, o)).cloned().unwrap_or_default();
        sub.set(key, Object::Reference(id));
        res.set(category, Object::Dictionary(sub));
    };
    if let Some(f) = font {
        add(&mut res, "Font", FONT_RES, f);
    }
    if let Some(g) = gs {
        add(&mut res, "ExtGState", GS_RES, g);
    }
    let page = doc.get_dictionary_mut(page_id).map_err(|e| e.to_string())?;
    page.set("Resources", Object::Dictionary(res));
    Ok(())
}

// ---------------------------------------------------------------- forms

fn apply_fields(
    doc: &mut Document,
    edits: &Edits,
    by_name: &HashMap<&str, &FieldEntry>,
    font_id: Option<ObjectId>,
    embed: &mut FontEmbed,
) -> Result<(), String> {
    for (name, value) in &edits.fields {
        let f = by_name[name.as_str()];
        match (f.ft.as_slice(), value) {
            (b"Tx", FieldValue::Text(text)) => {
                set_field(doc, f.id, "V", Object::String(forms::encode_text(text), StringFormat::Literal))?;
                let multiline = f.ff & forms::FF_MULTILINE != 0;
                for w in &f.widgets {
                    let lines = if multiline { None } else { Some(vec![text.replace(['\n', '\r'], " ")]) };
                    text_appearance(doc, f, *w, lines, text, multiline, font_id, embed)?;
                }
            }
            (b"Tx", _) => return Err(format!("Invalid text value for {name}.")),
            (b"Btn", v) if f.ff & forms::FF_RADIO != 0 => {
                let FieldValue::Text(choice) = v else { return Err(format!("Invalid radio selection for {name}.")) };
                let states: Vec<Option<String>> =
                    f.widgets.iter().map(|w| doc.get_dictionary(*w).ok().and_then(|d| forms::on_state(doc, d))).collect();
                // The value is an appearance state name; also accept an /Opt export value.
                let state = if choice.is_empty() {
                    "Off".to_string()
                } else if states.iter().any(|s| s.as_deref() == Some(choice.as_str())) {
                    choice.clone()
                } else {
                    let opts = forms::inherited(doc, f.id, b"Opt")
                        .and_then(|o| forms::array(doc, &o).map(|a| a.iter().filter_map(|x| forms::string(doc, x)).collect::<Vec<_>>()))
                        .unwrap_or_default();
                    opts.iter()
                        .position(|o| o == choice)
                        .and_then(|i| states.get(i).cloned().flatten())
                        .ok_or_else(|| format!("“{choice}” is not an option of {name}."))?
                };
                set_field(doc, f.id, "V", Object::Name(state.clone().into_bytes()))?;
                for w in &f.widgets {
                    let on = doc.get_dictionary(*w).ok().and_then(|d| forms::on_state(doc, d));
                    let s = if on.as_deref() == Some(state.as_str()) { state.clone() } else { "Off".into() };
                    set_field(doc, *w, "AS", Object::Name(s.into_bytes()))?;
                }
            }
            (b"Btn", FieldValue::Bool(checked)) => {
                let mut field_state = "Off".to_string();
                for w in &f.widgets {
                    let on = doc.get_dictionary(*w).ok().and_then(|d| forms::on_state(doc, d)).unwrap_or_else(|| "Yes".into());
                    let s = if *checked { on } else { "Off".into() };
                    if *checked {
                        field_state = s.clone();
                    }
                    set_field(doc, *w, "AS", Object::Name(s.into_bytes()))?;
                }
                set_field(doc, f.id, "V", Object::Name(field_state.into_bytes()))?;
            }
            (b"Btn", _) => return Err(format!("Invalid checkbox value for {name}.")),
            (b"Ch", v) => {
                let selected: Vec<String> = match v {
                    FieldValue::Text(s) if s.is_empty() => vec![],
                    FieldValue::Text(s) => vec![s.clone()],
                    FieldValue::List(l) => l.clone(),
                    FieldValue::Bool(_) => return Err(format!("Invalid choice for {name}.")),
                };
                let options = forms::choice_options(doc, f.id);
                let enc = |s: &str| Object::String(forms::encode_text(s), StringFormat::Literal);
                match selected.len() {
                    0 => {
                        let d = doc.get_dictionary_mut(f.id).map_err(|e| e.to_string())?;
                        d.remove(b"V");
                        d.remove(b"I");
                    }
                    1 => set_field(doc, f.id, "V", enc(&selected[0]))?,
                    _ => set_field(doc, f.id, "V", Object::Array(selected.iter().map(|s| enc(s)).collect()))?,
                }
                let indices: Vec<Object> =
                    options.iter().enumerate().filter(|(_, (e, _))| selected.contains(e)).map(|(i, _)| Object::Integer(i as i64)).collect();
                if f.ff & forms::FF_COMBO == 0 && !indices.is_empty() {
                    set_field(doc, f.id, "I", Object::Array(indices))?;
                }
                let display = |e: &str| options.iter().find(|(x, _)| x == e).map(|(_, d)| d.clone()).unwrap_or_else(|| e.to_string());
                for w in &f.widgets {
                    if f.ff & forms::FF_COMBO != 0 {
                        let shown = selected.first().map(|s| display(s)).unwrap_or_default();
                        text_appearance(doc, f, *w, Some(vec![shown.clone()]), &shown, false, font_id, embed)?;
                    } else {
                        list_appearance(doc, f, *w, &options, &selected, font_id, embed)?;
                    }
                }
            }
            _ => return Err(format!("Unsupported form field: {name}.")),
        }
    }
    if let Some(fid) = font_id {
        // Register the font in the form's default resources.
        let acro_ref = doc.catalog().ok().and_then(|c| c.get(b"AcroForm").ok().cloned());
        let mut acro = match &acro_ref {
            Some(Object::Reference(id)) => doc.get_dictionary(*id).cloned().unwrap_or_default(),
            Some(Object::Dictionary(d)) => d.clone(),
            _ => Dictionary::new(),
        };
        let mut dr = acro.get(b"DR").ok().and_then(|o| forms::dict(doc, o)).cloned().unwrap_or_default();
        let mut fonts = dr.get(b"Font").ok().and_then(|o| forms::dict(doc, o)).cloned().unwrap_or_default();
        fonts.set(FONT_RES, Object::Reference(fid));
        dr.set("Font", Object::Dictionary(fonts));
        acro.set("DR", Object::Dictionary(dr));
        match acro_ref {
            Some(Object::Reference(id)) => {
                doc.objects.insert(id, Object::Dictionary(acro));
            }
            _ => {
                if let Ok(cat) = doc.catalog_mut() {
                    cat.set("AcroForm", Object::Dictionary(acro));
                }
            }
        }
    }
    Ok(())
}

fn set_field(doc: &mut Document, id: ObjectId, key: &str, value: Object) -> Result<(), String> {
    doc.get_dictionary_mut(id).map_err(|e| e.to_string())?.set(key, value);
    Ok(())
}

struct WidgetStyle {
    w: f64,
    h: f64,
    size: f64,
    color: [f64; 3],
    align: i64,
}

fn widget_style(doc: &Document, f: &FieldEntry, wid: ObjectId) -> Result<WidgetStyle, String> {
    let w = doc.get_dictionary(wid).map_err(|e| e.to_string())?;
    let r = w.get(b"Rect").ok().and_then(|o| forms::rect(doc, o)).unwrap_or([0.0, 0.0, 100.0, 20.0]);
    let mut wdt = r[2] - r[0];
    let mut hgt = r[3] - r[1];
    let rotation = w
        .get(b"MK")
        .ok()
        .and_then(|o| forms::dict(doc, o))
        .and_then(|mk| mk.get(b"R").ok())
        .and_then(|o| forms::num(doc, o))
        .unwrap_or(0.0) as i64;
    if rotation.rem_euclid(180) == 90 {
        std::mem::swap(&mut wdt, &mut hgt);
    }
    let da = w
        .get(b"DA")
        .ok()
        .and_then(|o| forms::string(doc, o))
        .or_else(|| forms::inherited(doc, f.id, b"DA").and_then(|o| forms::string(doc, &o)))
        .or_else(|| forms::acroform(doc).and_then(|a| a.get(b"DA").ok()).and_then(|o| forms::string(doc, o)))
        .unwrap_or_default();
    let size = forms::parse_da_size(&da);
    let align = forms::inherited(doc, f.id, b"Q").and_then(|o| forms::num(doc, &o)).unwrap_or(0.0) as i64;
    Ok(WidgetStyle { w: wdt.max(1.0), h: hgt.max(1.0), size, color: parse_da_color(&da), align })
}

fn parse_da_color(da: &str) -> [f64; 3] {
    let t: Vec<&str> = da.split_whitespace().collect();
    for i in 0..t.len() {
        let num = |j: usize| t.get(j).and_then(|s| s.parse::<f64>().ok());
        match t[i] {
            "rg" if i >= 3 => {
                if let (Some(r), Some(g), Some(b)) = (num(i - 3), num(i - 2), num(i - 1)) {
                    return [r, g, b];
                }
            }
            "g" if i >= 1 => {
                if let Some(v) = num(i - 1) {
                    return [v, v, v];
                }
            }
            _ => {}
        }
    }
    [0.0, 0.0, 0.0]
}

fn wrap_line(text: &str, size: f64, max: f64) -> Vec<String> {
    let f = font::font();
    let mut out = Vec::new();
    for para in text.split('\n') {
        let mut line = String::new();
        for word in para.split(' ') {
            let candidate = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
            if f.measure(&candidate, size) <= max || line.is_empty() {
                line = candidate;
            } else {
                out.push(std::mem::take(&mut line));
                line = word.to_string();
            }
        }
        out.push(line);
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn text_appearance(
    doc: &mut Document,
    f: &FieldEntry,
    wid: ObjectId,
    lines: Option<Vec<String>>,
    full: &str,
    multiline: bool,
    font_id: Option<ObjectId>,
    embed: &mut FontEmbed,
) -> Result<(), String> {
    let st = widget_style(doc, f, wid)?;
    let fi = font::font();
    let asc = fi.ascender / fi.units_per_em;
    let desc = fi.descender / fi.units_per_em;
    let mut s = String::from("/Tx BMC\nq\n");
    let _ = writeln!(s, "1 1 {} {} re W n", n(st.w - 2.0), n(st.h - 2.0));
    if let (Some(fid), false) = (font_id, full.is_empty()) {
        let _ = fid;
        let size = if st.size > 0.0 {
            st.size
        } else if multiline {
            10.0
        } else {
            ((st.h - 4.0) / (asc - desc)).clamp(4.0, 12.0)
        };
        let lines = lines.unwrap_or_else(|| wrap_line(full, size, st.w - 4.0));
        let _ = writeln!(s, "BT /{FONT_RES} {} Tf {} {} {} rg", n(size), n(st.color[0]), n(st.color[1]), n(st.color[2]));
        for (i, line) in lines.iter().enumerate() {
            let width = fi.measure(line, size);
            let x = match st.align {
                1 => (st.w - width) / 2.0,
                2 => st.w - 2.0 - width,
                _ => 2.0,
            };
            let y =
                if multiline { st.h - 2.0 - asc * size - i as f64 * size * 1.15 } else { (st.h - (asc - desc) * size) / 2.0 - desc * size };
            let _ = writeln!(s, "1 0 0 1 {} {} Tm <{}> Tj", n(x), n(y), embed.hex(line));
        }
        s.push_str("ET\n");
    }
    s.push_str("Q\nEMC\n");
    set_appearance(doc, wid, &st, s, font_id)
}

#[allow(clippy::too_many_arguments)]
fn list_appearance(
    doc: &mut Document,
    f: &FieldEntry,
    wid: ObjectId,
    options: &[(String, String)],
    selected: &[String],
    font_id: Option<ObjectId>,
    embed: &mut FontEmbed,
) -> Result<(), String> {
    let st = widget_style(doc, f, wid)?;
    let fi = font::font();
    let asc = fi.ascender / fi.units_per_em;
    let desc = fi.descender / fi.units_per_em;
    let size = if st.size > 0.0 { st.size } else { 11.0 };
    let row = size * (asc - desc);
    let mut s = String::from("/Tx BMC\nq\n");
    let _ = writeln!(s, "1 1 {} {} re W n", n(st.w - 2.0), n(st.h - 2.0));
    for (i, (export, _)) in options.iter().enumerate() {
        if selected.contains(export) {
            let y = st.h - 1.0 - (i as f64 + 1.0) * row;
            let _ = writeln!(s, "0.6 0.757 0.855 rg 1 {} {} {} re f", n(y), n(st.w - 2.0), n(row));
        }
    }
    if font_id.is_some() {
        let _ = writeln!(s, "BT /{FONT_RES} {} Tf {} {} {} rg", n(size), n(st.color[0]), n(st.color[1]), n(st.color[2]));
        for (i, (_, display)) in options.iter().enumerate() {
            let y = st.h - 1.0 - (i as f64 + 1.0) * row - desc * size;
            let _ = writeln!(s, "1 0 0 1 2 {} Tm <{}> Tj", n(y), embed.hex(display));
        }
        s.push_str("ET\n");
    }
    s.push_str("Q\nEMC\n");
    set_appearance(doc, wid, &st, s, font_id)
}

fn set_appearance(doc: &mut Document, wid: ObjectId, st: &WidgetStyle, content: String, font_id: Option<ObjectId>) -> Result<(), String> {
    let mut d = Dictionary::new();
    d.set("Type", Object::Name(b"XObject".to_vec()));
    d.set("Subtype", Object::Name(b"Form".to_vec()));
    d.set("BBox", Object::Array(vec![0.into(), 0.into(), Object::Real(st.w as f32), Object::Real(st.h as f32)]));
    let mut res = Dictionary::new();
    if let Some(fid) = font_id {
        let mut fonts = Dictionary::new();
        fonts.set(FONT_RES, Object::Reference(fid));
        res.set("Font", Object::Dictionary(fonts));
    }
    d.set("Resources", Object::Dictionary(res));
    let ap_id = doc.add_object(flate_stream(d, content.as_bytes()));
    let w = doc.get_dictionary_mut(wid).map_err(|e| e.to_string())?;
    let mut ap = Dictionary::new();
    ap.set("N", Object::Reference(ap_id));
    w.set("AP", Object::Dictionary(ap));
    Ok(())
}

// ---------------------------------------------------------------- font

/// Collects used glyphs, then embeds a subset of DejaVu Sans as a Type0 font
/// with a ToUnicode map, so added text stays selectable and searchable.
struct FontEmbed {
    remapper: subsetter::GlyphRemapper,
    /// new gid -> (old gid, char)
    used: BTreeMap<u16, (u16, char)>,
}

impl FontEmbed {
    fn new() -> Self {
        let mut remapper = subsetter::GlyphRemapper::new();
        remapper.remap(0);
        FontEmbed { remapper, used: BTreeMap::new() }
    }

    fn add(&mut self, c: char) -> Option<u16> {
        let old = font::font().glyph(c)?;
        let new = self.remapper.remap(old);
        self.used.entry(new).or_insert((old, c));
        Some(new)
    }

    fn hex(&mut self, text: &str) -> String {
        let mut s = String::with_capacity(text.len() * 4);
        for c in text.chars() {
            if let Some(g) = self.add(c) {
                let _ = write!(s, "{g:04X}");
            }
        }
        s
    }

    fn write(&self, doc: &mut Document, font_id: ObjectId) -> Result<(), String> {
        let fi = font::font();
        let data = subsetter::subset(font::FONT_BYTES, 0, &self.remapper).map_err(|e| format!("Font subsetting failed: {e:?}"))?;
        let scale = 1000.0 / fi.units_per_em;
        let mut file_dict = Dictionary::new();
        file_dict.set("Length1", Object::Integer(data.len() as i64));
        let file_id = doc.add_object(flate_stream(file_dict, &data));
        let bbox = fi.bbox();
        let mut desc = Dictionary::new();
        desc.set("Type", Object::Name(b"FontDescriptor".to_vec()));
        desc.set("FontName", Object::Name(format!("PDFMRK+{FAMILY}DejaVuSans").into_bytes()));
        desc.set("Flags", Object::Integer(32));
        desc.set("FontBBox", Object::Array(bbox.iter().map(|v| Object::Integer((v * scale).round() as i64)).collect()));
        desc.set("ItalicAngle", Object::Integer(0));
        desc.set("Ascent", Object::Integer((fi.ascender * scale).round() as i64));
        desc.set("Descent", Object::Integer((fi.descender * scale).round() as i64));
        desc.set("CapHeight", Object::Integer((fi.ascender * scale * 0.8).round() as i64));
        desc.set("StemV", Object::Integer(80));
        desc.set("FontFile2", Object::Reference(file_id));
        let desc_id = doc.add_object(desc);

        let mut widths = Vec::new();
        for (new, (old, _)) in &self.used {
            widths.push(Object::Integer(*new as i64));
            widths.push(Object::Array(vec![Object::Integer((fi.advance(*old) * scale).round() as i64)]));
        }
        let mut info = Dictionary::new();
        info.set("Registry", Object::string_literal("Adobe"));
        info.set("Ordering", Object::string_literal("Identity"));
        info.set("Supplement", Object::Integer(0));
        let mut cid = Dictionary::new();
        cid.set("Type", Object::Name(b"Font".to_vec()));
        cid.set("Subtype", Object::Name(b"CIDFontType2".to_vec()));
        cid.set("BaseFont", Object::Name(b"PDFMRK+DejaVuSans".to_vec()));
        cid.set("CIDSystemInfo", Object::Dictionary(info));
        cid.set("FontDescriptor", Object::Reference(desc_id));
        cid.set("W", Object::Array(widths));
        cid.set("DW", Object::Integer(600));
        cid.set("CIDToGIDMap", Object::Name(b"Identity".to_vec()));
        let cid_id = doc.add_object(cid);

        let to_unicode = doc.add_object(flate_stream(Dictionary::new(), self.cmap().as_bytes()));
        let mut t0 = Dictionary::new();
        t0.set("Type", Object::Name(b"Font".to_vec()));
        t0.set("Subtype", Object::Name(b"Type0".to_vec()));
        t0.set("BaseFont", Object::Name(b"PDFMRK+DejaVuSans".to_vec()));
        t0.set("Encoding", Object::Name(b"Identity-H".to_vec()));
        t0.set("DescendantFonts", Object::Array(vec![Object::Reference(cid_id)]));
        t0.set("ToUnicode", Object::Reference(to_unicode));
        doc.objects.insert(font_id, Object::Dictionary(t0));
        Ok(())
    }

    fn cmap(&self) -> String {
        let mut s = String::from(
            "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n/CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
        );
        let entries: Vec<(&u16, &(u16, char))> = self.used.iter().collect();
        for chunk in entries.chunks(100) {
            let _ = writeln!(s, "{} beginbfchar", chunk.len());
            for (new, (_, c)) in chunk {
                let mut buf = [0u16; 2];
                let hex: String = c.encode_utf16(&mut buf).iter().map(|u| format!("{u:04X}")).collect();
                let _ = writeln!(s, "<{:04X}> <{hex}>", new);
            }
            s.push_str("endbfchar\n");
        }
        s.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_formatting() {
        assert_eq!(n(1.0), "1");
        assert_eq!(n(-0.00001), "0");
        assert_eq!(n(2.5), "2.5");
        assert_eq!(n(1.23456), "1.2346");
        assert_eq!(n(f64::NAN), "0");
    }

    #[test]
    fn wraps_text() {
        let lines = wrap_line("one two three four five six", 12.0, 60.0);
        assert!(lines.len() > 1);
        assert_eq!(lines.join(" "), "one two three four five six");
    }
}
