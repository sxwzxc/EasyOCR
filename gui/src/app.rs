use crate::i18n;
use crate::ocr::{self, OcrResult};
use crate::settings::{Decoder, Settings, UiLanguage};
use egui::{
    Color32, ColorImage, FontId, RichText, Rounding, Stroke, TextureHandle, Vec2,
};
use std::path::PathBuf;
use std::sync::mpsc::Receiver;

#[derive(PartialEq, Clone, Copy)]
enum SetupStatus {
    /// Background check is in progress.
    Checking,
    /// easyocr CLI was found.
    Ready,
    /// easyocr CLI could not be found.
    Missing,
}

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Ocr,
    Settings,
}

/// Holds an image that has been loaded and is ready for display + OCR.
struct LoadedImage {
    path: PathBuf,
    texture: TextureHandle,
    width: u32,
    height: u32,
}

enum OcrState {
    Idle,
    Running(Receiver<OcrResult>),
    Done,
    Error(String),
}

pub struct EasyOcrApp {
    tab: Tab,
    image: Option<LoadedImage>,
    ocr_state: OcrState,
    ocr_result_text: String,
    status_message: String,
    settings: Settings,
    settings_save_msg: Option<(String, bool)>, // (message, is_error)
    // Copy-to-clipboard confirmation timer
    copied_timer: f32,
    // Setup / dependency check state
    setup_status: SetupStatus,
    setup_rx: Option<Receiver<bool>>,
    show_setup_dialog: bool,
}

