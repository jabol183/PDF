//! AcroForm field discovery (via lopdf) and small PDF object helpers shared with
//! the exporter.

use crate::model::FieldValue;
use lopdf::{Dictionary, Document, Object, ObjectId};
use std::collections::HashMap;

pub const FF_READ_ONLY: i64 = 1;
pub const FF_MULTILINE: i64 = 1 << 12;
pub const FF_PASSWORD: i64 = 1 << 13;
pub const FF_RADIO: i64 = 1 << 15;
pub const FF_PUSHBUTTON: i64 = 1 << 16;
pub const FF_COMBO: i64 = 1 << 17;
pub const FF_MULTISELECT: i64 = 1 << 21;
pub const FF_COMB: i64 = 1 << 24;

#[derive(Clone, Debug, PartialEq)]
pub enum WidgetKind {
    Text {
        multiline: bool,
        password: bool,
        max_len: Option<usize>,
    },
    Checkbox {
        on: String,
    },
    Radio {
        on: String,
    },
    /// Options are (export value, display text).
    Combo {
        options: Vec<(String, String)>,
    },
    List {
        options: Vec<(String, String)>,
        multi: bool,
    },
}

#[derive(Clone, Debug)]
pub struct Widget {
    pub field: String,
    pub kind: WidgetKind,
    /// Widget rectangle in PDF user space [x0, y0, x1, y1].
    pub rect: [f64; 4],
    pub read_only: bool,
    pub initial: FieldValue,
    /// Font size from /DA (0 = auto).
    pub font_size: f64,
    pub label: String,
    /// Widget rotation from /MK /R (degrees).
    pub rotation: i64,
}

#[derive(Clone, Debug, Default)]
pub struct FormInfo {
    pub widgets: Vec<Vec<Widget>>,
    pub has_fields: bool,
    pub has_signature: bool,
    pub has_xfa: bool,
}

impl FormInfo {
    pub fn page(&self, index: usize) -> &[Widget] {
        self.widgets.get(index).map(|v| v.as_slice()).unwrap_or(&[])
    }
}

/// A terminal form field and its widget annotations.
#[derive(Clone, Debug)]
pub struct FieldEntry {
    pub name: String,
    pub id: ObjectId,
    pub ft: Vec<u8>,
    pub ff: i64,
    pub widgets: Vec<ObjectId>,
}

pub fn load(bytes: &[u8], password: Option<&str>) -> Result<Document, String> {
    let options = match password {
        Some(p) => lopdf::LoadOptions::with_password(p),
        None => lopdf::LoadOptions::default(),
    };
    Document::load_mem_with_options(bytes, options).map_err(|e| e.to_string())
}

