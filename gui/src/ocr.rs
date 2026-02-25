use crate::settings::Settings;
use std::path::Path;
use std::process::Command;
use std::sync::mpsc;
use std::thread;

/// Result returned from the OCR worker thread.
#[derive(Debug)]
pub struct OcrResult {
    pub lines: Vec<OcrLine>,
    pub error: Option<String>,
}

/// A single recognised text line with bounding box and confidence.
#[derive(Debug, Clone)]
pub struct OcrLine {
    #[allow(dead_code)]
    pub bbox: [[f32; 2]; 4],
    pub text: String,
    pub confidence: f32,
}

/// Resolves the effective easyocr command.
///
/// Returns `(program, prepended_args)`.  When the `easyocr` script is found
/// directly, `prepended_args` is empty.  When only the Python module is
/// available, it returns e.g. `("python3", ["-m", "easyocr.cli"])`.
///
/// If `configured_exe` is non-empty, only that path is attempted.
pub fn resolve_easyocr_cmd(configured_exe: &str) -> Option<(String, Vec<String>)> {
    if !configured_exe.is_empty() {
        // User provided a custom path — use it unconditionally.
        return Some((configured_exe.to_string(), vec![]));
    }

    // 1. Try the `easyocr` script on PATH.
    if probe_cmd("easyocr", &[]) {
        return Some(("easyocr".to_string(), vec![]));
    }

    // 2. Try via Python module (handles pip installs where the script
    //    directory is not in the GUI's PATH).
    for python in &["python3", "python"] {
        if probe_cmd(python, &["-m", "easyocr.cli"]) {
            return Some((
                python.to_string(),
                vec!["-m".to_string(), "easyocr.cli".to_string()],
            ));
        }
    }

    None
}