impl EasyOcrApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let settings = Settings::load();
        // Start a background check for the easyocr CLI so the window opens
        // immediately without any freeze.
        let setup_rx = ocr::check_easyocr_async(&settings.easyocr_exe);
        let s = i18n::get(&settings.ui_language);
        Self {
            tab: Tab::Ocr,
            image: None,
            ocr_state: OcrState::Idle,
            ocr_result_text: String::new(),
            status_message: s.status_load_image.into(),
            settings,
            settings_save_msg: None,
            copied_timer: 0.0,
            setup_status: SetupStatus::Checking,
            setup_rx: Some(setup_rx),
            show_setup_dialog: false,
        }
    }

    /// Shorthand to retrieve the current i18n string table.
    fn s(&self) -> &'static i18n::Strings {
        i18n::get(&self.settings.ui_language)
    }

    // ── image loading helpers ────────────────────────────────────────────────

    fn load_image_from_path(&mut self, path: PathBuf, ctx: &egui::Context) {
        match load_color_image_from_path(&path) {
            Ok(color_image) => {
                let w = color_image.size[0] as u32;
                let h = color_image.size[1] as u32;
                let texture = ctx.load_texture(
                    "ocr_image",
                    color_image,
                    egui::TextureOptions::LINEAR,
                );
                self.image = Some(LoadedImage { path: path.clone(), texture, width: w, height: h });
                self.ocr_state = OcrState::Idle;
                self.ocr_result_text.clear();
                let name = path.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                self.status_message = self.s().status_image_loaded.replacen("{}", &name, 1);
            }
            Err(e) => {
                self.status_message = self.s().status_failed_load_image.replacen("{}", &e, 1);
            }
        }
    }

    fn load_image_from_rgba(
        &mut self,
        rgba: Vec<u8>,
        width: usize,
        height: usize,
        ctx: &egui::Context,
        label: &str,
    ) {
        // Save to a temp PNG so that the easyocr CLI can read it.
        let tmp_path = std::env::temp_dir().join("easyocr_gui_tmp.png");
        if let Err(e) = save_rgba_as_png(&rgba, width as u32, height as u32, &tmp_path) {
            self.status_message = self.s().status_cant_save_tmp.replacen("{}", &e, 1);
            return;
        }

        let color_image = ColorImage::from_rgba_unmultiplied([width, height], &rgba);
        let texture = ctx.load_texture("ocr_image", color_image, egui::TextureOptions::LINEAR);
        self.image = Some(LoadedImage {
            path: tmp_path,
            texture,
            width: width as u32,
            height: height as u32,
        });
        self.ocr_state = OcrState::Idle;
        self.ocr_result_text.clear();
        self.status_message = self.s().status_image_loaded.replacen("{}", label, 1);
    }

    // ── actions ──────────────────────────────────────────────────────────────

    fn action_open_file(&mut self, ctx: &egui::Context) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Images", &["png", "jpg", "jpeg", "bmp", "gif", "tiff", "webp"])
            .pick_file()
        {
            self.load_image_from_path(path, ctx);
        }
    }

    fn action_paste_clipboard(&mut self, ctx: &egui::Context) {
        match arboard::Clipboard::new() {
            Ok(mut clipboard) => match clipboard.get_image() {
                Ok(img) => {
                    let w = img.width;
                    let h = img.height;
                    let bytes: Vec<u8> = img.bytes.into_owned();
                    let label = self.s().btn_paste_image;
                    self.load_image_from_rgba(bytes, w, h, ctx, label);
                }
                Err(_) => {
                    self.status_message = self.s().status_no_image_clipboard.into();
                }
            },
            Err(e) => {
                self.status_message = self.s().status_clipboard_unavailable.replacen("{}", &e.to_string(), 1);
            }
        }
    }

    fn action_screenshot(&mut self, ctx: &egui::Context) {
        match screenshots::Screen::all() {
            Ok(screens) => {
                if screens.is_empty() {
                    self.status_message = self.s().status_no_screens.into();
                    return;
                }
                let screen = &screens[0];
                match screen.capture() {
                    Ok(img) => {
                        let w = img.width() as usize;
                        let h = img.height() as usize;
                        let rgba: Vec<u8> = img.into_raw();
                        let label = self.s().btn_screenshot;
                        self.load_image_from_rgba(rgba, w, h, ctx, label);
                    }
                    Err(e) => {
                        self.status_message = self.s().status_screenshot_failed.replacen("{}", &e.to_string(), 1);
                    }
                }
            }
            Err(e) => {
                self.status_message = self.s().status_cant_enum_screens.replacen("{}", &e.to_string(), 1);
            }
        }
    }

    fn action_run_ocr(&mut self) {
        if let Some(loaded) = &self.image {
            self.ocr_state =
                OcrState::Running(ocr::run_ocr_async(&loaded.path, &self.settings));
            self.status_message = self.s().status_running_ocr.into();
            self.ocr_result_text.clear();
        }
    }

    fn action_copy_results(&mut self, ctx: &egui::Context) {
        if !self.ocr_result_text.is_empty() {
            ctx.output_mut(|o| o.copied_text = self.ocr_result_text.clone());
            self.copied_timer = 2.0; // show "Copied!" for 2 seconds
        }
    }

    // ── poll OCR thread ──────────────────────────────────────────────────────

    fn poll_ocr(&mut self) {
        let result = if let OcrState::Running(rx) = &self.ocr_state {
            rx.try_recv().ok()
        } else {
            None
        };

        if let Some(res) = result {
            if let Some(err) = res.error {
                self.status_message = self.s().status_ocr_failed.replacen(
                    "{}",
                    err.lines().next().unwrap_or(""),
                    1,
                );
                self.ocr_state = OcrState::Error(err);
            } else {
                let count = res.lines.len();
                self.ocr_result_text = res
                    .lines
                    .iter()
                    .map(|l| format!("{} ({:.1}%)", l.text, l.confidence * 100.0))
                    .collect::<Vec<_>>()
                    .join("\n");
                self.status_message = self.s().status_ocr_complete.replacen(
                    "{}",
                    &count.to_string(),
                    1,
                );
                self.ocr_state = OcrState::Done;
            }
        }
    }

    // ── setup / dependency dialog ─────────────────────────────────────────────

    fn draw_setup_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_setup_dialog {
            return;
        }

        let s = self.s();
        let mut open = true;
        egui::Window::new(s.setup_title)
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .min_width(500.0)
            .show(ctx, |ui| {
                ui.add_space(4.0);

                ui.label(
                    RichText::new(s.setup_not_found)
                        .strong()
                        .color(Color32::from_rgb(251, 191, 36)),
                );
                ui.label(s.setup_must_install);
                ui.add_space(12.0);

                // ── Step 1 ───────────────────────────────────────────────────
                ui.label(RichText::new(s.setup_step1_title).strong());
                ui.label(s.setup_step1_desc);
                ui.label(
                    RichText::new("  https://www.python.org/downloads/")
                        .monospace()
                        .color(Color32::from_rgb(96, 165, 250)),
                );
                ui.add_space(8.0);

                // ── Step 2 ───────────────────────────────────────────────────
                ui.label(RichText::new(s.setup_step2_title).strong());
                ui.label(s.setup_step2_desc);
                ui.label(
                    RichText::new("  pip install easyocr")
                        .monospace()
                        .color(Color32::from_rgb(74, 222, 128))
                        .size(14.0),
                );
                ui.add_space(8.0);

                // ── Step 3 ───────────────────────────────────────────────────
                ui.label(RichText::new(s.setup_step3_title).strong());
                ui.label(s.setup_step3_desc);
                ui.add_space(10.0);

                ui.separator();
                ui.add_space(6.0);

                ui.label(
                    RichText::new(s.setup_tip)
                        .color(Color32::GRAY)
                        .small(),
                );
                ui.add_space(10.0);

                // ── Buttons ──────────────────────────────────────────────────
                ui.horizontal(|ui| {
                    let checking = self.setup_status == SetupStatus::Checking;
                    ui.add_enabled_ui(!checking, |ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new(s.setup_btn_check)
                                        .color(Color32::WHITE)
                                        .strong(),
                                )
                                .fill(Color32::from_rgb(37, 99, 235))
                                .min_size(Vec2::new(120.0, 30.0)),
                            )
                            .clicked()
                        {
                            self.setup_rx =
                                Some(ocr::check_easyocr_async(&self.settings.easyocr_exe));
                            self.setup_status = SetupStatus::Checking;
                        }
                    });

                    if ui.button(s.setup_btn_continue).clicked() {
                        self.show_setup_dialog = false;
                    }

                    if self.setup_status == SetupStatus::Checking {
                        ui.spinner();
                        ui.label(
                            RichText::new(s.setup_checking).color(Color32::GRAY).small(),
                        );
                    }
                });
                ui.add_space(4.0);
            });

        if !open {
            self.show_setup_dialog = false;
        }
    }

    // ── UI helpers ───────────────────────────────────────────────────────────

    fn draw_tab_bar(&mut self, ui: &mut egui::Ui) {
        let s = self.s();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            tab_button(ui, s.tab_ocr, self.tab == Tab::Ocr, || {
                self.tab = Tab::Ocr
            });
            tab_button(ui, s.tab_settings, self.tab == Tab::Settings, || {
                self.tab = Tab::Settings
            });
        });
    }

    fn draw_ocr_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let s = self.s();
        // ── Toolbar ──────────────────────────────────────────────────────────
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if toolbar_button(ui, s.btn_open_image).clicked() {
                self.action_open_file(ctx);
            }
            if toolbar_button(ui, s.btn_paste_image).clicked() {
                self.action_paste_clipboard(ctx);
            }
            if toolbar_button(ui, s.btn_screenshot).clicked() {
                self.action_screenshot(ctx);
            }
            if self.setup_status == SetupStatus::Missing {
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new(s.btn_setup).color(Color32::WHITE).strong(),
                        )
                        .fill(Color32::from_rgb(202, 138, 4))
                        .rounding(Rounding::same(4.0))
                        .min_size(Vec2::new(80.0, 32.0)),
                    )
                    .on_hover_text(s.tooltip_setup)
                    .clicked()
                {
                    self.show_setup_dialog = true;
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let has_image = self.image.is_some();
                let is_running = matches!(self.ocr_state, OcrState::Running(_));
                ui.add_enabled_ui(has_image && !is_running, |ui| {
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new(s.btn_run_ocr).color(Color32::WHITE).strong(),
                            )
                            .fill(Color32::from_rgb(37, 99, 235))
                            .min_size(Vec2::new(120.0, 32.0)),
                        )
                        .clicked()
                    {
                        self.action_run_ocr();
                    }
                });
            });
        });
        ui.add_space(8.0);
        ui.separator();

        // ── Main content area (two-column layout) ────────────────────────────
        let panel_height = ui.available_height() - 40.0; // reserve status bar
        ui.horizontal(|ui| {
            // Left: image preview
            ui.allocate_ui(Vec2::new(ui.available_width() * 0.55, panel_height), |ui| {
                egui::Frame::dark_canvas(ui.style())
                    .rounding(Rounding::same(6.0))
                    .show(ui, |ui| {
                        ui.set_min_size(Vec2::new(ui.available_width(), panel_height - 2.0));
                        if let Some(loaded) = &self.image {
                            let max = ui.available_size() - Vec2::splat(8.0);
                            let img_aspect =
                                loaded.width as f32 / loaded.height as f32;
                            let (w, h) = fit_into(max.x, max.y, img_aspect);
                            ui.centered_and_justified(|ui| {
                                ui.image(egui::load::SizedTexture::new(
                                    loaded.texture.id(),
                                    Vec2::new(w, h),
                                ));
                            });
                        } else {
                            ui.centered_and_justified(|ui| {
                                ui.label(
                                    RichText::new(s.placeholder_drop_image)
                                        .color(Color32::GRAY)
                                        .size(16.0),
                                );
                            });
                        }
                    });
            });

            ui.add_space(8.0);

            // Right: results
            ui.vertical(|ui| {
                ui.set_height(panel_height);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(s.lbl_results).strong());
                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            let btn_label = if self.copied_timer > 0.0 {
                                s.btn_copied
                            } else {
                                s.btn_copy
                            };
                            if ui
                                .add_enabled(
                                    !self.ocr_result_text.is_empty(),
                                    egui::Button::new(btn_label),
                                )
                                .clicked()
                            {
                                self.action_copy_results(ctx);
                            }
                        },
                    );
                });
                ui.add_space(4.0);

                match &self.ocr_state {
                    OcrState::Running(_) => {
                        ui.centered_and_justified(|ui| {
                            ui.spinner();
                        });
                    }
                    OcrState::Error(err) => {
                        egui::ScrollArea::vertical()
                            .id_salt("err_scroll")
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new(err.as_str())
                                        .color(Color32::from_rgb(248, 113, 113))
                                        .monospace(),
                                );
                            });
                    }
                    _ => {
                        egui::ScrollArea::vertical()
                            .id_salt("result_scroll")
                            .show(ui, |ui| {
                                ui.add(
                                    egui::TextEdit::multiline(&mut self.ocr_result_text)
                                        .desired_width(f32::INFINITY)
                                        .desired_rows(30)
                                        .font(FontId::monospace(14.0)),
                                );
                            });
                    }
                }
            });
        });

        // ── Status bar ───────────────────────────────────────────────────────
        ui.separator();
        ui.horizontal(|ui| {
            let is_running = matches!(self.ocr_state, OcrState::Running(_));
            if is_running {
                ui.spinner();
                ui.add_space(4.0);
            }
            ui.label(
                RichText::new(&self.status_message)
                    .color(Color32::LIGHT_GRAY)
                    .small(),
            );
        });
    }

    fn draw_settings_tab(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            let s = self.s();
            ui.add_space(12.0);
            section_header(ui, s.section_languages);
            ui.horizontal(|ui| {
                ui.label(s.lbl_language_codes);
                ui.add(
                    egui::TextEdit::singleline(&mut self.settings.languages)
                        .desired_width(200.0)
                        .hint_text(s.hint_language_codes),
                );
                ui.label(
                    RichText::new(s.lbl_comma_separated)
                        .color(Color32::GRAY)
                        .small(),
                );
            });
            ui.add_space(12.0);

            section_header(ui, s.section_hardware);
            ui.checkbox(&mut self.settings.gpu, s.lbl_gpu);
            ui.horizontal(|ui| {
                ui.label(s.lbl_workers);
                ui.add(
                    egui::DragValue::new(&mut self.settings.workers)
                        .range(0..=64)
                        .suffix(s.lbl_workers_suffix),
                );
                ui.label(
                    RichText::new(s.hint_workers_auto).color(Color32::GRAY).small(),
                );
            });
            ui.checkbox(&mut self.settings.quantize, s.lbl_quantize);
            ui.add_space(12.0);

            section_header(ui, s.section_decoder);
            for dec in Decoder::all() {
                let label = match dec {
                    Decoder::Greedy => s.decoder_greedy,
                    Decoder::BeamSearch => s.decoder_beam,
                    Decoder::WordBeamSearch => s.decoder_word_beam,
                };
                ui.radio_value(&mut self.settings.decoder, dec.clone(), label);
            }
            ui.add_space(4.0);
            ui.add_enabled_ui(
                matches!(
                    self.settings.decoder,
                    Decoder::BeamSearch | Decoder::WordBeamSearch
                ),
                |ui| {
                    ui.horizontal(|ui| {
                        ui.label(s.lbl_beam_width);
                        ui.add(
                            egui::DragValue::new(&mut self.settings.beam_width)
                                .range(1..=50),
                        );
                    });
                },
            );
            ui.add_space(12.0);

            section_header(ui, s.section_recognition);
            ui.horizontal(|ui| {
                ui.label(s.lbl_batch_size);
                ui.add(
                    egui::DragValue::new(&mut self.settings.batch_size)
                        .range(1..=64),
                );
            });
            ui.horizontal(|ui| {
                ui.label(s.lbl_min_size);
                ui.add(
                    egui::DragValue::new(&mut self.settings.min_size)
                        .range(1..=200),
                );
            });
            ui.checkbox(&mut self.settings.paragraph, s.lbl_paragraph);
            ui.horizontal(|ui| {
                ui.label(s.lbl_margin);
                ui.add(
                    egui::Slider::new(&mut self.settings.add_margin, 0.0..=0.5)
                        .fixed_decimals(2),
                );
            });
            ui.add_space(12.0);

            section_header(ui, s.section_thresholds);
            threshold_row(ui, s.lbl_text_threshold, &mut self.settings.text_threshold, s.hint_text_threshold);
            threshold_row(ui, s.lbl_low_text, &mut self.settings.low_text, s.hint_low_text);
            threshold_row(ui, s.lbl_link_threshold, &mut self.settings.link_threshold, s.hint_link_threshold);
            threshold_row(ui, s.lbl_contrast_ths, &mut self.settings.contrast_ths, s.hint_contrast_ths);
            ui.horizontal(|ui| {
                ui.label(s.lbl_adjust_contrast);
                ui.add(
                    egui::Slider::new(&mut self.settings.adjust_contrast, 0.0..=1.0)
                        .fixed_decimals(2),
                );
                ui.label(
                    RichText::new(s.hint_adjust_contrast)
                        .color(Color32::GRAY)
                        .small(),
                );
            });
            ui.add_space(12.0);

            section_header(ui, s.section_paths);
            ui.horizontal(|ui| {
                ui.label(s.lbl_model_dir);
                ui.add(
                    egui::TextEdit::singleline(&mut self.settings.model_storage_directory)
                        .desired_width(260.0)
                        .hint_text(s.hint_model_dir),
                );
                if ui.small_button(s.btn_browse).clicked() {
                    if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                        self.settings.model_storage_directory =
                            dir.to_string_lossy().to_string();
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label(s.lbl_easyocr_exe);
                ui.add(
                    egui::TextEdit::singleline(&mut self.settings.easyocr_exe)
                        .desired_width(260.0)
                        .hint_text(s.hint_easyocr_exe),
                );
                if ui.small_button(s.btn_browse).clicked() {
                    if let Some(f) = rfd::FileDialog::new().pick_file() {
                        self.settings.easyocr_exe = f.to_string_lossy().to_string();
                    }
                }
            });
            ui.add_space(12.0);

            section_header(ui, s.section_ui);
            ui.horizontal(|ui| {
                ui.label(s.lbl_ui_language);
                ui.radio_value(&mut self.settings.ui_language, UiLanguage::Chinese, "中文");
                ui.radio_value(&mut self.settings.ui_language, UiLanguage::English, "English");
            });
            ui.add_space(16.0);

            ui.horizontal(|ui| {
                let s = self.s();
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new(s.btn_save_settings)
                                .color(Color32::WHITE)
                                .strong(),
                        )
                        .fill(Color32::from_rgb(37, 99, 235))
                        .min_size(Vec2::new(140.0, 32.0)),
                    )
                    .clicked()
                {
                    match self.settings.save() {
                        Ok(()) => {
                            self.settings_save_msg =
                                Some((s.msg_settings_saved.into(), false));
                        }
                        Err(e) => {
                            self.settings_save_msg =
                                Some((s.msg_settings_failed.replacen("{}", &e, 1), true));
                        }
                    }
                }

                if ui.button(s.btn_reset).clicked() {
                    self.settings = Settings::default();
                    self.settings_save_msg = None;
                }

                if let Some((msg, is_err)) = &self.settings_save_msg {
                    ui.label(
                        RichText::new(msg.as_str()).color(if *is_err {
                            Color32::from_rgb(248, 113, 113)
                        } else {
                            Color32::from_rgb(74, 222, 128)
                        }),
                    );
                }
            });
            ui.add_space(20.0);
        });
    }
}

