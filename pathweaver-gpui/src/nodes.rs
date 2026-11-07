use crate::theme::{Role, color as ui_color};
use crate::{
    app::{Drag, Editing, Workspace},
    drawing::{View, dot, line, normal_bounds, rect, text, wire},
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
#[derive(Clone, Copy)]
pub enum Socket {
    Shape(usize),
    Number(Parameter),
}
fn height(n: &Node) -> f32 {
    74. + (n.inputs.len() + n.parameters().len()).saturating_sub(1) as f32 * 24.
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
impl Workspace {
    pub fn fit_nodes(&mut self) {
        if let Some(g) = self.session.graph() {
            let b = self.node_bounds.get();
            let minx = g
                .nodes
                .iter()
                .map(|n| n.position[0])
                .fold(f32::INFINITY, f32::min);
            let miny = g
                .nodes
                .iter()
                .map(|n| n.position[1])
                .fold(f32::INFINITY, f32::min);
            let maxx = g
                .nodes
                .iter()
                .map(|n| n.position[0] + WIDTH)
                .fold(0., f32::max);
            let maxy = g
                .nodes
                .iter()
                .map(|n| n.position[1] + height(n))
                .fold(0., f32::max);
            let zoom = ((f32::from(b.size.width).max(700.) - 80.) / (maxx - minx).max(1.))
                .min((f32::from(b.size.height).max(200.) - 50.) / (maxy - miny).max(1.))
                .clamp(0.2, 1.2);
            self.node_view = View {
                zoom,
                pan: [40. - minx * zoom, 25. - miny * zoom],
            };
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
        let tolerance = 10. / self.node_view.zoom as f64;
        for n in self.session.graph()?.nodes.iter().rev() {
            for (s, at, _) in sockets(n) {
                if at.distance(p) < tolerance {
                    return Some((n.id, s));
                }
            }
        }
        None
    }
    pub fn node_down(&mut self, e: &MouseDownEvent, w: &mut Window, cx: &mut Context<Self>) {
        if self.editing.is_some() {
            return;
        }
        self.focus.focus(w);
        self.node_region = true;
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
        if let Some(g) = self.session.graph() {
            for n in g.nodes.iter().rev() {
                if n.kind != Kind::Output
                    && output(n).distance(p) < 10. / self.node_view.zoom as f64
                {
                    let from = n.id;
                    self.session.begin();
                    self.drag = Drag::Connect {
                        from,
                        at: e.position,
                        lazy: false,
                    };
                    cx.notify();
                    return;
                }
            }
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
            if e.modifiers.shift {
                if self.node_selection.contains(&id) {
                    self.node_selection.retain(|n| *n != id);
                } else {
                    self.node_selection.push(id);
                }
            } else if !self.node_selection.contains(&id) {
                self.node_selection = vec![id];
            }
            if e.click_count == 2 {
                let n = self.session.graph().unwrap().node(id).unwrap();
                let label = n.label.clone();
                self.start_edit(Editing::NodeName, label, w, cx);
                return;
            }
            self.session.begin();
            self.drag = Drag::Nodes { last: p };
        } else {
            if !e.modifiers.shift {
                self.node_selection.clear();
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
                let selected = self.node_selection.clone();
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
            Drag::NodeBox {
                start,
                current,
                add,
            } => {
                if !add {
                    self.node_selection.clear();
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
                        if !self.node_selection.contains(&id) {
                            self.node_selection.push(id);
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
                self.add_open = !self.add_open;
                self.library_open = false;
                self.search.clear();
            }
            "n" => self.inspector = !self.inspector,
            "home" | "f" => self.fit_nodes(),
            "g" => {
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
            "d" if k.modifiers.shift => self.duplicate_nodes(),
            "m" => {
                let ids = self.node_selection.clone();
                self.session.edit(|s| {
                    if let Some(g) = s.graph_mut() {
                        for n in &mut g.nodes {
                            if ids.contains(&n.id) && !matches!(n.kind, Kind::Input | Kind::Output)
                            {
                                n.bypass = !n.bypass;
                            }
                        }
                    }
                });
            }
            "x" | "backspace" | "delete" => {
                let ids = self.node_selection.clone();
                self.session.edit(|s| {
                    if let Some(g) = s.graph_mut() {
                        for id in ids {
                            g.remove(id);
                        }
                    }
                });
                self.node_selection.clear();
            }
            "f2" => {
                if let Some(n) = self
                    .node_selection
                    .last()
                    .and_then(|id| self.session.graph()?.node(*id))
                {
                    self.start_edit(Editing::NodeName, n.label.clone(), w, cx);
                }
            }
            _ => {}
        }
        self.refresh(cx);
    }
    fn duplicate_nodes(&mut self) {
        let ids = self.node_selection.clone();
        self.session.begin();
        if let Some(g) = self.session.graph_mut() {
            let originals: Vec<_> = g
                .nodes
                .iter()
                .filter(|n| ids.contains(&n.id) && !matches!(n.kind, Kind::Input | Kind::Output))
                .cloned()
                .collect();
            let before = g.clone();
            let mut map = std::collections::HashMap::new();
            let mut failed = false;
            for n in &originals {
                match g.add(n.kind, [n.position[0] + 30., n.position[1] + 30.]) {
                    Ok(id) => {
                        map.insert(n.id, id);
                    }
                    Err(_) => {
                        failed = true;
                        break;
                    }
                }
            }
            if failed {
                *g = before;
            } else {
                for n in originals {
                    let id = map[&n.id];
                    let mut copy = n;
                    copy.id = id;
                    copy.position[0] += 30.;
                    copy.position[1] += 30.;
                    for source in copy.inputs.iter_mut().flatten() {
                        *source = *map.get(source).unwrap_or(source);
                    }
                    for (_, source) in &mut copy.bindings {
                        *source = *map.get(source).unwrap_or(source);
                    }
                    *g.node_mut(id).unwrap() = copy;
                }
                self.node_selection = map.values().copied().collect();
            }
        }
        self.session.commit();
    }
    fn add_node(&mut self, kind: Kind, cx: &mut Context<Self>) {
        self.session.begin();
        let source = self.node_selection.last().copied();
        let mut new_id = None;
        if let Some(g) = self.session.graph_mut() {
            let source = source
                .filter(|id| g.node(*id).is_some_and(|n| n.kind != Kind::Output))
                .unwrap_or(1);
            let pos = g
                .node(source)
                .map(|n| [n.position[0] + 230., n.position[1]])
                .unwrap_or([250., 100.]);
            match g.add(kind, pos) {
                Ok(id) => {
                    new_id = Some(id);
                    if kind.inputs() > 0 && g.node(source).is_some_and(|n| !n.kind.is_number()) {
                        let destinations: Vec<_> = g
                            .nodes
                            .iter()
                            .filter(|n| n.id != id)
                            .flat_map(|n| {
                                n.inputs.iter().enumerate().filter_map(move |(port, from)| {
                                    (*from == Some(source)).then_some((n.id, port))
                                })
                            })
                            .collect();
                        let _ = g.connect(source, id, 0);
                        if !kind.is_number() {
                            for (to, port) in destinations {
                                let _ = g.connect(id, to, port);
                            }
                        }
                    }
                }
                Err(e) => self.status = e,
            }
        }
        if let Some(id) = new_id {
            self.node_selection = vec![id];
        }
        self.session.commit();
        self.add_open = false;
        self.fit_nodes();
        self.refresh(cx);
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
                                            "セットを読み込みました。名前を押すと適用できます"
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
            let step = 24. * self.node_view.zoom;
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
                    if self.node_selection.contains(&n.id) {
                        ui_color(Role::Accent)
                    } else {
                        ui_color(Role::Border)
                    },
                    if self.node_selection.contains(&n.id) {
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
                let title = if n.label.is_empty() {
                    n.kind.label().to_string()
                } else {
                    n.label.clone()
                };
                let title = title.chars().take(14).collect::<String>();
                text(
                    w,
                    cx,
                    origin + point(px(9. * z), px(3. * z)),
                    title,
                    ui_color(Role::Text),
                    (12. * z).max(8.),
                );
                if !n.label.is_empty() {
                    text(
                        w,
                        cx,
                        origin + point(px(9. * z), px(28. * z)),
                        n.kind.label(),
                        ui_color(Role::Muted),
                        (9. * z).max(7.),
                    );
                }
                if n.bypass {
                    text(
                        w,
                        cx,
                        origin + point(px(130. * z), px(28. * z)),
                        "無効",
                        ui_color(Role::Muted),
                        (10. * z).max(7.),
                    );
                }
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
                    let label = match socket {
                        Socket::Number(p) => format!("{}  {:.2}", label, n.number(p)),
                        _ => label.into(),
                    };
                    text(
                        w,
                        cx,
                        at + point(px(10. * z), px(-8. * z)),
                        label,
                        ui_color(Role::Text),
                        (11. * z).max(7.),
                    );
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
            .node_selection
            .last()
            .and_then(|id| self.session.graph()?.node(*id))
        {
            let id = n.id;
            let label = n.label.clone();
            col = col
                .child(
                    div()
                        .text_color(rgb(ui_color(Role::Muted)))
                        .child(n.kind.label()),
                )
                .child(self.button(
                    if label.is_empty() {
                        "名前を付ける".into()
                    } else {
                        label.clone()
                    },
                    false,
                    cx,
                    move |s, w, cx| s.start_edit(Editing::NodeName, label.clone(), w, cx),
                ));
            if !matches!(n.kind, Kind::Input | Kind::Output) {
                col = col.child(self.button(
                    if n.bypass {
                        "無効（M）"
                    } else {
                        "有効（M）"
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
                ));
            }
            for spec in n.parameters() {
                let p = spec.key;
                let bound = n.bindings.iter().any(|(k, _)| *k == p);
                if bound {
                    col = col.child(
                        div()
                            .flex()
                            .justify_between()
                            .child(spec.label)
                            .child("接続元で調整"),
                    );
                } else {
                    col = col.child(self.numeric_field(spec.label, Editing::Parameter(p), cx));
                }
            }
            if matches!(n.kind, Kind::Fill | Kind::Shadow | Kind::Glow) {
                let c = n.color;
                col = col.child(self.button(
                    format!("色  #{:02X}{:02X}{:02X}", c[0], c[1], c[2]),
                    false,
                    cx,
                    move |s, w, cx| {
                        s.node_region = true;
                        s.start_edit(
                            Editing::Color,
                            format!("{:02X}{:02X}{:02X}", c[0], c[1], c[2]),
                            w,
                            cx,
                        );
                    },
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
            col = col
                .child("ノードを選択すると設定を編集できます。")
                .child("Shift＋クリック：複数選択")
                .child("中ボタンドラッグ：移動")
                .child("⌘＋スクロール：拡大縮小")
                .child("Option＋右ドラッグ：自動接続")
                .child("⌘＋Shift＋クリック：出力へ接続")
                .child("⌘＋右ドラッグ：接続を切る");
        }
        col.into_any_element()
    }
    fn add_sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut col = div()
            .id("add-nodes")
            .w(px(205.))
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
                    "検索：{}",
                    if self.search.is_empty() {
                        "すべて"
                    } else {
                        &self.search
                    }
                ),
                false,
                cx,
                |s, w, cx| s.start_edit(Editing::Search, s.search.clone(), w, cx),
            ));
        for kind in KINDS {
            if self.search.is_empty()
                || kind.label().contains(&self.search)
                || format!("{kind:?}")
                    .to_lowercase()
                    .contains(&self.search.to_lowercase())
            {
                col = col.child(self.button(kind.label(), false, cx, move |s, _, cx| {
                    s.add_node(kind, cx)
                }));
            }
        }
        col.into_any_element()
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
                    .child("保存済みセット / クリックで適用"),
            );
        for (i, p) in self.session.doc.appearance_presets.iter().enumerate() {
            let preset = p.clone();
            col = col
                .child(self.button(
                    format!("{} / {}", p.category, p.name),
                    false,
                    cx,
                    move |s, _, cx| {
                        if s.session.active().is_none() {
                            s.status = "適用先のパスを選択してください".into();
                            return;
                        }
                        s.session.edit(|s| {
                            if let Some(p) = s.active_mut() {
                                p.appearance = Some(preset.apply());
                            }
                        });
                        s.set_name = preset.name.clone();
                        s.category = preset.category.clone();
                        s.fit_nodes();
                        s.refresh(cx);
                    },
                ))
                .child(self.button(
                    format!("セット {} を削除", i + 1),
                    false,
                    cx,
                    move |s, _, _| {
                        s.session.edit(|s| {
                            s.doc.appearance_presets.remove(i);
                        })
                    },
                ));
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
                    move |b, _, w, cx| view.update(cx, |s, cx| s.paint_nodes(b, w, cx)),
                )
                .size_full(),
            )
            .on_mouse_down(MouseButton::Left, cx.listener(Self::node_down))
            .on_mouse_down(MouseButton::Right, cx.listener(Self::node_down))
            .on_mouse_down(MouseButton::Middle, cx.listener(Self::node_down))
            .on_scroll_wheel(cx.listener(|s, e, _, cx| s.scroll(e, true, cx)));
        let name = self
            .session
            .active()
            .map(|s| s.name.as_str())
            .unwrap_or("パス未選択");
        let mut bar = div()
            .h(px(36.))
            .px_2()
            .flex()
            .items_center()
            .gap_1()
            .bg(rgb(ui_color(Role::Toolbar)))
            .child(div().w(px(135.)).child("ノードエディター"))
            .child(self.button("追加 ⇧A", self.add_open, cx, |s, _, _| {
                s.add_open = !s.add_open;
                s.library_open = false;
            }))
            .child(self.button("セット", self.library_open, cx, |s, _, _| {
                s.library_open = !s.library_open;
                s.add_open = false;
                if let Some(g) = s.session.graph() {
                    s.set_name = g.name.clone();
                }
            }))
            .child(self.button("全ノード Home", false, cx, |s, _, _| s.fit_nodes()))
            .child(self.button("設定 N", self.inspector, cx, |s, _, _| {
                s.inspector = !s.inspector
            }))
            .child(
                div()
                    .ml_2()
                    .text_color(rgb(ui_color(Role::Muted)))
                    .child(name.to_string()),
            );
        if let Some(g) = self.session.graph() {
            bar = bar.child(self.button(
                if g.enabled {
                    "効果：オン"
                } else {
                    "効果：オフ"
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
            .child(self.button("閉じる", false, cx, |s, _, _| s.nodes_open = false));
        let mut body = div().flex().flex_1().min_h_0();
        if self.add_open {
            body = body.child(self.add_sidebar(cx));
        }
        if self.library_open {
            body = body.child(self.library_sidebar(cx));
        }
        if self.session.graph().is_some() {
            body = body.child(node_canvas);
            if self.inspector {
                body = body.child(self.node_inspector(cx));
            }
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
        div()
            .h(px(self.node_height))
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
                        cx.listener(|s, _, _, _| s.drag = Drag::Resize),
                    ),
            )
            .child(bar)
            .child(body)
            .overflow_hidden()
            .with_animation(
                "node-dock-reveal",
                Animation::new(std::time::Duration::from_millis(180)).with_easing(ease_in_out),
                {
                    let h = self.node_height;
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
    use super::{Graph, Kind, Parameter, lazy_connect, point, px, wire_crosses};
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
