//! Document transactions for the real Tweeq components hosted in WKWebView.
//! The web view never owns the document: every message is checked against the
//! selection and document revision which produced its controls.
use crate::{
    app::{Tool, Workspace},
    session::Session,
};
use gpui::{Context, Window};
use kurbo::{Affine, Vec2};
use pathweaver_core::{
    appearance::{Graph, Kind, MathOperation, Operation, Parameter},
    appearance_effects::{BandPosition, OffsetJoin},
    editing::{Anchor, AnchorKind, MergeTarget},
    model::{Document, Join},
};
use serde_json::{Value, json};
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

#[derive(Clone, PartialEq)]
struct Target {
    paths: Vec<u64>,
    anchors: Vec<Anchor>,
    nodes: Vec<u64>,
    segment: Option<Anchor>,
}
impl Target {
    fn capture(s: &Session) -> Self {
        Self {
            paths: s.selected.clone(),
            anchors: s.anchors.clone(),
            nodes: s.node_selection.clone(),
            segment: s.segment,
        }
    }
}
struct Transaction {
    panel: String,
    field: String,
    revision: String,
    target: Target,
    before: Document,
    preview: Document,
    value: Option<Value>,
}
#[derive(Default)]
pub(crate) struct TweeqBridge {
    transaction: Option<Transaction>,
    // Keep the same revision across messages queued by a single web event
    // (commit field A, then begin B or click an action). An outside document
    // or selection change invalidates it immediately.
    completed: Option<(String, String, Target, Document)>,
    epoch: u64,
    error: Option<String>,
}
impl TweeqBridge {
    fn revision(&self, s: &Session, panel: &str) -> String {
        if let Some(t) = &self.transaction
            && t.panel == panel
            && t.target == Target::capture(s)
            && t.preview == s.doc
        {
            return t.revision.clone();
        }
        if let Some((owner, revision, target, doc)) = &self.completed
            && owner == panel
            && *target == Target::capture(s)
            && *doc == s.doc
        {
            return revision.clone();
        }
        let mut hash = DefaultHasher::new();
        self.epoch.hash(&mut hash);
        panel.hash(&mut hash);
        serde_json::to_vec(&s.doc)
            .unwrap_or_default()
            .hash(&mut hash);
        s.selected.hash(&mut hash);
        s.node_selection.hash(&mut hash);
        for a in &s.anchors {
            a.stroke.hash(&mut hash);
            a.node.hash(&mut hash);
        }
        s.segment.map(|a| (a.stroke, a.node)).hash(&mut hash);
        format!("{panel}-{:016x}", hash.finish())
    }
    pub(crate) fn finish(&mut self, s: &mut Session, commit: bool) {
        if let Some(t) = self.transaction.take() {
            let unchanged = t.preview == s.doc && t.target == Target::capture(s);
            // If another command already changed the document, do not restore
            // a stale web preview over that command.
            if commit || !unchanged {
                s.commit();
            } else {
                s.cancel();
            }
            self.epoch = self.epoch.wrapping_add(1);
            self.completed =
                unchanged.then(|| (t.panel, t.revision, Target::capture(s), s.doc.clone()));
            self.error = None;
        }
    }
    fn update(&mut self, s: &mut Session, panel: &str, m: &Value) -> Result<(), String> {
        let current = m["revision"].as_str() == Some(&self.revision(s, panel));
        let result = self.update_inner(s, panel, m);
        if current && let Err(error) = &result {
            self.error = Some(error.clone());
        }
        result
    }
    fn update_inner(&mut self, s: &mut Session, panel: &str, m: &Value) -> Result<(), String> {
        let kind = m["type"].as_str().ok_or("操作がありません")?;
        let revision = m["revision"].as_str().ok_or("編集対象がありません")?;
        if revision != self.revision(s, panel) {
            return Err("選択が変わりました。現在の項目を編集してください".into());
        }
        if kind == "cancel" {
            self.finish(s, false);
            return Ok(());
        }
        let id = m["id"].as_str().ok_or("項目がありません")?;
        if !field_allowed(s, panel, id) {
            return Err("この項目は編集できません".into());
        }
        let same = self
            .transaction
            .as_ref()
            .is_some_and(|t| t.panel == panel && t.field == id);
        if same
            && kind == "commit"
            && m.get("value").is_none()
            && let Some(error) = &self.error
        {
            // Tweeq emits its value before confirm. An invalid value must not
            // become a successful commit of the previous preview on confirm.
            return Err(error.clone());
        }
        if !same {
            if self
                .transaction
                .as_ref()
                .is_some_and(|t| t.target == Target::capture(s))
                && let Some(error) = &self.error
            {
                // A queued focus on the next control must not turn a rejected
                // confirm into a successful commit of the old preview.
                return Err(error.clone());
            }
            self.finish(s, true);
            self.error = None;
            s.begin();
            self.transaction = Some(Transaction {
                panel: panel.into(),
                field: id.into(),
                revision: revision.into(),
                target: Target::capture(s),
                before: s.doc.clone(),
                preview: s.doc.clone(),
                value: None,
            });
        }
        if kind == "begin" {
            return Ok(());
        }
        if !matches!(kind, "change" | "commit") {
            return Err("未対応の操作です".into());
        }
        if let Some(value) = m.get("value") {
            let t = self.transaction.as_mut().unwrap();
            let old = s.doc.clone();
            s.doc = t.before.clone();
            if let Err(e) = apply(s, id, value) {
                s.doc = old;
                return Err(e);
            }
            if let Err(e) = s.doc.validate() {
                s.doc = old;
                return Err(e);
            }
            t.preview = s.doc.clone();
            t.value = Some(value.clone());
        }
        self.error = None;
        if kind == "commit" {
            self.finish(s, true);
        }
        Ok(())
    }
}

