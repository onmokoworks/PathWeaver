use kurbo::ParamCurve;
pub const MITER: &str = include_str!("icons/miter.svg");
pub const ROUND: &str = include_str!("icons/round.svg");
pub const BEVEL: &str = include_str!("icons/bevel.svg");
pub const CLOTHOID: &str = include_str!("icons/clothoid.svg");
pub const FILL: &str = include_str!("icons/fill.svg");
pub const OUTLINE: &str = include_str!("icons/outline.svg");
pub const EDITING: &str = include_str!("icons/editing.svg");
/// Render the bundled SVG path as vector strokes, without a raster dependency.
pub fn choice(ui: &mut egui::Ui, svg: &str, label: &str, selected: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(70., 49.), egui::Sense::click());
    let style = ui.style().interact_selectable(&response, selected);
    ui.painter().rect(
        rect,
        3.,
        style.bg_fill,
        style.bg_stroke,
        egui::StrokeKind::Inside,
    );
    let color = style.text_color();
    if let Some(d) = svg
        .split("<path d=\"")
        .nth(1)
        .and_then(|s| s.split('"').next())
    {
        if let Ok(path) = kurbo::BezPath::from_svg(d) {
            let origin = rect.center_top() + egui::vec2(-14., 1.);
            for seg in path.segments() {
                let points = (0..=24)
                    .map(|i| {
                        let p = seg.eval(i as f64 / 24.);
                        origin + egui::vec2(p.x as f32, p.y as f32) * 0.85
                    })
                    .collect();
                ui.painter()
                    .add(egui::Shape::line(points, egui::Stroke::new(1.1_f32, color)));
            }
        }
    }
    ui.painter().text(
        rect.center_bottom() - egui::vec2(0., 5.),
        egui::Align2::CENTER_BOTTOM,
        label,
        egui::FontId::proportional(11.),
        color,
    );
    response.on_hover_text(label)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bundled_svg_icons_have_renderable_vector_paths() {
        for svg in [MITER, ROUND, BEVEL, CLOTHOID, FILL, OUTLINE, EDITING] {
            let d = svg
                .split("<path d=\"")
                .nth(1)
                .unwrap()
                .split('"')
                .next()
                .unwrap();
            assert!(kurbo::BezPath::from_svg(d).unwrap().segments().count() > 0);
        }
    }
}
