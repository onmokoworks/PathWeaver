use crate::theme::{Role, color as ui_color};
use crate::{
    app::{Drag, Editing, Workspace},
    drawing::{View, dot, line, normal_bounds, rect, wire},
};
use gpui::{prelude::*, *};
use kurbo::{Point as KPoint, Rect};
use pathweaver_core::{
    appearance::{Graph, Kind, MathOperation, Node, Operation, Parameter},
    appearance_assets::{self, Preset},
};
const WIDTH: f32 = 180.;
const KINDS: [Kind; 23] = [
    Kind::Rough,
    Kind::Zigzag,
    Kind::Transform,
    Kind::Offset,
    Kind::Round,
    Kind::Band,
    Kind::Pucker,
    Kind::Simplify,
    Kind::Streak,
    Kind::Dissolve,
    Kind::Halftone,
    Kind::Fill,
    Kind::Opacity,
    Kind::Shadow,
    Kind::Glow,
    Kind::Blur,
    Kind::Stack,
    Kind::Boolean,
    Kind::Pathfinder,
    Kind::Reroute,
    Kind::Value,
    Kind::Math,
    Kind::Measure,
];
fn matching_kinds(query: &str) -> Vec<Kind> {
    let query = query.trim().to_lowercase();
    KINDS
        .into_iter()
        .filter(|kind| {
            kind.label().to_lowercase().contains(&query)
                || format!("{kind:?}").to_lowercase().contains(&query)
        })
        .collect()
}
fn popup_origin(
    anchor: Point<Pixels>,
    popup: Size<Pixels>,
    viewport: Size<Pixels>,
) -> Point<Pixels> {
    point(
        anchor
            .x
            .max(px(8.))
            .min((viewport.width - popup.width - px(8.)).max(px(8.))),
        anchor
            .y
            .max(px(8.))
            .min((viewport.height - popup.height - px(8.)).max(px(8.))),
    )
}
fn add_unconnected(graph: &mut Graph, kind: Kind, at: KPoint) -> Result<u64, String> {
    graph.add(kind, [at.x as f32, at.y as f32])
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Socket {
    Shape(usize),
    Number(Parameter),
}
fn height(n: &Node) -> f32 {
    74. + (n.inputs.len() + n.parameters().len()).saturating_sub(1) as f32 * 24.
}
fn node_rect(n: &Node) -> Rect {
    Rect::new(
        n.position[0] as f64,
        n.position[1] as f64,
        (n.position[0] + WIDTH) as f64,
        (n.position[1] + height(n)) as f64,
    )
}
#[derive(Clone, Copy, Debug, PartialEq)]
enum NodeHit {
    Body(u64),
    Input(u64, Socket),
    Output(u64),
}
// Visit each visible node as one unit: its body obscures every node below it.
fn node_hit(g: &Graph, p: KPoint, zoom: f32) -> Option<NodeHit> {
    let tolerance = 10. / zoom as f64;
    for n in g.nodes.iter().rev() {
        if n.kind != Kind::Output && output(n).distance(p) < tolerance {
            return Some(NodeHit::Output(n.id));
        }
        for (socket, at, _) in sockets(n) {
            if at.distance(p) < tolerance {
                return Some(NodeHit::Input(n.id, socket));
            }
        }
        if node_rect(n).contains(p) {
            return Some(NodeHit::Body(n.id));
        }
    }
    None
}
fn fitted_view(g: &Graph, width: f32, height: f32) -> Option<View> {
    if g.nodes.is_empty() || width <= 1. || height <= 1. {
        return None;
    }
    let bounds = g.nodes.iter().map(node_rect).reduce(|a, b| a.union(b))?;
    let padding = 24_f32.min(width * 0.1).min(height * 0.1);
    let zoom = ((width - padding * 2.) / bounds.width().max(1.) as f32)
        .min((height - padding * 2.) / bounds.height().max(1.) as f32)
        .min(1.2);
    Some(View {
        zoom,
        pan: [
            width * 0.5 - bounds.center().x as f32 * zoom,
            height * 0.5 - bounds.center().y as f32 * zoom,
        ],
    })
}
fn format_node_number(value: f64) -> String {
    if !value.is_finite() {
        return "—".into();
    }
    if value == 0. {
        return "0".into();
    }
    if value.abs() < 0.001 || value.abs() >= 1e10 {
        return format!("{value:.3e}");
    }
    let value = format!("{value:.3}");
    value.trim_end_matches('0').trim_end_matches('.').into()
}
fn socket_label(n: &Node, socket: Socket, label: &str, effective: Option<f64>) -> String {
    match socket {
        Socket::Number(p) if n.bindings.iter().any(|(key, _)| *key == p) => {
            let value = effective
                .map(format_node_number)
                .unwrap_or_else(|| "—".into());
            format!("{label}  ↗ {value}")
        }
        Socket::Number(p) => format!("{}  {}", label, format_node_number(n.number(p))),
        Socket::Shape(_) => label.into(),
    }
}
/// Keep text and line height in node coordinates, without a fixed font floor.
fn node_text(
    window: &mut Window,
    cx: &mut App,
    at: Point<Pixels>,
    value: impl Into<SharedString>,
    color: u32,
    font_size: f32,
    zoom: f32,
) {
    let value = value.into();
    let run = TextRun {
        len: value.len(),
        font: font(crate::assets::UI_FONT),
        color: rgb(color).into(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let line = window
        .text_system()
        .shape_line(value, px(font_size * zoom), &[run], None);
    let _ = line.paint(at, px(16. * zoom), window, cx);
}
// A deselection click must not turn into a drag of the remaining selection.
fn select_node(ids: &mut Vec<u64>, id: u64, extend: bool) -> bool {
    if extend && ids.contains(&id) {
        ids.retain(|selected| *selected != id);
        return false;
    }
    if !extend && !ids.contains(&id) {
        ids.clear();
    }
    ids.retain(|selected| *selected != id);
    ids.push(id);
    true
}
fn color(k: Kind) -> u32 {
    match k {
        Kind::Input | Kind::Output => ui_color(Role::NodeIo),
        Kind::Value | Kind::Math | Kind::Measure => ui_color(Role::NodeNumber),
        Kind::Fill | Kind::Opacity | Kind::Shadow | Kind::Glow | Kind::Blur => {
            ui_color(Role::NodePaint)
        }
        Kind::Stack | Kind::Boolean | Kind::Pathfinder => ui_color(Role::NodeCombine),
        _ => ui_color(Role::NodeGeometry),
    }
}
fn output(n: &Node) -> KPoint {
    KPoint::new((n.position[0] + WIDTH) as f64, (n.position[1] + 54.) as f64)
}
fn sockets(n: &Node) -> Vec<(Socket, KPoint, &'static str)> {
    let mut v = vec![];
    for i in 0..n.inputs.len() {
        v.push((
            Socket::Shape(i),
            KPoint::new(
                n.position[0] as f64,
                (n.position[1] + 54. + i as f32 * 24.) as f64,
            ),
            if n.inputs.len() == 1 {
                "形状"
            } else if i == 0 {
                "形状 A"
            } else {
                "形状 B"
            },
        ));
    }
    for (i, spec) in n.parameters().iter().enumerate() {
        v.push((
            Socket::Number(spec.key),
            KPoint::new(
                n.position[0] as f64,
                (n.position[1] + 54. + (n.inputs.len() + i) as f32 * 24.) as f64,
            ),
            spec.label,
        ));
    }
    v
}
fn connect(g: &mut Graph, from: u64, to: u64, socket: Socket) -> Result<(), String> {
    match socket {
        Socket::Shape(port) => g.connect(from, to, port),
        Socket::Number(p) => g.connect_parameter(from, to, p),
    }
}
fn disconnect(g: &mut Graph, to: u64, socket: Socket) {
    if let Some(n) = g.node_mut(to) {
        match socket {
            Socket::Shape(p) => n.inputs[p] = None,
            Socket::Number(p) => n.bindings.retain(|(k, _)| *k != p),
        }
    }
}
pub fn lazy_connect(g: &mut Graph, from: u64, to: u64) -> Result<(), String> {
    let numeric = g.node(from).ok_or("接続元がありません")?.kind.is_number();
    let n = g.node(to).ok_or("接続先がありません")?;
    if numeric {
        let specs = n.parameters();
        let spec = specs
            .iter()
            .find(|s| !n.bindings.iter().any(|(p, _)| *p == s.key))
            .or(specs.first())
            .ok_or("数値の入力がありません")?;
        g.connect_parameter(from, to, spec.key)
    } else {
        let port = n.inputs.iter().position(Option::is_none).unwrap_or(0);
        g.connect(from, to, port)
    }
}
fn duplicate_selection(g: &mut Graph, ids: &[u64]) -> Result<Vec<u64>, String> {
    let originals: Vec<_> = ids
        .iter()
        .filter_map(|id| g.node(*id))
        .filter(|n| !matches!(n.kind, Kind::Input | Kind::Output))
        .cloned()
        .collect();
    let mut copy_graph = g.clone();
    let mut map = std::collections::HashMap::new();
    for n in &originals {
        let id = copy_graph.add(n.kind, n.position)?;
        map.insert(n.id, id);
    }
    for mut copy in originals {
        copy.id = map[&copy.id];
        for source in copy.inputs.iter_mut().flatten() {
            *source = *map.get(source).unwrap_or(source);
        }
        for (_, source) in &mut copy.bindings {
            *source = *map.get(source).unwrap_or(source);
        }
        let id = copy.id;
        *copy_graph.node_mut(id).unwrap() = copy;
    }
    *g = copy_graph;
    Ok(ids.iter().filter_map(|id| map.get(id).copied()).collect())
}
impl Workspace {
    pub fn fit_nodes(&mut self) {
        let b = self.node_bounds.get();
        if let Some(view) = self
            .session
            .graph()
            .and_then(|g| fitted_view(g, f32::from(b.size.width), f32::from(b.size.height)))
        {
            self.node_view = view;
            self.node_view_needs_fit = false;
        }
    }
    fn node_at(&self, p: KPoint) -> Option<u64> {
        self.session
            .graph()?
            .nodes
            .iter()
            .rev()
            .find(|n| {
                Rect::new(
                    n.position[0] as f64,
                    n.position[1] as f64,
                    (n.position[0] + WIDTH) as f64,
                    (n.position[1] + height(n)) as f64,
                )
                .contains(p)
            })
            .map(|n| n.id)
    }
    fn nearest_node(&self, p: KPoint, allow_output: bool) -> Option<u64> {
        self.node_at(p)
            .filter(|id| {
                allow_output
                    || self.session.graph().unwrap().node(*id).unwrap().kind != Kind::Output
            })
            .or_else(|| {
                self.session
                    .graph()?
                    .nodes
                    .iter()
                    .filter(|n| allow_output || n.kind != Kind::Output)
                    .min_by(|a, b| {
                        KPoint::new(a.position[0] as f64 + 90., a.position[1] as f64 + 40.)
                            .distance(p)
                            .total_cmp(
                                &KPoint::new(
                                    b.position[0] as f64 + 90.,
                                    b.position[1] as f64 + 40.,
                                )
                                .distance(p),
                            )
                    })
                    .map(|n| n.id)
            })
    }
    fn socket_at(&self, p: KPoint) -> Option<(u64, Socket)> {
        match node_hit(self.session.graph()?, p, self.node_view.zoom) {
            Some(NodeHit::Input(id, socket)) => Some((id, socket)),
            _ => None,
        }
    }
    fn output_at(&self, p: KPoint) -> Option<u64> {
        match node_hit(self.session.graph()?, p, self.node_view.zoom) {
            Some(NodeHit::Output(id)) => Some(id),
            _ => None,
        }
    }
    pub fn node_down(&mut self, e: &MouseDownEvent, w: &mut Window, cx: &mut Context<Self>) {
        self.leave_tweeq_for_canvas();
        if self.editing.is_some() {
            self.commit_edit(w, cx);
            if self.editing.is_some() {
                return;
            }
        }
        if self.handle_drag_down(e, cx) {
            return;
        }
        self.focus.focus(w);
        self.node_region = true;
        self.active_nodes = true;
        let p = self.node_view.world(e.position, self.node_bounds.get());
        if e.button == MouseButton::Middle {
            self.drag = Drag::Pan {
                nodes: true,
                last: e.position,
            };
            return;
        }
        if e.button == MouseButton::Right {
            if e.modifiers.alt {
                if let Some(from) = self.nearest_node(p, false) {
                    self.session.begin();
                    self.drag = Drag::Connect {
                        from,
                        at: e.position,
                        lazy: true,
                    };
                }
            } else if e.modifiers.control || e.modifiers.platform {
                self.session.begin();
                self.drag = Drag::Cut {
                    start: e.position,
                    current: e.position,
                };
            } else if let Some((id, socket)) = self.socket_at(p) {
                self.session.edit(|s| {
                    if let Some(g) = s.graph_mut() {
                        disconnect(g, id, socket);
                    }
                });
                self.refresh(cx);
            }
            return;
        }
        if e.button != MouseButton::Left {
            return;
        }
        if matches!(self.drag, Drag::Nodes { .. }) {
            self.session.commit();
            self.drag = Drag::None;
            return;
        }
        if let Some(from) = self.output_at(p) {
            self.session.begin();
            self.drag = Drag::Connect {
                from,
                at: e.position,
                lazy: false,
            };
            cx.notify();
            return;
        }
        if let Some((id, socket)) = self.socket_at(p) {
            let source = self
                .session
                .graph()
                .and_then(|g| g.node(id))
                .and_then(|n| match socket {
                    Socket::Shape(i) => n.inputs[i],
                    Socket::Number(p) => n
                        .bindings
                        .iter()
                        .find(|(key, _)| *key == p)
                        .map(|(_, id)| *id),
                });
            if let Some(from) = source {
                self.session.begin();
                disconnect(self.session.graph_mut().unwrap(), id, socket);
                self.drag = Drag::Connect {
                    from,
                    at: e.position,
                    lazy: false,
                };
                self.refresh(cx);
            } else {
                self.session.begin();
                self.drag = Drag::ConnectInput {
                    to: id,
                    socket,
                    at: e.position,
                };
                cx.notify();
            }
            return;
        }
        if let Some(id) = self.node_at(p) {
            if e.modifiers.shift && (e.modifiers.control || e.modifiers.platform) {
                self.session.begin();
                let result = self.session.graph_mut().map(|g| {
                    let out = g.output;
                    g.connect(id, out, 0)
                });
                if let Some(Err(e)) = result {
                    self.status = e;
                }
                self.session.commit();
                self.refresh(cx);
                return;
            }
            if !select_node(&mut self.session.node_selection, id, e.modifiers.shift) {
                cx.notify();
                return;
            }
            if e.click_count == 2 {
                let n = self.session.graph().unwrap().node(id).unwrap();
                let label = n.label.clone();
                self.start_node_edit(Editing::NodeName, id, label, w, cx);
                return;
            }
            self.session.begin();
            self.drag = Drag::Nodes { last: p };
        } else {
            if !e.modifiers.shift {
                self.session.node_selection.clear();
            }
            self.drag = Drag::NodeBox {
                start: p,
                current: p,
                add: e.modifiers.shift,
            };
        }
        cx.notify();
    }
    pub fn node_drag(&mut self, e: &MouseMoveEvent) {
        let p = self.node_view.world(e.position, self.node_bounds.get());
        match self.drag.clone() {
            Drag::Nodes { last } => {
                let selected = self.session.node_selection.clone();
                if let Some(g) = self.session.graph_mut() {
                    for n in &mut g.nodes {
                        if selected.contains(&n.id) {
                            n.position[0] += (p.x - last.x) as f32;
                            n.position[1] += (p.y - last.y) as f32;
                        }
                    }
                }
                self.drag = Drag::Nodes { last: p };
            }
            Drag::NodeBox { start, add, .. } => {
                self.drag = Drag::NodeBox {
                    start,
                    current: p,
                    add,
                }
            }
            Drag::Connect { from, lazy, .. } => {
                self.drag = Drag::Connect {
                    from,
                    at: e.position,
                    lazy,
                }
            }
            Drag::ConnectInput { to, socket, .. } => {
                self.drag = Drag::ConnectInput {
                    to,
                    socket,
                    at: e.position,
                };
            }
            Drag::Cut { start, .. } => {
                self.drag = Drag::Cut {
                    start,
                    current: e.position,
                }
            }
            _ => {}
        }
    }
    pub fn node_up(&mut self, e: &MouseUpEvent) {
        let p = self.node_view.world(e.position, self.node_bounds.get());
        match self.drag.clone() {
            Drag::Connect { from, lazy, .. } => {
                if !self.node_bounds.get().contains(&e.position) {
                    self.session.cancel();
                    return;
                }
                let target = if lazy {
                    self.nearest_node(p, true).map(|id| (id, None))
                } else {
                    self.socket_at(p).map(|(id, s)| (id, Some(s)))
                };
                if let Some((id, socket)) = target {
                    if let Some(g) = self.session.graph_mut() {
                        let result = if let Some(s) = socket {
                            connect(g, from, id, s)
                        } else {
                            lazy_connect(g, from, id)
                        };
                        if let Err(e) = result {
                            self.session.cancel();
                            self.status = e;
                        }
                    }
                } else {
                    self.session.cancel();
                }
            }
            Drag::ConnectInput { to, socket, .. } => {
                let source = self
                    .node_bounds
                    .get()
                    .contains(&e.position)
                    .then(|| self.output_at(p))
                    .flatten();
                if let Some(from) = source {
                    if let Some(g) = self.session.graph_mut()
                        && let Err(error) = connect(g, from, to, socket)
                    {
                        self.session.cancel();
                        self.status = error;
                    }
                } else {
                    self.session.cancel();
                }
            }
            Drag::NodeBox {
                start,
                current,
                add,
            } => {
                if !add {
                    self.session.node_selection.clear();
                }
                if let Some(g) = self.session.graph() {
                    let r = Rect::from_points(start, current);
                    let ids: Vec<_> = g
                        .nodes
                        .iter()
                        .filter(|n| {
                            r.intersect(Rect::new(
                                n.position[0] as f64,
                                n.position[1] as f64,
                                (n.position[0] + WIDTH) as f64,
                                (n.position[1] + height(n)) as f64,
                            ))
                            .area()
                                > 0.
                        })
                        .map(|n| n.id)
                        .collect();
                    for id in ids {
                        if !self.session.node_selection.contains(&id) {
                            self.session.node_selection.push(id);
                        }
                    }
                }
            }
            Drag::Cut { start, current } => {
                let view = self.node_view;
                let b = self.node_bounds.get();
                if let Some(g) = self.session.graph() {
                    let mut cuts = vec![];
                    for n in &g.nodes {
                        for (socket, pos, _) in sockets(n) {
                            let source = match socket {
                                Socket::Shape(i) => n.inputs[i],
                                Socket::Number(p) => {
                                    n.bindings.iter().find(|(k, _)| *k == p).map(|(_, id)| *id)
                                }
                            };
                            if let Some(src) = source.and_then(|id| g.node(id)) {
                                let a = view.screen(output(src), b);
                                let z = view.screen(pos, b);
                                if wire_crosses(a, z, start, current) {
                                    cuts.push((n.id, socket));
                                }
                            }
                        }
                    }
                    if let Some(g) = self.session.graph_mut() {
                        for (id, s) in cuts {
                            disconnect(g, id, s);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    pub fn node_key(&mut self, e: &KeyDownEvent, w: &mut Window, cx: &mut Context<Self>) {
        let k = &e.keystroke;
        match k.key.as_str() {
            "a" if k.modifiers.shift => {
                self.open_node_add(w, cx);
            }
            "n" => self.inspector = !self.inspector,
            "home" | "f" => self.fit_nodes(),
            "g" => {
                self.node_move_modal = true;
                self.session.begin();
                self.drag = Drag::Nodes {
                    last: self.node_view.world(self.cursor, self.node_bounds.get()),
                };
            }
            "b" => {
                let p = self.node_view.world(self.cursor, self.node_bounds.get());
                self.drag = Drag::NodeBox {
                    start: p,
                    current: p,
                    add: k.modifiers.shift,
                };
            }
            "d" if k.modifiers.shift => self.duplicate_nodes(cx),
            "m" => self.toggle_nodes_bypass(),
            "x" | "backspace" | "delete" => self.delete_nodes(),
            "f2" => {
                if let Some(n) = self
                    .session
                    .node_selection
                    .last()
                    .and_then(|id| self.session.graph()?.node(*id))
                {
                    self.start_node_edit(Editing::NodeName, n.id, n.label.clone(), w, cx);
                }
            }
            _ => {}
        }
        self.refresh(cx);
    }
    pub fn toggle_nodes_bypass(&mut self) {
        let ids = self.session.node_selection.clone();
        self.session.edit(|s| {
            if let Some(g) = s.graph_mut() {
                for n in &mut g.nodes {
                    if ids.contains(&n.id) && !matches!(n.kind, Kind::Input | Kind::Output) {
                        n.bypass = !n.bypass;
                    }
                }
            }
        });
    }
    pub fn delete_nodes(&mut self) {
        let ids = self.session.node_selection.clone();
        self.session.edit(|s| {
            if let Some(g) = s.graph_mut() {
                for id in ids {
                    g.remove(id);
                }
            }
        });
        self.session.node_selection.clear();
    }
    pub fn duplicate_nodes(&mut self, cx: &mut Context<Self>) {
        let ids = self.session.node_selection.clone();
        self.session.begin();
        let result = self
            .session
            .graph_mut()
            .map(|g| duplicate_selection(g, &ids));
        match result {
            Some(Ok(new_ids)) if !new_ids.is_empty() => {
                self.session.node_selection = new_ids;
                self.node_move_modal = true;
                self.drag = Drag::Nodes {
                    last: self.node_view.world(self.cursor, self.node_bounds.get()),
                };
            }
            Some(Err(error)) => {
                self.session.cancel();
                self.status = error;
            }
            _ => self.session.cancel(),
        }
        self.refresh(cx);
    }
    fn add_node(&mut self, kind: Kind, w: &mut Window, cx: &mut Context<Self>) {
        let at = self.node_view.world(self.cursor, self.node_bounds.get());
        self.session.begin();
        let result = self
            .session
            .graph_mut()
            .map(|g| add_unconnected(g, kind, at));
        match result {
            Some(Ok(id)) => {
                self.session.node_selection = vec![id];
                self.node_move_modal = true;
                self.drag = Drag::Nodes { last: at };
                self.status = "クリック／Enter：配置　Esc／右クリック：取消".into();
            }
            Some(Err(error)) => {
                self.session.cancel();
                self.status = error;
            }
            _ => self.session.cancel(),
        }
        self.close_node_add(w, cx);
        self.refresh(cx);
    }
    pub fn open_node_add(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        if self.session.graph().is_none() {
            self.status = "パスを選択し、ノードを有効にしてください".into();
            cx.notify();
            return;
        }
        if self.editing.is_some() {
            self.commit_edit(w, cx);
            if self.editing.is_some() {
                return;
            }
        }
        self.finish_interaction(true, cx);
        self.nodes_open = true;
        self.add_open = true;
        self.active_nodes = true;
        self.add_anchor = self.cursor;
        self.add_index = 0;
        self.search.clear();
        self.add_input
            .update(cx, |i, cx| i.set(String::new(), w, cx));
        cx.defer_in(w, |s, w, cx| {
            if s.add_open {
                s.add_input.focus_handle(cx).focus(w);
            }
        });
        cx.notify();
    }
    fn close_node_add(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        self.add_open = false;
        self.focus.focus(w);
        cx.notify();
    }
    pub fn node_add_key(
        &mut self,
        e: &KeyDownEvent,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.add_open {
            return false;
        }
        let k = &e.keystroke;
        let results = matching_kinds(&self.search);
        if self.add_input.read(cx).is_composing() {
            return true;
        }
        match k.key.as_str() {
            "escape" => self.close_node_add(w, cx),
            "down" if !results.is_empty() => self.add_index = (self.add_index + 1) % results.len(),
            "up" if !results.is_empty() => {
                self.add_index = (self.add_index + results.len() - 1) % results.len()
            }
            "enter" => {
                if let Some(kind) = results.get(self.add_index) {
                    self.add_node(*kind, w, cx);
                }
            }
            _ => {}
        }
        cx.notify();
        true
    }
    fn preset(&self) -> Result<Preset, String> {
        Preset::new(
            &self.set_name,
            &self.category,
            self.session
                .graph()
                .ok_or("パスにノードを追加してください")?,
        )
    }
    fn register_set(&mut self, cx: &mut Context<Self>) {
        match self.preset() {
            Ok(p) => {
                self.session.begin();
                match appearance_assets::register(&mut self.session.doc.appearance_presets, p) {
                    Ok(()) => self.status = "名前付きセットを文書に登録しました".into(),
                    Err(e) => self.status = e,
                }
                self.session.commit();
            }
            Err(e) => self.status = e,
        }
        cx.notify();
    }
    fn export_set(&mut self, cx: &mut Context<Self>) {
        match self.preset().and_then(|p| p.encode()) {
            Ok(text) => self.export_text("NodeSet.pathweaver-nodes", "pathweaver-nodes", text, cx),
            Err(e) => self.status = e,
        }
    }
    fn import_set(&mut self, cx: &mut Context<Self>) {
        let answer = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("ノードセットを読み込む".into()),
        });
        cx.spawn(async move |this, cx| {
            let result = answer.await;
            let _ = this.update(cx, move |s, cx| {
                match result {
                    Ok(Ok(Some(paths))) => {
                        if let Some(path) = paths.first() {
                            let result = (|| {
                                if std::fs::metadata(path).map_err(|e| e.to_string())?.len()
                                    > 2 * 1024 * 1024
                                {
                                    return Err("ノードセットは2MBまでです".into());
                                }
                                Preset::decode(&std::fs::read(path).map_err(|e| e.to_string())?)
                            })();
                            match result {
                                Ok(p) => {
                                    s.session.begin();
                                    s.status = match appearance_assets::register(
                                        &mut s.session.doc.appearance_presets,
                                        p,
                                    ) {
                                        Ok(()) => {
                                            "セットを読み込みました。選択して置き換えを実行できます"
                                                .into()
                                        }
                                        Err(e) => e,
                                    };
                                    s.session.commit();
                                }
                                Err(e) => s.status = e,
                            }
                        }
                    }
                    Ok(Err(e)) => s.status = e.to_string(),
                    _ => {}
                }
                cx.notify();
            });
        })
        .detach();
    }
    pub fn paint_nodes(&self, b: Bounds<Pixels>, w: &mut Window, cx: &mut App) {
        w.with_content_mask(Some(ContentMask { bounds: b }), |w| {
            rect(w, b, ui_color(Role::NodeCanvas), 0, 0., 0.);
            let step = (24. * self.node_view.zoom).max(12.);
            let offsetx = self.node_view.pan[0].rem_euclid(step);
            let offsety = self.node_view.pan[1].rem_euclid(step);
            for x in 0..(f32::from(b.size.width) / step) as i32 + 1 {
                for y in 0..(f32::from(b.size.height) / step) as i32 + 1 {
                    dot(
                        w,
                        b.origin
                            + point(px(offsetx + x as f32 * step), px(offsety + y as f32 * step)),
                        0.7,
                        ui_color(Role::Border),
                    );
                }
            }
            let Some(g) = self.session.graph() else {
                return;
            };
            for n in &g.nodes {
                for (socket, pos, _) in sockets(n) {
                    let source = match socket {
                        Socket::Shape(i) => n.inputs[i],
                        Socket::Number(p) => n
                            .bindings
                            .iter()
                            .find(|(key, _)| *key == p)
                            .map(|(_, id)| *id),
                    };
                    if let Some(src) = source.and_then(|id| g.node(id)) {
                        wire(
                            w,
                            self.node_view.screen(output(src), b),
                            self.node_view.screen(pos, b),
                            if src.kind.is_number() {
                                ui_color(Role::NumberSocket)
                            } else {
                                ui_color(Role::ShapeSocket)
                            },
                            2.,
                        );
                    }
                }
            }
            let z = self.node_view.zoom;
            for n in &g.nodes {
                let origin = self
                    .node_view
                    .screen(KPoint::new(n.position[0] as f64, n.position[1] as f64), b);
                let bounds = Bounds::new(origin, size(px(WIDTH * z), px(height(n) * z)));
                rect(
                    w,
                    bounds,
                    if n.bypass {
                        ui_color(Role::NodeMuted)
                    } else {
                        ui_color(Role::Node)
                    },
                    if self.session.node_selection.contains(&n.id) {
                        ui_color(Role::Accent)
                    } else {
                        ui_color(Role::Border)
                    },
                    if self.session.node_selection.contains(&n.id) {
                        2.
                    } else {
                        1.
                    },
                    4.,
                );
                rect(
                    w,
                    Bounds::new(origin, size(px(WIDTH * z), px(27. * z))),
                    color(n.kind),
                    0,
                    0.,
                    3.,
                );
                // Values remain visible through typical Fit zooms. Every row scales
                // and clips with its node, including at tiny overview zoom levels.
                w.with_content_mask(Some(ContentMask { bounds }), |w| {
                    let title = if n.label.is_empty() {
                        n.kind.label().to_string()
                    } else {
                        n.label.clone()
                    };
                    node_text(
                        w,
                        cx,
                        origin + point(px(9. * z), px(3. * z)),
                        title,
                        ui_color(Role::Text),
                        12.,
                        z,
                    );
                    if !n.label.is_empty() {
                        node_text(
                            w,
                            cx,
                            origin + point(px(9. * z), px(28. * z)),
                            n.kind.label(),
                            ui_color(Role::Muted),
                            9.,
                            z,
                        );
                    }
                    if n.bypass {
                        node_text(
                            w,
                            cx,
                            origin + point(px(130. * z), px(28. * z)),
                            "無効",
                            ui_color(Role::Muted),
                            10.,
                            z,
                        );
                    }
                });
                for (socket, pos, label) in sockets(n) {
                    let at = self.node_view.screen(pos, b);
                    let numeric = matches!(socket, Socket::Number(_));
                    dot(
                        w,
                        at,
                        4. * z.max(0.6),
                        if numeric {
                            ui_color(Role::NumberSocket)
                        } else {
                            ui_color(Role::ShapeSocket)
                        },
                    );
                    let row = Bounds::new(
                        at + point(px(8. * z), px(-10. * z)),
                        size(px((WIDTH - 16.) * z), px(20. * z)),
                    );
                    let effective = match socket {
                        Socket::Number(p) => self.node_parameter_values.get(&(n.id, p)).copied(),
                        Socket::Shape(_) => None,
                    };
                    w.with_content_mask(Some(ContentMask { bounds: row }), |w| {
                        node_text(
                            w,
                            cx,
                            at + point(px(10. * z), px(-8. * z)),
                            socket_label(n, socket, label, effective),
                            ui_color(Role::Text),
                            11.,
                            z,
                        );
                    });
                }
                if n.kind != Kind::Output {
                    dot(
                        w,
                        self.node_view.screen(output(n), b),
                        4. * z.max(0.6),
                        if n.kind.is_number() {
                            ui_color(Role::NumberSocket)
                        } else {
                            ui_color(Role::ShapeSocket)
                        },
                    );
                }
            }
            match self.drag {
                Drag::Connect { from, at, .. } => {
                    if let Some(n) = g.node(from) {
                        wire(
                            w,
                            self.node_view.screen(output(n), b),
                            at,
                            ui_color(Role::Accent),
                            2.,
                        );
                    }
                }
                Drag::ConnectInput { to, socket, at } => {
                    if let Some((_, pos, _)) = g
                        .node(to)
                        .and_then(|n| sockets(n).into_iter().find(|(s, _, _)| *s == socket))
                    {
                        wire(
                            w,
                            at,
                            self.node_view.screen(pos, b),
                            ui_color(Role::Accent),
                            2.,
                        );
                    }
                }
                Drag::NodeBox { start, current, .. } => w.paint_quad(quad(
                    normal_bounds(
                        self.node_view.screen(start, b),
                        self.node_view.screen(current, b),
                    ),
                    px(0.),
                    rgba(0x78a8db18),
                    px(1.),
                    rgb(ui_color(Role::Accent)),
                    Default::default(),
                )),
                Drag::Cut { start, current } => {
                    line(w, &[start, current], ui_color(Role::Error), 2.)
                }
                _ => {}
            }
        });
    }
    fn node_inspector(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.tweeq_panels.node.is_some() {
            return self.tweeq_panel("node", cx);
        }
        let mut col = div()
            .id("node-inspector")
            .w(px(280.))
            .flex_shrink_0()
            .h_full()
            .overflow_y_scroll()
            .p_3()
            .flex()
            .flex_col()
            .gap_2()
            .bg(rgb(ui_color(Role::Surface)));
        if let Some(n) = self
            .session
            .node_selection
            .last()
            .and_then(|id| self.session.graph()?.node(*id))
        {
            let id = n.id;
            let label = n.label.clone();
            let mut header = div().flex().items_center().gap_1().child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(rgb(ui_color(Role::Muted)))
                    .child(n.kind.label()),
            );
            if !matches!(n.kind, Kind::Input | Kind::Output) {
                header = header
                    .child(self.icon_button(
                        ("node-bypass", id),
                        "power",
                        if n.bypass {
                            "このノードを有効にする（M）"
                        } else {
                            "このノードを無効にする（M）"
                        },
                        !n.bypass,
                        cx,
                        move |s, _, cx| {
                            s.session.edit(|s| {
                                if let Some(n) = s.graph_mut().and_then(|g| g.node_mut(id)) {
                                    n.bypass = !n.bypass;
                                }
                            });
                            s.refresh(cx);
                        },
                    ))
                    .child(self.icon_button(
                        "inspector-duplicate-nodes",
                        "copy",
                        "選択ノードを複製（Shift+D）",
                        false,
                        cx,
                        |s, _, cx| s.duplicate_nodes(cx),
                    ))
                    .child(self.icon_button(
                        "inspector-delete-nodes",
                        "trash",
                        "選択ノードを削除（Delete）",
                        false,
                        cx,
                        |s, _, cx| {
                            s.delete_nodes();
                            s.refresh(cx);
                        },
                    ));
            }
            col = col.child(header).child(self.button_with_id(
                ("node-name", id),
                if label.is_empty() {
                    "名前を付ける".into()
                } else {
                    label.clone()
                },
                false,
                cx,
                move |s, w, cx| s.start_node_edit(Editing::NodeName, id, label.clone(), w, cx),
            ));
            for spec in n.parameters() {
                let p = spec.key;
                let bound = n.bindings.iter().any(|(k, _)| *k == p);
                if bound {
                    let effective = self.node_parameter_values.get(&(id, p)).copied();
                    let value = effective
                        .map(format_node_number)
                        .unwrap_or_else(|| "—".into());
                    col = col.child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .child(div().flex_1().min_w_0().truncate().child(spec.label))
                            .child(
                                div()
                                    .text_color(rgb(ui_color(Role::NumberSocket)))
                                    .child(format!("↗ {value}")),
                            )
                            .child(
                                div()
                                    .text_size(px(10.))
                                    .text_color(rgb(ui_color(Role::Muted)))
                                    .child(if effective.is_some() {
                                        "接続値"
                                    } else {
                                        "計算不可"
                                    }),
                            ),
                    );
                } else {
                    col = col.child(self.numeric_field(spec.label, Editing::Parameter(p), cx));
                }
            }
            if matches!(n.kind, Kind::Fill | Kind::Shadow | Kind::Glow) {
                col = col.child(self.color_field(
                    crate::color_picker::ColorSlot::Node(id),
                    Some(n.color),
                    cx,
                ));
            }

            if matches!(n.kind, Kind::Rough | Kind::Zigzag) {
                col = col.child(self.button(
                    if n.smooth {
                        "滑らか：オン"
                    } else {
                        "滑らか：オフ"
                    },
                    n.smooth,
                    cx,
                    move |s, _, cx| {
                        s.session.edit(|s| {
                            if let Some(n) = s.graph_mut().and_then(|g| g.node_mut(id)) {
                                n.smooth = !n.smooth;
                            }
                        });
                        s.refresh(cx);
                    },
                ));
            }
            if n.kind == Kind::Glow {
                col = col.child(self.button(
                    if n.inner {
                        "光彩：内側"
                    } else {
                        "光彩：外側"
                    },
                    false,
                    cx,
                    move |s, _, cx| {
                        s.session.edit(|s| {
                            if let Some(n) = s.graph_mut().and_then(|g| g.node_mut(id)) {
                                n.inner = !n.inner;
                            }
                        });
                        s.refresh(cx);
                    },
                ));
            }
            if n.kind == Kind::Measure {
                col = col.child(self.button(
                    if n.measure_height {
                        "計測：高さ"
                    } else {
                        "計測：幅"
                    },
                    false,
                    cx,
                    move |s, _, cx| {
                        s.session.edit(|s| {
                            if let Some(n) = s.graph_mut().and_then(|g| g.node_mut(id)) {
                                n.measure_height = !n.measure_height;
                            }
                        });
                        s.refresh(cx);
                    },
                ));
            }
            if n.kind == Kind::Boolean {
                for op in [
                    Operation::Union,
                    Operation::Difference,
                    Operation::Intersect,
                    Operation::Xor,
                ] {
                    col = col.child(self.button(
                        op.label(),
                        n.operation == op,
                        cx,
                        move |s, _, cx| {
                            s.session.edit(|s| {
                                if let Some(n) = s.graph_mut().and_then(|g| g.node_mut(id)) {
                                    n.operation = op;
                                }
                            });
                            s.refresh(cx);
                        },
                    ));
                }
            }
            if n.kind == Kind::Math {
                for op in [
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
                    col = col.child(self.button(op.label(), n.math == op, cx, move |s, _, cx| {
                        s.session.edit(|s| {
                            if let Some(n) = s.graph_mut().and_then(|g| g.node_mut(id)) {
                                n.math = op;
                            }
                        });
                        s.refresh(cx);
                    }));
                }
            }
            if matches!(n.kind, Kind::Offset | Kind::Round | Kind::Band) {
                use pathweaver_core::appearance_effects::{BandPosition, OffsetJoin};
                for (join, label) in [
                    (OffsetJoin::Round, "丸い角"),
                    (OffsetJoin::Miter, "鋭い角"),
                    (OffsetJoin::Bevel, "面取り"),
                ] {
                    col = col.child(self.button(
                        label,
                        n.effect.join == join,
                        cx,
                        move |s, _, cx| {
                            s.session.edit(|s| {
                                if let Some(n) = s.graph_mut().and_then(|g| g.node_mut(id)) {
                                    n.effect.join = join;
                                }
                            });
                            s.refresh(cx);
                        },
                    ));
                }
                if n.kind == Kind::Band {
                    for (position, label) in [
                        (BandPosition::Center, "中央"),
                        (BandPosition::Inner, "内側"),
                        (BandPosition::Outer, "外側"),
                    ] {
                        col = col.child(self.button(
                            label,
                            n.effect.position == position,
                            cx,
                            move |s, _, cx| {
                                s.session.edit(|s| {
                                    if let Some(n) = s.graph_mut().and_then(|g| g.node_mut(id)) {
                                        n.effect.position = position;
                                    }
                                });
                                s.refresh(cx);
                            },
                        ));
                    }
                }
            }
        } else {
            col = col.child(
                div()
                    .text_color(rgb(ui_color(Role::Muted)))
                    .child("ノードを選択して編集"),
            );
        }
        col.into_any_element()
    }
    pub fn node_add_popup(&self, w: &Window, cx: &mut Context<Self>) -> AnyElement {
        let matches = matching_kinds(&self.search);
        let visible = matches.len().clamp(1, 8);
        let popup_size = size(px(270.), px(90. + visible as f32 * 28.));
        let origin = popup_origin(self.add_anchor, popup_size, w.viewport_size());
        let mut list = div()
            .h(px(visible as f32 * 28.))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap_0();
        if matches.is_empty() {
            list = list.child(
                div()
                    .h(px(28.))
                    .px_2()
                    .text_color(rgb(ui_color(Role::Muted)))
                    .child("一致するノードがありません"),
            );
        }
        let start = self.add_index.saturating_sub(7);
        for (index, kind) in matches.iter().copied().enumerate().skip(start).take(8) {
            list = list.child(
                div()
                    .id(("node-add-result", index))
                    .h(px(28.))
                    .px_2()
                    .flex()
                    .items_center()
                    .rounded(px(3.))
                    .cursor_pointer()
                    .bg(rgb(ui_color(if index == self.add_index {
                        Role::Selected
                    } else {
                        Role::Surface
                    })))
                    .text_color(rgb(ui_color(if index == self.add_index {
                        Role::OnAccent
                    } else {
                        Role::Text
                    })))
                    .hover(|style| {
                        style
                            .bg(rgb(ui_color(Role::Hover)))
                            .text_color(rgb(ui_color(Role::Text)))
                    })
                    .child(kind.label())
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |s, _, w, cx| {
                            cx.stop_propagation();
                            s.add_node(kind, w, cx);
                        }),
                    ),
            );
        }
        let popup = div()
            .id("node-add-popup")
            .absolute()
            .left(origin.x)
            .top(origin.y)
            .w(popup_size.width)
            .h(popup_size.height)
            .p_2()
            .flex()
            .flex_col()
            .gap_1()
            .rounded(px(6.))
            .border_1()
            .border_color(rgb(ui_color(Role::Border)))
            .bg(rgb(ui_color(Role::Surface)))
            .shadow_lg()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .on_scroll_wheel(cx.listener(|s, e: &ScrollWheelEvent, _, cx| {
                cx.stop_propagation();
                let count = matching_kinds(&s.search).len();
                if count > 0 {
                    let delta = e.delta.pixel_delta(px(28.)).y;
                    if delta < px(0.) {
                        s.add_index = (s.add_index + 1).min(count - 1);
                    }
                    if delta > px(0.) {
                        s.add_index = s.add_index.saturating_sub(1);
                    }
                    cx.notify();
                }
            }))
            .child(
                div()
                    .h(px(16.))
                    .flex_shrink_0()
                    .line_height(px(16.))
                    .text_size(px(11.))
                    .text_color(rgb(ui_color(Role::Muted)))
                    .child("ノードを検索"),
            )
            .child(
                div()
                    .h(px(30.))
                    .flex_shrink_0()
                    .border_1()
                    .rounded(px(3.))
                    .border_color(rgb(ui_color(Role::Accent)))
                    .child(self.add_input.clone()),
            )
            .child(list)
            .child(
                div()
                    .h(px(14.))
                    .flex_shrink_0()
                    .line_height(px(14.))
                    .text_size(px(10.))
                    .text_color(rgb(ui_color(Role::Muted)))
                    .child(format!(
                        "↑↓ 選択　Enter 追加　Esc 取消　{}件",
                        matches.len()
                    )),
            );
        div()
            .absolute()
            .inset_0()
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|s, _, w, cx| {
                    cx.stop_propagation();
                    s.close_node_add(w, cx);
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|s, _, w, cx| {
                    cx.stop_propagation();
                    s.close_node_add(w, cx);
                }),
            )
            .child(popup)
            .into_any_element()
    }
    fn library_sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut col = div()
            .id("node-library")
            .w(px(260.))
            .flex_shrink_0()
            .h_full()
            .overflow_y_scroll()
            .p_2()
            .flex()
            .flex_col()
            .gap_1()
            .bg(rgb(ui_color(Role::Surface)))
            .child(self.button(
                format!(
                    "名前：{}",
                    if self.set_name.is_empty() {
                        "未設定"
                    } else {
                        &self.set_name
                    }
                ),
                false,
                cx,
                |s, w, cx| s.start_edit(Editing::SetName, s.set_name.clone(), w, cx),
            ))
            .child(self.button(
                format!(
                    "種類：{}",
                    if self.category.is_empty() {
                        "未設定"
                    } else {
                        &self.category
                    }
                ),
                false,
                cx,
                |s, w, cx| s.start_edit(Editing::Category, s.category.clone(), w, cx),
            ))
            .child(self.button("セットを登録・更新", false, cx, |s, _, cx| {
                s.register_set(cx)
            }))
            .child(self.button("セットを書き出す", false, cx, |s, _, cx| {
                s.export_set(cx)
            }))
            .child(self.button("セットを読み込む", false, cx, |s, _, cx| {
                s.import_set(cx)
            }))
            .child(
                div()
                    .py_2()
                    .text_color(rgb(ui_color(Role::Muted)))
                    .child("保存済みセット"),
            );
        for (i, p) in self.session.doc.appearance_presets.iter().enumerate() {
            let key = (p.category.clone(), p.name.clone());
            let selected = self.library_selection.as_ref() == Some(&key);
            col = col.child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(div().flex_1().min_w_0().child(self.button_with_id(
                        ("preset", i),
                        format!("{} / {}", p.category, p.name),
                        selected,
                        cx,
                        move |s, _, _| s.library_selection = Some(key.clone()),
                    )))
                    .child(self.icon_button(
                        ("preset-delete", i),
                        "trash",
                        "このセットを文書から削除",
                        false,
                        cx,
                        move |s, _, _| {
                            if let Some(preset) = s.session.doc.appearance_presets.get(i) {
                                let key = (preset.category.clone(), preset.name.clone());
                                if s.library_selection.as_ref() == Some(&key) {
                                    s.library_selection = None;
                                }
                                s.session.edit(|s| {
                                    s.doc.appearance_presets.remove(i);
                                });
                            }
                        },
                    )),
            );
        }
        let selected = self
            .library_selection
            .as_ref()
            .and_then(|(category, name)| {
                self.session
                    .doc
                    .appearance_presets
                    .iter()
                    .find(|p| &p.category == category && &p.name == name)
            });
        if let Some(preset) = selected {
            let preset = preset.clone();
            col = col.child(
                div()
                    .mt_2()
                    .pt_2()
                    .border_t_1()
                    .border_color(rgb(ui_color(Role::Border)))
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(format!("{} ノード", preset.graph.nodes.len()))
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(rgb(ui_color(Role::Muted)))
                            .child("選択中パスの効果全体を置き換えます。⌘Zで元に戻せます。"),
                    )
                    .when(self.session.active().is_some(), |d| {
                        d.child(self.button(
                            "ノードを置き換える",
                            false,
                            cx,
                            move |s, _, cx| match s.session.replace_appearance(preset.apply()) {
                                Ok(()) => {
                                    s.set_name = preset.name.clone();
                                    s.category = preset.category.clone();
                                    s.status = "選択パスのノードをセットで置き換えました".into();
                                    s.fit_nodes();
                                    s.refresh(cx);
                                }
                                Err(error) => s.status = error,
                            },
                        ))
                    })
                    .when(self.session.active().is_none(), |d| {
                        d.child(
                            div()
                                .text_color(rgb(ui_color(Role::Muted)))
                                .child("適用先のパスを選択"),
                        )
                    }),
            );
        }
        col.into_any_element()
    }
    pub fn nodes_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let view = cx.entity();
        let bounds = self.node_bounds.clone();
        let node_canvas = div()
            .id("node-canvas")
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_hidden()
            .child(
                canvas(
                    move |b, _, _| {
                        bounds.set(b);
                    },
                    move |b, _, w, cx| {
                        view.update(cx, |s, cx| {
                            // Wait for the reveal animation to reach the usable height.
                            if s.node_view_needs_fit
                                && f32::from(b.size.height) >= s.node_layout_height - 43.
                            {
                                s.node_bounds.set(b);
                                s.fit_nodes();
                                // The toolbar was laid out before this first fit.
                                // Refresh its zoom label on the following frame.
                                cx.on_next_frame(w, |_, _, cx| cx.notify());
                            }
                            s.paint_nodes(b, w, cx)
                        })
                    },
                )
                .size_full(),
            )
            .on_mouse_down(MouseButton::Left, cx.listener(Self::node_down))
            .on_mouse_down(MouseButton::Right, cx.listener(Self::node_down))
            .on_mouse_down(MouseButton::Middle, cx.listener(Self::node_down))
            .on_scroll_wheel(cx.listener(|s, e, _, cx| s.scroll(e, true, cx)));
        let mut bar = div()
            .h(px(36.))
            .px_2()
            .flex()
            .items_center()
            .gap_1()
            .bg(rgb(ui_color(Role::Toolbar)))
            .child(div().flex_shrink_0().px_1().child("ノード"))
            .child(self.icon_button(
                "node-add",
                "plus",
                "ノードを追加（Shift+A）",
                self.add_open,
                cx,
                |s, w, cx| {
                    s.open_node_add(w, cx);
                },
            ))
            .child(self.icon_button(
                "node-library-toggle",
                "library",
                "名前付きセット",
                self.library_open,
                cx,
                |s, _, _| {
                    s.library_open = true;
                    s.inspector = true;
                    s.add_open = false;
                    if let Some(g) = s.session.graph() {
                        s.set_name = g.name.clone();
                    }
                },
            ))
            .child(self.icon_button(
                "node-fit",
                "fit",
                "全ノードを表示（Home）",
                false,
                cx,
                |s, _, _| s.fit_nodes(),
            ))
            .child(self.icon_button(
                "node-inspector-toggle",
                "sliders",
                "ノード設定（N）",
                self.inspector,
                cx,
                |s, _, _| s.inspector = !s.inspector,
            ))
            .child(self.path_context());
        if let Some(g) = self.session.graph() {
            bar = bar.child(self.icon_button(
                "node-graph-enabled",
                "power",
                if g.enabled {
                    "このパスのノード効果を無効にする"
                } else {
                    "このパスのノード効果を有効にする"
                },
                g.enabled,
                cx,
                |s, _, cx| {
                    s.session.edit(|s| {
                        if let Some(g) = s.graph_mut() {
                            g.enabled = !g.enabled;
                        }
                    });
                    s.refresh(cx);
                },
            ));
        }
        bar = bar
            .child(div().flex_1())
            .child(
                div()
                    .id("node-zoom-reset")
                    .tooltip(|_, cx| {
                        cx.new(|_| {
                            crate::app::IconTooltip(
                                "ノードの表示倍率 · クリックで100%に戻す".into(),
                            )
                        })
                        .into()
                    })
                    .child(self.button(
                        format!("{:.0}%", self.node_view.zoom * 100.),
                        false,
                        cx,
                        |s, _, _| {
                            let b = s.node_bounds.get();
                            s.node_view.zoom_at(b.center(), b, 1. / s.node_view.zoom);
                        },
                    )),
            )
            .child(self.icon_button(
                "node-panel-collapse",
                "chevron-down",
                "ノードエディターを折りたたむ",
                false,
                cx,
                |s, _, _| s.nodes_open = false,
            ));
        let mut body = div().flex().flex_1().min_h_0();
        if self.session.graph().is_some() {
            body = body.child(node_canvas);
        } else {
            body = body.child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .justify_center()
                    .items_center()
                    .gap_3()
                    .child(if self.session.active().is_some() {
                        "選択したパスに、効果をノードで追加します。"
                    } else {
                        "上のキャンバスでパスを描くか、選択してください。"
                    })
                    .when(self.session.active().is_some(), |d| {
                        d.child(self.button("ノードを使う", true, cx, |s, _, cx| {
                            s.session.edit(|s| {
                                if let Some(p) = s.active_mut() {
                                    p.appearance = Some(Graph::default());
                                }
                            });
                            s.fit_nodes();
                            s.refresh(cx);
                        }))
                    }),
            );
        }
        if self.inspector {
            let tabs = div()
                .h(px(32.))
                .flex_shrink_0()
                .flex()
                .items_center()
                .px_2()
                .gap_1()
                .border_b_1()
                .border_color(rgb(ui_color(Role::Border)))
                .child(self.button("ノード", !self.library_open, cx, |s, _, _| {
                    s.library_open = false
                }))
                .child(self.button("セット", self.library_open, cx, |s, _, _| {
                    s.library_open = true
                }));
            let content = if self.library_open {
                self.library_sidebar(cx)
            } else {
                self.node_inspector(cx)
            };
            body = body.child(
                div()
                    .w(px(280.))
                    .flex_shrink_0()
                    .flex()
                    .flex_col()
                    .border_l_1()
                    .border_color(rgb(ui_color(Role::Border)))
                    .bg(rgb(ui_color(Role::Surface)))
                    .child(tabs)
                    .child(div().flex_1().min_h_0().overflow_hidden().child(content)),
            );
        }
        div()
            .h(px(self.node_layout_height))
            .max_h(relative(0.6))
            .min_h(px(170.))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .border_t_1()
            .border_color(rgb(ui_color(Role::Border)))
            .child(
                div()
                    .id("node-resize")
                    .h(px(5.))
                    .flex_shrink_0()
                    .cursor(CursorStyle::ResizeUpDown)
                    .bg(rgb(ui_color(Role::Border)))
                    .hover(|s| s.bg(rgb(ui_color(Role::Accent))))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|s, e: &MouseDownEvent, _, cx| {
                            if !s.handle_drag_down(e, cx) {
                                s.drag = Drag::Resize;
                            }
                        }),
                    ),
            )
            .child(bar)
            .child(body)
            .overflow_hidden()
            .with_animation(
                "node-dock-reveal",
                Animation::new(std::time::Duration::from_millis(180)).with_easing(ease_in_out),
                {
                    let h = self.node_layout_height;
                    move |d, t| d.min_h(px(0.)).h(px(h * t))
                },
            )
            .into_any_element()
    }
}
fn wire_crosses(
    a: Point<Pixels>,
    b: Point<Pixels>,
    start: Point<Pixels>,
    end: Point<Pixels>,
) -> bool {
    let p = |p: Point<Pixels>| KPoint::new(f32::from(p.x) as f64, f32::from(p.y) as f64);
    let a = p(a);
    let b = p(b);
    let dx = ((b.x - a.x).abs() * 0.5).max(40.);
    let curve = kurbo::CubicBez::new(
        a,
        a + kurbo::Vec2::new(dx, 0.),
        b - kurbo::Vec2::new(dx, 0.),
        b,
    );
    let start = p(start);
    let end = p(end);
    use kurbo::ParamCurve;
    for i in 0..24 {
        let x = curve.eval(i as f64 / 24.);
        let y = curve.eval((i + 1) as f64 / 24.);
        let v = y - x;
        let u = end - start;
        let det = v.cross(u);
        if det.abs() > 1e-9 {
            let t = (start - x).cross(u) / det;
            let s = (start - x).cross(v) / det;
            if (0. ..=1.).contains(&t) && (0. ..=1.).contains(&s) {
                return true;
            }
        }
    }
    false
}
#[cfg(test)]
mod tests {
    use super::{
        Graph, KPoint, Kind, NodeHit, Parameter, Socket, connect, duplicate_selection, fitted_view,
        lazy_connect, node_hit, node_rect, output, point, px, select_node, socket_label,
        wire_crosses,
    };
    #[test]
    fn numeric_labels_show_effective_values_without_stale_local_fallback() {
        let mut graph = Graph::rough();
        let value = graph.add(Kind::Value, [0., 0.]).unwrap();
        graph.node_mut(value).unwrap().value = 17.5;
        graph
            .connect_parameter(value, 3, Parameter::Amount)
            .unwrap();
        let effective = pathweaver_core::appearance::parameter_values(&graph, &[]).unwrap();
        let node = graph.node(3).unwrap();
        assert_eq!(
            socket_label(
                node,
                Socket::Number(Parameter::Amount),
                "大きさ",
                effective.get(&(3, Parameter::Amount)).copied()
            ),
            "大きさ  ↗ 17.5"
        );
        assert_eq!(
            socket_label(node, Socket::Number(Parameter::Amount), "大きさ", None),
            "大きさ  ↗ —"
        );
        assert_eq!(
            socket_label(node, Socket::Number(Parameter::Detail), "細かさ", None),
            format!("細かさ  {}", super::format_node_number(node.detail))
        );
    }
    #[test]
    fn compact_numeric_format_keeps_small_values_and_seed_integers() {
        assert_eq!(super::format_node_number(17.), "17");
        assert_eq!(super::format_node_number(17.125), "17.125");
        assert_eq!(super::format_node_number(-0.), "0");
        assert_eq!(super::format_node_number(0.000_012), "1.200e-5");
        assert_eq!(super::format_node_number(u32::MAX as f64), "4294967295");
        assert_eq!(super::format_node_number(f64::NAN), "—");
    }
    #[test]
    fn add_search_matches_japanese_english_and_reports_no_results() {
        assert_eq!(super::matching_kinds(" rough "), vec![Kind::Rough]);
        assert_eq!(super::matching_kinds("ラフ"), vec![Kind::Rough]);
        assert_eq!(super::matching_kinds("VaLuE"), vec![Kind::Value]);
        assert!(super::matching_kinds("zz-no-such-node").is_empty());
        assert_eq!(super::matching_kinds("").len(), super::KINDS.len());
    }
    #[test]
    fn add_popup_stays_inside_viewport_at_every_corner() {
        let viewport = gpui::size(px(980.), px(640.));
        let popup = gpui::size(px(270.), px(90. + 8. * 28.));
        for anchor in [
            point(px(-10.), px(-20.)),
            point(px(979.), px(639.)),
            point(px(300.), px(200.)),
        ] {
            let p = super::popup_origin(anchor, popup, viewport);
            assert!(p.x >= px(8.) && p.y >= px(8.));
            assert!(p.x + popup.width <= viewport.width - px(8.));
            assert!(p.y + popup.height <= viewport.height - px(8.));
        }
        assert_eq!(
            super::popup_origin(point(px(300.), px(200.)), popup, viewport),
            point(px(300.), px(200.))
        );
    }
    #[test]
    fn duplicate_placement_history_restores_original_and_copied_selection_order() {
        let mut session = crate::session::Session::default();
        session.new_path(KPoint::ZERO);
        session.active_mut().unwrap().appearance = Some(Graph::rough());
        session.reconcile();
        let extra = session
            .graph_mut()
            .unwrap()
            .add(Kind::Value, [250., 200.])
            .unwrap();
        let original_ids = vec![extra, 3];
        session.node_selection = original_ids.clone();
        let original = session.doc.clone();
        session.begin();
        let copies = duplicate_selection(session.graph_mut().unwrap(), &original_ids).unwrap();
        session.node_selection = copies;
        session.cancel();
        assert_eq!(session.doc, original);
        assert_eq!(session.node_selection, original_ids);
        session.begin();
        let copies = duplicate_selection(session.graph_mut().unwrap(), &original_ids).unwrap();
        session.node_selection = copies.clone();
        session.commit();
        session.undo();
        assert_eq!(session.node_selection, original_ids);
        session.redo();
        assert_eq!(session.node_selection, copies);
        assert_eq!(
            session
                .graph()
                .unwrap()
                .node(*copies.last().unwrap())
                .unwrap()
                .kind,
            Kind::Rough
        );
    }

