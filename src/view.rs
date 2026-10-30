//! All drawing and input handling for the main window.

use crate::app::*;
use crate::forms::{Widget, WidgetKind};
use crate::icons;
use crate::model::*;
use crate::theme::*;
use egui::Shape as EShape;
use egui::{
    Align, Align2, Color32, CornerRadius, CursorIcon, FontFamily, FontId, Frame, Id, Key, Layout, Margin, Modal, Modifiers, Order, Pos2,
    RichText, ScrollArea, Sense, Stroke, StrokeKind, TextEdit, Ui, ViewportCommand, pos2, vec2,
};

const PAGE_MARGIN: f32 = 32.0;

impl eframe::App for PdfMarkApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll();
        self.progress_search();

        // Drag & drop.
        let dropped: Vec<std::path::PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
        if let Some(path) = dropped.into_iter().next()
            && self.busy.is_none()
            && self.dialog.is_none()
        {
            self.open_path(&path);
        }

        // Closing with unsaved work.
        if ctx.input(|i| i.viewport().close_requested()) {
            if self.dirty && !self.allow_close {
                ctx.send_viewport_cmd(ViewportCommand::CancelClose);
                if self.dialog.is_none() {
                    self.dialog = Some(Dialog::ConfirmClose);
                }
            } else {
                self.save_draft_now();
            }
        }

        let title = match &self.doc {
            Some(d) => format!("{}{} — PDFMark", if self.dirty { "• " } else { "" }, d.name),
            None => "PDFMark — PDF Reader & Annotator".to_string(),
        };
        if title != self.title {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.title = title;
        }

        self.shortcuts(ctx);

        if self.smoke_test && self.doc.is_some() && self.main_tex.as_ref().is_some_and(|t| t.page == self.page) {
            println!("PDFMark smoke test: page {} rendered", self.page + 1);
            self.smoke_test = false;
            self.allow_close = true;
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }

        if let Some(t) = &self.toast {
            let now = std::time::Instant::now();
            if now >= t.until {
                self.toast = None;
            } else {
                ctx.request_repaint_after(t.until - now);
            }
        }
        if self.search.pending.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.header(ui);
        if self.doc.is_some() {
            self.toolbar(ui);
        }
        self.status_bar(ui);
        if self.doc.is_some() && self.sidebar {
            self.sidebar_panel(ui);
        }
        egui::CentralPanel::default().frame(Frame::new().fill(BG)).show(ui, |ui| {
            if self.doc.is_some() {
                self.workspace(ui);
            } else {
                self.welcome(ui);
            }
        });
        let ctx = ui.ctx().clone();
        self.selection_bar(&ctx);
        self.dialogs(&ctx);
        self.overlays(&ctx);
    }

    fn on_exit(&mut self) {
        self.save_draft_now();
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        BG.to_normalized_gamma_f32()
    }
}

fn primary_button(ui: &mut Ui, text: &str, enabled: bool) -> egui::Response {
    let btn = egui::Button::new(RichText::new(text).color(Color32::WHITE).strong())
        .fill(if enabled { ACCENT } else { Color32::from_rgb(0x9a, 0xae, 0xa6) })
        .min_size(vec2(0.0, 34.0))
        .corner_radius(CornerRadius::same(7));
    ui.add_enabled(enabled, btn)
}

fn secondary_button(ui: &mut Ui, text: &str, enabled: bool) -> egui::Response {
    ui.add_enabled(enabled, egui::Button::new(text).min_size(vec2(0.0, 34.0)))
}

fn icon_button(ui: &mut Ui, tooltip: &str, enabled: bool, draw: impl FnOnce(&egui::Painter, egui::Rect, Color32)) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(32.0, 32.0), if enabled { Sense::click() } else { Sense::hover() });
    if enabled && resp.hovered() {
        ui.painter().rect_filled(rect, 7.0, HOVER_BG);
    }
    let color = if enabled { INK } else { Color32::from_rgb(0xc2, 0xc8, 0xcf) };
    draw(ui.painter(), rect.shrink(6.0), color);
    if enabled { resp.on_hover_text(tooltip).on_hover_cursor(CursorIcon::PointingHand) } else { resp }
}

impl PdfMarkApp {
    // ------------------------------------------------------------ panels