/// Parses form widgets and per-page UserUnit values.
pub fn parse(bytes: &[u8], password: Option<&str>) -> Result<(FormInfo, Vec<f64>), String> {
    let doc = load(bytes, password)?;
    let pages = doc.get_pages();
    let mut page_index: HashMap<ObjectId, usize> = HashMap::new();
    let mut annot_page: HashMap<ObjectId, usize> = HashMap::new();
    let mut user_units = Vec::with_capacity(pages.len());
    for (i, (_, &pid)) in pages.iter().enumerate() {
        page_index.insert(pid, i);
        let page = doc.get_dictionary(pid).ok();
        let uu = page.and_then(|p| p.get(b"UserUnit").ok()).and_then(|o| num(&doc, o)).filter(|u| *u > 0.0 && *u < 100.0).unwrap_or(1.0);
        user_units.push(uu);
        if let Some(annots) = page.and_then(|p| p.get(b"Annots").ok()).and_then(|o| array(&doc, o)) {
            for a in annots {
                if let Object::Reference(id) = a {
                    annot_page.insert(*id, i);
                }
            }
        }
    }
    let mut info = FormInfo { widgets: vec![Vec::new(); pages.len()], ..Default::default() };
    let Some(acro) = acroform(&doc) else {
        return Ok((info, user_units));
    };
    info.has_xfa = acro.has(b"XFA");
    let da_default = acro.get(b"DA").ok().and_then(|o| string(&doc, o)).unwrap_or_default();
    let fields = collect_fields(&doc);
    info.has_fields = !fields.is_empty();
    for f in fields {
        let field = match doc.get_dictionary(f.id) {
            Ok(d) => d,
            Err(_) => continue,
        };
        if f.ft == b"Sig" {
            info.has_signature = true;
            continue;
        }
        let read_only = f.ff & FF_READ_ONLY != 0;
        let da = inherited(&doc, f.id, b"DA").and_then(|o| string(&doc, &o)).unwrap_or(da_default.clone());
        let font_size = parse_da_size(&da);
        let value = inherited(&doc, f.id, b"V");
        let label = field.get(b"TU").ok().and_then(|o| string(&doc, o)).unwrap_or_else(|| f.name.clone());
        let radio_opts: Vec<String> = inherited(&doc, f.id, b"Opt")
            .and_then(|o| array(&doc, &o).map(|a| a.iter().filter_map(|x| string(&doc, x)).collect()))
            .unwrap_or_default();
        for (widx, wid) in f.widgets.iter().enumerate() {
            let Some(&page) = annot_page.get(wid).or_else(|| {
                doc.get_dictionary(*wid)
                    .ok()
                    .and_then(|w| w.get(b"P").ok())
                    .and_then(|p| p.as_reference().ok())
                    .and_then(|p| page_index.get(&p))
            }) else {
                continue;
            };
            let Ok(w) = doc.get_dictionary(*wid) else { continue };
            let flags = w.get(b"F").ok().and_then(|o| num(&doc, o)).unwrap_or(0.0) as i64;
            if flags & (2 | 32) != 0 {
                continue; // hidden / no-view
            }
            let Some(rect) = w.get(b"Rect").ok().and_then(|o| rect(&doc, o)) else { continue };
            let on = on_state(&doc, w);
            let rotation =
                w.get(b"MK").ok().and_then(|o| dict(&doc, o)).and_then(|mk| mk.get(b"R").ok()).and_then(|o| num(&doc, o)).unwrap_or(0.0)
                    as i64;
            let (kind, initial) = match f.ft.as_slice() {
                b"Tx" => {
                    let max_len = inherited(&doc, f.id, b"MaxLen").and_then(|o| num(&doc, &o)).map(|n| n as usize).filter(|n| *n > 0);
                    let text = value.as_ref().and_then(|v| string(&doc, v)).unwrap_or_default();
                    (
                        WidgetKind::Text { multiline: f.ff & FF_MULTILINE != 0, password: f.ff & FF_PASSWORD != 0, max_len },
                        FieldValue::Text(text),
                    )
                }
                b"Btn" if f.ff & FF_PUSHBUTTON != 0 => continue,
                b"Btn" if f.ff & FF_RADIO != 0 => {
                    let v = value.as_ref().and_then(|v| name(&doc, v)).unwrap_or_else(|| "Off".into());
                    let label_for = radio_opts.get(widx).cloned();
                    let kind = WidgetKind::Radio { on: on.clone().unwrap_or_default() };
                    if let Some(l) = label_for {
                        info.widgets[page].push(Widget {
                            field: f.name.clone(),
                            kind,
                            rect,
                            read_only,
                            initial: FieldValue::Text(v),
                            font_size,
                            label: format!("{label}: {l}"),
                            rotation,
                        });
                        continue;
                    }
                    (kind, FieldValue::Text(v))
                }
                b"Btn" => {
                    let on = on.clone().unwrap_or_else(|| "Yes".into());
                    let v = value
                        .as_ref()
                        .and_then(|v| name(&doc, v))
                        .or_else(|| w.get(b"AS").ok().and_then(|o| name(&doc, o)))
                        .unwrap_or_else(|| "Off".into());
                    (WidgetKind::Checkbox { on }, FieldValue::Bool(v != "Off" && !v.is_empty()))
                }
                b"Ch" => {
                    let options = choice_options(&doc, f.id);
                    let selected: Vec<String> = match value.as_ref() {
                        Some(Object::Array(a)) => a.iter().filter_map(|o| string(&doc, o)).collect(),
                        Some(o) => string(&doc, o).into_iter().collect(),
                        None => vec![],
                    };
                    if f.ff & FF_COMBO != 0 {
                        (WidgetKind::Combo { options }, FieldValue::Text(selected.first().cloned().unwrap_or_default()))
                    } else {
                        let multi = f.ff & FF_MULTISELECT != 0;
                        (WidgetKind::List { options, multi }, FieldValue::List(selected))
                    }
                }
                _ => continue,
            };
            info.widgets[page].push(Widget {
                field: f.name.clone(),
                kind,
                rect,
                read_only,
                initial,
                font_size,
                label: label.clone(),
                rotation,
            });
        }
    }
    Ok((info, user_units))
}