    #[test]
    fn adding_node_preserves_existing_wires_and_cancels_atomically() {
        let mut session = crate::session::Session::default();
        let path = session.new_path(KPoint::ZERO);
        session.selected = vec![path];
        session.active_mut().unwrap().appearance = Some(Graph::rough());
        let before = session.doc.clone();
        session.begin();
        let graph = session.graph_mut().unwrap();
        let id = super::add_unconnected(graph, Kind::Offset, KPoint::new(-123., 456.)).unwrap();
        assert_eq!(graph.node(id).unwrap().position, [-123., 456.]);
        assert!(graph.node(id).unwrap().inputs.iter().all(Option::is_none));
        assert_eq!(
            &graph.nodes[..graph.nodes.len() - 1],
            before.strokes[0]
                .appearance
                .as_ref()
                .unwrap()
                .nodes
                .as_slice()
        );
        session.cancel();
        assert_eq!(session.doc, before);
        session.begin();
        super::add_unconnected(session.graph_mut().unwrap(), Kind::Value, KPoint::ZERO).unwrap();
        session.commit();
        session.undo();
        assert_eq!(session.doc, before);
    }
    #[test]
    fn front_node_blocks_hidden_sockets() {
        let mut g = Graph::default();
        let hidden_output = output(g.node(1).unwrap());
        g.node_mut(2).unwrap().position = [180., 110.];
        assert_eq!(node_hit(&g, hidden_output, 1.), Some(NodeHit::Body(2)));
        // Once the body no longer covers it, the very same output is usable.
        g.node_mut(2).unwrap().position = [520., 90.];
        assert_eq!(node_hit(&g, hidden_output, 1.), Some(NodeHit::Output(1)));
        let back = g.add(Kind::Rough, [80., 100.]).unwrap();
        g.nodes.swap(1, 2); // Output is now the frontmost body.
        g.node_mut(2).unwrap().position = [40., 120.];
        assert_eq!(
            node_hit(&g, KPoint::new(80., 154.), 1.),
            Some(NodeHit::Body(2))
        );
        assert_eq!(g.node(back).unwrap().inputs[0], None);
    }
    #[test]
    fn deselection_does_not_start_a_move_and_click_sets_active() {
        let mut selected = vec![10, 20];
        assert!(!select_node(&mut selected, 10, true));
        assert_eq!(selected, vec![20]);
        assert!(select_node(&mut selected, 10, true));
        assert_eq!(selected, vec![20, 10]);
        assert!(select_node(&mut selected, 20, false));
        assert_eq!(selected, vec![10, 20]);
    }
    #[test]
    fn fit_uses_actual_small_viewport_and_negative_positions() {
        let mut g = Graph::rough();
        for n in &mut g.nodes {
            n.position[0] -= 1000.;
            n.position[1] -= 500.;
        }
        for (width, height) in [(160., 100.), (700., 240.), (40., 25.)] {
            let view = fitted_view(&g, width, height).unwrap();
            for n in &g.nodes {
                let r = node_rect(n);
                assert!(view.pan[0] + r.x0 as f32 * view.zoom >= -0.001);
                assert!(view.pan[1] + r.y0 as f32 * view.zoom >= -0.001);
                assert!(view.pan[0] + r.x1 as f32 * view.zoom <= width + 0.001);
                assert!(view.pan[1] + r.y1 as f32 * view.zoom <= height + 0.001);
            }
        }
        assert!(fitted_view(&g, 0., 0.).is_none());
    }
    #[test]
    fn input_started_connection_uses_same_type_and_cycle_rules() {
        let mut g = Graph::rough();
        let value = g.add(Kind::Value, [0., 0.]).unwrap();
        connect(&mut g, value, 3, Socket::Number(Parameter::Amount)).unwrap();
        let label = socket_label(
            g.node(3).unwrap(),
            Socket::Number(Parameter::Amount),
            "量",
            None,
        );
        assert_eq!(label, "量  ↗ —");
        let before = g.clone();
        assert!(connect(&mut g, value, 3, Socket::Shape(0)).is_err());
        assert!(connect(&mut g, 3, 3, Socket::Shape(0)).is_err());
        assert_eq!(g, before);
    }

