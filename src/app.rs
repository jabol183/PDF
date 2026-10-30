//! Application state and actions. Drawing lives in `view.rs`.

use crate::engine::{DocInfo, Engine, PageGeom, RenderKind, Request, Response, TextChar};
use crate::font;
use crate::model::*;
use crate::project::{self, DraftJob};
use egui::{Context, TextureHandle, TextureOptions};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Zoom {
    Fit,
    Factor(f32),
}

pub const ZOOM_PRESETS: [f32; 7] = [0.5, 0.75, 1.0, 1.25, 1.5, 2.0, 3.0];
pub const STROKES: [f64; 4] = [1.0, 2.5, 5.0, 9.0];

pub struct Doc {
    pub bytes: Arc<Vec<u8>>,
    pub name: String,
    pub path: Option<PathBuf>,
    pub info: DocInfo,
}

pub struct PendingOpen {
    pub bytes: Arc<Vec<u8>>,
    pub name: String,
    pub path: Option<PathBuf>,
    pub restored: Option<Edits>,
    pub page: usize,
    pub password: Option<String>,
}

pub enum Gesture {
    Create { start: Pt },
    Move { start: Pt, original: Mark },
    TextSelect { anchor: usize },
}

pub struct TextEditState {
    pub position: Pt,
    pub page: u32,
    pub editing: Option<Mark>,
    pub text: String,
    pub size: u32,
    pub color: [u8; 3],
    pub error: Option<String>,
}

pub enum Dialog {
    Text(TextEditState),
    Password { message: String, value: String },
    Url { value: String },
    Help,
    ConfirmReplace(Box<PendingOpen>),
    ConfirmClose,
    ConfirmForget,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SidebarTab {
    Pages,
    Notes,
}

pub struct SearchMatch {
    pub page: usize,
    pub rects: Vec<Rect>,
}

#[derive(Default)]
pub struct Search {
    pub input: String,
    pub query: String,
    pub matches: Vec<SearchMatch>,
    pub cursor: Option<usize>,
    pub pending: Option<String>,
    pub status: String,
    pub focus: bool,
}

pub struct Toast {
    pub message: String,
    pub error: bool,
    pub until: Instant,
}

pub enum Job {
    Downloaded(Result<(Vec<u8>, String), String>),
    ProjectSaved(Result<PathBuf, String>),
    Draft(Result<(), String>),
}

pub struct MainTexture {
    pub page: usize,
    pub width: u32,
    pub texture: TextureHandle,
}

pub struct PdfMarkApp {
    pub ctx: Context,
    pub engine: Engine,
    /// Epoch of the open document (used for render/text/export requests).
    pub epoch: u64,
    /// Epoch of the most recent open attempt.
    pub open_epoch: u64,
    pub doc: Option<Doc>,
    pub pending: Option<PendingOpen>,
    pub fatal: Option<String>,

    pub tool: Tool,
    pub tool_colors: HashMap<Tool, [u8; 3]>,
    pub color: [u8; 3],
    pub stroke: f64,
    pub font_size: u32,

    pub history: History,
    pub edits: Edits,
    pub selected: Option<String>,
    pub preview: Option<Mark>,
    pub gesture: Option<Gesture>,
    pub form_buffers: HashMap<String, String>,
    pub form_dirty: bool,
    /// Rotated text field being edited in a pop-up editor (field name, needs focus).
    pub popup_field: Option<(String, bool)>,

    pub page: usize,
    pub zoom: Zoom,
    pub scale: f32,
    pub scroll_to: Option<egui::Vec2>,
    pub geoms: Vec<Option<PageGeom>>,
    pub main_tex: Option<MainTexture>,
    pub main_requested: Option<(usize, u32, u32)>,
    pub thumbs: HashMap<usize, TextureHandle>,
    pub thumbs_requested: HashSet<usize>,
    pub texts: HashMap<usize, Vec<TextChar>>,
    pub texts_requested: HashSet<usize>,
    pub text_selection: Option<(usize, usize)>,

    pub search: Search,
    pub sidebar: bool,
    pub sidebar_tab: SidebarTab,
    pub dialog: Option<Dialog>,
    pub toast: Option<Toast>,
    pub busy: Option<String>,

