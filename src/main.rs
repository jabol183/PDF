// Hide the console window on Windows release builds.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use eframe::egui_wgpu::{WgpuSetup, wgpu};
use pdfmark::app::PdfMarkApp;
use std::path::PathBuf;

fn native_options(backends: Option<wgpu::Backends>) -> eframe::NativeOptions {
    let icon = image::load_from_memory(include_bytes!("../assets/icon128.png")).ok().map(|img| {
        let rgba = img.to_rgba8();
        egui::IconData { width: rgba.width(), height: rgba.height(), rgba: rgba.into_raw() }
    });
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("PDFMark — PDF Reader & Annotator")
        .with_app_id("pdfmark")
        .with_inner_size([1280.0, 860.0])
        .with_min_inner_size([760.0, 520.0])
        .with_drag_and_drop(true);
    if let Some(icon) = icon {
        viewport = viewport.with_icon(icon);
    }
    let mut options = eframe::NativeOptions { viewport, ..Default::default() };
    if let (Some(b), WgpuSetup::CreateNew(setup)) = (backends, &mut options.wgpu_options.wgpu_setup) {
        setup.instance_descriptor.backends = b;
    }
    options
}

fn main() {
    let initial: Option<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).find(|p| p.is_file());

    // Try the best graphics API first (Direct3D 12 / Vulkan), then plain OpenGL,
    // which also works on old GPUs, virtual machines and remote desktops.
    let attempts: [Option<wgpu::Backends>; 2] = [None, Some(wgpu::Backends::GL)];
    let mut last_error = String::new();
    for backends in attempts {
        let file = initial.clone();
        let result = eframe::run_native(
            "PDFMark",
            native_options(backends),
            Box::new(move |cc| Ok(Box::new(PdfMarkApp::new(cc.egui_ctx.clone(), file)))),
        );
        match result {
            Ok(()) => return,
            Err(e) => {
                eprintln!("Graphics start-up failed ({backends:?}): {e}");
                last_error = e.to_string();
            }
        }
    }
    if std::env::var_os("PDFMARK_SMOKE_TEST").is_some() {
        eprintln!("PDFMark could not start: {last_error}");
        std::process::exit(2);
    }
    rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title("PDFMark could not start")
        .set_description(format!(
            "PDFMark could not start its graphics system. Updating your graphics driver usually fixes this.\n\nDetails: {last_error}"
        ))
        .show();
    std::process::exit(1);
}
