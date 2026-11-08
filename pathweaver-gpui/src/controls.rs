//! Compact parameter controls inspired by Tweeq's InputNumber interactions.
use crate::app::{Drag, Editing, Workspace};
use crate::theme::{Role, color as ui_color};
use gpui::{prelude::*, *};
use kurbo::{Affine, Vec2};

pub fn is_numeric(kind: &Editing) -> bool {
    matches!(
        kind,
        Editing::AnchorX
            | Editing::AnchorY
            | Editing::Width
            | Editing::GridSpacing
            | Editing::Parameter(_)
            | Editing::Rotation
            | Editing::Scale
    )
}
fn display(v: f64) -> String {
    if !v.is_finite() {
        return "—".into();
    }
    if v == 0. {
        return "0".into();
    }
    if v.abs() < 0.001 || v.abs() >= 1e10 {
        return format!("{v:.3e}");
    }
    let s = format!("{v:.3}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}
pub fn integer_parameter(key: &Editing) -> bool {
    matches!(
        key,
        Editing::Parameter(pathweaver_core::appearance::Parameter::Seed)
    )
}
/// Keep integer parameter previews identical to the value sent to the model.
pub fn numeric_value(key: &Editing, value: f64) -> f64 {
    if integer_parameter(key) {
        value.round()
    } else {
        value
    }
}
pub fn numeric_modifiers(key: &Editing, alt: bool, shift: bool, snap: bool) -> (bool, bool, bool) {
    (alt && !integer_parameter(key), shift, snap)
}
fn validate_numeric_kind(key: &Editing, value: f64) -> Result<(), String> {
    if integer_parameter(key) && value.fract() != 0. {
        Err("シードは整数で入力してください".into())
    } else {
        Ok(())
    }
}
pub fn adjusted(
    value: f64,
    dx: f64,
    step: f64,
    min: f64,
    max: f64,
    modifiers: (bool, bool, bool),
) -> f64 {
    let (alt, shift, snap) = modifiers;
    let speed = step * if alt { 0.1 } else { 1. } * if shift { 10. } else { 1. };
    let v = (value + dx * speed).clamp(min, max);
    if snap {
        let tick = step * 10.;
        ((v / tick).round() * tick).clamp(min, max)
    } else {
        v
    }
}
impl Workspace {
    pub fn toggle_field(
        &self,
        label: impl Into<SharedString>,
        on: bool,
        cx: &mut Context<Self>,
        f: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        let label = label.into();
        div()
            .id(label.clone())
            .h(px(26.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(9.))
            .cursor_pointer()
            .child(
                div()
                    .size(px(16.))
                    .rounded(px(4.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(rgb(ui_color(if on { Role::Selected } else { Role::Input })))
                    .text_color(rgb(ui_color(Role::OnAccent)))
                    .child(if on { "✓" } else { "" }),
            )
            .child(label)
            .on_click(cx.listener(move |s, _, w, cx| {
                if s.editing.is_some() {
                    s.commit_edit(w, cx);
                    if s.editing.is_some() {
                        return;
                    }
                }
                s.finish_interaction(true, cx);
                f(s, w, cx);
                cx.notify();
            }))
            .into_any_element()
    }
    pub fn numeric_meta(&self, key: &Editing) -> Option<(f64, f64, f64, f64)> {
        Some(match key {
            Editing::AnchorX => (self.session.anchor_position()?.x, -1e6, 1e6, 1.),
            Editing::AnchorY => (self.session.anchor_position()?.y, -1e6, 1e6, 1.),
            Editing::Width => {
                let p = self.session.active()?;
                (p.line_width(), 0.1, 1000., 0.2)
            }
            Editing::GridSpacing => (self.session.doc.grid_spacing, 1., 10000., 1.),
            Editing::Rotation => (0., -360000., 360000., 1.),
            Editing::Scale => (1., 0.01, 100., 0.01),
            Editing::Parameter(p) => {
                let n = self
                    .session
                    .node_selection
                    .last()
                    .and_then(|id| self.session.graph()?.node(*id))?;
                let spec = n.parameters().into_iter().find(|s| s.key == *p)?;
                (
                    n.number(*p),
                    spec.min,
                    spec.max,
                    if spec.max - spec.min <= 2. {
                        0.01
                    } else if spec.max - spec.min <= 100. {
                        0.1
                    } else {
                        1.
                    },
                )
            }
            _ => return None,
        })
    }
    pub fn apply_numeric(&mut self, key: &Editing, value: f64) -> Result<(), String> {
        let (_, min, max, _) = self.numeric_meta(key).ok_or("編集対象を選択してください")?;
        if !value.is_finite() || value < min || value > max {
            return Err(format!("{min}〜{max}の範囲で入力してください"));
        }
        validate_numeric_kind(key, value)?;
        match key {
            Editing::AnchorX | Editing::AnchorY => {
                let p = self.session.anchor_position().unwrap();
                self.session
                    .move_anchors(if matches!(key, Editing::AnchorX) {
                        Vec2::new(value - p.x, 0.)
                    } else {
                        Vec2::new(0., value - p.y)
                    });
            }
            Editing::Width => {
                for s in &mut self.session.doc.strokes {
                    if self.session.selected.contains(&s.id) {
                        s.resize_line_width(value)?;
                    }
                }
            }
            Editing::GridSpacing => self.session.doc.grid_spacing = value,
            Editing::Parameter(p) => {
                let id = *self
                    .session
                    .node_selection
                    .last()
                    .ok_or("ノードを選択してください")?;
                let n = self
                    .session
                    .graph_mut()
                    .and_then(|g| g.node_mut(id))
                    .ok_or("ノードを選択してください")?;
                if n.bindings.iter().any(|(k, _)| k == p) {
                    return Err("接続元の数値ノードで調整してください".into());
                }
                n.set_number(*p, value);
            }
            Editing::Rotation | Editing::Scale => {
                if let Some(r) = self.session.bounds(true) {
                    let m = if matches!(key, Editing::Scale) {
                        Affine::scale(value)
                    } else {
                        Affine::rotate(value.to_radians())
                    };
                    self.session.transform(
                        Affine::translate(r.center().to_vec2())
                            * m
                            * Affine::translate(-r.center().to_vec2()),
                    );
                }
            }
            _ => return Err("数値ではない項目です".into()),
        }
        self.session.doc.validate()
    }
    pub fn numeric_field(
        &self,
        label: impl Into<SharedString>,
        key: Editing,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let label = label.into();
        let Some((mut value, min, max, step)) = self.numeric_meta(&key) else {
            return div().into_any_element();
        };
        if let Drag::Numeric {
            key: dragging,
            value: preview,
            moved: true,
            ..
        } = &self.drag
            && dragging == &key
        {
            value = if self.numeric_snap {
                adjusted(*preview, 0., step, min, max, (false, false, true))
            } else {
                *preview
            };
        }
        value = numeric_value(&key, value);
        let editing = self.editing.as_ref() == Some(&key);
        let mut row = div()
            .flex()
            .gap_2()
            .items_center()
            .h(px(26.))
            .flex_shrink_0()
            .child(
                div()
                    .w(px(64.))
                    .flex_shrink_0()
                    .text_color(rgb(ui_color(Role::Muted)))
                    .child(label.clone()),
            );
        if editing {
            row = row.child(
                div()
                    .flex_1()
                    .min_w_0()
                    .border_1()
                    .border_color(rgb(if self.numeric_error {
                        ui_color(Role::Error)
                    } else {
                        ui_color(Role::Accent)
                    }))
                    .rounded(px(4.))
                    .child(self.input.clone()),
            );
        } else {
            let id = SharedString::from(format!("number-{label}"));
            let key2 = key.clone();
            let mut field = div()
                .id(id)
                .relative()
                .flex_1()
                .min_w_0()
                .h(px(24.))
                .px_2()
                .flex()
                .items_center()
                .justify_center()
                .bg(rgb(ui_color(Role::Input)))
                .rounded(px(4.))
                .border_1()
                .border_color(rgb(ui_color(Role::Input)))
                .cursor(CursorStyle::ResizeLeftRight)
                .hover(|s| s.border_color(rgb(ui_color(Role::Accent))))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |s, e: &MouseDownEvent, w, cx| {
                        if s.editing.is_some() {
                            s.commit_edit(w, cx);
                            if s.editing.is_some() {
                                return;
                            }
                        }
                        if s.handle_drag_down(e, cx) {
                            cx.stop_propagation();
                            return;
                        }
                        s.focus.focus(w);
                        s.session.begin();
                        s.numeric_before = Some(s.session.doc.clone());
                        s.numeric_error = false;
                        s.numeric_snap = false;
                        s.drag = Drag::Numeric {
                            key: key2.clone(),
                            start: e.position,
                            last: e.position,
                            value,
                            initial: value,
                            step,
                            min,
                            max,
                            moved: false,
                        };
                        s.status =
                            "数値：横ドラッグ / Option 微調整 / Shift 10倍 / Q 刻み / Escape 取消"
                                .into();
                        cx.stop_propagation();
                        cx.notify();
                    }),
                );
            if max - min <= 10000. && !matches!(key, Editing::Rotation | Editing::Scale) {
                field = field.child(
                    div()
                        .absolute()
                        .left_0()
                        .bottom_0()
                        .h_full()
                        .w(relative(((value - min) / (max - min)).clamp(0., 1.) as f32))
                        .bg(rgb(ui_color(Role::Soft))),
                );
            }
            field = field.child(div().relative().child(display(value)));
            row = row.child(field);
        }
        row.into_any_element()
    }
    pub fn numeric_move(&mut self, e: &MouseMoveEvent, cx: &mut Context<Self>) -> bool {
        let Drag::Numeric {
            key,
            start,
            last,
            value,
            initial,
            step,
            min,
            max,
            moved,
        } = self.drag.clone()
        else {
            return false;
        };
        let moved = moved || f32::from(e.position.x - start.x).abs() > 3.;
        if moved {
            let dx = self
                .numeric_capture
                .as_ref()
                .and_then(|capture| capture.delta_x())
                .unwrap_or(f32::from(e.position.x - last.x) as f64);
            if self.numeric_capture.is_none() {
                self.numeric_capture = crate::native_input::CursorCapture::start();
            }
            let next = adjusted(
                value,
                dx,
                step,
                min,
                max,
                numeric_modifiers(&key, e.modifiers.alt, e.modifiers.shift, false),
            );
            let applied = if self.numeric_snap {
                adjusted(next, 0., step, min, max, (false, false, true))
            } else {
                next
            };
            let applied = numeric_value(&key, applied);
            if let Some(doc) = &self.numeric_before {
                self.session.doc = doc.clone();
            }
            let before = self.session.doc.clone();
            if let Err(error) = self.apply_numeric(&key, applied) {
                self.session.doc = before;
                self.status = error;
            }
            self.drag = Drag::Numeric {
                key,
                start,
                last: e.position,
                value: next,
                initial,
                step,
                min,
                max,
                moved,
            };
            self.refresh(cx);
        }
        true
    }
}

/// Arithmetic only: no script execution, file access, or external evaluation.
pub fn expression(text: &str, current: f64) -> Result<f64, String> {
    if text.len() > 256 {
        return Err("式は256文字以内です".into());
    }
    struct Parser<'a> {
        s: &'a [u8],
        i: usize,
        x: f64,
        depth: usize,
    }
    impl Parser<'_> {
        fn ws(&mut self) {
            while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
                self.i += 1;
            }
        }
        fn eat(&mut self, c: u8) -> bool {
            self.ws();
            if self.s.get(self.i) == Some(&c) {
                self.i += 1;
                true
            } else {
                false
            }
        }
        fn sum(&mut self) -> Option<f64> {
            let mut v = self.product()?;
            loop {
                if self.eat(b'+') {
                    v += self.product()?;
                } else if self.eat(b'-') {
                    v -= self.product()?;
                } else {
                    break;
                }
            }
            Some(v)
        }
        fn product(&mut self) -> Option<f64> {
            let mut v = self.atom()?;
            loop {
                if self.eat(b'*') {
                    v *= self.atom()?;
                } else if self.eat(b'/') {
                    v /= self.atom()?;
                } else {
                    break;
                }
            }
            Some(v)
        }
        fn atom(&mut self) -> Option<f64> {
            self.depth += 1;
            if self.depth > 32 {
                return None;
            }
            let result = if self.eat(b'(') {
                let v = self.sum()?;
                if !self.eat(b')') {
                    return None;
                }
                Some(v)
            } else if self.eat(b'-') {
                Some(-self.atom()?)
            } else if self.eat(b'+') {
                self.atom()
            } else if self.eat(b'x') {
                Some(self.x)
            } else {
                self.ws();
                let start = self.i;
                while self.i < self.s.len()
                    && (self.s[self.i].is_ascii_digit() || self.s[self.i] == b'.')
                {
                    self.i += 1;
                }
                if self.s.get(self.i).is_some_and(|c| *c == b'e' || *c == b'E') {
                    self.i += 1;
                    if self.s.get(self.i).is_some_and(|c| *c == b'+' || *c == b'-') {
                        self.i += 1;
                    }
                    while self.s.get(self.i).is_some_and(u8::is_ascii_digit) {
                        self.i += 1;
                    }
                }
                std::str::from_utf8(&self.s[start..self.i])
                    .ok()?
                    .parse()
                    .ok()
            };
            self.depth -= 1;
            result
        }
    }
    let mut p = Parser {
        s: text.as_bytes(),
        i: 0,
        x: current,
        depth: 0,
    };
    let v = p.sum().filter(|v| v.is_finite());
    p.ws();
    v.filter(|_| p.i == p.s.len())
        .ok_or_else(|| "数値または式を入力してください（例：x * 1.2、100 / 3）".into())
}
#[cfg(test)]
mod tests {
    use super::{adjusted, expression};
    #[test]
    fn expressions_are_bounded_arithmetic() {
        assert_eq!(expression("(x + 10) / 2", 30.).unwrap(), 20.);
        assert_eq!(expression("-1e2 + 2*3", 0.).unwrap(), -94.);
        for invalid in ["1/0", "NaN", "x +", "1;2", "Math.random()"] {
            assert!(expression(invalid, 1.).is_err());
        }
    }
    #[test]
    fn seed_preview_and_editing_respect_the_integer_model() {
        let seed = crate::app::Editing::Parameter(pathweaver_core::appearance::Parameter::Seed);
        assert!(super::validate_numeric_kind(&seed, 1.9).is_err());
        assert!(super::validate_numeric_kind(&seed, 2.).is_ok());
        assert_eq!(super::numeric_value(&seed, 1.9), 2.);
        assert_eq!(
            super::numeric_modifiers(&seed, true, false, false),
            (false, false, false)
        );
        assert_eq!(super::display(0.000_012), "1.200e-5");
        assert_eq!(super::display(u32::MAX as f64), "4294967295");
    }
    #[test]
    fn fine_coarse_snap_and_limits() {
        assert_eq!(adjusted(10., 10., 1., 0., 100., (true, false, false)), 11.);
        assert_eq!(adjusted(10., 10., 1., 0., 100., (false, true, false)), 100.);
        assert_eq!(adjusted(13., 0., 1., 0., 100., (false, false, true)), 10.);
    }
}