    fn header(&mut self, ui: &mut Ui) {
        egui::Panel::top("header")
            .exact_size(60.0)
            .frame(Frame::new().fill(PANEL).inner_margin(Margin::symmetric(18, 0)).stroke(Stroke::new(1.0, BORDER)))
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    if let Some(logo) = &self.logo {
                        ui.add(egui::Image::new((logo.id(), vec2(28.0, 28.0))));
                    }
                    ui.label(RichText::new("PDF").size(20.0).strong().color(INK));
                    ui.add_space(-8.0);
                    ui.label(RichText::new("Mark").size(20.0).color(INK));
                    ui.add_space(10.0);
                    ui.separator();
                    ui.add_space(6.0);
                    let busy = self.busy.is_some();
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let has_doc = self.doc.is_some();
                        let export_enabled = has_doc && !self.encrypted() && !busy;
                        let export = primary_button(ui, "  Export PDF  ", export_enabled);
                        if export.clicked() {
                            self.export_pdf();
                        }
                        if has_doc {
                            export.on_hover_text(if self.encrypted() {
                                "Encrypted PDFs can be read and saved as an editable project, but not exported."
                            } else {
                                "Save a new PDF with all your annotations (Ctrl+S)"
                            });
                        }
                        ui.menu_button(RichText::new(" ••• ").strong(), |ui| {
                            ui.set_min_width(240.0);
                            if ui.button("Open from web address…").clicked() {
                                self.dialog = Some(Dialog::Url { value: String::new() });
                                ui.close();
                            }
                            if ui.button("Open editable project…").clicked() {
                                ui.close();
                                self.choose_open_project();
                            }
                            if ui.add_enabled(has_doc, egui::Button::new("Save editable project…   Ctrl+Shift+S")).clicked() {
                                ui.close();
                                self.save_project();
                            }
                            ui.separator();
                            if ui.add_enabled(self.draft_exists, egui::Button::new("Restore last local draft")).clicked() {
                                ui.close();
                                self.restore_draft();
                            }
                            if ui.button("Forget saved draft").clicked() {
                                ui.close();
                                self.dialog = Some(Dialog::ConfirmForget);
                            }
                            ui.separator();
                            if ui.button("Help & keyboard shortcuts").clicked() {
                                ui.close();
                                self.dialog = Some(Dialog::Help);
                            }
                        });
                        if secondary_button(ui, "Open PDF", !busy).on_hover_text("Open a PDF or .pdfmark project (Ctrl+O)").clicked() {
                            self.choose_open();
                        }
                        ui.add_space(8.0);
                        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                            ui.vertical(|ui| {
                                ui.add_space(12.0);
                                let name = self.doc.as_ref().map(|d| d.name.clone()).unwrap_or_else(|| "Your PDF, your notes.".into());
                                ui.add(egui::Label::new(RichText::new(name).strong().size(13.0)).truncate());
                                ui.label(RichText::new(&self.save_status).size(11.0).color(MUTED));
                            });
                        });
                    });
                });
            });
    }

    fn toolbar(&mut self, ui: &mut Ui) {
        egui::Panel::top("toolbar")
            .frame(Frame::new().fill(PANEL).inner_margin(Margin::symmetric(16, 8)).stroke(Stroke::new(1.0, BORDER)))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    for tool in Tool::ALL {
                        if self.tool_button(ui, tool).clicked() {
                            self.set_tool(tool);
                        }
                    }
                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(8.0);
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let before = self.color;
                    egui::color_picker::color_edit_button_srgb(ui, &mut self.color).on_hover_text("Annotation color");
                    if before != self.color && self.tool.default_color().is_some() {
                        self.tool_colors.insert(self.tool, self.color);
                    }
                    ui.label(RichText::new("Stroke").size(11.0).color(MUTED));
                    egui::ComboBox::from_id_salt("stroke").width(70.0).selected_text(format!("{} pt", self.stroke)).show_ui(ui, |ui| {
                        for s in STROKES {
                            ui.selectable_value(&mut self.stroke, s, format!("{s} pt"));
                        }
                    });
                    ui.label(RichText::new("Text").size(11.0).color(MUTED));
                    ui.add(egui::DragValue::new(&mut self.font_size).range(5..=72).suffix(" pt"))
                        .on_hover_text("Text size from 5 to 72 (drag or type)");
                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(4.0);
                    if icon_button(ui, "Undo (Ctrl+Z)", self.history.can_undo(), |p, r, c| icons::undo(p, r, c, false)).clicked() {
                        self.undo();
                    }
                    if icon_button(ui, "Redo (Ctrl+Shift+Z)", self.history.can_redo(), |p, r, c| icons::undo(p, r, c, true)).clicked() {
                        self.redo();
                    }
                    if icon_button(ui, "Delete selected annotation (Delete)", self.selected.is_some(), icons::trash).clicked()
                        && let Some(id) = self.selected.clone()
                    {
                        self.remove_mark(&id);
                    }
                });
            });
    }

    fn tool_button(&self, ui: &mut Ui, tool: Tool) -> egui::Response {
        let active = self.tool == tool;
        let font = FontId::proportional(12.5);
        let galley = ui.painter().layout_no_wrap(tool.label().to_string(), font, if active { ACTIVE_FG } else { INK });
        let size = vec2(galley.size().x + 38.0, 34.0);
        let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
        if active {
            ui.painter().rect_filled(rect, 7.0, ACTIVE_BG);
        } else if resp.hovered() {
            ui.painter().rect_filled(rect, 7.0, HOVER_BG);
        }
        let icon_rect = egui::Rect::from_center_size(pos2(rect.left() + 17.0, rect.center().y), vec2(18.0, 18.0));
        icons::tool(ui.painter(), icon_rect, tool, if active { ACTIVE_FG } else { INK });
        ui.painter().galley(pos2(rect.left() + 30.0, rect.center().y - galley.size().y / 2.0), galley, INK);
        resp.on_hover_text(format!("{} ({})", tool.hint(), tool.shortcut())).on_hover_cursor(CursorIcon::PointingHand)
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        egui::Panel::bottom("status")
            .exact_size(28.0)
            .frame(Frame::new().fill(PANEL).inner_margin(Margin::symmetric(16, 0)).stroke(Stroke::new(1.0, BORDER)))
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    let hint = if self.doc.is_some() { self.tool.hint() } else { "Open a PDF to get started." };
                    ui.label(RichText::new(hint).size(12.0).color(MUTED));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if let Some(d) = &self.doc {
                            let n = d.info.page_count;
                            ui.label(
                                RichText::new(format!(
                                    "{n} page{} · {:.1} MB{}",
                                    if n == 1 { "" } else { "s" },
                                    d.bytes.len() as f64 / 1024.0 / 1024.0,
                                    if d.info.encrypted { " · encrypted" } else { "" }
                                ))
                                .size(12.0)
                                .color(MUTED),
                            );
                        }
                    });
                });
            });
    }

    fn sidebar_panel(&mut self, ui: &mut Ui) {
        egui::Panel::left("sidebar")
            .exact_size(196.0)
            .resizable(false)
            .frame(Frame::new().fill(Color32::from_rgb(0xf7, 0xf9, 0xfa)).inner_margin(Margin::same(10)).stroke(Stroke::new(1.0, BORDER)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let tabs =
                        [(SidebarTab::Pages, "Pages".to_string()), (SidebarTab::Notes, format!("Notes  {}", self.edits.marks.len()))];
                    for (tab, label) in tabs {
                        let active = self.sidebar_tab == tab;
                        let text = RichText::new(label).size(13.0).color(if active { ACTIVE_FG } else { MUTED });
                        let b = egui::Button::new(if active { text.strong() } else { text })
                            .fill(if active { ACTIVE_BG } else { Color32::TRANSPARENT })
                            .stroke(Stroke::NONE)
                            .min_size(vec2(82.0, 28.0));
                        if ui.add(b).clicked() {
                            self.sidebar_tab = tab;
                        }
                    }
                });
                ui.add_space(6.0);
                egui::Panel::bottom("privacy").frame(Frame::new()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
                        ui.painter().circle_filled(r.center(), 4.0, Color32::from_rgb(0x78, 0x9b, 0x88));
                        ui.label(RichText::new("On-device editing").size(11.0).color(MUTED));
                    });
                });
                match self.sidebar_tab {
                    SidebarTab::Pages => self.thumbnails(ui),
                    SidebarTab::Notes => self.notes(ui),
                }
            });
    }

    fn thumbnails(&mut self, ui: &mut Ui) {
        let ppp = ui.ctx().pixels_per_point();
        let n = self.page_count();
        let current = self.page;
        let mut go = None;
        let mut visible = (usize::MAX, 0usize);
        ScrollArea::vertical().id_salt("thumbs").auto_shrink(false).show(ui, |ui| {
            let width = ui.available_width();
            for i in 0..n {
                let (pw, ph) = self.page_size(i);
                let tw = 140.0f32.min(width - 20.0);
                let th = (tw as f64 * ph / pw).clamp(20.0, 400.0) as f32;
                let (rect, resp) = ui.allocate_exact_size(vec2(width, th + 30.0), Sense::click());
                if ui.is_rect_visible(rect) {
                    visible.0 = visible.0.min(i);
                    visible.1 = visible.1.max(i);
                    let img = egui::Rect::from_min_size(pos2(rect.center().x - tw / 2.0, rect.top() + 4.0), vec2(tw, th));
                    if i == current {
                        ui.painter().rect_filled(img.expand(5.0), 6.0, ACTIVE_BG);
                        ui.painter().rect_stroke(img.expand(5.0), 6.0, Stroke::new(1.5, ACCENT), StrokeKind::Inside);
                    } else if resp.hovered() {
                        ui.painter().rect_filled(img.expand(5.0), 6.0, HOVER_BG);
                    }
                    ui.painter().rect_filled(img.translate(vec2(0.0, 1.0)), 1.0, Color32::from_black_alpha(25));
                    match self.thumbs.get(&i) {
                        Some(t) => {
                            ui.painter().image(t.id(), img, egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                        }
                        None => {
                            ui.painter().rect_filled(img, 1.0, Color32::WHITE);
                            self.request_thumb(i, (tw * ppp).round() as u32, (th * ppp).round() as u32);
                        }
                    }
                    ui.painter().text(
                        pos2(rect.center().x, img.bottom() + 13.0),
                        Align2::CENTER_CENTER,
                        format!("{}", i + 1),
                        FontId::proportional(11.5),
                        if i == current { ACTIVE_FG } else { MUTED },
                    );
                }
                if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                    go = Some(i);
                }
            }
        });
        if let Some(i) = go {
            self.go_to_page(i);
        }
        // Keep GPU memory bounded on long documents: drop thumbnails far off-screen.
        if visible.0 != usize::MAX && self.thumbs.len() > 160 {
            let (lo, hi) = (visible.0.saturating_sub(60), visible.1 + 60);
            self.thumbs.retain(|p, _| (lo..=hi).contains(p));
            self.thumbs_requested.retain(|p| (lo..=hi).contains(p));
        }
    }

    fn notes(&mut self, ui: &mut Ui) {
        if self.edits.marks.is_empty() {
            ui.add_space(12.0);
            ui.label(RichText::new("Your highlights, drawings, and text will appear here.").size(12.0).color(MUTED));
            return;
        }
        let mut pick = None;
        ScrollArea::vertical().id_salt("notes").auto_shrink(false).show(ui, |ui| {
            for m in &self.edits.marks {
                let active = self.selected.as_deref() == Some(m.id.as_str());
                let sub = match &m.shape {
                    Shape::Text { text, .. } => text.chars().take(110).collect::<String>(),
                    _ => "Click to select and move".into(),
                };
                let resp = Frame::new()
                    .fill(if active { ACTIVE_BG } else { Color32::WHITE })
                    .stroke(Stroke::new(1.0, if active { ACCENT } else { BORDER }))
                    .corner_radius(8)
                    .inner_margin(Margin::symmetric(10, 7))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            let (r, _) = ui.allocate_exact_size(vec2(4.0, 30.0), Sense::hover());
                            let c = m.rgb();
                            ui.painter().rect_filled(r, 2.0, Color32::from_rgb(c[0], c[1], c[2]));
                            ui.vertical(|ui| {
                                ui.label(RichText::new(format!("{} · Page {}", m.kind_label(), m.page)).strong().size(12.0));
                                ui.add(egui::Label::new(RichText::new(sub).size(11.0).color(MUTED)).truncate());
                            });
                        });
                    })
                    .response
                    .interact(Sense::click())
                    .on_hover_cursor(CursorIcon::PointingHand);
                if resp.clicked() {
                    pick = Some((m.id.clone(), m.page));
                }
                ui.add_space(4.0);
            }
        });
        if let Some((id, page)) = pick {
            self.set_tool(Tool::Select);
            self.selected = Some(id);
            self.go_to_page(page as usize - 1);
        }
    }

    fn welcome(&mut self, ui: &mut Ui) {
        let avail = ui.available_rect_before_wrap();
        ui.scope_builder(egui::UiBuilder::new().max_rect(avail).layout(Layout::top_down(Align::Center)), |ui| {
            ui.add_space((avail.height() * 0.16).max(20.0));
            if let Some(logo) = &self.logo {
                ui.add(egui::Image::new((logo.id(), vec2(88.0, 88.0))));
            }
            ui.add_space(14.0);
            ui.label(RichText::new("A LITTLE LESS PAPERWORK").size(11.0).color(ACCENT).strong());
            ui.add_space(4.0);
            ui.label(RichText::new("Make your mark.").size(40.0).strong().color(INK));
            ui.add_space(6.0);
            ui.label(RichText::new("Read, highlight, sketch, and write on your PDFs.").size(16.0).color(MUTED));
            ui.label(RichText::new("Everything stays right here on your computer.").size(16.0).color(MUTED));
            ui.add_space(22.0);
            let b = egui::Button::new(RichText::new("+  Open a PDF").size(16.0).color(Color32::WHITE).strong())
                .fill(ACCENT)
                .min_size(vec2(200.0, 46.0))
                .corner_radius(9);
            if ui.add_enabled(self.busy.is_none(), b).clicked() {
                self.choose_open();
            }
            ui.add_space(8.0);
            ui.label(RichText::new("or drop a PDF anywhere in this window").size(12.0).color(MUTED));
            if self.draft_exists {
                ui.add_space(6.0);
                if ui.link(RichText::new("Continue your last draft →").size(14.0)).clicked() {
                    self.restore_draft();
                }
            }
            if let Some(f) = &self.fatal {
                ui.add_space(12.0);
                ui.label(RichText::new(f).color(ERROR).size(13.0));
            }
            ui.add_space(30.0);
            ui.horizontal(|ui| {
                // Measure the pills so the row is truly centred.
                let labels = ["✎  Handwritten notes", "T  Typed text", "⬇  Real PDF export"];
                let gap = ui.spacing().item_spacing.x;
                let total: f32 = labels
                    .iter()
                    .map(|t| ui.painter().layout_no_wrap(t.to_string(), FontId::proportional(12.5), INK).size().x + 28.0)
                    .sum::<f32>()
                    + gap * (labels.len() - 1) as f32;
                ui.add_space(((ui.available_width() - total) / 2.0).max(0.0));
                for t in labels {
                    Frame::new().fill(Color32::WHITE).corner_radius(20).inner_margin(Margin::symmetric(14, 7)).show(ui, |ui| {
                        ui.label(RichText::new(t).size(12.5).color(INK));
                    });
                }
            });
        });
    }

    // ------------------------------------------------------------ workspace

    fn workspace(&mut self, ui: &mut Ui) {
        Frame::new().fill(PANEL).inner_margin(Margin::symmetric(12, 6)).stroke(Stroke::new(1.0, BORDER)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            self.document_controls(ui);
        });
        self.stage(ui);
    }

    fn document_controls(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            if icon_button(ui, "Toggle sidebar", true, |p, r, c| {
                p.rect_stroke(r.shrink(1.0), 2.0, Stroke::new(1.5, c), StrokeKind::Inside);
                p.line_segment(
                    [pos2(r.left() + r.width() * 0.38, r.top() + 1.0), pos2(r.left() + r.width() * 0.38, r.bottom() - 1.0)],
                    Stroke::new(1.5, c),
                );
            })
            .clicked()
            {
                self.sidebar = !self.sidebar;
            }
            ui.separator();
            let n = self.page_count();
            if icon_button(ui, "Previous page (Page Up)", self.page > 0, |p, r, c| {
                p.line_segment([pos2(r.center().x + 3.0, r.top() + 3.0), pos2(r.center().x - 3.0, r.center().y)], Stroke::new(1.8, c));
                p.line_segment([pos2(r.center().x - 3.0, r.center().y), pos2(r.center().x + 3.0, r.bottom() - 3.0)], Stroke::new(1.8, c));
            })
            .clicked()
            {
                self.go_to_page(self.page.saturating_sub(1));
            }
            let mut page_no = self.page + 1;
            let r = ui.add(egui::DragValue::new(&mut page_no).range(1..=n.max(1)).speed(0.1));
            if r.changed() && page_no >= 1 {
                self.go_to_page(page_no - 1);
            }
            ui.label(RichText::new(format!("/ {n}")).color(MUTED));
            if icon_button(ui, "Next page (Page Down)", self.page + 1 < n, |p, r, c| {
                p.line_segment([pos2(r.center().x - 3.0, r.top() + 3.0), pos2(r.center().x + 3.0, r.center().y)], Stroke::new(1.8, c));
                p.line_segment([pos2(r.center().x + 3.0, r.center().y), pos2(r.center().x - 3.0, r.bottom() - 3.0)], Stroke::new(1.8, c));
            })
            .clicked()
            {
                self.go_to_page(self.page + 1);
            }
            ui.separator();
            if icon_button(ui, "Zoom out (Ctrl+-)", true, |p, r, c| {
                p.line_segment([pos2(r.left() + 3.0, r.center().y), pos2(r.right() - 3.0, r.center().y)], Stroke::new(1.8, c));
            })
            .clicked()
            {
                self.zoom_by(1.0 / 1.2);
            }
            let label = match self.zoom {
                Zoom::Fit => "Fit width".to_string(),
                Zoom::Factor(f) => format!("{}%", (f * 100.0).round()),
            };
            egui::ComboBox::from_id_salt("zoom").width(92.0).selected_text(label).show_ui(ui, |ui| {
                ui.selectable_value(&mut self.zoom, Zoom::Fit, "Fit width");
                for z in ZOOM_PRESETS {
                    ui.selectable_value(&mut self.zoom, Zoom::Factor(z), format!("{}%", (z * 100.0).round()));
                }
            });
            if icon_button(ui, "Zoom in (Ctrl++)", true, |p, r, c| {
                p.line_segment([pos2(r.left() + 3.0, r.center().y), pos2(r.right() - 3.0, r.center().y)], Stroke::new(1.8, c));
                p.line_segment([pos2(r.center().x, r.top() + 3.0), pos2(r.center().x, r.bottom() - 3.0)], Stroke::new(1.8, c));
            })
            .clicked()
            {
                self.zoom_by(1.2);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(&self.search.status).size(12.0).color(MUTED));
                if icon_button(ui, "Find next (Enter)", true, |p, r, c| {
                    p.circle_stroke(pos2(r.center().x - 2.0, r.center().y - 2.0), 5.0, Stroke::new(1.6, c));
                    p.line_segment(
                        [pos2(r.center().x + 2.0, r.center().y + 2.0), pos2(r.right() - 1.0, r.bottom() - 1.0)],
                        Stroke::new(1.8, c),
                    );
                })
                .clicked()
                {
                    self.find_next();
                }
                let search = ui.add(TextEdit::singleline(&mut self.search.input).hint_text("Find in document…").desired_width(200.0));
                if self.search.focus {
                    self.search.focus = false;
                    search.request_focus();
                }
                if search.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    self.find_next();
                    search.request_focus();
                }
                if search.changed() && self.search.input.trim().is_empty() {
                    self.search = Search::default();
                }
            });
        });
    }

    fn stage(&mut self, ui: &mut Ui) {
        let avail = ui.available_size();
        let ppp = ui.ctx().pixels_per_point();
        let (pw, ph) = self.page_size(self.page);
        self.scale = match self.zoom {
            Zoom::Fit => ((avail.x - 2.0 * PAGE_MARGIN - 16.0) as f64 / pw).clamp(0.1, 3.0) as f32,
            Zoom::Factor(f) => f,
        };
        let scale = self.scale;
        let page_size = vec2((pw * scale as f64) as f32, (ph * scale as f64) as f32);

        // Ask for a crisp bitmap at the current zoom and screen density.
        let (mut bw, mut bh) = (page_size.x * ppp, page_size.y * ppp);
        let q = (16_000_000.0 / (bw * bh)).sqrt().min(16000.0 / bw).min(16000.0 / bh).min(1.0);
        bw *= q;
        bh *= q;
        let need = self.main_tex.as_ref().is_none_or(|t| t.page != self.page || (t.width as f32 - bw.round()).abs() > 1.0);
        if need {
            self.request_main(bw.round().max(1.0) as u32, bh.round().max(1.0) as u32);
        }
        self.request_text(self.page);

        let mut area = ScrollArea::both().id_salt("stage").auto_shrink(false);
        if let Some(off) = self.scroll_to.take() {
            area = area.scroll_offset(off);
        }
        area.show(ui, |ui| {
            let content = vec2((page_size.x + 2.0 * PAGE_MARGIN).max(ui.available_width()), page_size.y + 2.0 * PAGE_MARGIN);
            let (outer, _) = ui.allocate_exact_size(content, Sense::hover());
            let page_rect = egui::Rect::from_min_size(pos2(outer.center().x - page_size.x / 2.0, outer.top() + PAGE_MARGIN), page_size);
            self.page_canvas(ui, page_rect);
        });

        // Ctrl + mouse wheel zoom.
        let zoom = ui.input(|i| i.zoom_delta());
        if zoom != 1.0 && ui.rect_contains_pointer(ui.min_rect()) {
            self.zoom = Zoom::Factor((self.scale * zoom).clamp(0.25, 4.0));
        }
    }

    fn page_canvas(&mut self, ui: &mut Ui, page_rect: egui::Rect) {
        let scale = self.scale as f64;
        let origin = page_rect.min;
        let (pw, ph) = self.page_size(self.page);
        let to_screen = move |x: f64, y: f64| pos2(origin.x + (x * scale) as f32, origin.y + (y * scale) as f32);
        let from_screen =
            move |p: Pos2| Pt::new(((p.x - origin.x) as f64 / scale).clamp(0.0, pw), ((p.y - origin.y) as f64 / scale).clamp(0.0, ph));

        let resp = ui.interact(page_rect, Id::unique("page-canvas"), Sense::click_and_drag());
        self.handle_pointer(ui, &resp, from_screen);

        let painter = ui.painter_at(page_rect.expand(12.0));
        painter.rect_filled(page_rect.translate(vec2(0.0, 3.0)).expand(1.0), 3.0, Color32::from_black_alpha(22));
        painter.rect_filled(page_rect, 0.0, Color32::WHITE);
        match &self.main_tex {
            Some(t) if t.page == self.page => {
                painter.image(t.texture.id(), page_rect, egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            }
            _ => {
                painter.text(page_rect.center(), Align2::CENTER_CENTER, "Rendering page…", FontId::proportional(14.0), MUTED);
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
            }
        }

        // Search results on this page.
        if let Some(m) = self.search.cursor.and_then(|c| self.search.matches.get(c))
            && m.page == self.page
        {
            for r in &m.rects {
                let rr = egui::Rect::from_min_max(to_screen(r.x, r.y), to_screen(r.right(), r.bottom()));
                painter.rect_filled(rr, 1.0, Color32::from_rgba_unmultiplied(0xfb, 0xa6, 0x2c, 80));
                painter.rect_stroke(rr, 1.0, Stroke::new(0.8, Color32::from_rgb(0xdd, 0x8d, 0x1b)), StrokeKind::Middle);
            }
        }
        // Original-text selection.
        for r in self.selection_rects() {
            let rr = egui::Rect::from_min_max(to_screen(r.x, r.y), to_screen(r.right(), r.bottom()));
            painter.rect_filled(rr, 0.0, Color32::from_rgba_unmultiplied(0x4c, 0x82, 0xc3, 70));
        }

        self.form_layer(ui, &to_screen);

        // Annotations (drawn above form fields, like the extension's SVG layer).
        let painter = ui.painter_at(page_rect);
        let preview_id = self.preview.as_ref().map(|p| p.id.clone());
        for m in self.page_marks() {
            if Some(&m.id) != preview_id.as_ref() {
                paint_mark(&painter, m, &to_screen, scale);
            }
        }
        if let Some(p) = &self.preview {
            paint_mark(&painter, p, &to_screen, scale);
        }
        let selected = self
            .preview
            .as_ref()
            .filter(|p| Some(&p.id) == self.selected.as_ref())
            .or_else(|| self.page_marks().find(|m| Some(&m.id) == self.selected.as_ref()));
        if let Some(m) = selected {
            let b = m.bounds();
            let r = egui::Rect::from_min_max(to_screen(b.x, b.y), to_screen(b.right(), b.bottom())).expand(4.0);
            let pts = vec![r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
            painter.extend(EShape::dashed_line(&pts, Stroke::new(1.0, SELECT_BLUE), 4.0, 3.0));
        }
    }

    fn handle_pointer(&mut self, ui: &mut Ui, resp: &egui::Response, from_screen: impl Fn(Pos2) -> Pt) {
        if self.busy.is_some() || self.dialog.is_some() {
            return;
        }
        let tolerance = 5.0 / self.scale as f64;
        let hover = resp.hover_pos().map(&from_screen);
        let press_origin = ui.input(|i| i.pointer.press_origin());
        let top_mark = |app: &PdfMarkApp, p: Pt| -> Option<Mark> { app.page_marks().filter(|m| m.hit_test(p, tolerance)).last().cloned() };

        // Cursor feedback.
        if let Some(p) = hover {
            let cursor = match self.tool {
                Tool::Read => {
                    if self.texts.get(&self.page).is_some_and(|c| char_at(c, p, 2.0).is_some()) {
                        CursorIcon::Text
                    } else {
                        CursorIcon::Default
                    }
                }
                Tool::Select => {
                    if matches!(self.gesture, Some(Gesture::Move { .. })) {
                        CursorIcon::Grabbing
                    } else if top_mark(self, p).is_some() {
                        CursorIcon::Grab
                    } else {
                        CursorIcon::Default
                    }
                }
                Tool::Text => CursorIcon::Text,
                Tool::Eraser => {
                    if top_mark(self, p).is_some() {
                        CursorIcon::PointingHand
                    } else {
                        CursorIcon::Default
                    }
                }
                _ => CursorIcon::Crosshair,
            };
            ui.ctx().set_cursor_icon(cursor);
        }

        if resp.drag_started() {
            let start = from_screen(press_origin.unwrap_or_else(|| resp.interact_pointer_pos().unwrap_or_default()));
            match self.tool {
                Tool::Read => {
                    self.text_selection = None;
                    if let Some(chars) = self.texts.get(&self.page)
                        && let Some(i) = char_at(chars, start, 6.0).or_else(|| nearest_char(chars, start, 40.0))
                    {
                        self.gesture = Some(Gesture::TextSelect { anchor: i });
                        self.text_selection = Some((i, i));
                    }
                }
                Tool::Select => {
                    let mark = top_mark(self, start);
                    self.selected = mark.as_ref().map(|m| m.id.clone());
                    if let Some(m) = mark {
                        self.preview = Some(m.clone());
                        self.gesture = Some(Gesture::Move { start, original: m });
                    }
                }
                Tool::Eraser => {
                    if let Some(m) = top_mark(self, start) {
                        self.remove_mark(&m.id);
                    }
                }
                Tool::Text => {}
                t => {
                    self.selected = None;
                    let color = color_hex(self.color);
                    let width = self.stroke;
                    let shape = match t {
                        Tool::Pen => Shape::Pen { points: vec![start], width },
                        Tool::Arrow => Shape::Arrow { x: start.x, y: start.y, x2: start.x, y2: start.y, width },
                        Tool::Highlight => Shape::Highlight { x: start.x, y: start.y, w: 0.0, h: 0.0, width: 1.0 },
                        Tool::Rectangle => Shape::Rectangle { x: start.x, y: start.y, w: 0.0, h: 0.0, width },
                        _ => Shape::Ellipse { x: start.x, y: start.y, w: 0.0, h: 0.0, width },
                    };
                    self.preview = Some(Mark { id: new_id(), page: self.page as u32 + 1, color, shape });
                    self.gesture = Some(Gesture::Create { start });
                }
            }
        }

        if resp.dragged() && self.gesture.is_some() {
            let positions: Vec<Pos2> =
                ui.input(|i| i.events.iter().filter_map(|e| if let egui::Event::PointerMoved(p) = e { Some(*p) } else { None }).collect());
            let current = resp.interact_pointer_pos();
            let points: Vec<Pt> = if positions.is_empty() {
                current.into_iter().map(&from_screen).collect()
            } else {
                positions.into_iter().map(&from_screen).collect()
            };
            let (pw, ph) = self.page_size(self.page);
            let min_step = 0.5 / self.scale as f64;
            match (&self.gesture, &mut self.preview) {
                (Some(Gesture::TextSelect { anchor }), _) => {
                    let anchor = *anchor;
                    if let (Some(p), Some(chars)) = (points.last(), self.texts.get(&self.page))
                        && let Some(i) = char_at(chars, *p, 6.0).or_else(|| nearest_char(chars, *p, 60.0))
                    {
                        self.text_selection = Some((anchor, i));
                    }
                }
                (Some(Gesture::Move { start, original }), _) => {
                    if let Some(p) = points.last() {
                        let b = original.bounds();
                        let dx = (p.x - start.x).clamp(-b.x, pw - b.x - b.w);
                        let dy = (p.y - start.y).clamp(-b.y, ph - b.y - b.h);
                        self.preview = Some(original.translated(dx, dy));
                    }
                }
                (Some(Gesture::Create { start }), Some(preview)) => {
                    let start = *start;
                    for p in points {
                        match &mut preview.shape {
                            Shape::Pen { points, .. } => {
                                let last = *points.last().unwrap();
                                if points.len() < 10_000 && (last.x - p.x).hypot(last.y - p.y) > min_step {
                                    points.push(p);
                                }
                            }
                            Shape::Arrow { x2, y2, .. } => {
                                *x2 = p.x;
                                *y2 = p.y;
                            }
                            Shape::Highlight { x, y, w, h, .. }
                            | Shape::Rectangle { x, y, w, h, .. }
                            | Shape::Ellipse { x, y, w, h, .. } => {
                                let r = Rect::between(start, p);
                                (*x, *y, *w, *h) = (r.x, r.y, r.w, r.h);
                            }
                            Shape::Text { .. } => {}
                        }
                    }
                }
                _ => {}
            }
        }

        if resp.drag_stopped() {
            let gesture = self.gesture.take();
            let preview = self.preview.take();
            match (gesture, preview) {
                (Some(Gesture::Move { original, .. }), Some(moved)) => {
                    if moved != original {
                        self.replace_mark(moved);
                    }
                }
                (Some(Gesture::Create { .. }), Some(mark)) => {
                    let b = mark.bounds();
                    if matches!(mark.shape, Shape::Pen { .. }) || b.w.max(b.h) > 2.0 / self.scale as f64 {
                        self.selected = Some(mark.id.clone());
                        self.add_marks(vec![mark]);
                    }
                }
                _ => {}
            }
        }

        if resp.clicked() {
            let Some(p) = resp.interact_pointer_pos().map(&from_screen) else { return };
            match self.tool {
                Tool::Read => self.text_selection = None,
                Tool::Text => self.open_text_dialog(p, None),
                Tool::Eraser => {
                    if let Some(m) = top_mark(self, p) {
                        self.remove_mark(&m.id);
                    }
                }
                Tool::Select => self.selected = top_mark(self, p).map(|m| m.id),
                Tool::Pen => {
                    let mark = Mark {
                        id: new_id(),
                        page: self.page as u32 + 1,
                        color: color_hex(self.color),
                        shape: Shape::Pen { points: vec![p], width: self.stroke },
                    };
                    self.add_marks(vec![mark]);
                }
                _ => {}
            }
        }
        if resp.double_clicked()
            && self.tool == Tool::Select
            && let Some(p) = resp.interact_pointer_pos().map(&from_screen)
            && let Some(m) = top_mark(self, p).filter(|m| matches!(m.shape, Shape::Text { .. }))
        {
            let b = m.bounds();
            self.open_text_dialog(Pt::new(b.x, b.y), Some(m));
        }
    }

    // ------------------------------------------------------------ forms

    fn form_layer(&mut self, ui: &mut Ui, to_screen: &impl Fn(f64, f64) -> Pos2) {
        let Some(doc) = &self.doc else { return };
        let Some(Some(geom)) = self.geoms.get(self.page).copied() else { return };
        let widgets: Vec<Widget> = doc.info.forms.page(self.page).to_vec();
        if widgets.is_empty() {
            return;
        }
        let interactive = self.tool == Tool::Read && self.busy.is_none() && self.dialog.is_none();
        let scale = self.scale as f64;
        for (idx, w) in widgets.iter().enumerate() {
            let a = apply(&geom.transform, w.rect[0], w.rect[1]);
            let b = apply(&geom.transform, w.rect[2], w.rect[3]);
            let rect = egui::Rect::from_two_pos(to_screen(a.0, a.1), to_screen(b.0, b.1));
            if rect.width() < 2.0 || rect.height() < 2.0 {
                continue;
            }
            let value = self.edits.fields.get(&w.field).cloned().unwrap_or_else(|| w.initial.clone());
            let fs =
                ((if w.font_size > 0.0 { w.font_size } else { 12.0 }) * scale).clamp(8.0, (rect.height() - 2.0).max(8.0) as f64) as f32;
            let font = FontId::proportional(fs);
            let enabled = interactive && !w.read_only;
            let id = Id::unique(("field", &w.field, self.page, idx));
            let painter = ui.painter().clone();
            match &w.kind {
                WidgetKind::Text { multiline, password, max_len } => {
                    painter.rect_filled(rect, 0.0, FORM_BG);
                    let current = match &value {
                        FieldValue::Text(s) => s.clone(),
                        FieldValue::List(v) => v.join(", "),
                        FieldValue::Bool(_) => String::new(),
                    };
                    if *password {
                        painter.text(
                            rect.left_center() + vec2(3.0, 0.0),
                            Align2::LEFT_CENTER,
                            "•".repeat(current.chars().count().min(40)),
                            font,
                            INK,
                        );
                        continue;
                    }
                    let angle = (page_rotation(&geom.transform) - w.rotation).rem_euclid(360);
                    if angle != 0 {
                        // egui widgets cannot rotate: draw rotated text and edit in a pop-up.
                        let fs = ((if w.font_size > 0.0 { w.font_size } else { 12.0 }) * scale)
                            .clamp(8.0, (rect.width().min(rect.height()) - 2.0).max(8.0) as f64) as f32;
                        let galley = painter.layout_no_wrap(current.replace('\n', " "), FontId::proportional(fs), INK);
                        let h = galley.size().y;
                        let (pos, rot) = match angle {
                            90 => (pos2(rect.center().x + h / 2.0, rect.top() + 3.0), std::f32::consts::FRAC_PI_2),
                            180 => (pos2(rect.right() - 3.0, rect.center().y + h / 2.0), std::f32::consts::PI),
                            _ => (pos2(rect.center().x - h / 2.0, rect.bottom() - 3.0), 3.0 * std::f32::consts::FRAC_PI_2),
                        };
                        painter.with_clip_rect(rect).add(egui::epaint::TextShape::new(pos, galley, INK).with_angle(rot));
                        let resp = ui.interact(rect, id, if enabled { Sense::click() } else { Sense::hover() });
                        if enabled && resp.on_hover_cursor(CursorIcon::Text).on_hover_text(&w.label).clicked() {
                            self.popup_field = Some((w.field.clone(), true));
                        }
                        if self.popup_field.as_ref().is_some_and(|(f, _)| *f == w.field) && enabled {
                            let mut buf = self.form_buffers.get(&w.field).cloned().unwrap_or(current.clone());
                            let mut close = false;
                            let mut changed = false;
                            egui::Area::new(id.with("popup")).order(Order::Foreground).fixed_pos(rect.left_bottom() + vec2(0.0, 6.0)).show(
                                ui.ctx(),
                                |ui| {
                                    Frame::popup(ui.style()).show(ui, |ui| {
                                        ui.label(RichText::new(&w.label).size(12.0).color(MUTED));
                                        let edit = if *multiline {
                                            TextEdit::multiline(&mut buf).desired_rows(3)
                                        } else {
                                            TextEdit::singleline(&mut buf)
                                        };
                                        let mut edit = edit.desired_width(280.0);
                                        if let Some(n) = max_len {
                                            edit = edit.char_limit(*n);
                                        }
                                        let r = ui.add(edit);
                                        if let Some((_, focus)) = &mut self.popup_field
                                            && *focus
                                        {
                                            r.request_focus();
                                            *focus = false;
                                        }
                                        changed = r.changed();
                                        if r.lost_focus() {
                                            close = true;
                                        }
                                    });
                                },
                            );
                            if changed {
                                self.type_field(&w.field, buf.clone());
                            }
                            self.form_buffers.insert(w.field.clone(), buf);
                            if close {
                                self.popup_field = None;
                                self.commit_typing();
                            }
                        }
                        continue;
                    }
                    if !enabled {
                        let galley = painter.layout(current, font, INK, rect.width() - 6.0);
                        let y = if *multiline { rect.top() + 2.0 } else { rect.center().y - galley.size().y / 2.0 };
                        painter.with_clip_rect(rect).galley(pos2(rect.left() + 3.0, y), galley, INK);
                        continue;
                    }
                    let mut buf = self.form_buffers.get(&w.field).cloned().unwrap_or(current.clone());
                    let mut edit = if *multiline { TextEdit::multiline(&mut buf).desired_rows(1) } else { TextEdit::singleline(&mut buf) };
                    edit = edit
                        .id(id)
                        .font(font)
                        .text_color(INK)
                        .frame(egui::Frame::NONE)
                        .margin(Margin::symmetric(3, 1))
                        .desired_width(rect.width());
                    if let Some(n) = max_len {
                        edit = edit.char_limit(*n);
                    }
                    // Keep the editor (and its click area) inside the field's box.
                    let r = ui
                        .scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                            ui.set_clip_rect(rect.intersect(ui.clip_rect()));
                            if *multiline {
                                ScrollArea::vertical()
                                    .id_salt(id.with("scroll"))
                                    .max_height(rect.height())
                                    .auto_shrink(false)
                                    .show(ui, |ui| ui.add(edit))
                                    .inner
                            } else {
                                ui.add_sized(rect.size(), edit)
                            }
                        })
                        .inner
                        .on_hover_text(&w.label);
                    if r.has_focus() {
                        painter.rect_stroke(rect, 0.0, Stroke::new(1.5, SELECT_BLUE), StrokeKind::Outside);
                    }
                    if r.changed() {
                        self.type_field(&w.field, buf.clone());
                    }
                    self.form_buffers.insert(w.field.clone(), buf);
                    if r.lost_focus() {
                        self.commit_typing();
                    }
                }
                WidgetKind::Checkbox { .. } => {
                    let checked = matches!(value, FieldValue::Bool(true));
                    let resp = ui.interact(rect, id, if enabled { Sense::click() } else { Sense::hover() });
                    painter.rect_filled(rect, 0.0, if resp.hovered() && enabled { HOVER_BG } else { FORM_BG });
                    painter.rect_stroke(rect, 0.0, Stroke::new(1.0, Color32::from_rgb(0x9a, 0xa8, 0xb8)), StrokeKind::Inside);
                    if checked {
                        let r = rect.shrink(rect.width().min(rect.height()) * 0.2);
                        painter.line(
                            vec![pos2(r.left(), r.center().y), pos2(r.left() + r.width() * 0.4, r.bottom()), pos2(r.right(), r.top())],
                            Stroke::new((rect.height() * 0.12).max(1.5), INK),
                        );
                    }
                    if enabled && resp.on_hover_cursor(CursorIcon::PointingHand).on_hover_text(&w.label).clicked() {
                        self.set_field(&w.field, FieldValue::Bool(!checked));
                    }
                }
                WidgetKind::Radio { on } => {
                    let checked = matches!(&value, FieldValue::Text(s) if s == on);
                    let resp = ui.interact(rect, id, if enabled { Sense::click() } else { Sense::hover() });
                    let rad = rect.width().min(rect.height()) / 2.0;
                    painter.circle_filled(rect.center(), rad, if resp.hovered() && enabled { HOVER_BG } else { FORM_BG });
                    painter.circle_stroke(rect.center(), rad - 0.5, Stroke::new(1.0, Color32::from_rgb(0x9a, 0xa8, 0xb8)));
                    if checked {
                        painter.circle_filled(rect.center(), rad * 0.45, INK);
                    }
                    if enabled && resp.on_hover_cursor(CursorIcon::PointingHand).on_hover_text(&w.label).clicked() && !checked {
                        self.set_field(&w.field, FieldValue::Text(on.clone()));
                    }
                }
                WidgetKind::Combo { options } => {
                    let current = match &value {
                        FieldValue::Text(s) => s.clone(),
                        FieldValue::List(v) => v.first().cloned().unwrap_or_default(),
                        FieldValue::Bool(_) => String::new(),
                    };
                    let shown = options.iter().find(|(e, _)| *e == current).map(|(_, d)| d.clone()).unwrap_or(current.clone());
                    painter.rect_filled(rect, 0.0, FORM_BG);
                    if !enabled {
                        painter.with_clip_rect(rect).text(rect.left_center() + vec2(3.0, 0.0), Align2::LEFT_CENTER, shown, font, INK);
                        continue;
                    }
                    let mut chosen = None;
                    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                        ui.style_mut().override_font_id = Some(font.clone());
                        egui::ComboBox::from_id_salt(id).width(rect.width() - 8.0).selected_text(shown).show_ui(ui, |ui| {
                            for (e, d) in options {
                                if ui.selectable_label(*e == current, d).clicked() {
                                    chosen = Some(e.clone());
                                }
                            }
                        });
                    });
                    if let Some(c) = chosen
                        && c != current
                    {
                        self.set_field(&w.field, FieldValue::Text(c));
                    }
                }
                WidgetKind::List { options, multi } => {
                    let selected: Vec<String> = match &value {
                        FieldValue::List(v) => v.clone(),
                        FieldValue::Text(s) if !s.is_empty() => vec![s.clone()],
                        _ => vec![],
                    };
                    painter.rect_filled(rect, 0.0, FORM_BG);
                    let mut toggled = None;
                    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                        ui.set_clip_rect(rect);
                        ScrollArea::vertical().id_salt(id).max_height(rect.height()).auto_shrink(false).show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = 0.0;
                            for (e, d) in options {
                                let sel = selected.contains(e);
                                let r = ui.add_enabled(
                                    enabled,
                                    egui::Button::selectable(sel, RichText::new(d).font(font.clone()))
                                        .min_size(vec2(rect.width() - 4.0, 0.0)),
                                );
                                if r.clicked() {
                                    toggled = Some(e.clone());
                                }
                            }
                        });
                    });
                    if let Some(e) = toggled {
                        let next = if *multi {
                            let mut v = selected.clone();
                            if let Some(i) = v.iter().position(|x| *x == e) {
                                v.remove(i);
                            } else {
                                v.push(e);
                            }
                            v
                        } else {
                            vec![e]
                        };
                        self.set_field(&w.field, FieldValue::List(next));
                    }
                }
            }
        }
    }

    // ------------------------------------------------------------ floating UI

    fn selection_bar(&mut self, ctx: &egui::Context) {
        if self.tool != Tool::Read || self.text_selection.is_none() || self.dialog.is_some() {
            return;
        }
        if self.selection_rects().is_empty() {
            return;
        }
        egui::Area::new(Id::unique("selection-bar")).order(Order::Foreground).anchor(Align2::CENTER_BOTTOM, vec2(0.0, -46.0)).show(
            ctx,
            |ui| {
                Frame::new().fill(INK).corner_radius(10).inner_margin(Margin::symmetric(8, 6)).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let style = |t: &str| {
                            egui::Button::new(RichText::new(t).color(Color32::WHITE)).fill(Color32::TRANSPARENT).stroke(Stroke::NONE)
                        };
                        if ui.add(style("▰  Highlight selected text")).clicked() {
                            self.highlight_selection();
                        }
                        if ui.add(style("Copy")).on_hover_text("Ctrl+C").clicked() {
                            let s = self.selection_text();
                            ctx.copy_text(s);
                            self.toast("Copied.", false);
                        }
                    });
                });
            },
        );
    }

    fn overlays(&mut self, ctx: &egui::Context) {
        if let Some(msg) = self.busy.clone() {
            Modal::new(Id::unique("busy")).backdrop_color(Color32::from_black_alpha(40)).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().size(20.0).color(ACCENT));
                    ui.label(RichText::new(msg).size(14.0));
                });
            });
        }
        if ctx.input(|i| !i.raw.hovered_files.is_empty()) {
            let screen = ctx.content_rect();
            let painter = ctx.layer_painter(egui::LayerId::new(Order::Foreground, Id::unique("drop")));
            painter.rect_filled(screen, 0.0, Color32::from_rgba_unmultiplied(0x33, 0x5c, 0x4d, 200));
            painter.rect_stroke(screen.shrink(18.0), 16.0, Stroke::new(2.0, Color32::WHITE), StrokeKind::Inside);
            painter.text(
                screen.center() - vec2(0.0, 14.0),
                Align2::CENTER_CENTER,
                "Drop your PDF here",
                FontId::proportional(28.0),
                Color32::WHITE,
            );
            painter.text(
                screen.center() + vec2(0.0, 22.0),
                Align2::CENTER_CENTER,
                "or an editable .pdfmark project",
                FontId::proportional(15.0),
                Color32::WHITE,
            );
        }
        if let Some(t) = &self.toast {
            let (msg, error) = (t.message.clone(), t.error);
            let mut dismiss = false;
            egui::Area::new(Id::unique("toast")).order(Order::Tooltip).anchor(Align2::CENTER_BOTTOM, vec2(0.0, -40.0)).show(ctx, |ui| {
                let r = Frame::new()
                    .fill(if error { ERROR } else { INK })
                    .corner_radius(10)
                    .inner_margin(Margin::symmetric(16, 11))
                    .show(ui, |ui| {
                        ui.set_max_width(560.0);
                        ui.add(egui::Label::new(RichText::new(msg).color(Color32::WHITE).size(13.5)).wrap());
                    })
                    .response
                    .interact(Sense::click());
                if r.on_hover_text("Click to dismiss").clicked() {
                    dismiss = true;
                }
            });
            if dismiss {
                self.toast = None;
            }
        }
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.dialog.take() else { return };
        let mut keep = true;
        let modal =
            |id: &str| Modal::new(Id::unique(id)).frame(Frame::popup(&ctx.global_style()).inner_margin(Margin::same(24)).corner_radius(14));
        match &mut dialog {
            Dialog::Text(st) => {
                let mut apply = false;
                let r = modal("text-dialog").show(ctx, |ui| {
                    ui.set_width(440.0);
                    ui.label(RichText::new(if st.editing.is_some() { "Edit text" } else { "Add text" }).size(22.0).strong());
                    ui.label(RichText::new("Write a note, complete a blank, or add your name.").color(MUTED));
                    ui.add_space(10.0);
                    ui.label(RichText::new("Your text").size(12.0).strong());
                    // Ctrl+Enter applies; consume it before the text box turns it into a newline.
                    if ui.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::Enter)) {
                        apply = true;
                    }
                    let edit = ui.add(
                        TextEdit::multiline(&mut st.text)
                            .desired_rows(5)
                            .desired_width(f32::INFINITY)
                            .char_limit(10_000)
                            .font(FontId::new(15.0, FontFamily::Name(crate::font::FAMILY.into())))
                            .hint_text("Start typing…"),
                    );
                    if ui.memory(|m| m.focused().is_none()) {
                        edit.request_focus();
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.label("Size");
                        ui.add(egui::DragValue::new(&mut st.size).range(5..=72).suffix(" pt"));
                        ui.add_space(16.0);
                        ui.label("Color");
                        egui::color_picker::color_edit_button_srgb(ui, &mut st.color);
                    });
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new("Added text stays movable in your editable project. The original page text is preserved.")
                            .size(11.5)
                            .color(MUTED),
                    );
                    if let Some(e) = &st.error {
                        ui.add_space(4.0);
                        ui.label(RichText::new(e).color(ERROR).size(12.5));
                    }
                    ui.add_space(12.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if primary_button(ui, "  Apply text  ", true).on_hover_text("Ctrl+Enter").clicked() {
                            apply = true;
                        }
                        if secondary_button(ui, "Cancel", true).clicked() {
                            keep = false;
                        }
                    });
                });
                if r.should_close() {
                    keep = false;
                }
                if apply {
                    let snapshot = TextEditState {
                        position: st.position,
                        page: st.page,
                        editing: st.editing.clone(),
                        text: st.text.clone(),
                        size: st.size,
                        color: st.color,
                        error: None,
                    };
                    match self.apply_text(&snapshot) {
                        Ok(()) => keep = false,
                        Err(e) => st.error = Some(e),
                    }
                }
                if !keep {
                    self.selected = None;
                    self.set_tool(Tool::Read);
                }
            }
            Dialog::Password { message, value } => {
                let mut submit = false;
                let mut cancel = false;
                modal("password-dialog").show(ctx, |ui| {
                    ui.set_width(380.0);
                    ui.label(RichText::new("Password-protected PDF").size(20.0).strong());
                    ui.label(RichText::new(message.as_str()).color(if message.contains("incorrect") { ERROR } else { MUTED }));
                    ui.add_space(8.0);
                    let r = ui.add(TextEdit::singleline(value).password(true).desired_width(f32::INFINITY));
                    if ui.memory(|m| m.focused().is_none()) {
                        r.request_focus();
                    }
                    if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                        submit = true;
                    }
                    ui.add_space(6.0);
                    ui.label(RichText::new("Reading and annotating are supported. Encrypted PDF export is unavailable; save an editable project instead. Passwords are never stored.").size(11.5).color(MUTED));
                    ui.add_space(12.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if primary_button(ui, "  Open  ", !value.is_empty()).clicked() {
                            submit = true;
                        }
                        if secondary_button(ui, "Cancel", true).clicked() {
                            cancel = true;
                        }
                    });
                    if ui.input(|i| i.key_pressed(Key::Escape)) {
                        cancel = true;
                    }
                });
                if submit && !value.is_empty() {
                    keep = false;
                    let pw = std::mem::take(value);
                    self.submit_password(pw);
                } else if cancel {
                    keep = false;
                    self.cancel_open();
                }
            }
            Dialog::Url { value } => {
                let mut go = false;
                let r = modal("url-dialog").show(ctx, |ui| {
                    ui.set_width(460.0);
                    ui.label(RichText::new("Open from a web address").size(20.0).strong());
                    ui.label(RichText::new("Paste a direct link to a PDF. Pages that need you to sign in usually can't be downloaded this way — download the file in your browser and use Open PDF instead.").color(MUTED));
                    ui.add_space(8.0);
                    let r = ui.add(TextEdit::singleline(value).hint_text("https://example.com/document.pdf").desired_width(f32::INFINITY));
                    if ui.memory(|m| m.focused().is_none()) {
                        r.request_focus();
                    }
                    if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                        go = true;
                    }
                    ui.add_space(12.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if primary_button(ui, "  Download & open  ", !value.trim().is_empty()).clicked() {
                            go = true;
                        }
                        if secondary_button(ui, "Cancel", true).clicked() {
                            keep = false;
                        }
                    });
                });
                if r.should_close() {
                    keep = false;
                }
                if go && !value.trim().is_empty() {
                    keep = false;
                    let url = value.trim().to_string();
                    self.open_url(url);
                }
            }
            Dialog::Help => {
                let r = modal("help-dialog").show(ctx, |ui| {
                    ui.set_width(560.0);
                    ui.label(RichText::new("Make yourself at home.").size(22.0).strong());
                    ui.add_space(8.0);
                    ScrollArea::vertical().max_height(440.0).show(ui, |ui| {
                        for (title, body) in HELP {
                            ui.label(RichText::new(*title).strong());
                            ui.label(RichText::new(*body).color(INK).size(13.0));
                            ui.add_space(8.0);
                        }
                    });
                    ui.add_space(8.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if primary_button(ui, "  Got it  ", true).clicked() {
                            keep = false;
                        }
                    });
                });
                if r.should_close() {
                    keep = false;
                }
            }
            Dialog::ConfirmReplace(_) => {
                let mut open = false;
                let r = modal("confirm-replace").show(ctx, |ui| {
                    ui.set_width(420.0);
                    ui.label(RichText::new("Open a different document?").size(20.0).strong());
                    ui.add_space(6.0);
                    ui.label("Export your edited PDF or save an editable project first if you need to keep this work. The local recovery draft will be replaced.");
                    ui.add_space(14.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if primary_button(ui, "  Open anyway  ", true).clicked() {
                            open = true;
                        }
                        if secondary_button(ui, "Keep editing", true).clicked() {
                            keep = false;
                        }
                    });
                });
                if r.should_close() {
                    keep = false;
                }
                if open {
                    keep = false;
                    if let Dialog::ConfirmReplace(p) = std::mem::replace(&mut dialog, Dialog::Help) {
                        self.dirty = false;
                        self.start_open(*p);
                    }
                }
            }
            Dialog::ConfirmClose => {
                let r = modal("confirm-close").show(ctx, |ui| {
                    ui.set_width(440.0);
                    ui.label(RichText::new("Close with unsaved changes?").size(20.0).strong());
                    ui.add_space(6.0);
                    ui.label("Your latest changes are kept in the local recovery draft, but they are not in any file yet. Export the PDF or save an editable project to keep a copy.");
                    ui.add_space(14.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if primary_button(ui, "  Close anyway  ", true).clicked() {
                            keep = false;
                            self.allow_close = true;
                            ctx.send_viewport_cmd(ViewportCommand::Close);
                        }
                        if secondary_button(ui, "Save project…", true).clicked() {
                            keep = false;
                            self.save_project();
                        }
                        if secondary_button(ui, "Cancel", true).clicked() {
                            keep = false;
                        }
                    });
                });
                if r.should_close() {
                    keep = false;
                }
            }
            Dialog::ConfirmForget => {
                let r = modal("confirm-forget").show(ctx, |ui| {
                    ui.set_width(420.0);
                    ui.label(RichText::new("Delete the local recovery draft?").size(20.0).strong());
                    ui.add_space(6.0);
                    ui.label("Your open document and any saved files stay as they are. Autosave pauses until you open a document again.");
                    ui.add_space(14.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if primary_button(ui, "  Delete draft  ", true).clicked() {
                            keep = false;
                            self.forget_draft();
                        }
                        if secondary_button(ui, "Cancel", true).clicked() {
                            keep = false;
                        }
                    });
                });
                if r.should_close() {
                    keep = false;
                }
            }
        }
        if keep && self.dialog.is_none() {
            self.dialog = Some(dialog);
        }
    }

    // ------------------------------------------------------------ keyboard

    fn shortcuts(&mut self, ctx: &egui::Context) {
        if self.dialog.is_some() {
            return;
        }
        let cmd = Modifiers::COMMAND;
        let cmd_shift = Modifiers::COMMAND | Modifiers::SHIFT;
        if ctx.input_mut(|i| i.consume_key(cmd_shift, Key::S)) {
            self.save_project();
            return;
        }
        if ctx.input_mut(|i| i.consume_key(cmd, Key::S)) {
            self.export_pdf();
            return;
        }
        if ctx.input_mut(|i| i.consume_key(cmd, Key::O)) {
            self.choose_open();
            return;
        }
        if self.busy.is_some() || self.doc.is_none() {
            return;
        }
        if ctx.input_mut(|i| i.consume_key(cmd, Key::F)) {
            self.search.focus = true;
            return;
        }
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        if ctx.input_mut(|i| i.consume_key(cmd_shift, Key::Z)) || ctx.input_mut(|i| i.consume_key(cmd, Key::Y)) {
            self.redo();
            return;
        }
        if ctx.input_mut(|i| i.consume_key(cmd, Key::Z)) {
            self.undo();
            return;
        }
        let copy = ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Copy)));
        if copy && self.text_selection.is_some() {
            let s = self.selection_text();
            ctx.copy_text(s);
            return;
        }
        if ctx.input_mut(|i| i.consume_key(cmd, Key::Plus) || i.consume_key(cmd, Key::Equals)) {
            self.zoom_by(1.2);
        }
        if ctx.input_mut(|i| i.consume_key(cmd, Key::Minus)) {
            self.zoom_by(1.0 / 1.2);
        }
        if ctx.input_mut(|i| i.consume_key(cmd, Key::Num0)) {
            self.zoom = Zoom::Fit;
        }
        let pressed = |k: Key| ctx.input_mut(|i| i.consume_key(Modifiers::NONE, k));
        if pressed(Key::Delete) || pressed(Key::Backspace) {
            if let Some(id) = self.selected.clone() {
                self.remove_mark(&id);
            }
            return;
        }
        if pressed(Key::Escape) {
            self.cancel_gesture();
            self.selected = None;
            self.text_selection = None;
            return;
        }
        if pressed(Key::PageDown) || pressed(Key::ArrowRight) {
            self.go_to_page(self.page + 1);
        }
        if pressed(Key::PageUp) || pressed(Key::ArrowLeft) {
            self.go_to_page(self.page.saturating_sub(1));
        }
        if pressed(Key::Home) {
            self.go_to_page(0);
        }
        if pressed(Key::End) {
            self.go_to_page(self.page_count().saturating_sub(1));
        }
        for tool in Tool::ALL {
            let key = Key::from_name(&tool.shortcut().to_string());
            if let Some(k) = key
                && pressed(k)
            {
                self.set_tool(tool);
            }
        }
    }
}

