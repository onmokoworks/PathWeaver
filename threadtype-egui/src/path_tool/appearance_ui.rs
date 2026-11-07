//! Compact node canvas. Graph edits remain in the document's ordinary undo history.
use super::appearance::{Graph, Kind, MathOperation, Operation};
use super::appearance_assets::{self, Preset};
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
    selection: Vec<u64>,
    show_parameters: bool,
    search: String,
    search_focus: bool,
    preset_category: String,
    lazy_from: Option<u64>,
}
impl Default for GraphEditor {
    fn default() -> Self {
        Self {
            selected: None,
            connecting: None,
            pan: Vec2::ZERO,
            error: String::new(),
            zoom: 1.,
            fit_requested: true,
            last_size: Vec2::new(700., 400.),
            selection: vec![],
            show_parameters: true,
            search: String::new(),
            search_focus: false,
            preset_category: String::new(),
            lazy_from: None,
        }
    }
}
impl GraphEditor {
    pub fn show(&mut self, ui: &mut egui::Ui, graph: &mut Graph, library: &mut Vec<Preset>) {
        let over_editor = ui
            .max_rect()
            .contains(ui.ctx().pointer_hover_pos().unwrap_or(Pos2::ZERO));
        if over_editor && !ui.ctx().wants_keyboard_input() {
            if ui.input(|i| i.key_pressed(egui::Key::Home)) {
                self.fit_requested = true;
            }
            if ui
                .input(|i| i.key_pressed(egui::Key::N) && !i.modifiers.command && !i.modifiers.ctrl)
            {
                self.show_parameters = !self.show_parameters;
            }
        }
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut graph.enabled, "効果を有効にする");
            ui.separator();
            let add = ui.button("追加 ▾").on_hover_text("Shift+A");
            let popup_id = ui.id().with("node_add_menu");
            if over_editor
                && !ui.ctx().wants_keyboard_input()
                && ui.input(|i| i.modifiers.shift && i.key_pressed(egui::Key::A))
            {
                self.search.clear();
                self.search_focus = true;
                egui::Popup::open_id(ui.ctx(), popup_id);
            }
            egui::Popup::menu(&add)
                .id(popup_id)
                .show(|ui| self.add_menu(ui, graph));
            self.set_menu(ui, graph, library);
            ui.menu_button("テンプレート", |ui| {
                if ui.button("シンプル").clicked() {
                    *graph = Graph::default();
                    self.reset_view();
                    ui.close();
                }
                if ui.button("ラフ").clicked() {
                    *graph = Graph::rough();
                    self.reset_view();
                    ui.close();
                }
                if ui.button("ジグザグ").clicked() {
                    *graph = Graph::rough();
                    graph.node_mut(3).unwrap().kind = Kind::Zigzag;
                    graph.node_mut(3).unwrap().amount = 8.;
                    self.reset_view();
                    ui.close();
                }
                if ui.button("ずらして重ねる").clicked() {
                    *graph = Graph::default();
                    let moved = graph.add(Kind::Transform, [240., 240.]).unwrap();
                    graph.node_mut(moved).unwrap().translation = [18., 18.];
                    let color = graph.add(Kind::Fill, [440., 240.]).unwrap();
                    graph.node_mut(color).unwrap().color = [90, 90, 100];
                    let stack = graph.add(Kind::Stack, [640., 90.]).unwrap();
                    graph.node_mut(2).unwrap().position = [840., 90.];
                    for (from, to, port) in [
                        (1, moved, 0),
                        (moved, color, 0),
                        (color, stack, 0),
                        (1, stack, 1),
                        (stack, 2, 0),
                    ] {
                        graph.connect(from, to, port).unwrap();
                    }
                    self.reset_view();
                    ui.close();
                }
            });
            if ui.button("全体表示").clicked() {
                self.fit_requested = true;
            }
            if ui.button("100%").clicked() {
                self.zoom = 1.;
                self.pan = Vec2::ZERO;
            }
            ui.toggle_value(&mut self.show_parameters, "設定")
                .on_hover_text("N：設定欄の表示切替");
            ui.menu_button("操作", |ui| {
                ui.label("Shift+A：追加 / Home：全体 / N：設定");
                ui.label("Shift+クリック：複数選択 / X・Delete：削除");
                ui.label("中ボタンドラッグ・2本指スクロール：移動");
                ui.label("ホイール・ピンチ：ズーム");
                ui.label("Ctrl/Cmd+Shift+クリック：出力に接続して確認");
                ui.label("Option+右ドラッグ：ノード間を自動接続");
                ui.label("Option+入力ソケット：接続解除");
            });
        });
        if !self.error.is_empty() {
            ui.colored_label(Color32::LIGHT_RED, &self.error);
        }
        ui.separator();
        if self.show_parameters {
            egui::SidePanel::right("appearance_parameters")
                .resizable(false)
                .exact_width(210.)
                .show_inside(ui, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| self.parameters(ui, graph));
                });
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show_inside(ui, |ui| {
                self.canvas(ui, graph);
            });
    }
    fn add_menu(&mut self, ui: &mut egui::Ui, graph: &mut Graph) {
        ui.set_min_width(220.);
        let search =
            ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("ノードを検索…"));
        if self.search_focus {
            search.request_focus();
            self.search_focus = false;
        }
        let query = self.search.trim().to_lowercase();
        for (label, kinds) in [
            (
                "輪郭",
                &[
                    Kind::Rough,
                    Kind::Zigzag,
                    Kind::Pucker,
                    Kind::Round,
                    Kind::Offset,
                    Kind::Simplify,
                    Kind::Transform,
                    Kind::Band,
                    Kind::Dissolve,
                    Kind::Streak,
                    Kind::Halftone,
                ][..],
            ),
            (
                "色と効果",
                &[
                    Kind::Fill,
                    Kind::Opacity,
                    Kind::Shadow,
                    Kind::Glow,
                    Kind::Blur,
                ][..],
            ),
            (
                "合成",
                &[Kind::Pathfinder, Kind::Boolean, Kind::Stack, Kind::Reroute][..],
            ),
            ("数値", &[Kind::Value, Kind::Math, Kind::Measure][..]),
        ] {
            let mut choices = |ui: &mut egui::Ui| {
                for kind in kinds {
                    if !query.is_empty()
                        && !kind.label().contains(&query)
                        && !format!("{kind:?}").to_lowercase().contains(&query)
                    {
                        continue;
                    }
                    if ui.button(kind.label()).clicked() {
                        let position = (self.last_size * 0.5 - self.pan) / self.zoom
                            - Vec2::new(WIDTH * 0.5, 40.);
                        match graph.add(*kind, [position.x, position.y]) {
                            Ok(id) => {
                                self.selected = Some(id);
                                self.selection = vec![id];
                                self.error.clear();
                            }
                            Err(e) => self.error = e,
                        }
                        ui.close();
                    }
                }
            };
            if query.is_empty() {
                ui.menu_button(label, choices);
            } else {
                choices(ui);
            }
        }
    }
    fn set_menu(&mut self, ui: &mut egui::Ui, graph: &mut Graph, library: &mut Vec<Preset>) {
        ui.menu_button("セット ▾", |ui| {
            ui.set_min_width(260.);
            ui.label("名前付きノードセット");
            ui.add(
                egui::TextEdit::singleline(&mut graph.name)
                    .hint_text("セット名")
                    .char_limit(80),
            );
            ui.add(
                egui::TextEdit::singleline(&mut self.preset_category)
                    .hint_text("種類（文字・紙・装飾など）")
                    .char_limit(40),
            );
            let valid_name = !graph.name.trim().is_empty();
            if ui
                .add_enabled(valid_name, egui::Button::new("文書に登録・更新"))
                .clicked()
            {
                self.error = Preset::new(&graph.name, &self.preset_category, graph)
                    .and_then(|p| appearance_assets::register(library, p))
                    .err()
                    .unwrap_or_default();
                ui.close();
            }
            if ui
                .add_enabled(valid_name, egui::Button::new("セットを書き出す…"))
                .clicked()
            {
                let result =
                    Preset::new(&graph.name, &self.preset_category, graph).and_then(|p| p.encode());
                match result {
                    Ok(data) => {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("PathWeaver Nodes", &["pathweaver-nodes"])
                            .set_file_name("NodeSet.pathweaver-nodes")
                            .save_file()
                        {
                            self.error = std::fs::write(path, data)
                                .err()
                                .map_or(String::new(), |e| e.to_string());
                        }
                    }
                    Err(e) => self.error = e,
                }
                ui.close();
            }
            if ui.button("セットを読み込む…").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("PathWeaver Nodes", &["pathweaver-nodes"])
                    .pick_file()
                {
                    let result = (|| -> Result<Preset, String> {
                        if std::fs::metadata(&path).map_err(|e| e.to_string())?.len()
                            > 2 * 1024 * 1024
                        {
                            return Err("ノードセットは2MBまでです".into());
                        }
                        Preset::decode(&std::fs::read(path).map_err(|e| e.to_string())?)
                    })();
                    self.error = result
                        .and_then(|p| appearance_assets::register(library, p))
                        .err()
                        .unwrap_or_default();
                }
                ui.close();
            }
            ui.separator();
            ui.label("この文書のセット（コピーを適用）");
            let mut remove = None;
            for (index, preset) in library.iter().enumerate() {
                ui.horizontal(|ui| {
                    let label = if preset.category.is_empty() {
                        preset.name.clone()
                    } else {
                        format!("{} / {}", preset.category, preset.name)
                    };
                    if ui.button(label).clicked() {
                        *graph = preset.apply();
                        self.preset_category = preset.category.clone();
                        self.reset_view();
                        ui.close();
                    }
                    if ui.small_button("削除").clicked() {
                        remove = Some(index);
                    }
                });
            }
            if let Some(index) = remove {
                library.remove(index);
            }
            if library.is_empty() {
                ui.weak("登録したセットはプロジェクトと一緒に保存されます");
            }
        })
        .response
        .on_hover_text(if graph.name.is_empty() {
            "名前を付けて保存・再利用"
        } else {
            &graph.name
        });
    }
    fn lazy_connect(graph: &mut Graph, from: u64, to: u64) -> Result<(), String> {
        let source = graph.node(from).ok_or("接続元がありません")?;
        let target = graph.node(to).ok_or("接続先がありません")?;
        if source.kind.is_number() {
            let parameter = target
                .parameters()
                .into_iter()
                .find(|p| !target.bindings.iter().any(|(key, _)| *key == p.key))
                .ok_or("空いている数値入力がありません")?
                .key;
            graph.connect_parameter(from, to, parameter)
        } else {
            let port = target
                .inputs
                .iter()
                .position(Option::is_none)
                .or_else(|| (!target.inputs.is_empty()).then_some(0))
                .ok_or("外観入力がありません")?;
            graph.connect(from, to, port)
        }
    }

    fn reset_view(&mut self) {
        self.selected = None;
        self.selection.clear();
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
        ui.label("名前");
        ui.add(
            egui::TextEdit::singleline(&mut node.label)
                .hint_text(node.kind.label())
                .char_limit(80),
        );
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
            ui.available_size().max(Vec2::new(80., 40.)),
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
        if rect.contains(ui.ctx().pointer_hover_pos().unwrap_or(Pos2::ZERO)) {
            if ui.input(|i| i.pointer.button_down(egui::PointerButton::Middle)) {
                self.pan += ui.input(|i| i.pointer.delta());
            } else if !ui.ctx().input(|i| i.pointer.any_down()) {
                if let Some(p) = ui.ctx().pointer_hover_pos() {
                    for event in ui.input(|i| i.events.clone()) {
                        let factor = match event {
                            egui::Event::MouseWheel {
                                unit,
                                delta,
                                modifiers,
                            } => {
                                if cfg!(target_os = "macos")
                                    && unit == egui::MouseWheelUnit::Point
                                    && !modifiers.command
                                    && !modifiers.ctrl
                                {
                                    self.pan += delta;
                                    continue;
                                }
                                let scale = match unit {
                                    egui::MouseWheelUnit::Point => 1.,
                                    egui::MouseWheelUnit::Line => 40.,
                                    egui::MouseWheelUnit::Page => 200.,
                                };
                                (delta.y * scale * 0.002).exp()
                            }
                            egui::Event::Zoom(factor) => factor,
                            _ => continue,
                        };
                        if factor.is_finite() && factor > 0. {
                            let world = (p - rect.min - self.pan) / self.zoom;
                            self.zoom = (self.zoom * factor).clamp(0.25, 2.);
                            self.pan = p - rect.min - world * self.zoom;
                        }
                    }
                }
            }
        }
        let zoom = self.zoom;
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0., Color32::from_gray(29));
        let grid = (24. * zoom).max(12.);
        let mut x = rect.left() + self.pan.x.rem_euclid(grid);
        while x < rect.right() {
            let mut y = rect.top() + self.pan.y.rem_euclid(grid);
            while y < rect.bottom() {
                painter.circle_filled(Pos2::new(x, y), 0.65, Color32::from_gray(55));
                y += grid;
            }
            x += grid;
        }
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
        let mut preview_node = None;
        let mut move_nodes = None;
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
            if response.clicked() {
                if ui.input(|i| (i.modifiers.ctrl || i.modifiers.command) && i.modifiers.shift)
                    && !node.kind.is_number()
                    && node.kind != Kind::Output
                {
                    preview_node = Some(node.id);
                } else if ui.input(|i| i.modifiers.shift) {
                    if self.selection.contains(&node.id) {
                        self.selection.retain(|id| *id != node.id);
                    } else {
                        self.selection.push(node.id);
                    }
                } else {
                    self.selection = vec![node.id];
                }
                self.selected = self.selection.last().copied();
            }
            if response.drag_started() {
                if !self.selection.contains(&node.id) {
                    self.selection = vec![node.id];
                }
                self.selected = Some(node.id);
            }
            if response.dragged() {
                move_nodes = Some(ui.ctx().input(|i| i.pointer.delta()) / zoom);
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
                    if self.selection.contains(&node.id) || self.selected == Some(node.id) {
                        Color32::from_rgb(255, 150, 60)
                    } else {
                        Color32::from_gray(70)
                    },
                ),
                egui::StrokeKind::Inside,
            );
            let header_color = match node.kind {
                Kind::Input | Kind::Output => Color32::from_rgb(107, 57, 62),
                Kind::Value | Kind::Math | Kind::Measure => Color32::from_rgb(88, 69, 113),
                Kind::Fill | Kind::Opacity | Kind::Shadow | Kind::Glow | Kind::Blur => {
                    Color32::from_rgb(113, 91, 42)
                }
                Kind::Stack | Kind::Boolean | Kind::Pathfinder | Kind::Reroute => {
                    Color32::from_rgb(57, 96, 71)
                }
                _ => Color32::from_rgb(48, 79, 103),
            };
            painter.rect_filled(
                Rect::from_min_size(
                    r.min + Vec2::splat(1.),
                    Vec2::new(
                        r.width() - 2.,
                        if node.label.is_empty() { 30. } else { 40. } * zoom,
                    ),
                ),
                4.,
                header_color,
            );
            let title = if node.label.is_empty() {
                node.kind.label().to_owned()
            } else {
                node.label.clone()
            };
            let title = if title.chars().count() > 12 {
                format!("{}…", title.chars().take(12).collect::<String>())
            } else {
                title
            };
            painter.text(
                r.min + Vec2::new(12., 16.) * zoom,
                egui::Align2::LEFT_CENTER,
                title,
                egui::FontId::proportional((13. * zoom).max(8.)),
                Color32::WHITE,
            );
            if !node.label.is_empty() {
                painter.text(
                    r.min + Vec2::new(12., 32.) * zoom,
                    egui::Align2::LEFT_CENTER,
                    node.kind.label(),
                    egui::FontId::proportional((9. * zoom).max(7.)),
                    Color32::from_gray(205),
                );
            }
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
        if let Some(delta) = move_nodes {
            for node in &mut graph.nodes {
                if self.selection.contains(&node.id) {
                    node.position[0] = (node.position[0] + delta.x).clamp(-100_000., 100_000.);
                    node.position[1] = (node.position[1] + delta.y).clamp(-100_000., 100_000.);
                }
            }
        }
        if let Some(id) = preview_node {
            self.error = graph.connect(id, graph.output, 0).err().unwrap_or_default();
        }
        if let Some(pointer) = ui.ctx().pointer_hover_pos().filter(|p| rect.contains(*p)) {
            let nearest = |include_output: bool| {
                graph
                    .nodes
                    .iter()
                    .filter(|n| n.kind != Kind::Output || include_output)
                    .min_by(|a, b| {
                        node_rect(a)
                            .center()
                            .distance(pointer)
                            .total_cmp(&node_rect(b).center().distance(pointer))
                    })
                    .map(|n| n.id)
            };
            if ui.input(|i| {
                i.modifiers.alt && i.pointer.button_pressed(egui::PointerButton::Secondary)
            }) {
                self.lazy_from = nearest(false);
            }
            if ui.input(|i| i.pointer.button_released(egui::PointerButton::Secondary)) {
                let to = nearest(true);
                if let (Some(from), Some(to)) = (self.lazy_from.take(), to) {
                    self.error = Self::lazy_connect(graph, from, to)
                        .err()
                        .unwrap_or_default();
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
            && ui.ctx().input(|i| {
                i.key_pressed(egui::Key::Delete)
                    || i.key_pressed(egui::Key::Backspace)
                    || i.key_pressed(egui::Key::X)
            })
        {
            if let Some(id) = self.selected.take() {
                graph.remove(id);
            }
            for id in self.selection.drain(..) {
                graph.remove(id);
            }
        }
        if background.dragged() && !any_node_hovered && self.connecting.is_none() {
            self.pan += ui.ctx().input(|i| i.pointer.delta());
        }
        if background.clicked() && !any_node_hovered {
            self.selected = None;
            self.selection.clear();
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
    fn modified_frame(
        ctx: &egui::Context,
        editor: &mut GraphEditor,
        graph: &mut Graph,
        time: f64,
        position: Pos2,
        pressed: Option<bool>,
        modifiers: egui::Modifiers,
    ) {
        let mut events = vec![egui::Event::PointerMoved(position)];
        if let Some(pressed) = pressed {
            events.push(egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers,
            });
        }
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1000., 800.))),
                time: Some(time),
                modifiers,
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| editor.canvas(ui, graph));
            },
        );
    }
    #[test]
    fn shift_selection_moves_nodes_together_and_preview_connects_to_output() {
        let ctx = egui::Context::default();
        let mut editor = GraphEditor {
            fit_requested: false,
            ..Default::default()
        };
        let mut graph = Graph::rough();
        frame(&ctx, &mut editor, &mut graph, 0., vec![]);
        let first = Pos2::new(90., 110.);
        let rough = Pos2::new(330., 110.);
        click(&ctx, &mut editor, &mut graph, first, 0.1);
        let shift = egui::Modifiers {
            shift: true,
            ..Default::default()
        };
        modified_frame(&ctx, &mut editor, &mut graph, 0.2, rough, Some(true), shift);
        modified_frame(
            &ctx,
            &mut editor,
            &mut graph,
            0.21,
            rough,
            Some(false),
            shift,
        );
        assert_eq!(editor.selection, vec![1, 3]);
        let original = graph.clone();
        modified_frame(
            &ctx,
            &mut editor,
            &mut graph,
            0.3,
            rough,
            Some(true),
            egui::Modifiers::NONE,
        );
        modified_frame(
            &ctx,
            &mut editor,
            &mut graph,
            0.4,
            rough + Vec2::new(20., 30.),
            None,
            egui::Modifiers::NONE,
        );
        modified_frame(
            &ctx,
            &mut editor,
            &mut graph,
            0.5,
            rough + Vec2::new(20., 30.),
            Some(false),
            egui::Modifiers::NONE,
        );
        for id in [1, 3] {
            let after = graph.node(id).unwrap().position;
            let before = original.node(id).unwrap().position;
            assert_eq!(after, [before[0] + 20., before[1] + 30.]);
        }
        assert_eq!(
            graph.node(2).unwrap().position,
            original.node(2).unwrap().position
        );
        graph.connect(1, graph.output, 0).unwrap();
        let preview = egui::Modifiers {
            command: true,
            mac_cmd: true,
            shift: true,
            ..Default::default()
        };
        let p = rough + Vec2::new(20., 30.);
        modified_frame(&ctx, &mut editor, &mut graph, 0.6, p, Some(true), preview);
        modified_frame(&ctx, &mut editor, &mut graph, 0.61, p, Some(false), preview);
        assert_eq!(graph.node(graph.output).unwrap().inputs[0], Some(3));
    }
    #[test]
    fn lazy_connections_choose_typed_inputs_and_reject_cycles() {
        let mut graph = Graph::rough();
        let value = graph.add(Kind::Value, [0., 300.]).unwrap();
        GraphEditor::lazy_connect(&mut graph, value, 3).unwrap();
        assert_eq!(graph.node(3).unwrap().bindings[0].1, value);
        let transform = graph.add(Kind::Transform, [400., 300.]).unwrap();
        GraphEditor::lazy_connect(&mut graph, 3, transform).unwrap();
        let before = graph.clone();
        assert!(GraphEditor::lazy_connect(&mut graph, transform, 3).is_err());
        assert_eq!(graph, before);
        assert!(GraphEditor::lazy_connect(&mut graph, 1, value).is_err());
    }
    #[test]
    fn sockets_connect_by_click_and_drag_and_numeric_ports_are_typed() {
        let ctx = egui::Context::default();
        let mut editor = GraphEditor {
            fit_requested: false,
            ..Default::default()
        };
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
