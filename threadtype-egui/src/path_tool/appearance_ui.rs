//! Compact node canvas. Graph edits remain in the document's ordinary undo history.
use super::appearance::{Graph, Kind, MathOperation, Operation};
use super::appearance_effects::{BandPosition, OffsetJoin};
use egui::{Color32, Pos2, Rect, Sense, Stroke, Vec2};

const WIDTH: f32 = 190.;
const NUMBER: Color32 = Color32::from_rgb(110, 180, 240);
const WIRE: Color32 = Color32::from_rgb(160, 190, 105);
pub struct GraphEditor {
    selected: Option<u64>,
    connecting: Option<u64>,
    pan: Vec2,
    error: String,
    zoom: f32,
    fit_requested: bool,
    last_size: Vec2,
}
impl Default for GraphEditor {
    fn default() -> Self {
        Self {
            selected: None,
            connecting: None,
            pan: Vec2::ZERO,
            error: String::new(),
            zoom: 1.,
            fit_requested: false,
            last_size: Vec2::new(700., 400.),
        }
    }
}
impl GraphEditor {
    pub fn show(&mut self, ctx: &egui::Context, open: &mut bool, graph: &mut Graph) {
        egui::Window::new("ノードで外観を編集")
            .id(egui::Id::new("appearance_graph"))
            .open(open).default_size([980., 580.]).min_size([700., 420.]).show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut graph.enabled, "効果を有効にする");
                    ui.separator();
                    ui.menu_button("ノードを追加", |ui| {
                        for (label,kinds) in [
                            ("輪郭", &[Kind::Rough, Kind::Zigzag, Kind::Pucker, Kind::Round, Kind::Offset, Kind::Simplify, Kind::Transform, Kind::Band, Kind::Dissolve, Kind::Streak, Kind::Halftone][..]),
                            ("色と効果", &[Kind::Fill, Kind::Opacity, Kind::Shadow, Kind::Glow, Kind::Blur][..]),
                            ("合成", &[Kind::Pathfinder, Kind::Boolean, Kind::Stack, Kind::Reroute][..]),
                            ("数値", &[Kind::Value, Kind::Math, Kind::Measure][..]),
                        ] {
                            ui.menu_button(label,|ui| {
                                for kind in kinds {
                                    if ui.button(kind.label()).clicked() {
                                        let position=(self.last_size*0.5-self.pan)/self.zoom-Vec2::new(WIDTH*0.5,40.);
                                        match graph.add(*kind,[position.x,position.y]) {Ok(id)=>{self.selected=Some(id);self.error.clear();},Err(e)=>self.error=e}
                                        ui.close();
                                    }
                                }
                            });
                        }
                    });
                    ui.menu_button("プリセット", |ui| {
                        if ui.button("シンプル").clicked() { *graph = Graph::default(); self.reset_view(); ui.close(); }
                        if ui.button("ラフ").clicked() { *graph = Graph::rough(); self.reset_view(); ui.close(); }
                        if ui.button("ジグザグ").clicked() {
                            *graph = Graph::rough(); graph.node_mut(3).unwrap().kind = Kind::Zigzag;
                            graph.node_mut(3).unwrap().amount = 8.; self.reset_view(); ui.close();
                        }
                        if ui.button("ずらして重ねる").clicked() {
                            *graph = Graph::default();
                            let moved = graph.add(Kind::Transform, [240., 240.]).unwrap();
                            graph.node_mut(moved).unwrap().translation = [18., 18.];
                            let color = graph.add(Kind::Fill, [440., 240.]).unwrap();
                            graph.node_mut(color).unwrap().color = [90, 90, 100];
                            let stack = graph.add(Kind::Stack, [640., 90.]).unwrap();
                            graph.node_mut(2).unwrap().position = [840., 90.];
                            for (from, to, port) in [(1, moved, 0), (moved, color, 0), (color, stack, 0), (1, stack, 1), (stack, 2, 0)] {
                                graph.connect(from, to, port).unwrap();
                            }
                            self.reset_view(); ui.close();
                        }
                    });
                    if ui.button("全体表示").clicked() { self.fit_requested = true; }
                    if ui.button("100%").clicked() { self.zoom=1.; self.pan=Vec2::ZERO; }
                });
                ui.small("右の丸 → 接続先の左の丸。ドラッグでも接続できます。Alt/Option＋入力の丸で切断。空白をドラッグして移動。ホイールで拡大・縮小。");
                ui.small("緑＝外観、青＝数値。値・計算・形の大きさを効果の青い丸につなぐとパラメータを制御できます。");
                ui.small("元の芯は保持します。効果の寸法は文書座標です。「重ねる」は上の入力が背面、下が前面。");
                if !self.error.is_empty() { ui.colored_label(Color32::LIGHT_RED, &self.error); }
                ui.separator();
                egui::SidePanel::right("appearance_parameters").resizable(false).exact_width(210.).show_inside(ui, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| self.parameters(ui, graph));
                });
                egui::CentralPanel::default().frame(egui::Frame::NONE).show_inside(ui, |ui| { self.canvas(ui, graph); });
            });
    }
    fn reset_view(&mut self) {
        self.selected = None;
        self.connecting = None;
        self.pan = Vec2::ZERO;
        self.zoom = 1.;
        self.fit_requested = true;
        self.error.clear();
    }
    fn parameters(&mut self, ui: &mut egui::Ui, graph: &mut Graph) {
        let Some(id) = self.selected else {
            ui.label("ノードを選んで調整");
            return;
        };
        let Some(node) = graph.node_mut(id) else {
            self.selected = None;
            return;
        };
        ui.strong(node.kind.label());
        if !matches!(node.kind, Kind::Input | Kind::Output) {
            ui.checkbox(&mut node.bypass, "このノードを無効化");
        }
        ui.separator();
        for spec in node.parameters() {
            ui.horizontal(|ui| {
                ui.label(spec.label);
                let linked = node.bindings.iter().any(|(key, _)| *key == spec.key);
                let mut value = node.number(spec.key);
                if ui
                    .add_enabled(
                        !linked,
                        egui::DragValue::new(&mut value)
                            .speed(if spec.max <= 1. { 0.01 } else { 0.2 })
                            .range(spec.min..=spec.max),
                    )
                    .changed()
                {
                    node.set_number(spec.key, value);
                }
                if linked
                    && ui
                        .small_button("切断")
                        .on_hover_text("数値ノードとの接続を解除")
                        .clicked()
                {
                    node.bindings.retain(|(key, _)| *key != spec.key);
                }
            });
        }
        match node.kind {
            Kind::Rough => {
                ui.checkbox(&mut node.smooth, "滑らかにする");
                if ui.button("別の揺らぎ").clicked() {
                    node.seed = node.seed.wrapping_add(1);
                }
            }
            Kind::Fill | Kind::Shadow | Kind::Glow => {
                ui.color_edit_button_srgb(&mut node.color);
                if node.kind == Kind::Glow {
                    ui.checkbox(&mut node.inner, "内側に光彩");
                }
            }
            Kind::Offset | Kind::Band => {
                ui.label("角の接合");
                for (join, name) in [
                    (OffsetJoin::Round, "ラウンド"),
                    (OffsetJoin::Miter, "マイター"),
                    (OffsetJoin::Bevel, "ベベル"),
                ] {
                    ui.selectable_value(&mut node.effect.join, join, name);
                }
                if node.kind == Kind::Band {
                    ui.label("縁取りの位置");
                    for (position, name) in [
                        (BandPosition::Outer, "外側"),
                        (BandPosition::Inner, "内側"),
                        (BandPosition::Center, "中央"),
                    ] {
                        ui.selectable_value(&mut node.effect.position, position, name);
                    }
                }
            }
            Kind::Pathfinder | Kind::Boolean => {
                for operation in [
                    Operation::Union,
                    Operation::Difference,
                    Operation::Intersect,
                    Operation::Xor,
                ] {
                    ui.selectable_value(&mut node.operation, operation, operation.label());
                }
                ui.small(if node.kind == Kind::Pathfinder {
                    "入力内のレイヤーを背面から順に合成。先頭の色を使用。"
                } else {
                    "上が背面、下が前面。背面の色を使用。"
                });
            }
            Kind::Math => {
                for operation in [
                    MathOperation::Add,
                    MathOperation::Subtract,
                    MathOperation::Multiply,
                    MathOperation::Divide,
                    MathOperation::Min,
                    MathOperation::Max,
                    MathOperation::Power,
                    MathOperation::Modulo,
                    MathOperation::Snap,
                ] {
                    ui.selectable_value(&mut node.math, operation, operation.label());
                }
            }
            Kind::Measure => {
                ui.selectable_value(&mut node.measure_height, false, "幅");
                ui.selectable_value(&mut node.measure_height, true, "高さ");
            }
            Kind::Stack => {
                ui.label("上：背面の外観\n下：前面の外観");
            }
            Kind::Input => {
                ui.label("編集可能な元パスの塗りと線を入力します。");
            }
            Kind::Output => {
                ui.label(
                    "この入力がキャンバスとSVGに反映されます。未接続の場合は何も描画しません。",
                );
            }
            Kind::Reroute => {
                ui.label("入力した外観をそのまま出力します。");
            }
            _ => {}
        }
        if matches!(node.kind, Kind::Blur | Kind::Glow | Kind::Shadow) {
            ui.small("ぼかしはベクター輪郭の重ね合わせによる近似です。");
        }
        ui.separator();
        for (i, input) in node.inputs.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.label(format!("入力 {}", i + 1));
                if input.is_some() && ui.small_button("切断").clicked() {
                    *input = None;
                } else if input.is_none() {
                    ui.weak("未接続");
                }
            });
        }
        if !matches!(node.kind, Kind::Input | Kind::Output) && ui.button("ノードを削除").clicked()
        {
            graph.remove(id);
            self.selected = None;
            if self.connecting == Some(id) {
                self.connecting = None;
            }
        }
    }
    fn canvas(&mut self, ui: &mut egui::Ui, graph: &mut Graph) {
        let (rect, background) = ui.allocate_exact_size(
            ui.available_size().max(Vec2::new(100., 200.)),
            Sense::click_and_drag(),
        );
        self.last_size = rect.size();
        let size = |node: &super::appearance::Node| {
            Vec2::new(
                WIDTH,
                54. + (node.inputs.len() + node.parameters().len()).max(1) as f32 * 22.,
            )
        };
        if self.fit_requested {
            let mut bounds = Rect::NOTHING;
            for node in &graph.nodes {
                bounds = bounds.union(Rect::from_min_size(
                    Pos2::new(node.position[0], node.position[1]),
                    size(node),
                ));
            }
            if bounds.is_finite() {
                let available = (rect.size() - Vec2::splat(48.)).max(Vec2::splat(1.));
                self.zoom = (available.x / bounds.width().max(1.))
                    .min(available.y / bounds.height().max(1.))
                    .clamp(0.25, 1.5);
                self.pan = rect.size() * 0.5 - bounds.center().to_vec2() * self.zoom;
            }
            self.fit_requested = false;
        }
        if background.contains_pointer() && !ui.ctx().input(|i| i.pointer.any_down()) {
            if let Some(p) = ui.ctx().pointer_hover_pos() {
                let delta: f32 = ui.ctx().input(|i| {
                    i.events
                        .iter()
                        .filter_map(|e| {
                            if let egui::Event::MouseWheel { unit, delta, .. } = e {
                                Some(
                                    delta.y
                                        * match unit {
                                            egui::MouseWheelUnit::Point => 1.,
                                            egui::MouseWheelUnit::Line => 40.,
                                            egui::MouseWheelUnit::Page => 200.,
                                        },
                                )
                            } else {
                                None
                            }
                        })
                        .sum()
                });
                if delta != 0. {
                    let world = (p - rect.min - self.pan) / self.zoom;
                    self.zoom = (self.zoom * (delta * 0.002).exp()).clamp(0.25, 2.);
                    self.pan = p - rect.min - world * self.zoom;
                }
            }
        }
        let zoom = self.zoom;
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 6., Color32::from_gray(17));
        let origin = rect.min + self.pan;
        let node_rect = |node: &super::appearance::Node| {
            Rect::from_min_size(
                origin + Vec2::new(node.position[0], node.position[1]) * zoom,
                size(node) * zoom,
            )
        };
        let input_pos =
            |r: Rect, port: usize| Pos2::new(r.left(), r.top() + (48. + port as f32 * 22.) * zoom);
        let output_pos = |r: Rect| Pos2::new(r.right(), r.top() + 48. * zoom);
        let wire = |from: Pos2, to: Pos2, color: Color32| {
            let dx = ((to.x - from.x).abs() * 0.5).max(50.);
            painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
                [from, from + Vec2::new(dx, 0.), to - Vec2::new(dx, 0.), to],
                false,
                Color32::TRANSPARENT,
                Stroke::new(2_f32, color),
            ));
        };
        for node in &graph.nodes {
            for (port, source) in node.inputs.iter().enumerate() {
                if let Some(source) = source.and_then(|id| graph.node(id)) {
                    wire(
                        output_pos(node_rect(source)),
                        input_pos(node_rect(node), port),
                        WIRE,
                    );
                }
            }
            for (parameter, source) in &node.bindings {
                if let (Some(source), Some(index)) = (
                    graph.node(*source),
                    node.parameters().iter().position(|s| s.key == *parameter),
                ) {
                    wire(
                        output_pos(node_rect(source)),
                        input_pos(node_rect(node), node.inputs.len() + index),
                        NUMBER,
                    );
                }
            }
        }
        if let (Some(id), Some(pointer)) = (self.connecting, ui.ctx().pointer_hover_pos()) {
            if let Some(source) = graph.node(id) {
                wire(
                    output_pos(node_rect(source)),
                    pointer,
                    if source.kind.is_number() {
                        NUMBER
                    } else {
                        WIRE
                    },
                );
            }
        }
        let mut connection = None;
        let mut parameter_connection = None;
        let mut any_node_hovered = false;
        for node in &mut graph.nodes {
            let r = node_rect(node);
            if !r.expand(12.).intersects(rect) {
                continue;
            }
            let response = ui.interact(r, ui.id().with(("node", node.id)), Sense::click_and_drag());
            any_node_hovered |= response.hovered();
            if response.clicked() || response.drag_started() {
                self.selected = Some(node.id);
            }
            if response.dragged() {
                let delta = ui.ctx().input(|i| i.pointer.delta()) / zoom;
                node.position[0] = (node.position[0] + delta.x).clamp(-100_000., 100_000.);
                node.position[1] = (node.position[1] + delta.y).clamp(-100_000., 100_000.);
            }
            painter.rect_filled(
                r,
                6.,
                if node.bypass {
                    Color32::from_gray(24)
                } else {
                    Color32::from_gray(38)
                },
            );
            painter.rect_stroke(
                r,
                6.,
                Stroke::new(
                    1_f32,
                    if self.selected == Some(node.id) {
                        Color32::from_rgb(255, 150, 60)
                    } else {
                        Color32::from_gray(70)
                    },
                ),
                egui::StrokeKind::Inside,
            );
            painter.text(
                r.min + Vec2::new(12., 16.) * zoom,
                egui::Align2::LEFT_CENTER,
                node.kind.label(),
                egui::FontId::proportional((13. * zoom).max(8.)),
                Color32::WHITE,
            );
            for port in 0..node.inputs.len() {
                let p = input_pos(r, port);
                let socket = ui.interact(
                    Rect::from_center_size(p, Vec2::splat(20.)),
                    ui.id().with(("input", node.id, port)),
                    Sense::click(),
                );
                any_node_hovered |= socket.hovered();
                painter.circle_filled(
                    p,
                    5.,
                    if node.inputs[port].is_some() {
                        WIRE
                    } else {
                        Color32::from_gray(90)
                    },
                );
                painter.text(
                    p + Vec2::new(12., 0.),
                    egui::Align2::LEFT_CENTER,
                    if node.kind == Kind::Stack {
                        if port == 0 {
                            "背面"
                        } else {
                            "前面"
                        }
                    } else {
                        "外観"
                    },
                    egui::FontId::proportional((10. * zoom).max(7.)),
                    Color32::GRAY,
                );
                if socket.clicked() && ui.ctx().input(|i| i.modifiers.alt) {
                    node.inputs[port] = None;
                    self.connecting = None;
                } else if (socket.clicked()
                    || (socket.contains_pointer()
                        && ui.ctx().input(|i| i.pointer.primary_released())))
                    && self.connecting.is_some()
                {
                    connection = Some((self.connecting.take().unwrap(), node.id, port));
                }
            }
            for (index, spec) in node.parameters().iter().enumerate() {
                let p = input_pos(r, node.inputs.len() + index);
                let socket = ui.interact(
                    Rect::from_center_size(p, Vec2::splat(20.)),
                    ui.id().with(("parameter", node.id, spec.key)),
                    Sense::click(),
                );
                any_node_hovered |= socket.hovered();
                let connected = node.bindings.iter().any(|(key, _)| *key == spec.key);
                painter.circle_filled(
                    p,
                    5.,
                    if connected {
                        NUMBER
                    } else {
                        Color32::from_gray(90)
                    },
                );
                painter.text(
                    p + Vec2::new(12., 0.),
                    egui::Align2::LEFT_CENTER,
                    format!("{}{}", spec.label, if connected { " ←" } else { "" }),
                    egui::FontId::proportional((10. * zoom).max(7.)),
                    NUMBER,
                );
                if socket.clicked() && ui.ctx().input(|i| i.modifiers.alt) {
                    node.bindings.retain(|(key, _)| *key != spec.key);
                    self.connecting = None;
                } else if (socket.clicked()
                    || (socket.contains_pointer()
                        && ui.ctx().input(|i| i.pointer.primary_released())))
                    && self.connecting.is_some()
                {
                    parameter_connection =
                        Some((self.connecting.take().unwrap(), node.id, spec.key));
                }
            }
            if node.kind != Kind::Output {
                let p = output_pos(r);
                let socket = ui.interact(
                    Rect::from_center_size(p, Vec2::splat(20.)),
                    ui.id().with(("output", node.id)),
                    Sense::click_and_drag(),
                );
                any_node_hovered |= socket.hovered();
                painter.circle_filled(p, 5., if node.kind.is_number() { NUMBER } else { WIRE });
                if socket.clicked() || socket.drag_started() {
                    self.connecting = Some(node.id);
                    self.error.clear();
                }
            }
        }
        if let Some((from, to, port)) = connection {
            self.error = graph.connect(from, to, port).err().unwrap_or_default();
        }
        if let Some((from, to, parameter)) = parameter_connection {
            self.error = graph
                .connect_parameter(from, to, parameter)
                .err()
                .unwrap_or_default();
        }
        if rect.contains(ui.ctx().pointer_hover_pos().unwrap_or(Pos2::ZERO))
            && !ui.ctx().wants_keyboard_input()
            && ui
                .ctx()
                .input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace))
        {
            if let Some(id) = self.selected.take() {
                graph.remove(id);
            }
        }
        if background.dragged() && !any_node_hovered && self.connecting.is_none() {
            self.pan += ui.ctx().input(|i| i.pointer.delta());
        }
        if background.clicked() && !any_node_hovered {
            self.selected = None;
            self.connecting = None;
        }
        if rect.contains(ui.ctx().pointer_hover_pos().unwrap_or(Pos2::ZERO))
            && !ui.ctx().wants_keyboard_input()
            && ui.ctx().input(|i| i.key_pressed(egui::Key::Escape))
        {
            self.connecting = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame(
        ctx: &egui::Context,
        editor: &mut GraphEditor,
        graph: &mut Graph,
        time: f64,
        events: Vec<egui::Event>,
    ) {
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1000., 800.))),
                time: Some(time),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| editor.canvas(ui, graph));
            },
        );
    }
    fn pointer(p: Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos: p,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }
    fn click(ctx: &egui::Context, editor: &mut GraphEditor, graph: &mut Graph, p: Pos2, time: f64) {
        frame(
            ctx,
            editor,
            graph,
            time,
            vec![egui::Event::PointerMoved(p), pointer(p, true)],
        );
        frame(ctx, editor, graph, time + 0.01, vec![pointer(p, false)]);
    }
    #[test]
    fn sockets_connect_by_click_and_drag_and_numeric_ports_are_typed() {
        let ctx = egui::Context::default();
        let mut editor = GraphEditor::default();
        let mut graph = Graph::default();
        graph.node_mut(2).unwrap().inputs[0] = None;
        frame(&ctx, &mut editor, &mut graph, 0., vec![]);
        let from = Pos2::new(8. + 30. + WIDTH, 8. + 90. + 48.);
        let to = Pos2::new(8. + 520., 8. + 90. + 48.);
        click(&ctx, &mut editor, &mut graph, from, 0.1);
        assert_eq!(editor.connecting, Some(1));
        click(&ctx, &mut editor, &mut graph, to, 0.2);
        assert_eq!(graph.node(2).unwrap().inputs[0], Some(1));
        graph.node_mut(2).unwrap().inputs[0] = None;
        frame(
            &ctx,
            &mut editor,
            &mut graph,
            0.3,
            vec![egui::Event::PointerMoved(from), pointer(from, true)],
        );
        frame(
            &ctx,
            &mut editor,
            &mut graph,
            0.4,
            vec![egui::Event::PointerMoved(from + Vec2::new(20., 0.))],
        );
        frame(
            &ctx,
            &mut editor,
            &mut graph,
            0.5,
            vec![egui::Event::PointerMoved(to)],
        );
        frame(&ctx, &mut editor, &mut graph, 0.6, vec![pointer(to, false)]);
        assert_eq!(graph.node(2).unwrap().inputs[0], Some(1));
        let value = graph.add(Kind::Value, [30., 300.]).unwrap();
        let rough = graph.add(Kind::Rough, [520., 300.]).unwrap();
        frame(&ctx, &mut editor, &mut graph, 0.7, vec![]);
        click(
            &ctx,
            &mut editor,
            &mut graph,
            Pos2::new(8. + 30. + WIDTH, 8. + 300. + 48.),
            0.8,
        );
        click(
            &ctx,
            &mut editor,
            &mut graph,
            Pos2::new(8. + 520., 8. + 300. + 48. + 22.),
            0.9,
        );
        assert_eq!(
            graph.node(rough).unwrap().bindings,
            vec![(super::super::appearance::Parameter::Amount, value)]
        );
    }
}
