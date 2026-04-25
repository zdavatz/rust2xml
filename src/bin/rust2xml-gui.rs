//! `rust2xml-gui` — desktop UI binary.  Launches the egui app from
//! `rust2xml::gui::GuiApp`.

use eframe::egui;
use rust2xml::gui::GuiApp;

/// 4K source PNG embedded into the binary at build time.  Decoded once
/// on startup and downscaled to a window-icon-sized RGBA buffer.
const ICON_PNG: &[u8] = include_bytes!("../../assets/icon.png");

fn load_icon() -> Option<egui::IconData> {
    let img = image::load_from_memory(ICON_PNG).ok()?;
    // 256x256 is plenty for taskbar / dock / window header on every OS.
    let resized = img.resize_exact(256, 256, image::imageops::FilterType::Lanczos3);
    let rgba = resized.to_rgba8();
    let (w, h) = rgba.dimensions();
    Some(egui::IconData {
        rgba: rgba.into_raw(),
        width: w,
        height: h,
    })
}

fn main() -> eframe::Result<()> {
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(format!("rust2xml v{}", rust2xml::VERSION))
        .with_inner_size([1280.0, 800.0])
        .with_min_inner_size([800.0, 500.0]);
    if let Some(icon) = load_icon() {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "rust2xml-gui",
        options,
        Box::new(|_cc| Ok(Box::new(GuiApp::default()))),
    )
}
