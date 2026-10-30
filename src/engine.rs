//! Background PDF worker. It owns the pdfium library and the open document,
//! renders pages and thumbnails, extracts text and produces exports. The UI
//! talks to it through channels so it never blocks on PDF work.

use crate::forms::{self, FormInfo};
use crate::model::{Edits, Matrix, Rect, apply};
use pdfium_render::prelude::*;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};

/// Page geometry in viewport units (100% zoom, rotation/crop/UserUnit applied).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageGeom {
    pub width: f64,
    pub height: f64,
    /// Maps PDF user space to viewport units.
    pub transform: Matrix,
}

#[derive(Clone, Debug)]
pub struct TextChar {
    pub text: String,
    pub rect: Rect,
    pub generated: bool,
}

#[derive(Clone, Debug)]
pub struct DocInfo {
    pub page_count: usize,
    /// Approximate page sizes (exact once the page geometry is loaded).
    pub sizes: Vec<(f64, f64)>,
    pub encrypted: bool,
    pub forms: Arc<FormInfo>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderKind {
    Main,
    Thumb,
}

pub struct RgbaImage {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
}

pub enum Request {
    Open { epoch: u64, bytes: Arc<Vec<u8>>, password: Option<String> },
    Render { epoch: u64, page: usize, kind: RenderKind, width: u32, height: u32 },
    Text { epoch: u64, page: usize },
    Export { epoch: u64, edits: Edits },
}

pub enum Response {
    Opened { epoch: u64, info: DocInfo },
    NeedPassword { epoch: u64, incorrect: bool },
    OpenFailed { epoch: u64, error: String },
    Rendered { epoch: u64, page: usize, kind: RenderKind, geom: PageGeom, image: RgbaImage },
    Text { epoch: u64, page: usize, geom: PageGeom, chars: Vec<TextChar> },
    Exported { epoch: u64, result: Result<Vec<u8>, String> },
    Fatal(String),
}

pub struct Engine {
    pub tx: Sender<Request>,
    pub rx: Receiver<Response>,
}

impl Engine {
    pub fn start(wake: impl Fn() + Send + 'static) -> Engine {
        let (req_tx, req_rx) = channel::<Request>();
        let (res_tx, res_rx) = channel::<Response>();
        std::thread::Builder::new().name("pdf-worker".into()).spawn(move || worker(req_rx, res_tx, wake)).expect("start PDF worker");
        Engine { tx: req_tx, rx: res_rx }
    }
}

/// Finds pdfium.dll / libpdfium next to the executable (or via PDFMARK_PDFIUM).
pub fn bind_pdfium() -> Result<Pdfium, String> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(p) = std::env::var("PDFMARK_PDFIUM") {
        candidates.push(PathBuf::from(p));
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        candidates.push(Pdfium::pdfium_platform_library_name_at_path(dir));
        if let Some(parent) = dir.parent() {
            candidates.push(Pdfium::pdfium_platform_library_name_at_path(parent));
        }
    }
    candidates.push(Pdfium::pdfium_platform_library_name_at_path("./"));
    let mut last = String::new();
    for c in candidates {
        if !c.exists() {
            continue;
        }
        match Pdfium::bind_to_library(&c) {
            Ok(b) => return Ok(Pdfium::new(b)),
            Err(e) => last = format!("{}: {e}", c.display()),
        }
    }
    match Pdfium::bind_to_system_library() {
        Ok(b) => Ok(Pdfium::new(b)),
        Err(e) => Err(format!(
            "The PDF engine ({}) could not be loaded. Keep it in the same folder as PDFMark. {last} {e}",
            Pdfium::pdfium_platform_library_name().to_string_lossy()
        )),
    }
}

struct OpenDoc<'a> {
    epoch: u64,
    doc: PdfDocument<'a>,
    /// Bytes lopdf can parse (the original, or a pdfium re-save if needed).
    export_bytes: Option<Arc<Vec<u8>>>,
    encrypted: bool,
    user_units: Vec<f64>,
    geoms: Vec<Option<PageGeom>>,
}