    pub dirty: bool,
    pub draft_enabled: bool,
    pub draft_exists: bool,
    pub draft_due: Option<Instant>,
    pub draft_source_sent: bool,
    pub save_status: String,
    pub draft_tx: Sender<DraftJob>,
    pub jobs_tx: Sender<Job>,
    pub jobs_rx: Receiver<Job>,
    pub export_target: Option<PathBuf>,
    pub allow_close: bool,
    pub title: String,
    pub logo: Option<TextureHandle>,
    pub signature_warned: bool,
    /// Set by PDFMARK_SMOKE_TEST: quit once a page has been rendered (used by CI).
    pub smoke_test: bool,
}

impl PdfMarkApp {
    pub fn new(ctx: Context, initial: Option<PathBuf>) -> Self {
        crate::theme::install(&ctx);
        let wake_ctx = ctx.clone();
        let engine = Engine::start(move || wake_ctx.request_repaint());
        let (jobs_tx, jobs_rx) = channel::<Job>();
        let draft_jobs = jobs_tx.clone();
        let draft_ctx = ctx.clone();
        let draft_tx = project::start_draft_writer(move |r| {
            let _ = draft_jobs.send(Job::Draft(r));
            draft_ctx.request_repaint();
        });
        let logo = image::load_from_memory(include_bytes!("../assets/icon128.png")).ok().map(|img| {
            let rgba = img.to_rgba8();
            let size = [rgba.width() as usize, rgba.height() as usize];
            ctx.load_texture("logo", egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()), TextureOptions::LINEAR)
        });
        let mut tool_colors = HashMap::new();
        for t in Tool::ALL {
            if let Some(c) = t.default_color() {
                tool_colors.insert(t, c);
            }
        }
        let mut app = PdfMarkApp {
            ctx,
            engine,
            epoch: 0,
            open_epoch: 0,
            doc: None,
            pending: None,
            fatal: None,
            tool: Tool::Read,
            tool_colors,
            color: Tool::Highlight.default_color().unwrap(),
            stroke: 2.5,
            font_size: 16,
            history: History::new(Edits::default()),
            edits: Edits::default(),
            selected: None,
            preview: None,
            gesture: None,
            form_buffers: HashMap::new(),
            form_dirty: false,
            popup_field: None,
            page: 0,
            zoom: Zoom::Fit,
            scale: 1.0,
            scroll_to: None,
            geoms: vec![],
            main_tex: None,
            main_requested: None,
            thumbs: HashMap::new(),
            thumbs_requested: HashSet::new(),
            texts: HashMap::new(),
            texts_requested: HashSet::new(),
            text_selection: None,
            search: Search::default(),
            sidebar: true,
            sidebar_tab: SidebarTab::Pages,
            dialog: None,
            toast: None,
            busy: None,
            dirty: false,
            draft_enabled: true,
            draft_exists: project::draft_exists(),
            draft_due: None,
            draft_source_sent: false,
            save_status: "Private. Local. Yours.".into(),
            draft_tx,
            jobs_tx,
            jobs_rx,
            export_target: None,
            allow_close: false,
            title: String::new(),
            logo,
            signature_warned: false,
            smoke_test: std::env::var_os("PDFMARK_SMOKE_TEST").is_some(),
        };
        if let Some(path) = initial {
            app.open_path(&path);
        }
        app
    }

    // ------------------------------------------------------------ messages

    pub fn toast(&mut self, message: impl Into<String>, error: bool) {
        let secs = if error { 10 } else { 5 };
        self.toast = Some(Toast { message: message.into(), error, until: Instant::now() + Duration::from_secs(secs) });
    }

    pub fn page_count(&self) -> usize {
        self.doc.as_ref().map(|d| d.info.page_count).unwrap_or(0)
    }

    pub fn encrypted(&self) -> bool {
        self.doc.as_ref().is_some_and(|d| d.info.encrypted)
    }

    /// Page size in viewport units (exact once the page has been loaded).
    pub fn page_size(&self, page: usize) -> (f64, f64) {
        if let Some(Some(g)) = self.geoms.get(page) {
            return (g.width, g.height);
        }
        self.doc.as_ref().and_then(|d| d.info.sizes.get(page).copied()).unwrap_or((612.0, 792.0))
    }

    // ------------------------------------------------------------ opening

