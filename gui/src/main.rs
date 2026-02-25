mod app;
mod i18n;
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
        Box::new(|cc| {
            setup_cjk_font(&cc.egui_ctx);
            Ok(Box::new(app::EasyOcrApp::new(cc)))
        }),
    )
}

/// Attempt to load a CJK-capable system font so that Chinese UI text renders
/// correctly.  If no suitable font is found the app still works — only CJK
/// glyphs will be shown as replacement boxes.
fn setup_cjk_font(ctx: &egui::Context) {
    // Candidate paths ordered by preference (Linux, macOS, Windows).
    let candidates: &[&str] = &[
        // Linux — Noto CJK
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/google-noto-cjk/NotoSansCJKsc-Regular.otf",
        // Linux — WQY
        "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
        "/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc",
        "/usr/share/fonts/wqy-microhei/wqy-microhei.ttc",
        // macOS
        "/System/Library/Fonts/PingFang.ttc",
        "/System/Library/Fonts/STHeiti Light.ttc",
        "/Library/Fonts/Arial Unicode.ttf",
        // Windows
        "C:\\Windows\\Fonts\\msyh.ttc",
        "C:\\Windows\\Fonts\\simsun.ttc",
        "C:\\Windows\\Fonts\\simhei.ttf",
    ];

    for path in candidates {
        if let Ok(font_bytes) = std::fs::read(path) {
            let mut fonts = egui::FontDefinitions::default();
            fonts.font_data.insert(
                "cjk_font".to_owned(),
                egui::FontData::from_owned(font_bytes),
            );
            // Add after the default proportional font so Latin glyphs keep
            // their original rendering, but CJK characters fall through to
            // this font.
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .push("cjk_font".to_owned());
            fonts
                .families
                .entry(egui::FontFamily::Monospace)
                .or_default()
                .push("cjk_font".to_owned());
            ctx.set_fonts(fonts);
            return;
        }
    }
}