fn priority(r: &Request) -> u8 {
    match r {
        Request::Open { .. } | Request::Export { .. } => 0,
        Request::Render { kind: RenderKind::Main, .. } => 1,
        Request::Text { .. } => 2,
        Request::Render { kind: RenderKind::Thumb, .. } => 3,
    }
}

fn worker(rx: Receiver<Request>, tx: Sender<Response>, wake: impl Fn()) {
    let pdfium = match bind_pdfium() {
        Ok(p) => p,
        Err(e) => {
            let _ = tx.send(Response::Fatal(e));
            wake();
            // Drain requests so senders never block; reply with failures.
            while let Ok(r) = rx.recv() {
                if let Request::Open { epoch, .. } = r {
                    let _ = tx.send(Response::OpenFailed { epoch, error: "The PDF engine is unavailable.".into() });
                    wake();
                }
            }
            return;
        }
    };
    let mut current: Option<OpenDoc> = None;
    let mut queue: VecDeque<Request> = VecDeque::new();
    loop {
        if queue.is_empty() {
            match rx.recv() {
                Ok(r) => queue.push_back(r),
                Err(_) => return,
            }
        }
        loop {
            match rx.try_recv() {
                Ok(r) => queue.push_back(r),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }
        // Only the latest open request matters.
        if let Some(last) = queue.iter().rposition(|r| matches!(r, Request::Open { .. })) {
            let mut i = 0;
            queue.retain(|r| {
                let keep = !matches!(r, Request::Open { .. }) || i == last;
                i += 1;
                keep
            });
        }
        // Only the latest main-page render matters.
        if let Some(last) = queue.iter().rposition(|r| matches!(r, Request::Render { kind: RenderKind::Main, .. })) {
            let mut i = 0;
            queue.retain(|r| {
                let keep = !matches!(r, Request::Render { kind: RenderKind::Main, .. }) || i == last;
                i += 1;
                keep
            });
        }
        let best = (0..queue.len()).min_by_key(|&i| (priority(&queue[i]), i)).unwrap();
        let req = queue.remove(best).unwrap();
        let live = |epoch: u64, cur: &Option<OpenDoc>| cur.as_ref().is_some_and(|d| d.epoch == epoch);
        let response = match req {
            Request::Open { epoch, bytes, password } => {
                // The current document stays open until the new one succeeds.
                match open(&pdfium, epoch, bytes, password) {
                    Ok((doc, info)) => {
                        current = Some(doc);
                        Some(Response::Opened { epoch, info })
                    }
                    Err(r) => Some(r),
                }
            }
            Request::Render { epoch, page, kind, width, height } => {
                if !live(epoch, &current) {
                    None
                } else {
                    let d = current.as_mut().unwrap();
                    match render(d, page, width, height, kind) {
                        Ok((geom, image)) => Some(Response::Rendered { epoch, page, kind, geom, image }),
                        Err(e) => {
                            eprintln!("render page {page}: {e}");
                            None
                        }
                    }
                }
            }
            Request::Text { epoch, page } => {
                if !live(epoch, &current) {
                    None
                } else {
                    let d = current.as_mut().unwrap();
                    match page_text(d, page) {
                        Ok((geom, chars)) => Some(Response::Text { epoch, page, geom, chars }),
                        Err(_) => geometry(d, page).ok().map(|geom| Response::Text { epoch, page, geom, chars: vec![] }),
                    }
                }
            }
            Request::Export { epoch, edits } => {
                let result = if !live(epoch, &current) {
                    Err("The document is no longer open.".to_string())
                } else {
                    export(current.as_mut().unwrap(), &edits)
                };
                Some(Response::Exported { epoch, result })
            }
        };
        if let Some(r) = response {
            if tx.send(r).is_err() {
                return;
            }
            wake();
        }
    }
}

fn open<'a>(pdfium: &'a Pdfium, epoch: u64, bytes: Arc<Vec<u8>>, password: Option<String>) -> Result<(OpenDoc<'a>, DocInfo), Response> {
    let doc = match pdfium.load_pdf_from_byte_vec(bytes.as_ref().clone(), password.as_deref()) {
        Ok(d) => d,
        Err(PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::PasswordError)) => {
            return Err(Response::NeedPassword { epoch, incorrect: password.is_some() });
        }
        Err(e) => {
            return Err(Response::OpenFailed { epoch, error: format!("This PDF could not be opened ({e:?}).") });
        }
    };
    let page_count = doc.pages().len() as usize;
    if page_count == 0 {
        return Err(Response::OpenFailed { epoch, error: "This PDF has no pages.".into() });
    }
    if page_count > crate::model::MAX_PAGES {
        return Err(Response::OpenFailed { epoch, error: "This reader supports documents with up to 3,000 pages.".into() });
    }
    let encrypted =
        password.is_some() || !matches!(doc.permissions().security_handler_revision(), Ok(PdfSecurityHandlerRevision::Unprotected));

    // lopdf gives us form fields and UserUnit, and is used for export.
    let mut export_bytes = None;
    let mut parsed = forms::parse(&bytes, password.as_deref());
    if parsed.is_ok() {
        export_bytes = Some(bytes.clone());
    } else if !encrypted {
        // Repair route: let pdfium re-save the file, then parse that.
        if let Ok(resaved) = doc.save_to_bytes() {
            let p = forms::parse(&resaved, None);
            if p.is_ok() {
                export_bytes = Some(Arc::new(resaved));
                parsed = p;
            }
        }
    }
    let (form_info, user_units) = match parsed {
        Ok((f, u)) => (f, u),
        Err(e) => {
            eprintln!("form parsing unavailable: {e}");
            (FormInfo::default(), vec![])
        }
    };
    let user_units: Vec<f64> = (0..page_count).map(|i| user_units.get(i).copied().unwrap_or(1.0)).collect();
    let sizes = (0..page_count)
        .map(|i| {
            let u = user_units[i];
            match doc.pages().page_size(i as PdfPageIndex) {
                Ok(r) => (r.width().value as f64 * u, r.height().value as f64 * u),
                Err(_) => (612.0, 792.0),
            }
        })
        .collect();
    let info = DocInfo { page_count, sizes, encrypted, forms: Arc::new(form_info) };
    Ok((OpenDoc { epoch, doc, export_bytes, encrypted, user_units, geoms: vec![None; page_count] }, info))
}