    pub fn open_path(&mut self, path: &Path) {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "document.pdf".into());
        let result = (|| -> Result<(), String> {
            let meta = std::fs::metadata(path).map_err(|e| format!("Could not open {name}: {e}"))?;
            if name.to_lowercase().ends_with(".pdfmark") {
                if meta.len() as usize > MAX_PROJECT_BYTES {
                    return Err("Project files up to 150 MB are supported.".into());
                }
                let bytes = std::fs::read(path).map_err(|e| format!("Could not open {name}: {e}"))?;
                let p = project::parse_project(&bytes)?;
                validate_edits(p.edits.clone(), MAX_PAGES)?;
                self.open_bytes(p.pdf, p.name, None, Some(p.edits), p.page);
                return Ok(());
            }
            if meta.len() as usize > MAX_PDF_BYTES {
                return Err("PDFs up to 100 MB are supported.".into());
            }
            let bytes = std::fs::read(path).map_err(|e| format!("Could not open {name}: {e}"))?;
            self.open_bytes(bytes, name.clone(), Some(path.to_path_buf()), None, 1);
            Ok(())
        })();
        if let Err(e) = result {
            self.toast(e, true);
        }
    }

    pub fn open_bytes(&mut self, bytes: Vec<u8>, name: String, path: Option<PathBuf>, restored: Option<Edits>, page: usize) {
        if bytes.len() > MAX_PDF_BYTES {
            self.toast("PDFs up to 100 MB are supported.", true);
            return;
        }
        if !is_pdf(&bytes) {
            self.toast("This file is not a PDF. Download the PDF first, then choose Open PDF.", true);
            return;
        }
        let pending = PendingOpen { bytes: Arc::new(bytes), name: safe_filename(&name), path, restored, page, password: None };
        if self.doc.is_some() && self.dirty {
            self.dialog = Some(Dialog::ConfirmReplace(Box::new(pending)));
            return;
        }
        self.start_open(pending);
    }

    pub fn start_open(&mut self, pending: PendingOpen) {
        self.open_epoch = self.open_epoch.max(self.epoch) + 1;
        let _ =
            self.engine.tx.send(Request::Open { epoch: self.open_epoch, bytes: pending.bytes.clone(), password: pending.password.clone() });
        self.busy = Some("Opening PDF…".into());
        self.pending = Some(pending);
    }

    pub fn submit_password(&mut self, password: String) {
        if let Some(mut p) = self.pending.take() {
            p.password = Some(password);
            self.start_open(p);
        }
    }

    pub fn cancel_open(&mut self) {
        // The worker keeps the current document until a new one opens, so nothing else to undo.
        self.pending = None;
        self.busy = None;
    }

    fn finish_open(&mut self, info: DocInfo) {
        let Some(p) = self.pending.take() else { return };
        let mut restore_error = None;
        let edits = match p.restored.clone() {
            Some(e) => match validate_edits(e, info.page_count) {
                Ok(e) => e,
                Err(err) => {
                    restore_error = Some(format!("{err} The PDF was opened without the saved annotations."));
                    Edits::default()
                }
            },
            None => Edits::default(),
        };
        self.main_tex = None;
        self.main_requested = None;
        self.thumbs.clear();
        self.thumbs_requested.clear();
        self.texts.clear();
        self.texts_requested.clear();
        self.text_selection = None;
        self.geoms = vec![None; info.page_count];
        self.search = Search::default();
        self.form_buffers.clear();
        self.selected = None;
        self.preview = None;
        self.gesture = None;
        self.page = p.page.clamp(1, info.page_count) - 1;
        self.scroll_to = Some(egui::Vec2::ZERO);
        let encrypted = info.encrypted;
        let forms = info.forms.clone();
        self.doc = Some(Doc { bytes: p.bytes, name: p.name, path: p.path, info });
        self.epoch = self.open_epoch;
        self.history = History::new(edits);
        self.edits = self.history.current();
        self.dirty = false;
        self.draft_enabled = true;
        self.draft_source_sent = false;
        self.signature_warned = false;
        self.queue_save(true);
        if encrypted {
            self.toast("Unlocked for reading. Save an editable project to keep annotations; encrypted PDF export is unavailable.", false);
        } else if forms.has_xfa {
            self.toast(
                "This PDF contains XFA form data. XFA editing is unsupported; use typed annotations or a compatible AcroForm PDF.",
                true,
            );
        } else if forms.has_fields {
            self.toast("Form fields can be filled in Read mode. Export PDF saves your changes.", false);
        }
        if forms.has_signature && !self.signature_warned {
            self.signature_warned = true;
            self.toast("This PDF contains a signature field. Editing/exporting may invalidate digital signatures.", true);
        }
        if let Some(e) = restore_error {
            self.toast(e, true);
        }
        self.busy = None;
        self.set_tool(Tool::Read);
    }

    // ------------------------------------------------------------ worker responses

    pub fn poll(&mut self) {
        while let Ok(r) = self.engine.rx.try_recv() {
            match r {
                Response::Fatal(e) => {
                    self.fatal = Some(e.clone());
                    self.toast(e, true);
                }
                Response::NeedPassword { epoch, incorrect } if epoch == self.open_epoch => {
                    self.busy = None;
                    self.dialog = Some(Dialog::Password {
                        message: if incorrect {
                            "That password was incorrect. Please try again.".into()
                        } else {
                            "Enter the password to read this document.".into()
                        },
                        value: String::new(),
                    });
                }
                Response::OpenFailed { epoch, error } if epoch == self.open_epoch => {
                    self.busy = None;
                    self.pending = None;
                    self.toast(error, true);
                }
                Response::Opened { epoch, info } if epoch == self.open_epoch => self.finish_open(info),
                Response::Rendered { epoch, page, kind, geom, image } if epoch == self.epoch => {
                    if let Some(slot) = self.geoms.get_mut(page) {
                        *slot = Some(geom);
                    }
                    let color = egui::ColorImage::from_rgba_unmultiplied([image.width, image.height], &image.pixels);
                    match kind {
                        RenderKind::Main => {
                            let width = image.width as u32;
                            let tex = self.ctx.load_texture(format!("page-{page}"), color, TextureOptions::LINEAR);
                            self.main_tex = Some(MainTexture { page, width, texture: tex });
                        }
                        RenderKind::Thumb => {
                            let tex = self.ctx.load_texture(format!("thumb-{page}"), color, TextureOptions::LINEAR);
                            self.thumbs.insert(page, tex);
                        }
                    }
                }
                Response::Text { epoch, page, geom, chars } if epoch == self.epoch => {
                    if let Some(slot) = self.geoms.get_mut(page) {
                        *slot = Some(geom);
                    }
                    self.texts.insert(page, chars);
                }
                Response::Exported { epoch, result } if epoch == self.epoch => {
                    self.busy = None;
                    let target = self.export_target.take();
                    match (result, target) {
                        (Ok(bytes), Some(path)) => match write_atomic(&path, &bytes) {
                            Ok(()) => {
                                self.dirty = false;
                                self.toast(
                                    format!(
                                        "Saved {}. Save a project too if you want to move or edit these marks later.",
                                        path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
                                    ),
                                    false,
                                );
                            }
                            Err(e) => self.toast(format!("Could not save the PDF: {e}"), true),
                        },
                        (Err(e), _) => self.toast(e, true),
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        while let Ok(job) = self.jobs_rx.try_recv() {
            match job {
                Job::Downloaded(Ok((bytes, name))) => {
                    self.busy = None;
                    self.open_bytes(bytes, name, None, None, 1);
                }
                Job::Downloaded(Err(e)) => {
                    self.busy = None;
                    self.toast(e, true);
                }
                Job::ProjectSaved(Ok(path)) => {
                    self.busy = None;
                    self.dirty = false;
                    self.toast(
                        format!(
                            "Saved editable project {}. Open it in PDFMark to continue editing.",
                            path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
                        ),
                        false,
                    );
                }
                Job::ProjectSaved(Err(e)) => {
                    self.busy = None;
                    self.toast(e, true);
                }
                Job::Draft(Ok(())) => {
                    if self.draft_enabled {
                        self.save_status = "Draft saved on this device".into();
                        self.draft_exists = true;
                    }
                }
                Job::Draft(Err(_)) => {
                    self.save_status = "Local draft could not be saved".into();
                    self.toast("The recovery draft could not be saved. Save an editable project to keep your work.", true);
                }
            }
        }
        if let Some(due) = self.draft_due {
            if Instant::now() >= due {
                self.draft_due = None;
                self.flush_draft();
            } else {
                self.ctx.request_repaint_after(due - Instant::now());
            }
        }
    }

    // ------------------------------------------------------------ requests

    pub fn request_main(&mut self, width: u32, height: u32) {
        let key = (self.page, width, height);
        if self.main_requested == Some(key) {
            return;
        }
        self.main_requested = Some(key);
        let _ = self.engine.tx.send(Request::Render { epoch: self.epoch, page: self.page, kind: RenderKind::Main, width, height });
    }

    pub fn request_thumb(&mut self, page: usize, width: u32, height: u32) {
        if self.thumbs_requested.insert(page) {
            let _ = self.engine.tx.send(Request::Render { epoch: self.epoch, page, kind: RenderKind::Thumb, width, height });
        }
    }

    pub fn request_text(&mut self, page: usize) {
        if !self.texts.contains_key(&page) && self.texts_requested.insert(page) {
            let _ = self.engine.tx.send(Request::Text { epoch: self.epoch, page });
        }
    }

    // ------------------------------------------------------------ editing

    pub fn page_marks(&self) -> impl Iterator<Item = &Mark> {
        let p = self.page as u32 + 1;
        self.edits.marks.iter().filter(move |m| m.page == p)
    }

    pub fn commit(&mut self, next: Edits) {
        if next.marks.len() > MAX_MARKS {
            self.toast("This document has reached the 20,000 annotation limit.", true);
            return;
        }
        self.history.push(next);
        self.edits = self.history.current();
        self.dirty = true;
        self.form_dirty = false;
        self.queue_save(false);
    }

    pub fn add_marks(&mut self, marks: Vec<Mark>) {
        let mut next = self.edits.clone();
        next.marks.extend(marks);
        self.commit(next);
    }

    pub fn replace_mark(&mut self, mark: Mark) {
        let mut next = self.edits.clone();
        for m in next.marks.iter_mut() {
            if m.id == mark.id {
                *m = mark.clone();
            }
        }
        self.commit(next);
    }

    pub fn remove_mark(&mut self, id: &str) {
        self.selected = None;
        let mut next = self.edits.clone();
        next.marks.retain(|m| m.id != id);
        self.commit(next);
    }

    pub fn set_field(&mut self, name: &str, value: FieldValue) {
        let mut next = self.edits.clone();
        next.fields.insert(name.to_string(), value);
        self.commit(next);
    }

    /// Live typing in a text field: update without an undo step until focus leaves.
    pub fn type_field(&mut self, name: &str, value: String) {
        self.edits.fields.insert(name.to_string(), FieldValue::Text(value));
        self.history.replace_current(self.edits.clone());
        self.form_dirty = true;
        self.dirty = true;
        self.queue_save(false);
    }

    /// Records a finished text field edit as one undo step.
    pub fn commit_typing(&mut self) {
        if self.form_dirty {
            self.form_dirty = false;
            // The current state already holds the typed text; give it its own history entry.
            let current = self.edits.clone();
            self.history.push(current);
            self.edits = self.history.current();
        }
    }

    pub fn undo(&mut self) {
        self.commit_typing();
        self.cancel_gesture();
        let e = self.history.undo();
        self.restore(e);
    }

    pub fn redo(&mut self) {
        self.cancel_gesture();
        let e = self.history.redo();
        self.restore(e);
    }

    fn restore(&mut self, e: Edits) {
        self.edits = e;
        self.dirty = true;
        if let Some(s) = &self.selected
            && !self.edits.marks.iter().any(|m| &m.id == s)
        {
            self.selected = None;
        }
        self.form_buffers.clear();
        self.queue_save(false);
    }

    pub fn set_tool(&mut self, tool: Tool) {
        self.cancel_gesture();
        if self.tool.default_color().is_some() {
            self.tool_colors.insert(self.tool, self.color);
        }
        self.tool = tool;
        if let Some(c) = self.tool_colors.get(&tool) {
            self.color = *c;
        }
        self.text_selection = None;
        if tool != Tool::Select {
            self.selected = None;
        }
    }

    pub fn cancel_gesture(&mut self) {
        self.gesture = None;
        self.preview = None;
    }

    pub fn go_to_page(&mut self, page: usize) {
        let n = self.page_count();
        if n == 0 {
            return;
        }
        self.commit_typing();
        let p = page.min(n - 1);
        if p != self.page {
            self.cancel_gesture();
            self.text_selection = None;
            self.page = p;
            self.scroll_to = Some(egui::Vec2::ZERO);
            self.queue_save(false);
        }
    }

    pub fn zoom_by(&mut self, factor: f32) {
        let next = (self.scale * factor).clamp(0.25, 4.0);
        self.zoom = Zoom::Factor((next * 100.0).round() / 100.0);
    }

    pub fn open_text_dialog(&mut self, position: Pt, editing: Option<Mark>) {
        let (text, size, color) = match &editing {
            Some(m) => match &m.shape {
                Shape::Text { text, size, .. } => (text.clone(), *size as u32, m.rgb()),
                _ => return,
            },
            None => (String::new(), self.font_size, self.color),
        };
        self.dialog = Some(Dialog::Text(TextEditState { position, page: self.page as u32 + 1, editing, text, size, color, error: None }));
    }

    /// Validates and applies the text dialog. Returns an error message to show in the dialog.
    pub fn apply_text(&mut self, st: &TextEditState) -> Result<(), String> {
        let text = st.text.replace('\r', "").replace('\t', "    ").trim().to_string();
        if text.is_empty() {
            return Err("Type some text first.".into());
        }
        if !(5..=72).contains(&st.size) {
            return Err("Choose a text size from 5 to 72.".into());
        }
        let missing = font::unsupported_chars(&text);
        if !missing.is_empty() {
            return Err(font::unsupported_message(&missing));
        }
        let size = st.size as f64;
        let lines: Vec<String> = text.split('\n').map(|s| s.to_string()).collect();
        let f = font::font();
        let w = lines.iter().map(|l| f.measure(l, size)).fold(size / 2.0, f64::max);
        let h = lines.len() as f64 * size * LINE_HEIGHT;
        let (pw, ph) = self.page_size(st.page as usize - 1);
        if w > pw || h > ph {
            return Err("This text is larger than the page. Add line breaks, shorten it, or reduce its size.".into());
        }
        let x = st.position.x.clamp(0.0, pw - w);
        let y = st.position.y.clamp(0.0, ph - h);
        let mark = Mark {
            id: st.editing.as_ref().map(|m| m.id.clone()).unwrap_or_else(new_id),
            page: st.page,
            color: color_hex(st.color),
            shape: Shape::Text { x, y, w, h, size, text, lines },
        };
        if st.editing.is_some() {
            self.replace_mark(mark);
        } else {
            self.add_marks(vec![mark]);
        }
        self.font_size = st.size;
        Ok(())
    }

    // ------------------------------------------------------------ text selection & search

    pub fn selection_rects(&self) -> Vec<Rect> {
        let Some((a, b)) = self.text_selection else { return vec![] };
        let Some(chars) = self.texts.get(&self.page) else { return vec![] };
        line_rects(chars, a.min(b), a.max(b))
    }

    pub fn selection_text(&self) -> String {
        let Some((a, b)) = self.text_selection else { return String::new() };
        let Some(chars) = self.texts.get(&self.page) else { return String::new() };
        let (a, b) = (a.min(b), a.max(b).min(chars.len().saturating_sub(1)));
        let s: String = chars[a..=b].iter().map(|c| c.text.as_str()).collect();
        s.replace("\r\n", "\n").replace('\r', "\n")
    }

    pub fn highlight_selection(&mut self) {
        let rects = self.selection_rects();
        if rects.is_empty() {
            return;
        }
        let color = color_hex(*self.tool_colors.get(&Tool::Highlight).unwrap_or(&[0xed, 0xc9, 0x49]));
        let page = self.page as u32 + 1;
        let marks = rects
            .into_iter()
            .map(|r| Mark {
                id: new_id(),
                page,
                color: color.clone(),
                shape: Shape::Highlight { x: r.x, y: r.y, w: r.w, h: r.h, width: 1.0 },
            })
            .collect();
        self.add_marks(marks);
        self.text_selection = None;
    }

    pub fn find_next(&mut self) {
        if self.doc.is_none() {
            return;
        }
        let query = self.search.input.trim().to_lowercase();
        if query.is_empty() {
            self.search = Search { input: self.search.input.clone(), ..Default::default() };
            return;
        }
        if query == self.search.query && !self.search.matches.is_empty() {
            let next = self.search.cursor.map(|c| (c + 1) % self.search.matches.len()).unwrap_or(0);
            self.search.cursor = Some(next);
            self.navigate_match();
            return;
        }
        self.search.pending = Some(query);
        self.search.status = "Searching…".into();
        for p in 0..self.page_count() {
            self.request_text(p);
        }
    }

    pub fn progress_search(&mut self) {
        let Some(query) = self.search.pending.clone() else { return };
        let n = self.page_count();
        let have = (0..n).filter(|p| self.texts.contains_key(p)).count();
        if have < n {
            self.search.status = format!("Searching… {}%", have * 100 / n.max(1));
            return;
        }
        let mut matches = Vec::new();
        'pages: for p in 0..n {
            let chars = &self.texts[&p];
            for (a, b) in find_in_chars(chars, &query) {
                let rects = line_rects(chars, a, b);
                matches.push(SearchMatch { page: p, rects });
                if matches.len() >= 10_000 {
                    break 'pages;
                }
            }
        }
        self.search.pending = None;
        self.search.query = query;
        self.search.matches = matches;
        if self.search.matches.is_empty() {
            self.search.cursor = None;
            self.search.status = "No matches".into();
        } else {
            // Start from the first match on or after the current page.
            let start = self.search.matches.iter().position(|m| m.page >= self.page).unwrap_or(0);
            self.search.cursor = Some(start);
            self.navigate_match();
        }
    }

    fn navigate_match(&mut self) {
        let Some(c) = self.search.cursor else { return };
        let Some(m) = self.search.matches.get(c) else { return };
        let (page, first) = (m.page, m.rects.first().copied());
        self.search.status = format!("{} / {}", c + 1, self.search.matches.len());
        self.go_to_page(page);
        if let Some(r) = first {
            self.scroll_to =
                Some(egui::vec2(((r.x - 40.0) * self.scale as f64).max(0.0) as f32, ((r.y - 120.0) * self.scale as f64).max(0.0) as f32));
        }
    }

    // ------------------------------------------------------------ saving

    pub fn queue_save(&mut self, immediate: bool) {
        if self.doc.is_none() || !self.draft_enabled {
            return;
        }
        self.save_status = "Saving local draft…".into();
        if immediate {
            self.flush_draft();
        } else {
            self.draft_due = Some(Instant::now() + Duration::from_millis(600));
        }
    }

    fn flush_draft(&mut self) {
        let Some(d) = &self.doc else { return };
        if !self.draft_enabled {
            return;
        }
        let source = if self.draft_source_sent { None } else { Some(d.bytes.clone()) };
        self.draft_source_sent = true;
        let _ = self.draft_tx.send(DraftJob::Save {
            source,
            name: d.name.clone(),
            edits: self.edits.clone(),
            page: self.page + 1,
            source_len: d.bytes.len(),
        });
    }

    /// Synchronous final draft write on exit.
    pub fn save_draft_now(&mut self) {
        if self.draft_due.take().is_some() {
            self.flush_draft();
            // Give the writer thread a moment to finish.
            std::thread::sleep(Duration::from_millis(300));
        }
    }

    pub fn forget_draft(&mut self) {
        self.draft_due = None;
        self.draft_enabled = false;
        let _ = self.draft_tx.send(DraftJob::Forget);
        self.draft_exists = false;
        self.save_status = "Autosave paused · save a project to keep edits".into();
        self.toast("Local draft deleted.", false);
    }

    pub fn restore_draft(&mut self) {
        match project::read_draft() {
            Ok(Some(d)) => self.open_bytes(d.pdf, d.name, None, Some(d.edits), d.page),
            Ok(None) => {
                self.draft_exists = false;
                self.toast("No local draft is available.", false);
            }
            Err(e) => self.toast(e, true),
        }
    }

    pub fn choose_open(&mut self) {
        if self.busy.is_some() {
            return;
        }
        let mut dialog = rfd::FileDialog::new()
            .set_title("Open PDF")
            .add_filter("PDF or PDFMark project", &["pdf", "pdfmark"])
            .add_filter("PDF document", &["pdf"])
            .add_filter("PDFMark project", &["pdfmark"]);
        if let Some(dir) = self.doc.as_ref().and_then(|d| d.path.as_ref()).and_then(|p| p.parent()) {
            dialog = dialog.set_directory(dir);
        }
        if let Some(path) = dialog.pick_file() {
            self.open_path(&path);
        }
    }

    pub fn choose_open_project(&mut self) {
        if self.busy.is_some() {
            return;
        }
        if let Some(path) =
            rfd::FileDialog::new().set_title("Open editable project").add_filter("PDFMark project", &["pdfmark"]).pick_file()
        {
            self.open_path(&path);
        }
    }

    pub fn open_url(&mut self, url: String) {
        let tx = self.jobs_tx.clone();
        let ctx = self.ctx.clone();
        self.busy = Some("Downloading PDF…".into());
        std::thread::spawn(move || {
            let r = crate::fetch::download(&url);
            let _ = tx.send(Job::Downloaded(r));
            ctx.request_repaint();
        });
    }

    pub fn export_pdf(&mut self) {
        if self.doc.is_none() || self.busy.is_some() {
            return;
        }
        if self.encrypted() {
            self.toast("Encrypted PDF export is unsupported. Save an editable project or use an unencrypted source.", true);
            return;
        }
        self.commit_typing();
        let Some(d) = &self.doc else { return };
        let default = format!("{}-annotated.pdf", stem(&d.name));
        let mut dialog = rfd::FileDialog::new().set_title("Export edited PDF").add_filter("PDF document", &["pdf"]).set_file_name(&default);
        if let Some(dir) = d.path.as_ref().and_then(|p| p.parent()) {
            dialog = dialog.set_directory(dir);
        }
        let Some(mut path) = dialog.save_file() else { return };
        if path.extension().is_none() {
            path.set_extension("pdf");
        }
        self.export_to(path);
    }

    pub fn export_to(&mut self, path: PathBuf) {
        self.export_target = Some(path);
        self.busy = Some("Preparing edited PDF…".into());
        let _ = self.engine.tx.send(Request::Export { epoch: self.epoch, edits: self.edits.clone() });
    }

    pub fn save_project(&mut self) {
        if self.doc.is_none() || self.busy.is_some() {
            return;
        }
        self.commit_typing();
        let Some(d) = &self.doc else { return };
        let default = format!("{}.pdfmark", stem(&d.name));
        let mut dialog =
            rfd::FileDialog::new().set_title("Save editable project").add_filter("PDFMark project", &["pdfmark"]).set_file_name(&default);
        if let Some(dir) = d.path.as_ref().and_then(|p| p.parent()) {
            dialog = dialog.set_directory(dir);
        }
        let Some(mut path) = dialog.save_file() else { return };
        if path.extension().is_none() {
            path.set_extension("pdfmark");
        }
        let (name, bytes, edits, page) = (d.name.clone(), d.bytes.clone(), self.edits.clone(), self.page + 1);
        let tx = self.jobs_tx.clone();
        let ctx = self.ctx.clone();
        self.busy = Some("Saving editable project…".into());
        std::thread::spawn(move || {
            let r = project::project_json(&name, &bytes, &edits, page)
                .and_then(|json| write_atomic(&path, &json).map_err(|e| format!("Could not save the project: {e}")))
                .map(|_| path);
            let _ = tx.send(Job::ProjectSaved(r));
            ctx.request_repaint();
        });
    }
}

pub fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let file_name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "output".into());
    let tmp = path.with_file_name(format!(".{file_name}.pdfmark-tmp"));
    std::fs::write(&tmp, data)?;
    if let Err(e) = std::fs::rename(&tmp, path) {
        // Fallback for targets where rename-over is refused (e.g. some network shares).
        let _ = std::fs::remove_file(&tmp);
        std::fs::write(path, data).map_err(|_| e)?;
    }
    Ok(())
}

