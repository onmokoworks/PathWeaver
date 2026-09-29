//! Shared original ThreadShape theme and docking chrome.
#![allow(dead_code)]
use egui::{Color32, FontFamily, FontId, Stroke, TextStyle, Vec2};
#[derive(Clone, Copy)]
pub(crate) struct Theme {
    pub(crate) canvas: Color32,
    pub(crate) panel: Color32,
    pub(crate) bg1: Color32,
    pub(crate) bg2: Color32,
    pub(crate) border: Color32,
    pub(crate) text1: Color32,
    pub(crate) text2: Color32,
    pub(crate) text3: Color32,
    pub(crate) accent: Color32,
    pub(crate) accent_bg: Color32,
    pub(crate) warning: Color32,
    pub(crate) error: Color32,
    pub(crate) bg: Color32,
    pub(crate) peg_stroke: Color32,
    pub(crate) peg_fill: Color32,
    pub(crate) catch: Color32,
    pub(crate) face_fill: Color32,
    pub(crate) thread: Color32,
    pub(crate) anchor: Color32,
    pub(crate) close_ok: Color32,
    pub(crate) close_ng: Color32,
    pub(crate) select: Color32,
    pub(crate) start: Color32,
    pub(crate) start_highlight: Color32,
    pub(crate) cmd_green: Color32,
    pub(crate) start_text: Color32,
}

impl Theme {
    pub(crate) fn dark() -> Self {
        Self {
            canvas: Color32::from_rgb(13, 13, 15),
            panel: Color32::from_rgb(16, 16, 18),
            bg1: Color32::from_rgb(21, 21, 24),
            bg2: Color32::from_rgb(34, 34, 39),
            border: Color32::from_rgb(34, 34, 38),
            text1: Color32::from_rgb(232, 232, 235),
            text2: Color32::from_rgb(170, 170, 178),
            text3: Color32::from_rgb(105, 105, 114),
            accent: Color32::from_rgb(94, 106, 210),
            accent_bg: Color32::from_rgb(34, 34, 40),
            warning: Color32::from_rgb(255, 160, 10),
            error: Color32::from_rgb(255, 69, 58),
            bg: Color32::from_rgb(13, 13, 15),
            peg_stroke: Color32::from_rgb(138, 138, 148),
            peg_fill: Color32::from_rgb(23, 23, 26),
            catch: Color32::from_rgb(72, 72, 80),
            face_fill: Color32::from_rgba_unmultiplied(214, 214, 218, 24),
            thread: Color32::from_rgb(222, 222, 226),
            anchor: Color32::from_rgb(222, 222, 226),
            close_ok: Color32::from_rgb(94, 106, 210),
            close_ng: Color32::from_rgb(255, 69, 58),
            select: Color32::from_rgb(94, 106, 210),
            start: Color32::from_rgb(160, 160, 184),
            start_highlight: Color32::from_rgb(255, 255, 255),
            cmd_green: Color32::from_rgb(255, 159, 10),
            start_text: Color32::from_rgb(245, 245, 247),
        }
    }

    pub(crate) fn light() -> Self {
        Self {
            canvas: Color32::from_rgb(232, 232, 232),
            panel: Color32::from_rgb(255, 255, 255),
            bg1: Color32::from_rgb(247, 247, 247),
            bg2: Color32::from_rgb(239, 239, 239),
            border: Color32::from_rgb(0, 0, 0).gamma_multiply(0.071),
            text1: Color32::from_rgb(29, 29, 31),
            text2: Color32::from_rgb(110, 110, 115),
            text3: Color32::from_rgb(174, 174, 178),
            accent: Color32::from_rgb(82, 91, 194),
            accent_bg: Color32::from_rgb(234, 235, 250),
            warning: Color32::from_rgb(255, 159, 10),
            error: Color32::from_rgb(255, 59, 48),
            bg: Color32::from_rgb(239, 239, 239),
            peg_stroke: Color32::from_rgb(130, 130, 142),
            peg_fill: Color32::from_rgb(224, 224, 228),
            catch: Color32::from_rgb(190, 190, 202),
            face_fill: Color32::from_rgb(212, 212, 216),
            thread: Color32::from_rgb(22, 22, 22),
            anchor: Color32::from_rgb(22, 22, 22),
            close_ok: Color32::from_rgb(82, 91, 194),
            close_ng: Color32::from_rgb(255, 59, 48),
            select: Color32::from_rgb(82, 91, 194),
            start: Color32::from_rgb(102, 102, 124),
            start_highlight: Color32::from_rgb(24, 24, 24),
            cmd_green: Color32::from_rgb(255, 159, 10),
            start_text: Color32::from_rgb(29, 29, 31),
        }
    }
}

