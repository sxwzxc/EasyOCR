mod app;
mod ocr;
mod settings;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("EasyOCR")
            .with_inner_size([1000.0, 700.0])
            .with_min_inner_size([700.0, 500.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };

    eframe::run_native(
        "EasyOCR",
        options,
        Box::new(|cc| Ok(Box::new(app::EasyOcrApp::new(cc)))),
    )
}