fn field(id: &str, label: &str, kind: &str, value: impl Into<Value>) -> Value {
    json!({"id":id,"label":label,"type":kind,"value":value.into()})
}
fn number(id: &str, label: &str, value: f64, min: f64, max: f64, step: f64) -> Value {
    json!({"id":id,"label":label,"type":"number","value":value,"min":min,"max":max,"step":step})
}
fn choice(id: &str, label: &str, value: &str, options: &[&str], labels: &[&str]) -> Value {
    json!({"id":id,"label":label,"type":"select","value":value,"options":options,"labels":labels})
}
fn color(c: Option<[u8; 3]>) -> Value {
    c.map(|c| Value::String(format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2])))
        .unwrap_or(Value::Null)
}
fn parameter(id: &str) -> Option<Parameter> {
    serde_json::from_value(Value::String(id.strip_prefix("param:")?.to_owned())).ok()
}
fn param_step(p: Parameter, min: f64, max: f64) -> f64 {
    if p == Parameter::Seed {
        1.
    } else if max - min <= 2. {
        0.01
    } else if max - min <= 100. {
        0.1
    } else {
        1.
    }
}
fn selected_node(s: &Session) -> Option<&pathweaver_core::appearance::Node> {
    s.graph()?.node(*s.node_selection.last()?)
}
fn field_allowed(s: &Session, panel: &str, id: &str) -> bool {
    match panel {
        "grid" => matches!(
            id,
            "grid.spacing" | "snap.tangent" | "snap.normal" | "snap.guides" | "snap.grid"
        ),
        "path" if s.active().is_some() => match id {
            "path.anchor_x" | "path.anchor_y" => !s.anchors.is_empty(),
            "path.name" | "path.width" | "path.stroke" | "path.fill" | "path.closed"
            | "path.join" | "path.rotation" | "path.scale" => true,
            _ => false,
        },
        "node" => selected_node(s).is_some_and(|n| {
            if let Some(p) = parameter(id) {
                return n.parameters().iter().any(|s| s.key == p)
                    && !n.bindings.iter().any(|(k, _)| *k == p);
            }
            match id {
                "node.name" => true,
                "node.bypass" => !matches!(n.kind, Kind::Input | Kind::Output),
                "node.color" => matches!(n.kind, Kind::Fill | Kind::Glow | Kind::Shadow),
                "node.smooth" => matches!(n.kind, Kind::Rough | Kind::Zigzag),
                "node.inner" => n.kind == Kind::Glow,
                "node.measure_height" => n.kind == Kind::Measure,
                "node.operation" => n.kind == Kind::Boolean,
                "node.math" => n.kind == Kind::Math,
                "node.join" => matches!(n.kind, Kind::Offset | Kind::Round | Kind::Band),
                "node.position" => n.kind == Kind::Band,
                _ => false,
            }
        }),
        _ => false,
    }
}
fn finite(v: &Value, min: f64, max: f64) -> Result<f64, String> {
    v.as_f64()
        .filter(|x| x.is_finite() && *x >= min && *x <= max)
        .ok_or_else(|| format!("{min}〜{max}の数値を入力してください"))
}
fn boolean(v: &Value) -> Result<bool, String> {
    v.as_bool()
        .ok_or_else(|| "オンまたはオフを選んでください".into())
}
fn text(v: &Value) -> Result<String, String> {
    v.as_str()
        .filter(|s| s.chars().count() <= 80)
        .map(str::to_owned)
        .ok_or_else(|| "名前は80文字以内です".into())
}
fn parse_color(v: &Value) -> Result<Option<[u8; 3]>, String> {
    if v.is_null() {
        return Ok(None);
    }
    let s = v
        .as_str()
        .and_then(|s| s.strip_prefix('#'))
        .filter(|s| s.len() == 6 && s.is_ascii())
        .ok_or("#RRGGBB 形式で色を指定してください")?;
    let n = u32::from_str_radix(s, 16).map_err(|_| "色の値が正しくありません")?;
    Ok(Some([(n >> 16) as u8, (n >> 8) as u8, n as u8]))
}
fn apply(s: &mut Session, id: &str, v: &Value) -> Result<(), String> {
    if let Some(p) = parameter(id) {
        let node_id = *s.node_selection.last().ok_or("ノードを選択してください")?;
        let n = s
            .graph_mut()
            .and_then(|g| g.node_mut(node_id))
            .ok_or("ノードを選択してください")?;
        let spec = n
            .parameters()
            .into_iter()
            .find(|spec| spec.key == p)
            .ok_or("このパラメーターはありません")?;
        if n.bindings.iter().any(|(k, _)| *k == p) {
            return Err("接続元の値を調整してください".into());
        }
        let value = finite(v, spec.min, spec.max)?;
        if p == Parameter::Seed && value.fract() != 0. {
            return Err("シードは整数で入力してください".into());
        }
        n.set_number(p, value);
        return Ok(());
    }
    match id {
        "grid.spacing" => s.doc.grid_spacing = finite(v, 1., 10000.)?,
        "snap.tangent" => s.doc.snap_tangent = boolean(v)?,
        "snap.normal" => s.doc.snap_normal = boolean(v)?,
        "snap.guides" => s.doc.snap_guides = boolean(v)?,
        "snap.grid" => s.doc.snap_grid = boolean(v)?,
        "path.width" => {
            let value = finite(v, 0.1, 1000.)?;
            for p in &mut s.doc.strokes {
                if s.selected.contains(&p.id) {
                    p.resize_line_width(value)?;
                }
            }
        }
        "path.anchor_x" | "path.anchor_y" => {
            let value = finite(v, -1e6, 1e6)?;
            let p = s.anchor_position().ok_or("点を選択してください")?;
            s.move_anchors(if id == "path.anchor_x" {
                Vec2::new(value - p.x, 0.)
            } else {
                Vec2::new(0., value - p.y)
            });
        }
        "path.rotation" | "path.scale" => {
            let transform = if id == "path.rotation" {
                Affine::rotate(finite(v, -360000., 360000.)?.to_radians())
            } else {
                Affine::scale(finite(v, 0.01, 100.)?)
            };
            let r = s.bounds(true).ok_or("パスを選択してください")?;
            s.transform(
                Affine::translate(r.center().to_vec2())
                    * transform
                    * Affine::translate(-r.center().to_vec2()),
            );
        }
        id if id.starts_with("path.") => {
            let p = s.active_mut().ok_or("パスを選択してください")?;
            match id {
                "path.name" => {
                    let name = text(v)?;
                    if name.trim().is_empty() {
                        return Err("名前は1〜80文字です".into());
                    }
                    p.name = name.trim().into();
                }
                "path.stroke" => p.edge = parse_color(v)?,
                "path.fill" => p.fill = parse_color(v)?,
                "path.closed" => p.closed = boolean(v)?,
                "path.join" => {
                    p.join = match v.as_str() {
                        Some("Miter") => Join::Miter,
                        Some("Round") => Join::Round,
                        Some("Bevel") => Join::Bevel,
                        _ => return Err("角の結合を選択してください".into()),
                    }
                }
                _ => return Err("項目がありません".into()),
            }
        }
        id if id.starts_with("node.") => {
            let node_id = *s.node_selection.last().ok_or("ノードを選択してください")?;
            let n = s
                .graph_mut()
                .and_then(|g| g.node_mut(node_id))
                .ok_or("ノードを選択してください")?;
            match id {
                "node.name" => n.label = text(v)?,
                "node.bypass" => n.bypass = boolean(v)?,
                "node.color" => n.color = parse_color(v)?.ok_or("このノードには色が必要です")?,
                "node.smooth" => n.smooth = boolean(v)?,
                "node.inner" => n.inner = boolean(v)?,
                "node.measure_height" => n.measure_height = boolean(v)?,
                "node.operation" => {
                    n.operation = serde_json::from_value(v.clone())
                        .map_err(|_| "合成方法を選択してください")?
                }
                "node.math" => {
                    n.math = serde_json::from_value(v.clone())
                        .map_err(|_| "計算方法を選択してください")?
                }
                "node.join" => {
                    n.effect.join =
                        serde_json::from_value(v.clone()).map_err(|_| "角の形を選択してください")?
                }
                "node.position" => {
                    n.effect.position =
                        serde_json::from_value(v.clone()).map_err(|_| "位置を選択してください")?
                }
                _ => return Err("項目がありません".into()),
            }
        }
        _ => return Err("項目がありません".into()),
    }
    Ok(())
}