/// Derives the page-space -> viewport transform from pdfium's own device mapping,
/// so the overlay matches the rendered bitmap exactly.
fn compute_geom(page: &PdfPage, user_unit: f64) -> Result<PageGeom, String> {
    let w = page.width().value as f64;
    let h = page.height().value as f64;
    if !(w > 0.0 && h > 0.0) || w > 30000.0 || h > 30000.0 {
        return Err("This PDF has page dimensions too large for this editor.".into());
    }
    const K: f64 = 256.0;
    let config = PdfRenderConfig::new().set_fixed_size((w * K).round() as i32, (h * K).round() as i32);
    let map = |x: f64, y: f64| -> Result<(f64, f64), String> {
        let (dx, dy) = page.points_to_pixels(PdfPoints::new(x as f32), PdfPoints::new(y as f32), &config).map_err(|e| format!("{e:?}"))?;
        Ok((dx as f64 / K, dy as f64 / K))
    };
    // Sample a point far from the origin to get the linear part precisely.
    let (e, f) = map(0.0, 0.0)?;
    let (x1, y1) = map(1000.0, 0.0)?;
    let (x2, y2) = map(0.0, 1000.0)?;
    let snap = |v: f64| {
        let r = v.round();
        if (v - r).abs() < 0.01 { r } else { v }
    };
    let a = snap((x1 - e) / 1000.0);
    let b = snap((y1 - f) / 1000.0);
    let c = snap((x2 - e) / 1000.0);
    let d = snap((y2 - f) / 1000.0);
    let u = user_unit;
    Ok(PageGeom { width: w * u, height: h * u, transform: [a * u, b * u, c * u, d * u, e * u, f * u] })
}