/// Returns `true` if running `program [extra_args] --help` succeeds.
fn probe_cmd(program: &str, extra_args: &[&str]) -> bool {
    std::process::Command::new(program)
        .args(extra_args)
        .arg("--help")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Returns `true` if the configured easyocr executable can be found and
/// launched.  The check is performed synchronously but is intended to be
/// called from a background thread so the UI is never blocked.
pub fn check_easyocr_available(exe: &str) -> bool {
    resolve_easyocr_cmd(exe).is_some()
}

/// Spawns a background thread that checks easyocr availability and sends the
/// result (true = available) through the returned receiver.
pub fn check_easyocr_async(exe: &str) -> mpsc::Receiver<bool> {
    let (tx, rx) = mpsc::channel();
    let exe = exe.to_string();
    thread::spawn(move || {
        let _ = tx.send(check_easyocr_available(&exe));
    });
    rx
}

/// Spawns a background thread that calls the `easyocr` CLI and sends the
/// result back through the returned receiver.
pub fn run_ocr_async(
    image_path: &Path,
    settings: &Settings,
) -> mpsc::Receiver<OcrResult> {
    let (tx, rx) = mpsc::channel();
    let image_path = image_path.to_owned();
    let settings = settings.clone();

    thread::spawn(move || {
        let result = run_ocr_sync(&image_path, &settings);
        let _ = tx.send(result);
    });

    rx
}

fn run_ocr_sync(image_path: &Path, settings: &Settings) -> OcrResult {
    // Resolve executable — try direct binary then Python module fallback.
    let (exe, prefix_args) = match resolve_easyocr_cmd(&settings.easyocr_exe) {
        Some(cmd) => cmd,
        None => {
            let tried = if settings.easyocr_exe.is_empty() {
                "'easyocr' and 'python -m easyocr.cli'".to_string()
            } else {
                format!("'{}'", settings.easyocr_exe)
            };
            return OcrResult {
                lines: vec![],
                error: Some(format!(
                    "EasyOCR command not found (tried {}).\n\nMake sure EasyOCR is installed:\n  pip install easyocr",
                    tried
                )),
            };
        }
    };

    // Build language list: split on comma/space, collect unique.
    let langs: Vec<String> = settings
        .languages
        .split([',', ' '])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect();

    if langs.is_empty() {
        return OcrResult {
            lines: vec![],
            error: Some("No languages configured. Please add at least one language code in Settings.".into()),
        };
    }

    let mut cmd = Command::new(&exe);
    cmd.args(&prefix_args);

    // Languages (-l can take multiple values in the EasyOCR CLI).
    cmd.arg("-l");
    for l in &langs {
        cmd.arg(l);
    }

    // Image file.
    cmd.arg("-f").arg(image_path.as_os_str());

    // GPU.
    cmd.arg("--gpu").arg(if settings.gpu { "True" } else { "False" });

    // Workers.
    cmd.arg("--workers").arg(settings.workers.to_string());

    // Decoder.
    cmd.arg("--decoder").arg(settings.decoder.as_str());

    // Beam width.
    cmd.arg("--beamWidth").arg(settings.beam_width.to_string());

    // Batch size.
    cmd.arg("--batch_size").arg(settings.batch_size.to_string());

    // Thresholds.
    cmd.arg("--text_threshold").arg(format!("{:.4}", settings.text_threshold));
    cmd.arg("--low_text").arg(format!("{:.4}", settings.low_text));
    cmd.arg("--link_threshold").arg(format!("{:.4}", settings.link_threshold));
    cmd.arg("--contrast_ths").arg(format!("{:.4}", settings.contrast_ths));
    cmd.arg("--adjust_contrast").arg(format!("{:.4}", settings.adjust_contrast));

    // Min size.
    cmd.arg("--min_size").arg(settings.min_size.to_string());

    // Paragraph.
    cmd.arg("--paragraph").arg(if settings.paragraph { "True" } else { "False" });

    // Quantize.
    cmd.arg("--quantize").arg(if settings.quantize { "True" } else { "False" });

    // Add margin.
    cmd.arg("--add_margin").arg(format!("{:.4}", settings.add_margin));

    // Detail level 1 = full output.
    cmd.arg("--detail").arg("1");

    // Model storage directory.
    if !settings.model_storage_directory.is_empty() {
        cmd.arg("--model_storage_directory")
            .arg(&settings.model_storage_directory);
    }

    // Capture stderr for error messages.
    cmd.stderr(std::process::Stdio::piped());
    cmd.stdout(std::process::Stdio::piped());

    let output = match cmd.output() {
        Ok(o) => o,
        Err(e) => {
            return OcrResult {
                lines: vec![],
                error: Some(format!(
                    "Failed to run '{}': {}\n\nMake sure EasyOCR is installed:\n  pip install easyocr",
                    exe, e
                )),
            }
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        return OcrResult {
            lines: vec![],
            error: Some(format!("EasyOCR exited with error:\n{}\n{}", stderr, stdout)),
        };
    }

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let lines = parse_easyocr_output(&stdout);

    OcrResult {
        lines,
        error: None,
    }
}

/// Parse the standard EasyOCR CLI output (detail=1).
///
/// Each line looks like:
///   ([[x1,y1],[x2,y2],[x3,y3],[x4,y4]], 'text', 0.99)
fn parse_easyocr_output(output: &str) -> Vec<OcrLine> {
    let mut lines = Vec::new();

    for raw_line in output.lines() {
        let raw_line = raw_line.trim();
        if raw_line.is_empty() {
            continue;
        }

        // Try to extract the text and confidence from the tuple representation.
        // Simplified parser: look for the last comma-separated float at the end.
        if let Some(parsed) = parse_line(raw_line) {
            lines.push(parsed);
        }
    }

    lines
}

fn parse_line(s: &str) -> Option<OcrLine> {
    // Strip outer parens: "(...)"
    let s = s.trim();
    let s = s.strip_prefix('(')?.strip_suffix(')')?;

    // Find the split between bbox tuple and the rest.
    // The bbox is "[[...]]", then a comma, then the text, then a comma, then the confidence.
    let bracket_end = s.find("]]")?;
    let bbox_str = &s[..bracket_end + 2];
    let rest = s[bracket_end + 2..].trim().strip_prefix(',')?.trim();

    // Parse bbox: [[x1,y1],[x2,y2],[x3,y3],[x4,y4]]
    let bbox = parse_bbox(bbox_str)?;

    // rest is now "'text', 0.99" or '"text", 0.99'
    // Find the last comma — before the confidence value.
    let last_comma = rest.rfind(',')?;
    let text_part = rest[..last_comma].trim();
    let conf_part = rest[last_comma + 1..].trim();

    // Strip quotes from text.
    let text = text_part
        .trim_start_matches(['\'', '"'])
        .trim_end_matches(['\'', '"'])
        .to_string();

    let confidence: f32 = conf_part.parse().ok()?;

    Some(OcrLine {
        bbox,
        text,
        confidence,
    })
}

fn parse_bbox(s: &str) -> Option<[[f32; 2]; 4]> {
    // s looks like [[x1, y1], [x2, y2], [x3, y3], [x4, y4]]
    let inner = s.strip_prefix("[[")?.strip_suffix("]]")?;
    // Split on "], ["
    let points: Vec<&str> = inner.split("], [").collect();
    if points.len() != 4 {
        return None;
    }
    let mut result = [[0.0f32; 2]; 4];
    for (i, p) in points.iter().enumerate() {
        let coords: Vec<&str> = p.split(',').collect();
        if coords.len() != 2 {
            return None;
        }
        result[i][0] = coords[0].trim().parse().ok()?;
        result[i][1] = coords[1].trim().parse().ok()?;
    }
    Some(result)
}