impl eframe::App for EasyOcrApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Poll setup availability check.
        if self.setup_status == SetupStatus::Checking {
            if let Some(rx) = &self.setup_rx {
                if let Ok(available) = rx.try_recv() {
                    self.setup_rx = None;
                    if available {
                        self.setup_status = SetupStatus::Ready;
                        self.show_setup_dialog = false;
                    } else {
                        self.setup_status = SetupStatus::Missing;
                        self.show_setup_dialog = true;
                        self.status_message = self.s().status_easyocr_missing.into();
                    }
                } else {
                    ctx.request_repaint();
                }
            }
        }

        // Poll background OCR thread.
        self.poll_ocr();
        if matches!(self.ocr_state, OcrState::Running(_)) {
            ctx.request_repaint();
        }

        // Tick copy confirmation timer.
        if self.copied_timer > 0.0 {
            let dt = ctx.input(|i| i.unstable_dt);
            self.copied_timer -= dt;
            if self.copied_timer < 0.0 {
                self.copied_timer = 0.0;
            }
            ctx.request_repaint();
        }

        // Handle file drag-and-drop.
        if !ctx.input(|i| i.raw.dropped_files.is_empty()) {
            if let Some(file) = ctx.input(|i| i.raw.dropped_files.first().cloned()) {
                if let Some(path) = file.path {
                    self.load_image_from_path(path, ctx);
                }
            }
        }

        // ── Top panel: title + tabs ──────────────────────────────────────────
        egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("EasyOCR")
                        .strong()
                        .size(20.0)
                        .color(Color32::from_rgb(96, 165, 250)),
                );
                ui.add_space(16.0);
                self.draw_tab_bar(ui);
            });
            ui.add_space(4.0);
        });

        // ── Central panel: content ───────────────────────────────────────────
        egui::CentralPanel::default().show(ctx, |ui| {
            match self.tab {
                Tab::Ocr => self.draw_ocr_tab(ui, ctx),
                Tab::Settings => self.draw_settings_tab(ui),
            }
        });

        // ── Setup dialog (rendered on top of everything else) ────────────────
        self.draw_setup_dialog(ctx);
    }
}