fn geometry(d: &mut OpenDoc, index: usize) -> Result<PageGeom, String> {
    if let Some(g) = d.geoms.get(index).copied().flatten() {
        return Ok(g);
    }
    let page = d.doc.pages().get(index as PdfPageIndex).map_err(|e| format!("{e:?}"))?;
    let g = compute_geom(&page, d.user_units[index])?;
    d.geoms[index] = Some(g);
    Ok(g)
}

fn render(d: &mut OpenDoc, index: usize, width: u32, height: u32, kind: RenderKind) -> Result<(PageGeom, RgbaImage), String> {
    let geom = geometry(d, index)?;
    let page = d.doc.pages().get(index as PdfPageIndex).map_err(|e| format!("{e:?}"))?;
    let w = width.clamp(1, 16000) as i32;
    let h = height.clamp(1, 16000) as i32;
    let config = PdfRenderConfig::new()
        .set_fixed_size(w, h)
        .render_annotations(true)
        // Keep form rendering on: it is also the code path that honours the fixed size.
        .render_form_data(true);
    let _ = kind;
    let bitmap = page.render_with_config(&config).map_err(|e| format!("{e:?}"))?;
    let pixels = bitmap.as_rgba_bytes();
    Ok((geom, RgbaImage { width: bitmap.width() as usize, height: bitmap.height() as usize, pixels }))
}

fn page_text(d: &mut OpenDoc, index: usize) -> Result<(PageGeom, Vec<TextChar>), String> {
    let geom = geometry(d, index)?;
    let page = d.doc.pages().get(index as PdfPageIndex).map_err(|e| format!("{e:?}"))?;
    let text = page.text().map_err(|e| format!("{e:?}"))?;
    let mut out = Vec::with_capacity(text.len().max(0) as usize);
    for ch in text.chars().iter() {
        let s = match ch.unicode_string() {
            Some(s) => s,
            None => continue,
        };
        let generated = ch.is_generated().unwrap_or(false);
        let rect = ch
            .loose_bounds()
            .ok()
            .map(|r| {
                let pts = [
                    apply(&geom.transform, r.left().value as f64, r.bottom().value as f64),
                    apply(&geom.transform, r.right().value as f64, r.bottom().value as f64),
                    apply(&geom.transform, r.left().value as f64, r.top().value as f64),
                    apply(&geom.transform, r.right().value as f64, r.top().value as f64),
                ];
                let x0 = pts.iter().map(|p| p.0).fold(f64::MAX, f64::min);
                let y0 = pts.iter().map(|p| p.1).fold(f64::MAX, f64::min);
                let x1 = pts.iter().map(|p| p.0).fold(f64::MIN, f64::max);
                let y1 = pts.iter().map(|p| p.1).fold(f64::MIN, f64::max);
                Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }
            })
            .unwrap_or(Rect { x: 0.0, y: 0.0, w: 0.0, h: 0.0 });
        out.push(TextChar { text: s, rect, generated });
    }
    Ok((geom, out))
}

fn export(d: &mut OpenDoc, edits: &Edits) -> Result<Vec<u8>, String> {
    if d.encrypted {
        return Err("Encrypted PDF export is not supported. Save an editable project, or open an unencrypted PDF.".into());
    }
    let bytes = d.export_bytes.clone().ok_or("This PDF's structure could not be read for export. Save an editable project instead.")?;
    let mut transforms = std::collections::HashMap::new();
    for m in &edits.marks {
        let i = m.page as usize - 1;
        if i >= d.geoms.len() {
            return Err(format!("Missing page geometry for page {}.", m.page));
        }
        transforms.insert(m.page, geometry(d, i)?.transform);
    }
    crate::export::export_pdf(&bytes, edits, &transforms)
}
