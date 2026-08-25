use crate::export::document_to_svg_styled;
use crate::model::{FontMetricsGuides, FontProject};
use kurbo::Point;
use serde::Serialize;
use std::path::Path;
use std::process::Command;

#[derive(Serialize)]
struct ExportGlyph {
    character: char,
    name: String,
    advance_width: u16,
    left_side_bearing: i16,
    svg: String,
}

#[derive(Serialize)]
struct ExportPayload {
    family_name: String,
    style_name: String,
    metrics: FontMetricsGuides,
    glyphs: Vec<ExportGlyph>,
}

pub fn export_ttf(
    project: &FontProject,
    canvas_center: Point,
    output: &Path,
) -> Result<(), String> {
    let payload = ExportPayload {
        family_name: project.family_name.clone(),
        style_name: project.style_name.clone(),
        metrics: project.metrics,
        glyphs: project
            .glyphs
            .iter()
            .map(|glyph| ExportGlyph {
                character: glyph.character,
                name: glyph.name.clone(),
                advance_width: glyph.advance_width,
                left_side_bearing: glyph.left_side_bearing,
                svg: document_to_svg_styled(
                    &glyph.document,
                    canvas_center,
                    project.render_style,
                    project.thread_width,
                ),
            })
            .collect(),
    };
    let nonce = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos()
    );
    let temp = std::env::temp_dir().join(format!("threadtype-font-{nonce}"));
    std::fs::create_dir_all(&temp).map_err(|error| error.to_string())?;
    let payload_path = temp.join("font.json");
    let script_path = temp.join("build_font.py");
    std::fs::write(
        &payload_path,
        serde_json::to_vec(&payload).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    std::fs::write(&script_path, include_str!("font_export.py"))
        .map_err(|error| error.to_string())?;

    let mut failures = Vec::new();
    for python in ["python3", "python"] {
        match Command::new(python)
            .arg(&script_path)
            .arg(&payload_path)
            .arg(output)
            .output()
        {
            Ok(result) if result.status.success() => {
                let _ = std::fs::remove_dir_all(&temp);
                return Ok(());
            }
            Ok(result) => {
                let stderr = String::from_utf8_lossy(&result.stderr).trim().to_owned();
                if !stderr.is_empty() {
                    failures.push(stderr);
                }
            }
            Err(_) => {}
        }
    }
    let _ = std::fs::remove_dir_all(&temp);
    if failures.is_empty() {
        Err("Python 3 が見つかりません".to_owned())
    } else {
        Err(failures.pop().unwrap())
    }
}
