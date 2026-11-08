//! Shared inline color controls, following Tweeq's pad / hue / color-code layout.
use crate::{
    app::{ButtonEditPolicy, EditTarget, Editing, IconTooltip, Workspace},
    drawing::{dot, rect},
    theme::{Role, color as ui_color},
};
use gpui::{prelude::*, *};
use pathweaver_core::model::Document;
use std::{cell::Cell, rc::Rc};

#[derive(Clone, Copy, PartialEq)]
pub enum ColorSlot {
    Stroke,
    Fill,
    Node(u64),
}
impl ColorSlot {
    fn name(self) -> &'static str {
        match self {
            Self::Stroke => "線色",
            Self::Fill => "塗り",
            Self::Node(_) => "色",
        }
    }
    fn id(self) -> SharedString {
        match self {
            Self::Stroke => "stroke-color".into(),
            Self::Fill => "fill-color".into(),
            Self::Node(id) => format!("node-color-{id}").into(),
        }
    }
}
#[derive(Clone, Copy)]
enum ColorDrag {
    Pad,
    Hue,
}
pub struct ColorPicker {
    slot: ColorSlot,
    target: EditTarget,
    before: Document,
    hsv: [f32; 3],
    drag: Option<ColorDrag>,
    pad_bounds: Rc<Cell<Bounds<Pixels>>>,
    hue_bounds: Rc<Cell<Bounds<Pixels>>>,
}
fn hex(c: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}
pub fn parse_hex(text: &str) -> Result<[u8; 3], String> {
    let text = text.trim().trim_start_matches('#');
    if text.len() != 6 || !text.is_ascii() {
        return Err("色は #RRGGBB で入力してください".into());
    }
    let value = u32::from_str_radix(text, 16).map_err(|_| "色は #RRGGBB で入力してください")?;
    Ok([(value >> 16) as u8, (value >> 8) as u8, value as u8])
}
fn packed(c: [u8; 3]) -> u32 {
    ((c[0] as u32) << 16) | ((c[1] as u32) << 8) | c[2] as u32
}
fn hsv_to_rgb([h, s, v]: [f32; 3]) -> [u8; 3] {
    let h = h.rem_euclid(1.) * 6.;
    let c = v * s;
    let x = c * (1. - (h % 2. - 1.).abs());
    let m = v - c;
    let rgb = match h as u8 {
        0 => [c, x, 0.],
        1 => [x, c, 0.],
        2 => [0., c, x],
        3 => [0., x, c],
        4 => [x, 0., c],
        _ => [c, 0., x],
    };
    rgb.map(|ch| ((ch + m).clamp(0., 1.) * 255.).round() as u8)
}
fn rgb_to_hsv(rgb: [u8; 3]) -> [f32; 3] {
    let [r, g, b] = rgb.map(|c| c as f32 / 255.);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d == 0. {
        0.
    } else if max == r {
        ((g - b) / d).rem_euclid(6.) / 6.
    } else if max == g {
        ((b - r) / d + 2.) / 6.
    } else {
        ((r - g) / d + 4.) / 6.
    };
    [h, if max == 0. { 0. } else { d / max }, max]
}
fn apply(
    target: &EditTarget,
    slot: ColorSlot,
    doc: &mut Document,
    color: [u8; 3],
) -> Result<(), String> {
    if slot == ColorSlot::Fill {
        target.path_mut(doc)?.fill = Some(color);
        Ok(())
    } else {
        target.apply_color(doc, color)
    }
}
impl Workspace {
    pub fn start_color(
        &mut self,
        slot: ColorSlot,
        color: [u8; 3],
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.editing.is_some() {
            self.commit_edit(w, cx);
            if self.editing.is_some() {
                return;
            }
        }
        self.finish_interaction(true, cx);
        self.active_nodes = matches!(slot, ColorSlot::Node(_));
        self.node_region = self.active_nodes;
        let node = if let ColorSlot::Node(id) = slot {
            Some(id)
        } else {
            None
        };
        let target = EditTarget::capture(&self.session, node);
        self.color_picker = Some(ColorPicker {
            slot,
            target: target.clone(),
            before: self.session.doc.clone(),
            hsv: rgb_to_hsv(color),
            drag: None,
            pad_bounds: Rc::new(Cell::new(Bounds::default())),
            hue_bounds: Rc::new(Cell::new(Bounds::default())),
        });
        self.edit_target = Some(target);
        self.editing = Some(if slot == ColorSlot::Fill {
            Editing::FillColor
        } else {
            Editing::Color
        });
        self.numeric_error = false;
        self.input.update(cx, |i, cx| i.set(hex(color), w, cx));
        cx.notify();
    }
    pub fn color_input_changed(&mut self, cx: &mut Context<Self>) {
        let Some(p) = &mut self.color_picker else {
            return;
        };
        let Ok(color) = parse_hex(&self.input.read(cx).content) else {
            return;
        };
        // Returning to the last valid color also clears a previous HEX error.
        self.numeric_error = false;
        // Programmatic pad changes preserve hue even while saturation is zero.
        if color == hsv_to_rgb(p.hsv) {
            cx.notify();
            return;
        }
        let hsv = rgb_to_hsv(color);
        p.hsv = [if hsv[1] == 0. { p.hsv[0] } else { hsv[0] }, hsv[1], hsv[2]];
        if let Err(e) = apply(&p.target, p.slot, &mut self.session.doc, color) {
            self.status = e;
            self.numeric_error = true;
        } else {
            self.numeric_error = false;
        }
        self.refresh(cx);
    }
    pub fn color_window_deactivated(&mut self) {
        if let Some(p) = &mut self.color_picker {
            p.drag = None;
        }
    }
    pub fn cancel_color(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        if let Some(p) = self.color_picker.take() {
            self.session.doc = p.before;
        }
        self.editing = None;
        self.edit_target = None;
        self.numeric_error = false;
        self.focus.focus(w);
        self.refresh(cx);
    }
    pub fn commit_color(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        let color = match parse_hex(&self.input.read(cx).content) {
            Ok(c) => c,
            Err(e) => {
                self.status = e;
                self.numeric_error = true;
                cx.notify();
                return;
            }
        };
        let Some(p) = self.color_picker.take() else {
            return;
        };
        let mut final_doc = p.before.clone();
        if let Err(e) =
            apply(&p.target, p.slot, &mut final_doc, color).and_then(|_| final_doc.validate())
        {
            self.status = e;
            self.numeric_error = true;
            self.color_picker = Some(p);
            cx.notify();
            return;
        }
        self.session.doc = p.before;
        self.session.begin();
        self.session.doc = final_doc;
        self.session.commit();
        self.editing = None;
        self.edit_target = None;
        self.numeric_error = false;
        self.focus.focus(w);
        self.refresh(cx);
    }
    fn preview_color(&mut self, color: [u8; 3], w: &mut Window, cx: &mut Context<Self>) {
        if let Some(p) = &self.color_picker
            && let Err(e) = apply(&p.target, p.slot, &mut self.session.doc, color)
        {
            self.status = e;
            self.numeric_error = true;
            cx.notify();
            return;
        }
        self.numeric_error = false;
        self.input.update(cx, |i, cx| i.set(hex(color), w, cx));
        self.refresh(cx);
    }
    fn color_drag_at(&mut self, at: Point<Pixels>, w: &mut Window, cx: &mut Context<Self>) {
        let Some(p) = &mut self.color_picker else {
            return;
        };
        let Some(drag) = p.drag else {
            return;
        };
        let b = match drag {
            ColorDrag::Pad => p.pad_bounds.get(),
            ColorDrag::Hue => p.hue_bounds.get(),
        };
        let x = (f32::from(at.x - b.left()) / f32::from(b.size.width).max(1.)).clamp(0., 1.);
        let y = (f32::from(at.y - b.top()) / f32::from(b.size.height).max(1.)).clamp(0., 1.);
        match drag {
            ColorDrag::Pad => {
                p.hsv[1] = x;
                p.hsv[2] = 1. - y;
            }
            ColorDrag::Hue => p.hsv[0] = x,
        }
        let color = hsv_to_rgb(p.hsv);
        self.preview_color(color, w, cx);
    }
    pub fn color_mouse_move(
        &mut self,
        e: &MouseMoveEvent,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.color_picker.as_ref().is_some_and(|p| p.drag.is_some()) {
            self.color_drag_at(e.position, w, cx);
            true
        } else {
            false
        }
    }
    pub fn color_mouse_up(&mut self, e: &MouseUpEvent) -> bool {
        if e.button == MouseButton::Left
            && let Some(p) = &mut self.color_picker
        {
            return p.drag.take().is_some();
        }
        false
    }
    pub fn color_field(
        &self,
        slot: ColorSlot,
        color: Option<[u8; 3]>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let editing = self.color_picker.as_ref().is_some_and(|p| p.slot == slot);
        let mut row = div()
            .flex()
            .items_center()
            .gap_2()
            .h(px(28.))
            .flex_shrink_0()
            .child(
                div()
                    .w(px(64.))
                    .text_color(rgb(ui_color(Role::Muted)))
                    .child(slot.name()),
            );
        row = row.child(
            div()
                .id(slot.id())
                .flex_1()
                .h_full()
                .flex()
                .items_center()
                .gap_2()
                .px_2()
                .rounded(px(4.))
                .bg(rgb(ui_color(Role::Button)))
                .cursor_pointer()
                .child(
                    div()
                        .size(px(18.))
                        .rounded(px(3.))
                        .border_1()
                        .border_color(rgb(ui_color(Role::Border)))
                        .bg(rgb(color.map(packed).unwrap_or(ui_color(Role::Surface))))
                        .child(if color.is_none() { "／" } else { "" }),
                )
                .child(color.map(hex).unwrap_or_else(|| "なし".into()))
                .on_click(cx.listener(move |s, _, w, cx| {
                    s.start_color(slot, color.unwrap_or([128; 3]), w, cx)
                })),
        );
        if !matches!(slot, ColorSlot::Node(_)) {
            row = row.child(self.icon_button(
                SharedString::from(format!("{}-enabled", slot.id())),
                if color.is_some() { "eye" } else { "eye-off" },
                if color.is_some() {
                    "色をなしにする"
                } else {
                    "色を有効にする"
                },
                false,
                cx,
                move |s, _, cx| {
                    s.session.edit(|session| {
                        if let Some(path) = session.active_mut() {
                            let c = if color.is_some() {
                                None
                            } else {
                                Some([128; 3])
                            };
                            if slot == ColorSlot::Fill {
                                path.fill = c;
                            } else {
                                path.edge = c;
                            }
                        }
                    });
                    s.refresh(cx);
                },
            ));
        }
        div()
            .flex()
            .flex_col()
            .gap_2()
            .flex_shrink_0()
            .child(row)
            .when(editing, |d| d.child(self.color_editor(cx)))
            .into_any_element()
    }
    fn color_editor(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = self.color_picker.as_ref().unwrap();
        let hsv = p.hsv;
        let bounds = p.pad_bounds.clone();
        let view = cx.entity();
        let pad = div()
            .id("color-pad")
            .h(px(116.))
            .w_full()
            .cursor_crosshair()
            .child(
                canvas(
                    move |b, _, _| {
                        bounds.set(b);
                    },
                    move |b, _, w, _| {
                        // Small interpolated strips avoid a shader-specific dependency.
                        for y in 0..64 {
                            let v = 1. - y as f32 / 63.;
                            let row = Bounds::new(
                                point(b.left(), b.top() + b.size.height * (y as f32 / 64.)),
                                size(b.size.width, b.size.height / 64. + px(0.5)),
                            );
                            w.paint_quad(quad(
                                row,
                                px(0.),
                                linear_gradient(
                                    90.,
                                    linear_color_stop(rgb(packed(hsv_to_rgb([hsv[0], 0., v]))), 0.),
                                    linear_color_stop(rgb(packed(hsv_to_rgb([hsv[0], 1., v]))), 1.),
                                ),
                                px(0.),
                                rgba(0),
                                Default::default(),
                            ));
                        }
                        let at = point(
                            b.left() + b.size.width * hsv[1],
                            b.top() + b.size.height * (1. - hsv[2]),
                        );
                        dot(w, at, 5., 0x000000);
                        dot(w, at, 3.5, 0xffffff);
                    },
                )
                .size_full(),
            )
            .on_mouse_down(MouseButton::Left, move |e, w, cx| {
                view.update(cx, |s, cx| {
                    if let Some(p) = &mut s.color_picker {
                        p.drag = Some(ColorDrag::Pad);
                    }
                    s.color_drag_at(e.position, w, cx);
                });
                cx.stop_propagation();
            });
        let bounds = p.hue_bounds.clone();
        let view = cx.entity();
        let hue = div()
            .id("color-hue")
            .h(px(16.))
            .w_full()
            .cursor_crosshair()
            .child(
                canvas(
                    move |b, _, _| {
                        bounds.set(b);
                    },
                    move |b, _, w, _| {
                        for x in 0..64 {
                            let cell = Bounds::new(
                                point(b.left() + b.size.width * (x as f32 / 64.), b.top()),
                                size(b.size.width / 64. + px(0.5), b.size.height),
                            );
                            rect(
                                w,
                                cell,
                                packed(hsv_to_rgb([x as f32 / 63., 1., 1.])),
                                0,
                                0.,
                                0.,
                            );
                        }
                        let at = point(b.left() + b.size.width * hsv[0], b.center().y);
                        dot(w, at, 5., 0x000000);
                        dot(w, at, 3.5, 0xffffff);
                    },
                )
                .size_full(),
            )
            .on_mouse_down(MouseButton::Left, move |e, w, cx| {
                view.update(cx, |s, cx| {
                    if let Some(p) = &mut s.color_picker {
                        p.drag = Some(ColorDrag::Hue);
                    }
                    s.color_drag_at(e.position, w, cx);
                });
                cx.stop_propagation();
            });
        let mut presets = div().flex().gap_1();
        for (i, c) in [
            [0, 0, 0],
            [255, 255, 255],
            [232, 65, 61],
            [255, 184, 48],
            [66, 164, 100],
            [66, 125, 222],
            [157, 91, 205],
        ]
        .into_iter()
        .enumerate()
        {
            let tip: SharedString = hex(c).into();
            presets = presets.child(
                div()
                    .id(("color-swatch", i))
                    .flex_1()
                    .h(px(20.))
                    .rounded(px(3.))
                    .border_1()
                    .border_color(rgb(ui_color(Role::Border)))
                    .bg(rgb(packed(c)))
                    .cursor_pointer()
                    .tooltip(move |_, cx| cx.new(|_| IconTooltip(tip.clone())).into())
                    .on_click(cx.listener(move |s, _, w, cx| {
                        if let Some(p) = &mut s.color_picker {
                            p.hsv = rgb_to_hsv(c);
                        }
                        s.preview_color(c, w, cx);
                    })),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap_2()
            .p_2()
            .bg(rgb(ui_color(Role::Input)))
            .rounded(px(4.))
            .child(pad)
            .child(hue)
            .child(presets)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(div().text_size(px(10.)).child("HEX"))
                    .child(div().flex_1().min_w_0().child(self.input.clone()))
                    .child(self.edit_button(
                        "color-apply".into(),
                        "適用".into(),
                        true,
                        ButtonEditPolicy::Explicit,
                        cx,
                        |s, w, cx| s.commit_color(w, cx),
                    ))
                    .child(self.edit_button(
                        "color-cancel".into(),
                        "取消".into(),
                        false,
                        ButtonEditPolicy::Explicit,
                        cx,
                        |s, w, cx| s.cancel_color(w, cx),
                    )),
            )
            .when(self.numeric_error, |d| {
                d.child(
                    div()
                        .text_size(px(10.))
                        .text_color(rgb(ui_color(Role::Error)))
                        .child("#RRGGBBで入力してください"),
                )
            })
            .into_any_element()
    }
}
#[cfg(test)]
mod tests {
    use super::{ColorSlot, EditTarget, apply, hex, hsv_to_rgb, parse_hex, rgb_to_hsv};
    #[test]
    fn rgb_hsv_roundtrips_and_hex_rejects_partial_input() {
        for r in [0, 1, 64, 127, 255] {
            for g in [0, 31, 128, 255] {
                for b in [0, 77, 255] {
                    let c = [r, g, b];
                    assert_eq!(hsv_to_rgb(rgb_to_hsv(c)), c);
                    assert_eq!(parse_hex(&hex(c)).unwrap(), c);
                }
            }
        }
        assert!(parse_hex("#ff").is_err());
        assert!(parse_hex("#GG0000").is_err());
        assert_eq!(parse_hex("123aEF").unwrap(), [18, 58, 239]);
    }
    #[test]
    fn fill_preview_cancel_and_commit_keep_stroke_and_single_undo() {
        let mut session = crate::session::Session::default();
        session.new_path(kurbo::Point::ZERO);
        let target = EditTarget::capture(&session, None);
        let before = session.doc.clone();
        let edge = session.active().unwrap().edge;
        apply(&target, ColorSlot::Fill, &mut session.doc, [12, 34, 56]).unwrap();
        apply(&target, ColorSlot::Fill, &mut session.doc, [78, 90, 12]).unwrap();
        assert_eq!(session.active().unwrap().edge, edge);
        session.doc = before.clone();
        assert_eq!(session.active().unwrap().fill, before.strokes[0].fill);
        session.begin();
        apply(&target, ColorSlot::Fill, &mut session.doc, [12, 34, 56]).unwrap();
        session.commit();
        session.undo();
        assert_eq!(session.doc, before);
        session.redo();
        assert_eq!(session.active().unwrap().fill, Some([12, 34, 56]));
    }
}