impl Workspace {
    pub(crate) fn finish_tweeq(&mut self, commit: bool) {
        self.tweeq.finish(&mut self.session, commit);
    }
    pub(crate) fn tweeq_snapshot(&self, panel: &str) -> Value {
        let s = &self.session;
        let mut fields = vec![];
        let mut actions = vec![];
        let mut title = String::new();
        let mut subtitle = String::new();
        let action = |id: &str, label: &str, icon: &str, disabled: bool| json!({"id":id,"label":label,"icon":icon,"disabled":disabled});
        match panel {
            "grid" => {
                title = "グリッド・ガイド".into();
                for (id, label, value) in [
                    ("snap.tangent", "接線", s.doc.snap_tangent),
                    ("snap.normal", "法線", s.doc.snap_normal),
                    ("snap.guides", "ガイド", s.doc.snap_guides),
                    ("snap.grid", "グリッド", s.doc.snap_grid),
                ] {
                    fields.push(field(id, label, "boolean", value));
                }
                fields.push(number(
                    "grid.spacing",
                    "間隔",
                    s.doc.grid_spacing,
                    1.,
                    10000.,
                    1.,
                ));
                subtitle = "Controlで一時的に吸着を解除".into();
            }
            "path" => {
                title = "パス".into();
                if let Some(p) = s.active() {
                    subtitle = if s.selected.len() > 1 {
                        format!(
                            "{} パス選択 · 名前・色は {} が対象",
                            s.selected.len(),
                            p.name
                        )
                    } else {
                        p.name.clone()
                    };
                    fields.push(field("path.name", "名前", "text", p.name.clone()));
                    if let Some(p) = s.anchor_position() {
                        fields.push(number("path.anchor_x", "X", p.x, -1e6, 1e6, 1.));
                        fields.push(number("path.anchor_y", "Y", p.y, -1e6, 1e6, 1.));
                    }
                    fields.push(number(
                        "path.width",
                        "最大幅",
                        p.line_width(),
                        0.1,
                        1000.,
                        0.2,
                    ));
                    fields.push(field("path.stroke", "線", "color", color(p.edge)));
                    fields.push(field("path.fill", "塗り", "color", color(p.fill)));
                    fields.push(field("path.closed", "閉じる", "boolean", p.closed));
                    fields.push(choice(
                        "path.join",
                        "角",
                        match p.join {
                            Join::Miter => "Miter",
                            Join::Round => "Round",
                            Join::Bevel => "Bevel",
                        },
                        &["Miter", "Round", "Bevel"],
                        &["鋭い角", "丸い角", "面取り"],
                    ));
                    fields.push(number("path.rotation", "回転", 0., -360000., 360000., 1.));
                    fields.last_mut().unwrap()["suffix"] = json!("°");
                    fields.push(number("path.scale", "拡大率", 1., 0.01, 100., 0.01));
                    actions.extend([
                        action("path_duplicate", "複製", "copy", false),
                        action("path_delete", "削除", "trash", false),
                        action("path_nodes", "ノード", "nodes", false),
                    ]);
                    if matches!(self.tool, Tool::Direct | Tool::Convert) {
                        if !s.anchors.is_empty() {
                            for (id, label) in [
                                ("anchor_corner", "角"),
                                ("anchor_smooth", "滑らか"),
                                ("anchor_independent", "独立"),
                            ] {
                                actions.push(action(id, label, "anchor", false));
                            }
                            actions.push(action("separate", "分離", "split", s.anchors.len() != 1));
                            actions.push(action("join", "連結", "join", s.anchors.len() != 2));
                            for (id, label) in [
                                ("merge_center", "中心へマージ"),
                                ("merge_first", "最初へマージ"),
                                ("merge_last", "最後へマージ"),
                            ] {
                                actions.push(action(id, label, "merge", s.anchors.len() < 2));
                            }
                        }
                        if s.segment.is_some() || !s.anchors.is_empty() {
                            actions.push(action("straighten", "次の線を直線に", "line", false));
                        }
                    }
                    if s.selected.len() > 1 {
                        actions.push(action("union", "輪郭を合体", "union", false));
                    }
                } else {
                    subtitle = "パスを選択して編集".into();
                }
            }
            "node" => {
                title = "ノード".into();
                if let Some(n) = selected_node(s) {
                    title = n.kind.label().into();
                    subtitle = s.active().map(|p| p.name.clone()).unwrap_or_default();
                    fields.push(field("node.name", "名前", "text", n.label.clone()));
                    if !matches!(n.kind, Kind::Input | Kind::Output) {
                        fields.push(field("node.bypass", "無効", "boolean", n.bypass));
                        actions.extend([
                            action("node_duplicate", "複製", "copy", false),
                            action("node_delete", "削除", "trash", false),
                        ]);
                    }
                    for spec in n.parameters() {
                        let bound = n.bindings.iter().any(|(k, _)| *k == spec.key);
                        let effective = self.node_parameter_values.get(&(n.id, spec.key)).copied();
                        let mut f = number(
                            &format!("param:{:?}", spec.key),
                            spec.label,
                            if bound {
                                effective.unwrap_or(0.)
                            } else {
                                n.number(spec.key)
                            },
                            spec.min,
                            spec.max,
                            param_step(spec.key, spec.min, spec.max),
                        );
                        if bound {
                            f["disabled"] = json!(true);
                            f["label"] = json!(format!("{} ↗", spec.label));
                            if effective.is_none() {
                                f["value"] = Value::Null;
                            }
                        }
                        fields.push(f);
                    }
                    if matches!(n.kind, Kind::Fill | Kind::Shadow | Kind::Glow) {
                        let mut f = field("node.color", "色", "color", color(Some(n.color)));
                        f["nullable"] = json!(false);
                        fields.push(f);
                    }
                    if matches!(n.kind, Kind::Rough | Kind::Zigzag) {
                        fields.push(field("node.smooth", "滑らか", "boolean", n.smooth));
                    }
                    if n.kind == Kind::Glow {
                        fields.push(field("node.inner", "内側", "boolean", n.inner));
                    }
                    if n.kind == Kind::Measure {
                        fields.push(field(
                            "node.measure_height",
                            "高さを計測",
                            "boolean",
                            n.measure_height,
                        ));
                    }
                    if n.kind == Kind::Boolean {
                        let ops = [
                            Operation::Union,
                            Operation::Difference,
                            Operation::Intersect,
                            Operation::Xor,
                        ];
                        fields.push(choice(
                            "node.operation",
                            "合成",
                            &format!("{:?}", n.operation),
                            &["Union", "Difference", "Intersect", "Xor"],
                            &ops.map(|o| o.label()),
                        ));
                    }
                    if n.kind == Kind::Math {
                        let ops = [
                            MathOperation::Add,
                            MathOperation::Subtract,
                            MathOperation::Multiply,
                            MathOperation::Divide,
                            MathOperation::Min,
                            MathOperation::Max,
                            MathOperation::Power,
                            MathOperation::Modulo,
                            MathOperation::Snap,
                        ];
                        fields.push(choice(
                            "node.math",
                            "計算",
                            &format!("{:?}", n.math),
                            &[
                                "Add", "Subtract", "Multiply", "Divide", "Min", "Max", "Power",
                                "Modulo", "Snap",
                            ],
                            &ops.map(|o| o.label()),
                        ));
                    }
                    if matches!(n.kind, Kind::Offset | Kind::Round | Kind::Band) {
                        fields.push(choice(
                            "node.join",
                            "角",
                            match n.effect.join {
                                OffsetJoin::Round => "Round",
                                OffsetJoin::Miter => "Miter",
                                OffsetJoin::Bevel => "Bevel",
                            },
                            &["Round", "Miter", "Bevel"],
                            &["丸い角", "鋭い角", "面取り"],
                        ));
                    }
                    if n.kind == Kind::Band {
                        fields.push(choice(
                            "node.position",
                            "位置",
                            match n.effect.position {
                                BandPosition::Center => "Center",
                                BandPosition::Inner => "Inner",
                                BandPosition::Outer => "Outer",
                            },
                            &["Center", "Inner", "Outer"],
                            &["中央", "内側", "外側"],
                        ));
                    }
                } else {
                    subtitle = "ノードを選択して編集".into();
                }
            }
            _ => {}
        }
        if let Some(t) = &self.tweeq.transaction
            && t.panel == panel
            && t.target == Target::capture(s)
            && let Some(value) = &t.value
        {
            for f in &mut fields {
                if f["id"] == t.field {
                    f["value"] = value.clone();
                }
            }
        }
        let pending = self
            .tweeq
            .error
            .as_ref()
            .and(self.tweeq.transaction.as_ref())
            .filter(|t| t.panel == panel)
            .map(|t| json!({"id":t.field,"revision":t.revision}));
        json!({"revision":self.tweeq.revision(s,panel),"theme":if crate::theme::is_dark(){"dark"}else{"light"},"title":title,"subtitle":subtitle,"sections":[{"fields":fields}],"actions":actions,"error":self.tweeq.error,"pending":pending})
    }
    pub(crate) fn tweeq_message(
        &mut self,
        panel: &str,
        body: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Ok(m) = serde_json::from_str::<Value>(body) else {
            return;
        };
        let kind = m["type"].as_str().unwrap_or("");
        if kind == "ready" {
            cx.notify();
            return;
        }
        if kind == "focus" {
            self.node_region = panel == "node";
            self.active_nodes = panel == "node";
            return;
        }
        if kind == "action" {
            use crate::menus::Command;
            let native = match m["id"].as_str() {
                Some("save") => Some(Command::Save),
                Some("saveAs") => Some(Command::SaveAs),
                Some("open") => Some(Command::Open),
                Some("new") => Some(Command::New),
                Some("close") => Some(Command::Close),
                Some("export") => Some(Command::Export),
                Some("undo") => Some(Command::Undo),
                Some("redo") => Some(Command::Redo),
                Some("selectAll") => Some(Command::SelectAll),
                _ => None,
            };
            if let Some(command) = native {
                if self.tweeq.transaction.is_some()
                    && self.tweeq.error.is_some()
                    && !matches!(command, Command::Close)
                {
                    // Match native field validation: saving must not silently
                    // persist the previous value after an invalid commit.
                    cx.notify();
                    return;
                }
                // A preceding commit changes the revision. Native commands are
                // intentionally selection-independent and follow that commit.
                self.tweeq_panels.release_focus();
                self.focus.focus(window);
                self.menu_command(command, window, cx);
                return;
            }
            if m["revision"].as_str() != Some(&self.tweeq.revision(&self.session, panel)) {
                return;
            }
            if self.tweeq.transaction.is_some() && self.tweeq.error.is_some() {
                cx.notify();
                return;
            }
            let id = m["id"].as_str().unwrap_or("");
            let snapshot = self.tweeq_snapshot(panel);
            if !snapshot["actions"]
                .as_array()
                .is_some_and(|a| a.iter().any(|a| a["id"] == id && a["disabled"] == false))
            {
                return;
            }
            self.finish_interaction(true, cx);
            use crate::path_edit::Operation as PathOp;
            match id {
                "path_duplicate" => self.session.duplicate(),
                "path_delete" => self.session.remove(),
                "path_nodes" => {
                    self.nodes_open = true;
                    self.node_region = true;
                    self.active_nodes = true;
                    if self.session.graph().is_none() {
                        self.session.edit(|s| {
                            if let Some(p) = s.active_mut() {
                                p.appearance = Some(Graph::default());
                            }
                        });
                        self.fit_nodes();
                    }
                }
                "node_duplicate" => self.duplicate_nodes(cx),
                "node_delete" => self.delete_nodes(),
                id => {
                    let op = match id {
                        "anchor_corner" => PathOp::Kind(AnchorKind::Corner),
                        "anchor_smooth" => PathOp::Kind(AnchorKind::Smooth),
                        "anchor_independent" => PathOp::Kind(AnchorKind::Independent),
                        "separate" => PathOp::Separate,
                        "join" => PathOp::Join,
                        "merge_center" => PathOp::Merge(MergeTarget::Center),
                        "merge_first" => PathOp::Merge(MergeTarget::First),
                        "merge_last" => PathOp::Merge(MergeTarget::Last),
                        "straighten" => PathOp::Straighten,
                        "union" => PathOp::Union,
                        _ => return,
                    };
                    self.path_operation(op, cx);
                }
            }
            self.focus.focus(window);
            self.refresh(cx);
            return;
        }
        if matches!(kind, "begin" | "change" | "commit") && self.tweeq.transaction.is_none() {
            if self.editing.is_some() {
                self.commit_edit(window, cx);
                if self.editing.is_some() {
                    return;
                }
            }
            self.finish_interaction(true, cx);
        }
        if let Err(e) = self.tweeq.update(&mut self.session, panel, &m) {
            self.status = e;
        }
        self.refresh(cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pathweaver_core::model::Stroke;
    fn session() -> Session {
        let mut s = Session::default();
        s.doc.strokes.push(Stroke::new(1, "A".into()));
        s.doc.strokes.push(Stroke::new(2, "B".into()));
        s.doc.next_id = 3;
        s.select(1, false);
        s
    }
    fn message(kind: &str, revision: &str, id: &str, value: Value) -> Value {
        json!({"type":kind,"revision":revision,"id":id,"value":value})
    }
    #[test]
    fn color_drag_is_one_undo_and_cancel_restores() {
        let mut s = session();
        let before = s.doc.clone();
        let mut b = TweeqBridge::default();
        let r = b.revision(&s, "path");
        b.update(
            &mut s,
            "path",
            &message("begin", &r, "path.fill", Value::Null),
        )
        .unwrap();
        for c in ["#112233", "#AABBCC", "#334455"] {
            b.update(
                &mut s,
                "path",
                &message("change", &r, "path.fill", json!(c)),
            )
            .unwrap();
        }
        b.update(
            &mut s,
            "path",
            &json!({"type":"commit","revision":r,"id":"path.fill"}),
        )
        .unwrap();
        assert_eq!(s.active().unwrap().fill, Some([0x33, 0x44, 0x55]));
        s.undo();
        assert_eq!(s.doc, before);
        assert!(!s.can_undo());
        let r = b.revision(&s, "path");
        b.update(
            &mut s,
            "path",
            &message("change", &r, "path.fill", json!("#FFFFFF")),
        )
        .unwrap();
        b.update(&mut s, "path", &json!({"type":"cancel","revision":r}))
            .unwrap();
        assert_eq!(s.doc, before);
    }
    #[test]
    fn selection_change_rejects_old_web_messages() {
        let mut s = session();
        let mut b = TweeqBridge::default();
        let r = b.revision(&s, "path");
        s.select(2, false);
        assert!(
            b.update(
                &mut s,
                "path",
                &message("change", &r, "path.name", json!("wrong"))
            )
            .is_err()
        );
        assert_eq!(s.active().unwrap().name, "B");
        assert!(
            b.error.is_none(),
            "late blur must not poison the new selection"
        );
    }
    #[test]
    fn invalid_color_never_mutates_document() {
        let mut s = session();
        let before = s.doc.clone();
        let mut b = TweeqBridge::default();
        let r = b.revision(&s, "path");
        assert!(
            b.update(
                &mut s,
                "path",
                &message("change", &r, "path.stroke", json!("#GG0000"))
            )
            .is_err()
        );
        assert_eq!(s.doc, before);
        b.finish(&mut s, true);
        assert!(!s.can_undo());
    }
    #[test]
    fn grid_gesture_is_undoable_and_out_of_range_rejected() {
        let mut s = session();
        let mut b = TweeqBridge::default();
        let r = b.revision(&s, "grid");
        b.update(
            &mut s,
            "grid",
            &message("change", &r, "grid.spacing", json!(25.)),
        )
        .unwrap();
        assert!(
            b.update(
                &mut s,
                "grid",
                &message("change", &r, "grid.spacing", json!(0.))
            )
            .is_err()
        );
        assert_eq!(s.doc.grid_spacing, 25.);
        b.finish(&mut s, true);
        s.undo();
        assert_eq!(s.doc.grid_spacing, 50.);
    }
    #[test]
    fn queued_commit_then_next_field_and_action_keep_revision() {
        let mut s = session();
        let mut b = TweeqBridge::default();
        let r = b.revision(&s, "path");
        b.update(
            &mut s,
            "path",
            &message("commit", &r, "path.name", json!("Renamed")),
        )
        .unwrap();
        // This is also the revision check made for an action queued by the
        // same click which blurred the name field.
        assert_eq!(b.revision(&s, "path"), r);
        b.update(
            &mut s,
            "path",
            &message("begin", &r, "path.fill", Value::Null),
        )
        .unwrap();
        b.update(
            &mut s,
            "path",
            &message("commit", &r, "path.fill", json!("#123456")),
        )
        .unwrap();
        assert_eq!(b.revision(&s, "path"), r);
        assert_eq!(s.active().unwrap().name, "Renamed");
        assert_eq!(s.active().unwrap().fill, Some([0x12, 0x34, 0x56]));
        s.undo();
        assert_ne!(b.revision(&s, "path"), r);
        assert_eq!(s.active().unwrap().name, "Renamed");
        s.undo();
        assert_eq!(s.active().unwrap().name, "A");
    }
    #[test]
    fn names_keep_native_validation() {
        let mut s = session();
        assert!(apply(&mut s, "path.name", &json!(" ")).is_err());
        assert!(apply(&mut s, "path.name", &json!("あ".repeat(81))).is_err());
        apply(&mut s, "path.name", &json!("あ".repeat(80))).unwrap();
    }
    #[test]
    fn invalid_confirm_stays_pending_until_corrected_or_cancelled() {
        let mut s = session();
        let mut b = TweeqBridge::default();
        let r = b.revision(&s, "path");
        b.update(
            &mut s,
            "path",
            &message("change", &r, "path.name", json!("Valid")),
        )
        .unwrap();
        assert!(
            b.update(
                &mut s,
                "path",
                &message("change", &r, "path.name", json!(""))
            )
            .is_err()
        );
        let confirm = json!({"type":"commit", "revision":r, "id":"path.name"});
        assert!(b.update(&mut s, "path", &confirm).is_err());
        assert!(b.transaction.is_some());
        assert!(b.error.is_some());
        assert!(
            b.update(
                &mut s,
                "path",
                &message("begin", &r, "path.fill", Value::Null)
            )
            .is_err()
        );
        assert_eq!(b.transaction.as_ref().unwrap().field, "path.name");
        b.update(&mut s, "path", &json!({"type":"cancel", "revision":r}))
            .unwrap();
        assert_eq!(s.active().unwrap().name, "A");
        assert!(!s.can_undo());
        b.update(
            &mut s,
            "path",
            &message("change", &r, "path.name", json!("")),
        )
        .unwrap_err();
        b.update(
            &mut s,
            "path",
            &message("change", &r, "path.name", json!("Corrected")),
        )
        .unwrap();
        b.update(&mut s, "path", &confirm).unwrap();
        assert!(b.error.is_none());
        assert!(b.transaction.is_none());
        s.undo();
        assert_eq!(s.active().unwrap().name, "A");
    }
    #[test]
    fn transform_previews_are_relative_to_gesture_start() {
        use kurbo::Point;
        use pathweaver_core::model::Node;
        let mut s = session();
        let p = s.active_mut().unwrap();
        p.nodes = vec![
            Node {
                id: 10,
                p: Point::new(0., 0.),
                incoming: Vec2::ZERO,
                outgoing: Vec2::ZERO,
                smooth: false,
                exact_incoming: false,
                exact_outgoing: false,
            },
            Node {
                id: 11,
                p: Point::new(10., 0.),
                incoming: Vec2::ZERO,
                outgoing: Vec2::ZERO,
                smooth: false,
                exact_incoming: false,
                exact_outgoing: false,
            },
        ];
        s.doc.next_id = 12;
        let before = s.doc.clone();
        let mut b = TweeqBridge::default();
        let r = b.revision(&s, "path");
        b.update(
            &mut s,
            "path",
            &message("change", &r, "path.scale", json!(2.)),
        )
        .unwrap();
        b.update(
            &mut s,
            "path",
            &message("commit", &r, "path.scale", json!(3.)),
        )
        .unwrap();
        let p = s.active().unwrap();
        assert!((p.nodes[1].p.x - p.nodes[0].p.x - 30.).abs() < 1e-8);
        s.undo();
        assert_eq!(s.doc, before);
    }
    #[test]
    fn connected_parameters_and_fractional_seeds_are_rejected() {
        let mut s = session();
        let mut graph = Graph::default();
        let id = graph.add(Kind::Rough, [100., 100.]).unwrap();
        s.active_mut().unwrap().appearance = Some(graph);
        s.node_selection = vec![id];
        assert!(apply(&mut s, "param:Seed", &json!(1.5)).is_err());
        s.graph_mut()
            .unwrap()
            .node_mut(id)
            .unwrap()
            .bindings
            .push((Parameter::Amount, 999));
        assert!(!field_allowed(&s, "node", "param:Amount"));
        assert!(apply(&mut s, "param:Amount", &json!(10.)).is_err());
    }
}