pub fn acroform(doc: &Document) -> Option<&Dictionary> {
    let cat = doc.catalog().ok()?;
    match cat.get(b"AcroForm").ok()? {
        Object::Reference(id) => doc.get_dictionary(*id).ok(),
        Object::Dictionary(d) => Some(d),
        _ => None,
    }
}

/// Walks the AcroForm field tree and returns terminal fields with full names.
pub fn collect_fields(doc: &Document) -> Vec<FieldEntry> {
    let mut out = Vec::new();
    let Some(acro) = acroform(doc) else { return out };
    let Some(roots) = acro.get(b"Fields").ok().and_then(|o| array(doc, o)) else { return out };
    let mut seen = std::collections::HashSet::new();
    for r in roots {
        if let Object::Reference(id) = r {
            visit(doc, *id, "", &mut out, &mut seen, 0);
        }
    }
    out
}

fn visit(
    doc: &Document,
    id: ObjectId,
    parent: &str,
    out: &mut Vec<FieldEntry>,
    seen: &mut std::collections::HashSet<ObjectId>,
    depth: usize,
) {
    if depth > 32 || !seen.insert(id) {
        return;
    }
    let Ok(d) = doc.get_dictionary(id) else { return };
    let partial = d.get(b"T").ok().and_then(|o| string(doc, o));
    let name = match (&partial, parent.is_empty()) {
        (Some(p), true) => p.clone(),
        (Some(p), false) => format!("{parent}.{p}"),
        (None, _) => parent.to_string(),
    };
    let kids: Vec<ObjectId> = d
        .get(b"Kids")
        .ok()
        .and_then(|o| array(doc, o))
        .map(|a| a.iter().filter_map(|o| o.as_reference().ok()).collect())
        .unwrap_or_default();
    let field_kids: Vec<ObjectId> =
        kids.iter().copied().filter(|k| doc.get_dictionary(*k).map(|kd| kd.has(b"T")).unwrap_or(false)).collect();
    for k in &field_kids {
        visit(doc, *k, &name, out, seen, depth + 1);
    }
    let widget_kids: Vec<ObjectId> = kids.iter().copied().filter(|k| !field_kids.contains(k)).collect();
    let is_terminal = field_kids.is_empty();
    if !is_terminal || name.is_empty() {
        return;
    }
    let widgets = if kids.is_empty() { vec![id] } else { widget_kids };
    let ft = inherited(doc, id, b"FT").and_then(|o| name_bytes(doc, &o)).unwrap_or_default();
    let ff = inherited(doc, id, b"Ff").and_then(|o| num(doc, &o)).unwrap_or(0.0) as i64;
    out.push(FieldEntry { name, id, ft, ff, widgets });
}