/// Merges the boxes of characters `a..=b` into one rectangle per text line.
pub fn line_rects(chars: &[TextChar], a: usize, b: usize) -> Vec<Rect> {
    let mut out: Vec<Rect> = Vec::new();
    let b = b.min(chars.len().saturating_sub(1));
    if chars.is_empty() || a > b {
        return out;
    }
    let mut cur: Option<Rect> = None;
    for c in &chars[a..=b] {
        let r = c.rect;
        if r.w <= 0.01 || r.h <= 0.01 || (c.generated && c.text.trim().is_empty()) {
            if (c.text.contains('\n') || c.text.contains('\r'))
                && let Some(x) = cur.take()
            {
                out.push(x);
            }
            continue;
        }
        cur = Some(match cur {
            Some(x) => {
                let same_line = same_line(&x, &r);
                if same_line {
                    x.union(&r)
                } else {
                    out.push(x);
                    r
                }
            }
            None => r,
        });
    }
    if let Some(x) = cur {
        out.push(x);
    }
    out.into_iter().filter(|r| r.w > 0.5 && r.h > 0.5).collect()
}

/// True when `r` continues the text run `x` (horizontal, rotated or vertical text).
fn same_line(x: &Rect, r: &Rect) -> bool {
    let overlap = |a0: f64, a1: f64, b0: f64, b1: f64| (a1.min(b1) - a0.max(b0)).max(0.0);
    let gap_x = (r.x - x.right()).max(x.x - r.right()).max(0.0);
    let gap_y = (r.y - x.bottom()).max(x.y - r.bottom()).max(0.0);
    let y_overlap = overlap(x.y, x.bottom(), r.y, r.bottom()) / x.h.min(r.h).max(0.01);
    let x_overlap = overlap(x.x, x.right(), r.x, r.right()) / x.w.min(r.w).max(0.01);
    (y_overlap > 0.5 && gap_x < 2.0 * x.h.max(r.h)) || (x_overlap > 0.5 && gap_y < 2.0 * x.w.max(r.w) && x.w < 3.0 * r.w)
}