    #[test]
    fn duplication_keeps_active_order_and_internal_connections() {
        let mut g = Graph::rough();
        let next = g.add(Kind::Offset, [500., 90.]).unwrap();
        g.connect(3, next, 0).unwrap();
        let copies = duplicate_selection(&mut g, &[next, 3]).unwrap();
        assert_eq!(copies.len(), 2);
        assert_eq!(g.node(copies[0]).unwrap().kind, Kind::Offset);
        assert_eq!(g.node(copies[1]).unwrap().kind, Kind::Rough);
        assert_eq!(g.node(copies[0]).unwrap().inputs[0], Some(copies[1]));
        assert_eq!(g.node(copies[1]).unwrap().inputs[0], Some(1));
        assert_eq!(g.node(next).unwrap().inputs[0], Some(3));
        // Shift+D starts a modal move at the originals, without a forced offset.
        assert_eq!(
            g.node(copies[0]).unwrap().position,
            g.node(next).unwrap().position
        );
        assert_eq!(
            g.node(copies[1]).unwrap().position,
            g.node(3).unwrap().position
        );
    }
    #[test]
    fn lazy_connection_respects_socket_types_and_cycles() {
        let mut g = Graph::rough();
        let value = g.add(Kind::Value, [0., 0.]).unwrap();
        lazy_connect(&mut g, value, 3).unwrap();
        assert_eq!(g.node(3).unwrap().bindings[0], (Parameter::Amount, value));
        let before = g.clone();
        assert!(lazy_connect(&mut g, 3, 1).is_err());
        assert_eq!(g, before);
    }
    #[test]
    fn cut_follows_curve_not_only_endpoints() {
        assert!(wire_crosses(
            point(px(0.), px(0.)),
            point(px(100.), px(100.)),
            point(px(50.), px(0.)),
            point(px(50.), px(100.))
        ));
    }
}