/// Looks up an inheritable field attribute, walking /Parent links.
pub fn inherited(doc: &Document, id: ObjectId, key: &[u8]) -> Option<Object> {
    let mut cur = Some(id);
    for _ in 0..32 {
        let d = doc.get_dictionary(cur?).ok()?;
        if let Ok(v) = d.get(key) {
            return Some(v.clone());
        }
        cur = d.get(b"Parent").ok().and_then(|p| p.as_reference().ok());
    }
    None
}

pub fn resolve<'a>(doc: &'a Document, o: &'a Object) -> &'a Object {
    match o {
        Object::Reference(id) => doc.get_object(*id).unwrap_or(o),
        _ => o,
    }
}

pub fn num(doc: &Document, o: &Object) -> Option<f64> {
    match resolve(doc, o) {
        Object::Integer(i) => Some(*i as f64),
        Object::Real(r) => Some(*r as f64),
        _ => None,
    }
}

pub fn array<'a>(doc: &'a Document, o: &'a Object) -> Option<&'a Vec<Object>> {
    match resolve(doc, o) {
        Object::Array(a) => Some(a),
        _ => None,
    }
}

pub fn dict<'a>(doc: &'a Document, o: &'a Object) -> Option<&'a Dictionary> {
    match resolve(doc, o) {
        Object::Dictionary(d) => Some(d),
        Object::Stream(s) => Some(&s.dict),
        _ => None,
    }
}

pub fn name_bytes(doc: &Document, o: &Object) -> Option<Vec<u8>> {
    match resolve(doc, o) {
        Object::Name(n) => Some(n.clone()),
        _ => None,
    }
}

pub fn name(doc: &Document, o: &Object) -> Option<String> {
    match resolve(doc, o) {
        Object::Name(n) => Some(String::from_utf8_lossy(n).into_owned()),
        Object::String(s, _) => Some(decode_text(s)),
        _ => None,
    }
}

pub fn string(doc: &Document, o: &Object) -> Option<String> {
    match resolve(doc, o) {
        Object::String(s, _) => Some(decode_text(s)),
        Object::Name(n) => Some(String::from_utf8_lossy(n).into_owned()),
        _ => None,
    }
}

pub fn rect(doc: &Document, o: &Object) -> Option<[f64; 4]> {
    let a = array(doc, o)?;
    if a.len() != 4 {
        return None;
    }
    let v: Vec<f64> = a.iter().filter_map(|x| num(doc, x)).collect();
    if v.len() != 4 {
        return None;
    }
    Some([v[0].min(v[2]), v[1].min(v[3]), v[0].max(v[2]), v[1].max(v[3])])
}

/// The "on" appearance state name of a checkbox / radio widget.
pub fn on_state(doc: &Document, widget: &Dictionary) -> Option<String> {
    let ap = widget.get(b"AP").ok().and_then(|o| dict(doc, o))?;
    for key in [b"N".as_slice(), b"D".as_slice()] {
        if let Some(n) = ap.get(key).ok().and_then(|o| dict(doc, o)) {
            for (k, _) in n.iter() {
                if k.as_slice() != b"Off" {
                    return Some(String::from_utf8_lossy(k).into_owned());
                }
            }
        }
    }
    None
}

pub fn choice_options(doc: &Document, id: ObjectId) -> Vec<(String, String)> {
    let Some(opt) = inherited(doc, id, b"Opt") else { return vec![] };
    let Some(items) = array(doc, &opt) else { return vec![] };
    items
        .iter()
        .filter_map(|item| match resolve(doc, item) {
            Object::Array(pair) if pair.len() >= 2 => Some((string(doc, &pair[0])?, string(doc, &pair[1])?)),
            other => string(doc, other).map(|s| (s.clone(), s)),
        })
        .collect()
}

pub fn parse_da_size(da: &str) -> f64 {
    let toks: Vec<&str> = da.split_whitespace().collect();
    for i in 1..toks.len() {
        if toks[i] == "Tf" {
            return toks[i - 1].parse::<f64>().ok().filter(|v| v.is_finite() && *v >= 0.0).unwrap_or(0.0);
        }
    }
    0.0
}