/// Draws one annotation in screen space.
pub fn paint_mark(painter: &egui::Painter, m: &Mark, to_screen: &impl Fn(f64, f64) -> Pos2, scale: f64) {
    let c = m.rgb();
    let color = Color32::from_rgb(c[0], c[1], c[2]);
    let stroke = Stroke::new(((m.stroke_width() * scale) as f32).max(0.6), color);
    match &m.shape {
        Shape::Highlight { x, y, w, h, .. } => {
            let r = egui::Rect::from_min_max(to_screen(*x, *y), to_screen(x + w, y + h));
            painter.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(c[0], c[1], c[2], 82));
        }
        Shape::Rectangle { x, y, w, h, .. } => {
            let r = egui::Rect::from_min_max(to_screen(*x, *y), to_screen(x + w, y + h));
            painter.rect_stroke(r, 0.0, stroke, StrokeKind::Middle);
        }
        Shape::Ellipse { x, y, w, h, .. } => {
            let (cx, cy, rx, ry) = (x + w / 2.0, y + h / 2.0, w / 2.0, h / 2.0);
            let pts: Vec<Pos2> = (0..72)
                .map(|i| {
                    let a = i as f64 / 72.0 * std::f64::consts::TAU;
                    to_screen(cx + rx * a.cos(), cy + ry * a.sin())
                })
                .collect();
            painter.add(EShape::closed_line(pts, stroke));
        }
        Shape::Pen { points, .. } => {
            let pts: Vec<Pos2> = pen_points(points).iter().map(|p| to_screen(p.x, p.y)).collect();
            if let (Some(a), Some(b)) = (pts.first(), pts.last()) {
                painter.circle_filled(*a, stroke.width / 2.0, color);
                painter.circle_filled(*b, stroke.width / 2.0, color);
            }
            if stroke.width > 2.5 {
                for p in &pts[1..pts.len().saturating_sub(1)] {
                    painter.circle_filled(*p, stroke.width / 2.0, color);
                }
            }
            painter.add(EShape::line(pts, stroke));
        }
        Shape::Arrow { x, y, x2, y2, width } => {
            let heads = arrow_heads(*x, *y, *x2, *y2, *width);
            let tip = to_screen(*x2, *y2);
            painter.line_segment([to_screen(*x, *y), tip], stroke);
            for h in heads {
                painter.line_segment([tip, to_screen(h.x, h.y)], stroke);
            }
            for p in [to_screen(*x, *y), tip] {
                painter.circle_filled(p, stroke.width / 2.0, color);
            }
        }
        Shape::Text { x, y, size, lines, .. } => {
            let px = (*size * scale) as f32;
            if px < 1.0 {
                return;
            }
            let font = FontId::new(px, FontFamily::Name(crate::font::FAMILY.into()));
            for (i, line) in lines.iter().enumerate() {
                if line.is_empty() {
                    continue;
                }
                let galley = painter.layout_no_wrap(line.clone(), font.clone(), color);
                let baseline_offset =
                    galley.rows.first().and_then(|r| r.row.glyphs.first().map(|g| r.pos.y + g.pos.y)).unwrap_or(px * 0.93);
                let base = to_screen(*x, y + size + i as f64 * size * LINE_HEIGHT);
                painter.galley(pos2(base.x, base.y - baseline_offset), galley, color);
            }
        }
    }
}