/// Case-insensitive search over page characters. Returns inclusive char index ranges.
pub fn find_in_chars(chars: &[TextChar], query: &str) -> Vec<(usize, usize)> {
    let mut flat = String::new();
    let mut owner: Vec<u32> = Vec::new();
    for (i, c) in chars.iter().enumerate() {
        for ch in c.text.chars() {
            let ch = if ch.is_whitespace() { ' ' } else { ch };
            // Collapse whitespace runs (line breaks become a single space).
            if ch == ' ' && (flat.is_empty() || flat.ends_with(' ')) {
                continue;
            }
            // Keep a 1:1 char mapping: use the first lowercase char only.
            let lc = ch.to_lowercase().next().unwrap_or(ch);
            let before = flat.len();
            flat.push(lc);
            owner.extend(std::iter::repeat_n(i as u32, flat.len() - before));
        }
    }
    let q: String = query.split_whitespace().collect::<Vec<_>>().join(" ").chars().map(|c| c.to_lowercase().next().unwrap_or(c)).collect();
    let mut out = Vec::new();
    if q.is_empty() {
        return out;
    }
    let mut start = 0;
    while let Some(pos) = flat[start..].find(&q) {
        let s = start + pos;
        let e = s + q.len() - 1;
        out.push((owner[s] as usize, owner[e] as usize));
        start = s + q.len().max(1);
        if start >= flat.len() {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(t: &str, x: f64, y: f64) -> TextChar {
        TextChar { text: t.into(), rect: Rect { x, y, w: 5.0, h: 10.0 }, generated: false }
    }

    #[test]
    fn search_and_line_rects() {
        let mut chars: Vec<TextChar> = "Hello".chars().enumerate().map(|(i, c)| ch(&c.to_string(), i as f64 * 5.0, 0.0)).collect();
        chars.push(TextChar { text: "\r\n".into(), rect: Rect { x: 0.0, y: 0.0, w: 0.0, h: 0.0 }, generated: true });
        chars.extend("WORLD".chars().enumerate().map(|(i, c)| ch(&c.to_string(), i as f64 * 5.0, 20.0)));
        let m = find_in_chars(&chars, "lo w");
        assert_eq!(m, vec![(3, 6)]);
        let rects = line_rects(&chars, 3, 6);
        assert_eq!(rects.len(), 2, "{rects:?}");
        // Right-to-left (180° rotated) runs merge into one box too.
        let rtl: Vec<TextChar> = (0..5).map(|i| ch("x", 100.0 - i as f64 * 5.0, 0.0)).collect();
        assert_eq!(line_rects(&rtl, 0, 4).len(), 1);
        // Vertical (90° rotated) runs as well.
        let vertical: Vec<TextChar> = (0..5)
            .map(|i| TextChar { text: "x".into(), rect: Rect { x: 0.0, y: i as f64 * 5.0, w: 10.0, h: 5.0 }, generated: false })
            .collect();
        assert_eq!(line_rects(&vertical, 0, 4).len(), 1);
        assert_eq!(find_in_chars(&chars, "world").len(), 1);
        assert!(find_in_chars(&chars, "xyz").is_empty());
    }
}