// ── widget helpers ────────────────────────────────────────────────────────────

fn tab_button(ui: &mut egui::Ui, label: &str, active: bool, on_click: impl FnOnce()) {
    let fill = if active {
        Color32::from_rgb(37, 99, 235)
    } else {
        Color32::TRANSPARENT
    };
    let text_color = if active {
        Color32::WHITE
    } else {
        Color32::LIGHT_GRAY
    };
    let btn = egui::Button::new(RichText::new(label).color(text_color))
        .fill(fill)
        .stroke(Stroke::NONE)
        .rounding(Rounding::same(4.0))
        .min_size(Vec2::new(110.0, 28.0));
    if ui.add(btn).clicked() {
        on_click();
    }
}

fn toolbar_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.add(
        egui::Button::new(label)
            .rounding(Rounding::same(4.0))
            .min_size(Vec2::new(130.0, 32.0)),
    )
}

fn section_header(ui: &mut egui::Ui, title: &str) {
    ui.label(RichText::new(title).strong().size(14.0));
    ui.separator();
    ui.add_space(4.0);
}

fn threshold_row(ui: &mut egui::Ui, label: &str, value: &mut f32, hint: &str) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(egui::Slider::new(value, 0.0..=1.0).fixed_decimals(2));
        ui.label(RichText::new(hint).color(Color32::GRAY).small());
    });
}

// ── image utilities ───────────────────────────────────────────────────────────

fn load_color_image_from_path(path: &std::path::Path) -> Result<ColorImage, String> {
    let img = image::open(path).map_err(|e| e.to_string())?;
    let rgba = img.to_rgba8();
    let (w, h) = (rgba.width() as usize, rgba.height() as usize);
    Ok(ColorImage::from_rgba_unmultiplied([w, h], &rgba))
}

fn save_rgba_as_png(
    rgba: &[u8],
    width: u32,
    height: u32,
    path: &std::path::Path,
) -> Result<(), String> {
    image::save_buffer(path, rgba, width, height, image::ColorType::Rgba8)
        .map_err(|e| e.to_string())
}

/// Scale (w, h) image to fit within (max_w, max_h) preserving aspect ratio.
fn fit_into(max_w: f32, max_h: f32, aspect: f32) -> (f32, f32) {
    let by_width = (max_w, max_w / aspect);
    let by_height = (max_h * aspect, max_h);
    if by_width.1 <= max_h {
        by_width
    } else {
        by_height
    }
}