/// Decodes a PDF text string (UTF-16BE with BOM, UTF-8 with BOM, or PDFDocEncoding).
pub fn decode_text(bytes: &[u8]) -> String {
    if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
        let units: Vec<u16> = bytes[2..].as_chunks::<2>().0.iter().map(|c| u16::from_be_bytes(*c)).collect();
        return String::from_utf16_lossy(&units);
    }
    if bytes.len() >= 3 && bytes[..3] == [0xEF, 0xBB, 0xBF] {
        return String::from_utf8_lossy(&bytes[3..]).into_owned();
    }
    bytes.iter().map(|&b| pdfdoc_char(b)).collect()
}

fn pdfdoc_char(b: u8) -> char {
    const HIGH: [u16; 32] = [
        0x2022, 0x2020, 0x2021, 0x2026, 0x2014, 0x2013, 0x0192, 0x2044, 0x2039, 0x203A, 0x2212, 0x2030, 0x201E, 0x201C, 0x201D, 0x2018,
        0x2019, 0x201A, 0x2122, 0xFB01, 0xFB02, 0x0141, 0x0152, 0x0160, 0x0178, 0x017D, 0x0131, 0x0142, 0x0153, 0x0161, 0x017E, 0xFFFD,
    ];
    match b {
        0x80..=0x9F => char::from_u32(HIGH[(b - 0x80) as usize] as u32).unwrap_or('\u{FFFD}'),
        0xA0 => '\u{20AC}',
        _ => b as char,
    }
}

/// Encodes a PDF text string: PDFDocEncoding-compatible ASCII as-is, otherwise UTF-16BE.
pub fn encode_text(s: &str) -> Vec<u8> {
    if s.chars().all(|c| (c as u32) >= 0x20 && (c as u32) < 0x7F || c == '\n' || c == '\r' || c == '\t') {
        return s.as_bytes().to_vec();
    }
    let mut out = vec![0xFE, 0xFF];
    for u in s.encode_utf16() {
        out.extend_from_slice(&u.to_be_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_strings_round_trip() {
        for s in ["Hello", "Łukasz Żółć", "line1\nline2"] {
            assert_eq!(decode_text(&encode_text(s)), s);
        }
        assert_eq!(decode_text(&[0x95]), "Ł");
        assert_eq!(parse_da_size("/Helv 12 Tf 0 g"), 12.0);
        assert_eq!(parse_da_size("/Helv 0 Tf"), 0.0);
        assert_eq!(parse_da_size(""), 0.0);
    }

    #[test]
    fn parses_fixture_form() {
        let bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/sample.pdf")).unwrap();
        let (info, units) = parse(&bytes, None).unwrap();
        assert_eq!(units.len(), 6);
        assert_eq!(units[5], 2.0);
        assert!(info.has_fields);
        let names: Vec<&str> = info.page(0).iter().map(|w| w.field.as_str()).collect();
        for n in ["customer.name", "approved", "service", "priority", "extras", "notes"] {
            assert!(names.contains(&n), "{n} in {names:?}");
        }
        assert_eq!(info.page(1).len(), 1, "second widget of customer.name");
        let name = info.page(0).iter().find(|w| w.field == "customer.name").unwrap();
        assert_eq!(name.initial, FieldValue::Text("Original".into()));
        let radios: Vec<_> = info.page(0).iter().filter(|w| w.field == "priority").collect();
        assert_eq!(radios.len(), 2);
        assert!(matches!(&radios[0].kind, WidgetKind::Radio { on } if !on.is_empty()));
        let service = info.page(0).iter().find(|w| w.field == "service").unwrap();
        assert!(matches!(&service.kind, WidgetKind::Combo { options } if options.len() == 2));
        assert_eq!(service.initial, FieldValue::Text("Repair".into()));
    }
}