/// Page rotation (0/90/180/270) from a viewport transform.
fn page_rotation(t: &Matrix) -> i64 {
    // Note: f64::signum(0.0) is 1.0, so compare explicitly.
    let sign = |v: f64| {
        if v > 1e-9 {
            1
        } else if v < -1e-9 {
            -1
        } else {
            0
        }
    };
    match (sign(t[0]), sign(t[1])) {
        (1, _) => 0,
        (-1, _) => 180,
        (0, 1) => 90,
        _ => 270,
    }
}

fn char_at(chars: &[crate::engine::TextChar], p: Pt, tol: f64) -> Option<usize> {
    chars.iter().position(|c| {
        let r = c.rect;
        r.w > 0.0 && p.x >= r.x - tol && p.x <= r.right() + tol && p.y >= r.y - tol * 0.3 && p.y <= r.bottom() + tol * 0.3
    })
}

fn nearest_char(chars: &[crate::engine::TextChar], p: Pt, max: f64) -> Option<usize> {
    let mut best: Option<(usize, f64)> = None;
    for (i, c) in chars.iter().enumerate() {
        let r = c.rect;
        if r.w <= 0.0 {
            continue;
        }
        let dx = if p.x < r.x {
            r.x - p.x
        } else if p.x > r.right() {
            p.x - r.right()
        } else {
            0.0
        };
        let dy = if p.y < r.y {
            r.y - p.y
        } else if p.y > r.bottom() {
            p.y - r.bottom()
        } else {
            0.0
        };
        let d = dx + dy * 3.0;
        if d <= max && best.is_none_or(|(_, bd)| d < bd) {
            best = Some((i, d));
        }
    }
    best.map(|(i, _)| i)
}

