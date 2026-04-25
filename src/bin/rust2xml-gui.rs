//! `rust2xml-gui` — desktop UI binary.  Launches the egui app from
//! `rust2xml::gui::GuiApp`.

use eframe::egui;
use rust2xml::gui::GuiApp;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(format!("rust2xml v{}", rust2xml::VERSION))
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([800.0, 500.0]),
        ..Default::default()
    };
    eframe::run_native(
        "rust2xml-gui",
        options,
        Box::new(|_cc| Ok(Box::new(GuiApp::default()))),
    )
}