impl Theme {
    pub(crate) fn apply_to_context(&self, ctx: &egui::Context, dark: bool) {
        let mut visuals = if dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        visuals.widgets.inactive.bg_fill = Color32::TRANSPARENT;
        visuals.widgets.active.bg_fill = self.bg2;
        visuals.widgets.hovered.bg_fill = self.bg2;
        visuals.widgets.noninteractive.bg_fill = self.panel;
        visuals.widgets.inactive.fg_stroke.color = self.text2;
        visuals.widgets.active.fg_stroke.color = self.text1;
        visuals.widgets.hovered.fg_stroke.color = self.text1;
        visuals.widgets.noninteractive.fg_stroke.color = self.text2;
        visuals.selection.bg_fill = self.accent_bg;
        visuals.selection.stroke.color = self.accent;
        visuals.widgets.inactive.corner_radius = 6.into();
        visuals.widgets.active.corner_radius = 6.into();
        visuals.widgets.hovered.corner_radius = 6.into();
        visuals.widgets.noninteractive.corner_radius = 6.into();
        visuals.window_corner_radius = 8.into();
        visuals.menu_corner_radius = 8.into();
        visuals.window_fill = self.panel;
        visuals.panel_fill = self.panel;
        visuals.extreme_bg_color = self.bg1;
        visuals.faint_bg_color = self.bg2;
        visuals.window_stroke = Stroke::new(1.0, self.border);
        visuals.window_shadow = egui::epaint::Shadow {
            offset: [0, 3],
            blur: 14,
            spread: 0,
            color: Color32::from_black_alpha(if dark { 90 } else { 36 }),
        };
        visuals.popup_shadow = egui::epaint::Shadow {
            offset: [0, 2],
            blur: 8,
            spread: 0,
            color: Color32::from_black_alpha(if dark { 80 } else { 28 }),
        };
        visuals.widgets.inactive.bg_stroke = Stroke::NONE;
        visuals.widgets.active.bg_stroke = Stroke::NONE;
        visuals.widgets.hovered.bg_stroke = Stroke::NONE;
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, self.border);
        // After Effects風: トラックは細い一本線、操作点は小さな丸。
        // 値まで塗りつぶさず、位置そのものをノブで読む編集UIにする。
        visuals.slider_trailing_fill = false;
        visuals.handle_shape = egui::style::HandleShape::Circle;
        ctx.set_visuals(visuals);

        let mut style = (*ctx.style()).clone();
        style.text_styles = [
            (TextStyle::Body, FontId::new(11.0, FontFamily::Proportional)),
            (
                TextStyle::Button,
                FontId::new(11.0, FontFamily::Proportional),
            ),
            (
                TextStyle::Small,
                FontId::new(10.0, FontFamily::Proportional),
            ),
            (
                TextStyle::Heading,
                FontId::new(12.0, FontFamily::Proportional),
            ),
            (
                TextStyle::Monospace,
                FontId::new(11.0, FontFamily::Monospace),
            ),
        ]
        .into_iter()
        .collect();
        style.spacing.item_spacing = Vec2::new(6.0, 4.0);
        style.spacing.button_padding = Vec2::new(8.0, 2.0);
        style.spacing.interact_size.y = 26.0;
        style.spacing.icon_width = 12.0;
        style.spacing.icon_width_inner = 6.0;
        style.spacing.icon_spacing = 4.0;
        style.spacing.slider_width = 144.0;
        style.spacing.slider_rail_height = 1.5;
        ctx.set_style(style);
    }
}

pub(crate) fn dock_style(ctx: &egui::Context, colors: Theme) -> egui_dock::Style {
    let mut dock_style = egui_dock::Style::from_egui(ctx.style().as_ref());
    // 区切り線の既定色は黒なので、テーマの枠色に上書きしてライトモードで
    // 黒い縁が出ないようにする
    dock_style.separator.width = 1.0;
    dock_style.separator.color_idle = colors.border;
    dock_style.separator.color_hovered = colors.accent;
    dock_style.separator.color_dragged = colors.accent;
    dock_style.main_surface_border_stroke = Stroke::NONE;
    dock_style.tab_bar.bg_fill = colors.bg1;
    dock_style.tab_bar.hline_color = colors.border;
    dock_style.tab_bar.height = 42.0;
    dock_style.tab_bar.inner_margin = egui::Margin::symmetric(6, 8);
    dock_style.tab_bar.corner_radius = egui::CornerRadius::same(0);
    dock_style.tab_bar.show_scroll_bar_on_overflow = true;
    dock_style.tab.spacing = 2.0;
    dock_style.tab.active.bg_fill = colors.bg2;
    dock_style.tab.active.text_color = colors.text1;
    dock_style.tab.active.corner_radius = egui::CornerRadius {
        nw: 5,
        ne: 5,
        sw: 0,
        se: 0,
    };
    dock_style.tab.active.outline_color = Color32::TRANSPARENT;
    dock_style.tab.focused = dock_style.tab.active.clone();
    dock_style.tab.inactive.bg_fill = Color32::TRANSPARENT;
    dock_style.tab.inactive.text_color = colors.text2;
    dock_style.tab.inactive.corner_radius = egui::CornerRadius::ZERO;
    dock_style.tab.inactive.outline_color = Color32::TRANSPARENT;
    dock_style.tab.hovered.bg_fill = colors.bg2;
    dock_style.tab.hovered.text_color = colors.text1;
    dock_style.tab.hovered.corner_radius = egui::CornerRadius {
        nw: 5,
        ne: 5,
        sw: 0,
        se: 0,
    };
    dock_style.tab.hovered.outline_color = Color32::TRANSPARENT;
    dock_style.tab.tab_body.bg_fill = colors.panel;
    dock_style.tab.tab_body.stroke = Stroke::NONE;
    dock_style.tab.tab_body.inner_margin = egui::Margin::same(6);
    dock_style
}