const HELP: &[(&str, &str)] = &[
    (
        "Open",
        "Click Open PDF, drop a PDF onto the window, or open PDFs with PDFMark from Explorer (right-click › Open with). The ••• menu can also download a PDF from a web address.",
    ),
    ("Highlight", "Drag a colored rectangle with Highlight, or select original text with Read and click “Highlight selected text”."),
    (
        "Write",
        "Use Pen for handwriting, or Text to place typed text. Use Move to drag your annotations; double-click a text annotation to edit it. Eraser removes whole added marks.",
    ),
    (
        "Forms",
        "Compatible text fields, checkboxes, dropdowns, list boxes and radio buttons appear directly on the page. Fill them in Read mode.",
    ),
    (
        "Save",
        "Export PDF writes your marks into a new PDF. Save editable project keeps the original PDF, editable annotations, and form changes in one .pdfmark file (compatible with the PDFMark browser extension). Your most recent working draft is also saved automatically on this computer.",
    ),
    (
        "Shortcuts",
        "V Read · M Move · H Highlight · P Pen · T Text · R Box · O Circle · A Arrow · E Eraser · Delete remove · Ctrl+Z undo · Ctrl+Shift+Z / Ctrl+Y redo · Ctrl+S export · Ctrl+Shift+S save project · Ctrl+O open · Ctrl+F search · Ctrl+C copy selected text · Page Up/Down change page · Ctrl+wheel or Ctrl +/− zoom · Escape deselect.",
    ),
    (
        "Limits",
        "This is an annotation editor, not an existing-text reflow editor or a secure redaction tool. Marks in exported PDFs are permanent page content; use the editable project to revise them. Digital signatures can become invalid after editing. Scanned PDFs work but have no text selection or search without OCR. Unicode text uses the included DejaVu Sans font; unsupported glyphs are rejected.",
    ),
    (
        "Privacy",
        "No accounts, analytics, or upload servers. Network access is used only when you ask to open a PDF from a web address. PDF scripts and links are not executed.",
    ),
];
