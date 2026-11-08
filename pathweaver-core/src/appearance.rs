//! Non-destructive, deterministic appearance graphs inspired by mv-text-creator.
use super::appearance_effects::{EffectKind, EffectParams};
use kurbo::{Point, Vec2};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

const MAX_NODES: usize = 64;
const MAX_VERTICES: usize = 200_000;
const MAX_LAYERS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    Input,
    Rough,
    Zigzag,
    Transform,
    Fill,
    Stack,
    Output,
    Reroute,
    Pathfinder,
    Boolean,
    Offset,
    Round,
    Band,
    Pucker,
    Simplify,
    Streak,
    Dissolve,
    Halftone,
    Opacity,
    Shadow,
    Glow,
    Blur,
    Value,
    Math,
    Measure,
}
impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Input => "元の外観",
            Self::Rough => "ラフ",
            Self::Zigzag => "ジグザグ",
            Self::Transform => "変形",
            Self::Fill => "色",
            Self::Stack => "重ねる",
            Self::Output => "出力",
            Self::Reroute => "中継",
            Self::Pathfinder => "パスファインダー",
            Self::Boolean => "形の合成",
            Self::Offset => "パスのオフセット",
            Self::Round => "角を丸くする",
            Self::Band => "縁取りの形",
            Self::Pucker => "パンク・膨張",
            Self::Simplify => "単純化",
            Self::Streak => "かすれ",
            Self::Dissolve => "エッジを溶かす",
            Self::Halftone => "ハーフトーン",
            Self::Value => "値",
            Self::Math => "計算",
            Self::Measure => "形の大きさ",
            Self::Opacity => "不透明度",
            Self::Shadow => "ドロップシャドウ",
            Self::Glow => "光彩",
            Self::Blur => "ぼかし",
        }
    }
    pub fn is_number(self) -> bool {
        matches!(self, Self::Value | Self::Math | Self::Measure)
    }
    pub fn inputs(self) -> usize {
        match self {
            Self::Input | Self::Value | Self::Math => 0,
            Self::Stack | Self::Boolean => 2,
            _ => 1,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Parameter {
    Amount,
    Detail,
    Seed,
    Angle,
    Spread,
    Opacity,
    Blur,
    X,
    Y,
    Rotation,
    Scale,
    Value,
    A,
    B,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MathOperation {
    Add,
    Subtract,
    Multiply,
    Divide,
    Min,
    Max,
    Power,
    Modulo,
    Snap,
}
impl MathOperation {
    pub fn label(self) -> &'static str {
        match self {
            Self::Add => "足す",
            Self::Subtract => "引く",
            Self::Multiply => "掛ける",
            Self::Divide => "割る",
            Self::Min => "最小",
            Self::Max => "最大",
            Self::Power => "べき乗",
            Self::Modulo => "余り",
            Self::Snap => "刻みで丸める",
        }
    }
    fn calculate(self, a: f64, b: f64) -> f64 {
        let value = match self {
            Self::Add => a + b,
            Self::Subtract => a - b,
            Self::Multiply => a * b,
            Self::Divide => {
                if b == 0. {
                    0.
                } else {
                    a / b
                }
            }
            Self::Min => a.min(b),
            Self::Max => a.max(b),
            Self::Power => a.powf(b),
            Self::Modulo => {
                if b == 0. {
                    0.
                } else {
                    a % b
                }
            }
            Self::Snap => {
                if b == 0. {
                    a
                } else {
                    (a / b).round() * b
                }
            }
        };
        if value.is_finite() {
            value
        } else {
            0.
        }
    }
}
pub struct ParameterSpec {
    pub key: Parameter,
    pub label: &'static str,
    pub min: f64,
    pub max: f64,
}
impl ParameterSpec {
    fn new(key: Parameter, label: &'static str, min: f64, max: f64) -> Self {
        Self {
            key,
            label,
            min,
            max,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Operation {
    Union,
    Difference,
    Intersect,
    Xor,
}
impl Operation {
    pub fn label(self) -> &'static str {
        match self {
            Self::Union => "合体",
            Self::Difference => "前面で型抜き",
            Self::Intersect => "交差",
            Self::Xor => "中マド",
        }
    }
    fn rule(self) -> i_overlay::core::overlay_rule::OverlayRule {
        use i_overlay::core::overlay_rule::OverlayRule;
        match self {
            Self::Union => OverlayRule::Union,
            Self::Difference => OverlayRule::Difference,
            Self::Intersect => OverlayRule::Intersect,
            Self::Xor => OverlayRule::Xor,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Node {
    #[serde(default)]
    pub label: String,
    pub id: u64,
    pub kind: Kind,
    pub position: [f32; 2],
    pub inputs: Vec<Option<u64>>,
    pub bypass: bool,
    pub amount: f64,
    pub detail: f64,
    pub seed: u32,
    pub smooth: bool,
    pub translation: [f64; 2],
    pub rotation: f64,
    pub scale: f64,
    pub color: [u8; 3],
    pub opacity: f64,
    pub blur: f64,
    pub spread: f64,
    pub inner: bool,
    pub effect: EffectParams,
    pub operation: Operation,
    pub value: f64,
    pub a: f64,
    pub b: f64,
    pub math: MathOperation,
    pub measure_height: bool,
    pub bindings: Vec<(Parameter, u64)>,
}
impl Node {
    pub fn new(id: u64, kind: Kind, position: [f32; 2]) -> Self {
        let effect = EffectParams {
            amount: match kind {
                Kind::Halftone | Kind::Dissolve => 40.,
                Kind::Streak => 25.,
                Kind::Pucker | Kind::Band => 20.,
                Kind::Simplify => 2.,
                _ => 10.,
            },
            detail: match kind {
                Kind::Halftone => 0.3,
                Kind::Dissolve => 8.,
                _ => 2.,
            },
            angle: if kind == Kind::Halftone { 15. } else { 0. },
            ..Default::default()
        };
        Self {
            label: String::new(),
            id,
            kind,
            position,
            inputs: vec![None; kind.inputs()],
            bypass: false,
            amount: if kind == Kind::Zigzag { 8. } else { 3. },
            detail: 6.,
            seed: 1,
            smooth: true,
            translation: if kind == Kind::Shadow {
                [20., 20.]
            } else {
                [0., 0.]
            },
            rotation: 0.,
            scale: 1.,
            color: if kind == Kind::Shadow {
                [70; 3]
            } else if kind == Kind::Glow {
                [130, 200, 255]
            } else {
                [224; 3]
            },
            opacity: 1.,
            blur: if kind == Kind::Blur { 8. } else { 0. },
            spread: 0.,
            inner: false,
            effect,
            operation: Operation::Union,
            value: 10.,
            a: 10.,
            b: 1.,
            math: MathOperation::Add,
            measure_height: false,
            bindings: vec![],
        }
    }

    pub fn parameters(&self) -> Vec<ParameterSpec> {
        use Parameter::*;
        let spec = ParameterSpec::new;
        match self.kind {
            Kind::Value => vec![spec(Value, "値", -1e6, 1e6)],
            Kind::Math => vec![spec(A, "A", -1e6, 1e6), spec(B, "B", -1e6, 1e6)],
            Kind::Rough => vec![
                spec(Amount, "大きさ", 0., 1000.),
                spec(Detail, "細かさ", 0.1, 100.),
                spec(Seed, "シード", 0., u32::MAX as f64),
            ],
            Kind::Zigzag => vec![
                spec(Amount, "大きさ", 0., 1000.),
                spec(Detail, "細かさ", 0.1, 100.),
            ],
            Kind::Transform => vec![
                spec(X, "横移動", -100_000., 100_000.),
                spec(Y, "縦移動", -100_000., 100_000.),
                spec(Rotation, "回転", -3600., 3600.),
                spec(Scale, "拡大率", 0.01, 100.),
            ],
            Kind::Fill | Kind::Opacity => vec![spec(Opacity, "不透明度", 0., 1.)],
            Kind::Blur => vec![spec(Blur, "ぼかし半径", 0., 100.)],
            Kind::Shadow => vec![
                spec(X, "横ずれ", -100_000., 100_000.),
                spec(Y, "縦ずれ", -100_000., 100_000.),
                spec(Spread, "広がり", -1000., 1000.),
                spec(Opacity, "不透明度", 0., 1.),
                spec(Blur, "ぼかし", 0., 100.),
            ],
            Kind::Glow => vec![
                spec(Amount, "大きさ", 0., 100.),
                spec(Opacity, "不透明度", 0., 1.),
            ],
            Kind::Offset => vec![spec(Amount, "距離", -1000., 1000.)],
            Kind::Round => vec![spec(Amount, "半径", 0., 1000.)],
            Kind::Band => vec![spec(Amount, "幅", 0., 1000.)],
            Kind::Pucker => vec![spec(Amount, "膨張（%）", -100., 100.)],
            Kind::Simplify => vec![spec(Amount, "許容距離", 0.1, 100.)],
            Kind::Streak => vec![
                spec(Amount, "本数", 0., 100.),
                spec(Detail, "細さ", 0.2, 20.),
                spec(Angle, "角度", -180., 180.),
                spec(Seed, "シード", 0., u32::MAX as f64),
            ],
            Kind::Dissolve => vec![
                spec(Amount, "量（%）", 0., 100.),
                spec(Detail, "粒の大きさ", 1., 60.),
                spec(Spread, "散らばり", 0., 100.),
                spec(Seed, "シード", 0., u32::MAX as f64),
            ],
            Kind::Halftone => vec![
                spec(Amount, "間隔", 8., 200.),
                spec(Detail, "点の半径比", 0.05, 0.5),
                spec(Angle, "角度", -180., 180.),
            ],
            _ => vec![],
        }
    }
    fn geometry_effect(&self) -> bool {
        matches!(
            self.kind,
            Kind::Offset
                | Kind::Round
                | Kind::Band
                | Kind::Pucker
                | Kind::Simplify
                | Kind::Streak
                | Kind::Dissolve
                | Kind::Halftone
        )
    }
    pub fn number(&self, key: Parameter) -> f64 {
        use Parameter::*;
        match key {
            Amount => {
                if self.geometry_effect() {
                    self.effect.amount
                } else {
                    self.amount
                }
            }
            Detail => {
                if self.geometry_effect() {
                    self.effect.detail
                } else {
                    self.detail
                }
            }
            Seed => {
                if self.geometry_effect() {
                    self.effect.seed as f64
                } else {
                    self.seed as f64
                }
            }
            Angle => self.effect.angle,
            Spread => {
                if self.kind == Kind::Dissolve {
                    self.effect.spread
                } else {
                    self.spread
                }
            }
            Opacity => self.opacity,
            Blur => self.blur,
            X => self.translation[0],
            Y => self.translation[1],
            Rotation => self.rotation,
            Scale => self.scale,
            Value => self.value,
            A => self.a,
            B => self.b,
        }
    }
    pub fn set_number(&mut self, key: Parameter, value: f64) {
        let Some(spec) = self.parameters().into_iter().find(|s| s.key == key) else {
            return;
        };
        let value = if value.is_finite() {
            value.clamp(spec.min, spec.max)
        } else {
            0_f64.clamp(spec.min, spec.max)
        };
        use Parameter::*;
        match key {
            Amount => {
                if self.geometry_effect() {
                    self.effect.amount = value
                } else {
                    self.amount = value
                }
            }
            Detail => {
                if self.geometry_effect() {
                    self.effect.detail = value
                } else {
                    self.detail = value
                }
            }
            Seed => {
                if self.geometry_effect() {
                    self.effect.seed = value as u32
                } else {
                    self.seed = value as u32
                }
            }
            Angle => self.effect.angle = value,
            Spread => {
                if self.kind == Kind::Dissolve {
                    self.effect.spread = value
                } else {
                    self.spread = value
                }
            }
            Opacity => self.opacity = value,
            Blur => self.blur = value,
            X => self.translation[0] = value,
            Y => self.translation[1] = value,
            Rotation => self.rotation = value,
            Scale => self.scale = value,
            Value => self.value = value,
            A => self.a = value,
            B => self.b = value,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Graph {
    #[serde(default)]
    pub name: String,
    pub enabled: bool,
    pub nodes: Vec<Node>,
    pub output: u64,
}
impl Default for Graph {
    fn default() -> Self {
        let input = Node::new(1, Kind::Input, [30., 90.]);
        let mut output = Node::new(2, Kind::Output, [520., 90.]);
        output.inputs[0] = Some(1);
        Self {
            name: String::new(),
            enabled: true,
            nodes: vec![input, output],
            output: 2,
        }
    }
}
impl Graph {
    /// Node positions are saved/undoable but do not change the rendered artwork.
    pub fn same_rendering(&self, other: &Self) -> bool {
        self.enabled == other.enabled
            && self.output == other.output
            && self.nodes.len() == other.nodes.len()
            && self.nodes.iter().zip(&other.nodes).all(|(a, b)| {
                let mut a = a.clone();
                let mut b = b.clone();
                a.label.clear();
                b.label.clear();
                a.position = [0., 0.];
                b.position = [0., 0.];
                a == b
            })
    }
    pub fn rough() -> Self {
        let mut graph = Self::default();
        let id = graph.add(Kind::Rough, [270., 90.]).unwrap();
        graph.connect(1, id, 0).unwrap();
        graph.connect(id, graph.output, 0).unwrap();
        graph
    }
    pub fn node(&self, id: u64) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }
    pub fn node_mut(&mut self, id: u64) -> Option<&mut Node> {
        self.nodes.iter_mut().find(|n| n.id == id)
    }
    pub fn add(&mut self, kind: Kind, position: [f32; 2]) -> Result<u64, String> {
        if self.nodes.len() >= MAX_NODES {
            return Err("ノードは64個までです".into());
        }
        if matches!(kind, Kind::Input | Kind::Output) {
            return Err("入力と出力は追加できません".into());
        }
        let id = self
            .nodes
            .iter()
            .map(|n| n.id)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or("ノードIDが不正です")?;
        self.nodes.push(Node::new(id, kind, position));
        Ok(id)
    }
    pub fn connect(&mut self, from: u64, to: u64, port: usize) -> Result<(), String> {
        if self
            .node(from)
            .is_none_or(|n| n.kind == Kind::Output || n.kind.is_number())
        {
            return Err("接続元が不正です".into());
        }
        let target = self.node_mut(to).ok_or("接続先がありません")?;
        let input = target
            .inputs
            .get_mut(port)
            .ok_or("接続先ソケットがありません")?;
        let old = input.replace(from);
        if let Err(error) = self.validate() {
            self.node_mut(to).unwrap().inputs[port] = old;
            return Err(error);
        }
        Ok(())
    }
    pub fn connect_parameter(
        &mut self,
        from: u64,
        to: u64,
        parameter: Parameter,
    ) -> Result<(), String> {
        if self.node(from).is_none_or(|n| !n.kind.is_number()) {
            return Err("数値ソケットには数値ノードを接続してください".into());
        }
        let target = self.node_mut(to).ok_or("接続先がありません")?;
        if !target.parameters().iter().any(|s| s.key == parameter) {
            return Err("数値パラメータがありません".into());
        }
        let old = target.bindings.clone();
        target.bindings.retain(|(p, _)| *p != parameter);
        target.bindings.push((parameter, from));
        if let Err(error) = self.validate() {
            self.node_mut(to).unwrap().bindings = old;
            return Err(error);
        }
        Ok(())
    }
    pub fn remove(&mut self, id: u64) {
        if self
            .node(id)
            .is_none_or(|n| matches!(n.kind, Kind::Input | Kind::Output))
        {
            return;
        }
        self.nodes.retain(|n| n.id != id);
        for n in &mut self.nodes {
            for input in &mut n.inputs {
                if *input == Some(id) {
                    *input = None;
                }
            }
            n.bindings.retain(|(_, source)| *source != id);
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.name.chars().count() > 80 || self.nodes.iter().any(|n| n.label.chars().count() > 80) {
            return Err("ノードとセットの名前は80文字以内で指定してください".into());
        }
        if self.nodes.len() > MAX_NODES
            || self.nodes.iter().filter(|n| n.kind == Kind::Input).count() != 1
            || self.nodes.iter().filter(|n| n.kind == Kind::Output).count() != 1
            || self
                .node(self.output)
                .is_none_or(|n| n.kind != Kind::Output)
        {
            return Err("外観グラフの入力・出力が不正です".into());
        }
        let mut ids = HashSet::new();
        for n in &self.nodes {
            if n.id == 0
                || !ids.insert(n.id)
                || n.inputs.len() != n.kind.inputs()
                || n.position
                    .iter()
                    .any(|v| !v.is_finite() || v.abs() > 100_000.)
                || !n.amount.is_finite()
                || !(0. ..=1000.).contains(&n.amount)
                || !n.detail.is_finite()
                || !(0.1..=100.).contains(&n.detail)
                || n.translation
                    .iter()
                    .any(|v| !v.is_finite() || v.abs() > 100_000.)
                || !n.rotation.is_finite()
                || n.rotation.abs() > 3600.
                || !n.scale.is_finite()
                || !(0.01..=100.).contains(&n.scale)
                || !n.opacity.is_finite()
                || !(0. ..=1.).contains(&n.opacity)
                || !n.blur.is_finite()
                || !(0. ..=100.).contains(&n.blur)
                || !n.spread.is_finite()
                || n.spread.abs() > 1000.
                || [
                    n.effect.amount,
                    n.effect.detail,
                    n.effect.angle,
                    n.effect.spread,
                ]
                .iter()
                .any(|v| !v.is_finite() || v.abs() > 10000.)
            {
                return Err("外観ノードの設定が不正です".into());
            }
            if [n.value, n.a, n.b]
                .iter()
                .any(|v| !v.is_finite() || v.abs() > 1e6)
            {
                return Err("数値ノードの値が不正です".into());
            }
            let mut bound = HashSet::new();
            for (parameter, id) in &n.bindings {
                if !bound.insert(*parameter)
                    || !n.parameters().iter().any(|s| s.key == *parameter)
                    || self.node(*id).is_none_or(|n| !n.kind.is_number())
                {
                    return Err("数値パラメータの接続が不正です".into());
                }
            }
            for id in n.inputs.iter().flatten() {
                if self
                    .node(*id)
                    .is_none_or(|n| n.kind == Kind::Output || n.kind.is_number())
                {
                    return Err("外観ノードの接続先がありません".into());
                }
            }
        }
        fn visit(
            graph: &Graph,
            id: u64,
            visiting: &mut HashSet<u64>,
            done: &mut HashSet<u64>,
        ) -> Result<(), String> {
            if done.contains(&id) {
                return Ok(());
            }
            if !visiting.insert(id) {
                return Err("循環する接続はできません".into());
            }
            let node = graph.node(id).unwrap();
            for input in node
                .inputs
                .iter()
                .flatten()
                .chain(node.bindings.iter().map(|(_, id)| id))
            {
                visit(graph, *input, visiting, done)?;
            }
            visiting.remove(&id);
            done.insert(id);
            Ok(())
        }
        let mut done = HashSet::new();
        for n in &self.nodes {
            visit(self, n.id, &mut HashSet::new(), &mut done)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub color: [u8; 3],
    pub opacity: f64,
    pub contours: Vec<Vec<Point>>,
}

#[derive(Clone)]
struct Value {
    layers: Vec<Layer>,
    number: f64,
}

pub fn evaluate(graph: &Graph, original: &[Layer]) -> Result<Vec<Layer>, String> {
    if !graph.enabled {
        graph.validate()?;
        return Ok(original.to_vec());
    }
    let mut values = evaluate_values(graph, original, &[graph.output])?;
    Ok(values.remove(&graph.output).unwrap().layers)
}

/// Effective values of connected parameters, using the rendering evaluator.
/// Only binding sources and their upstream dependencies are evaluated; target
/// effects and unrelated disconnected branches are not rendered for inspection.
pub fn parameter_values(
    graph: &Graph,
    original: &[Layer],
) -> Result<HashMap<(u64, Parameter), f64>, String> {
    let sources: Vec<_> = graph.nodes.iter()
        .flat_map(|node| node.bindings.iter().map(|(_, source)| *source)).collect();
    let values = evaluate_values(graph, original, &sources)?;
    let mut parameters = HashMap::new();
    for node in &graph.nodes {
        let mut effective = node.clone();
        for &(parameter, source) in &node.bindings {
            // The target owns range limits and integer conversion (e.g. Seed).
            effective.set_number(parameter, values[&source].number);
            parameters.insert((node.id, parameter), effective.number(parameter));
        }
    }
    Ok(parameters)
}

fn evaluate_values(
    graph: &Graph,
    original: &[Layer],
    requested: &[u64],
) -> Result<HashMap<u64, Value>, String> {
    graph.validate()?;
    fn visit(
        graph: &Graph,
        id: u64,
        original: &[Layer],
        cache: &mut HashMap<u64, Value>,
    ) -> Result<Value, String> {
        if let Some(value) = cache.get(&id) {
            return Ok(value.clone());
        }
        let mut node = graph.node(id).unwrap().clone();
        for (parameter, source) in node.bindings.clone() {
            let value = visit(graph, source, original, cache)?.number;
            node.set_number(parameter, value);
        }
        let mut inputs = Vec::new();
        for input in &node.inputs {
            inputs.push(match input {
                Some(id) => visit(graph, *id, original, cache)?.layers,
                None => vec![],
            });
        }
        let mut layers = if node.kind == Kind::Input {
            original.to_vec()
        } else if node.kind == Kind::Stack && !node.bypass {
            inputs.clone().into_iter().flatten().collect()
        } else {
            inputs.first().cloned().unwrap_or_default()
        };
        if !node.bypass {
            match node.kind {
                Kind::Rough | Kind::Zigzag if node.amount > 0. => {
                    let mut generated = 0;
                    for (layer_index, layer) in layers.iter_mut().enumerate() {
                        for (ring_index, ring) in layer.contours.iter_mut().enumerate() {
                            *ring = displace(ring, &node, (layer_index * 997 + ring_index) as u32);
                            generated += ring.len();
                            if generated > MAX_VERTICES {
                                return Err(
                                    "ラフの輪郭が複雑すぎます。細かさを減らしてください".into()
                                );
                            }
                        }
                        layer.contours = super::outline::boolean_contours(
                            &layer.contours,
                            &[],
                            i_overlay::core::overlay_rule::OverlayRule::Subject,
                        );
                    }
                }
                Kind::Transform => {
                    let points: Vec<_> = layers
                        .iter()
                        .flat_map(|l| l.contours.iter().flatten())
                        .collect();
                    if let Some(first) = points.first() {
                        let mut min = **first;
                        let mut max = min;
                        for p in &points {
                            min.x = min.x.min(p.x);
                            min.y = min.y.min(p.y);
                            max.x = max.x.max(p.x);
                            max.y = max.y.max(p.y);
                        }
                        let center = min.lerp(max, 0.5);
                        let angle = node.rotation.to_radians();
                        for p in layers
                            .iter_mut()
                            .flat_map(|l| l.contours.iter_mut().flatten())
                        {
                            let d = (*p - center) * node.scale;
                            *p = center
                                + Vec2::new(
                                    d.x * angle.cos() - d.y * angle.sin() + node.translation[0],
                                    d.x * angle.sin() + d.y * angle.cos() + node.translation[1],
                                );
                        }
                    }
                }
                Kind::Fill => {
                    for layer in &mut layers {
                        layer.color = node.color;
                        layer.opacity *= node.opacity;
                    }
                }
                Kind::Opacity => {
                    layers = super::appearance_paint::opacity(&layers, node.opacity)?;
                }
                Kind::Blur => {
                    layers = super::appearance_paint::blur(&layers, node.blur)?;
                }
                Kind::Shadow => {
                    layers = super::appearance_paint::shadow(
                        &layers,
                        node.translation[0],
                        node.translation[1],
                        node.spread,
                        node.color,
                        node.opacity,
                        node.blur,
                    )?;
                }
                Kind::Glow => {
                    layers = super::appearance_paint::glow(
                        &layers,
                        node.amount,
                        node.color,
                        node.opacity,
                        node.inner,
                    )?;
                }
                Kind::Pathfinder | Kind::Boolean => {
                    let base_color = layers.first().map(|l| l.color).unwrap_or([224; 3]);
                    let base_opacity = layers.first().map(|l| l.opacity).unwrap_or(1.);
                    let mut groups: Vec<Vec<Vec<Point>>> = if node.kind == Kind::Pathfinder {
                        layers.iter().map(|l| l.contours.clone()).collect()
                    } else {
                        inputs
                            .iter()
                            .map(|input| input.iter().flat_map(|l| l.contours.clone()).collect())
                            .collect()
                    };
                    let mut contours = if groups.is_empty() {
                        vec![]
                    } else {
                        groups.remove(0)
                    };
                    for group in groups {
                        contours = super::outline::boolean_contours(
                            &contours,
                            &group,
                            node.operation.rule(),
                        );
                    }
                    layers = vec![Layer {
                        color: base_color,
                        opacity: base_opacity,
                        contours,
                    }];
                }
                kind => {
                    let effect = match kind {
                        Kind::Offset => Some(EffectKind::Offset),
                        Kind::Round => Some(EffectKind::Round),
                        Kind::Band => Some(EffectKind::Band),
                        Kind::Pucker => Some(EffectKind::Pucker),
                        Kind::Simplify => Some(EffectKind::Simplify),
                        Kind::Streak => Some(EffectKind::Streak),
                        Kind::Dissolve => Some(EffectKind::Dissolve),
                        Kind::Halftone => Some(EffectKind::Halftone),
                        _ => None,
                    };
                    if let Some(kind) = effect {
                        for layer in &mut layers {
                            layer.contours = super::appearance_effects::apply_with_params(
                                kind,
                                &layer.contours,
                                &node.effect,
                            )?;
                        }
                    }
                }
            }
        }
        if layers.len() > MAX_LAYERS
            || layers
                .iter()
                .flat_map(|l| &l.contours)
                .map(Vec::len)
                .sum::<usize>()
                > MAX_VERTICES
        {
            return Err(
                "外観の計算量が上限を超えました。ノード数や細かさを減らしてください".into(),
            );
        }
        if layers
            .iter()
            .flat_map(|l| l.contours.iter().flatten())
            .any(|p| !p.x.is_finite() || !p.y.is_finite() || p.x.abs().max(p.y.abs()) > 1e6)
        {
            return Err("外観の座標が範囲を超えました".into());
        }
        let number = match node.kind {
            Kind::Value => node.value,
            Kind::Math => {
                if node.bypass {
                    node.a
                } else {
                    node.math.calculate(node.a, node.b)
                }
            }
            Kind::Measure => {
                let mut min = f64::INFINITY;
                let mut max = f64::NEG_INFINITY;
                for p in layers.iter().flat_map(|l| l.contours.iter().flatten()) {
                    let v = if node.measure_height { p.y } else { p.x };
                    min = min.min(v);
                    max = max.max(v);
                }
                if min.is_finite() {
                    max - min
                } else {
                    0.
                }
            }
            _ => 0.,
        };
        let result = Value { layers, number };
        cache.insert(id, result.clone());
        Ok(result)
    }
    let mut values = HashMap::new();
    for &id in requested {
        visit(graph, id, original, &mut values)?;
    }
    Ok(values)
}

fn random(seed: u32, index: usize) -> f64 {
    let mut x = seed.wrapping_add((index as u32).wrapping_mul(0x9e3779b9));
    x ^= x >> 16;
    x = x.wrapping_mul(0x85ebca6b);
    x ^= x >> 13;
    x = x.wrapping_mul(0xc2b2ae35);
    x ^= x >> 16;
    f64::from(x) / f64::from(u32::MAX) * 2. - 1.
}
fn periodic_noise(t: f64, count: usize, seed: u32, smooth: bool) -> f64 {
    let x = t * count as f64;
    let a = x.floor() as usize % count;
    let f = x.fract();
    let f = if smooth { f * f * (3. - 2. * f) } else { f };
    random(seed, a) * (1. - f) + random(seed, (a + 1) % count) * f
}
fn displace(ring: &[Point], node: &Node, salt: u32) -> Vec<Point> {
    if ring.len() < 3 {
        return ring.to_vec();
    }
    let mut lengths = Vec::with_capacity(ring.len());
    let mut total = 0.;
    for (a, b) in ring
        .iter()
        .zip(ring.iter().cycle().skip(1))
        .take(ring.len())
    {
        total += a.distance(*b);
        lengths.push(total);
    }
    if total < 1e-9 {
        return ring.to_vec();
    }
    let waves = (total * node.detail / 100.).ceil().clamp(3., 2500.) as usize;
    let count = if node.kind == Kind::Zigzag {
        (waves * 2).min(20000)
    } else {
        (waves * if node.smooth { 8 } else { 1 }).clamp(8, 20000)
    };
    let mut points = Vec::with_capacity(count);
    let mut segment = 0;
    let mut start = 0.;
    for i in 0..count {
        let s = total * i as f64 / count as f64;
        while segment < ring.len() - 1 && lengths[segment] <= s {
            start = lengths[segment];
            segment += 1;
        }
        let length = lengths[segment] - start;
        points.push(ring[segment].lerp(
            ring[(segment + 1) % ring.len()],
            if length > 1e-12 {
                (s - start) / length
            } else {
                0.
            },
        ));
    }
    let orientation = if ring
        .iter()
        .zip(ring.iter().cycle().skip(1))
        .take(ring.len())
        .map(|(a, b)| a.x * b.y - b.x * a.y)
        .sum::<f64>()
        < 0.
    {
        -1.
    } else {
        1.
    };
    points
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let tangent = points[(i + 1) % count] - points[(i + count - 1) % count];
            let noise = if node.kind == Kind::Zigzag {
                if i % 2 == 0 {
                    1.
                } else {
                    -1.
                }
            } else {
                periodic_noise(
                    i as f64 / count as f64,
                    waves,
                    node.seed.wrapping_add(salt),
                    node.smooth,
                )
            };
            *p + Vec2::new(tangent.y, -tangent.x) / tangent.hypot().max(1e-12)
                * (node.amount * noise * orientation)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn layers() -> Vec<Layer> {
        vec![Layer {
            color: [12, 34, 56],
            opacity: 1.,
            contours: vec![vec![
                Point::new(0., 0.),
                Point::new(100., 0.),
                Point::new(100., 100.),
                Point::new(0., 100.),
            ]],
        }]
    }
    #[test]
    fn inspected_parameters_share_numeric_clamping_and_integer_conversion() {
        let mut graph = Graph::rough();
        let value = graph.add(Kind::Value, [0., 0.]).unwrap();
        graph.node_mut(value).unwrap().value = 1234.75;
        graph.connect_parameter(value, 3, Parameter::Amount).unwrap();
        graph.connect_parameter(value, 3, Parameter::Seed).unwrap();
        let local = graph.node(3).unwrap().amount;
        let values = parameter_values(&graph, &layers()).unwrap();
        assert_eq!(values[&(3, Parameter::Amount)], 1000.);
        assert_eq!(values[&(3, Parameter::Seed)], 1234.);
        assert_eq!(graph.node(3).unwrap().amount, local);
        graph.node_mut(value).unwrap().value = -99.;
        let values = parameter_values(&graph, &layers()).unwrap();
        assert_eq!(values[&(3, Parameter::Amount)], 0.);
        assert_eq!(values[&(3, Parameter::Seed)], 0.);
    }
    #[test]
    fn inspected_measure_and_math_follow_upstream_geometry_and_bypass() {
        let mut graph = Graph::rough();
        let transform = graph.add(Kind::Transform, [0., 0.]).unwrap();
        graph.node_mut(transform).unwrap().scale = 2.;
        graph.connect(1, transform, 0).unwrap();
        let measure = graph.add(Kind::Measure, [0., 0.]).unwrap();
        graph.connect(transform, measure, 0).unwrap();
        let math = graph.add(Kind::Math, [0., 0.]).unwrap();
        graph.node_mut(math).unwrap().math = MathOperation::Divide;
        graph.node_mut(math).unwrap().b = 2.;
        graph.connect_parameter(measure, math, Parameter::A).unwrap();
        graph.connect_parameter(math, 3, Parameter::Amount).unwrap();
        let values = parameter_values(&graph, &layers()).unwrap();
        assert_eq!(values[&(math, Parameter::A)], 200.);
        assert_eq!(values[&(3, Parameter::Amount)], 100.);
        let mut direct = Graph::rough();
        direct.node_mut(3).unwrap().amount = values[&(3, Parameter::Amount)];
        assert_eq!(evaluate(&graph, &layers()).unwrap(), evaluate(&direct, &layers()).unwrap());
        graph.node_mut(transform).unwrap().bypass = true;
        assert_eq!(parameter_values(&graph, &layers()).unwrap()[&(3, Parameter::Amount)], 50.);
        graph.node_mut(math).unwrap().bypass = true;
        assert_eq!(parameter_values(&graph, &layers()).unwrap()[&(3, Parameter::Amount)], 100.);
    }
    #[test]
    fn parameter_inspection_skips_unrelated_rendering_but_rejects_cycles() {
        let mut graph = Graph::rough();
        let value = graph.add(Kind::Value, [0., 0.]).unwrap();
        graph.node_mut(value).unwrap().value = 42.;
        graph.connect_parameter(value, 3, Parameter::Amount).unwrap();
        let transform = graph.add(Kind::Transform, [0., 0.]).unwrap();
        graph.node_mut(transform).unwrap().translation[0] = 100_000.;
        graph.connect(1, transform, 0).unwrap();
        graph.connect(transform, 2, 0).unwrap();
        let mut input = layers();
        for point in input.iter_mut().flat_map(|l| l.contours.iter_mut().flatten()) {
            point.x += 999_000.;
        }
        assert!(evaluate(&graph, &input).is_err());
        assert_eq!(parameter_values(&graph, &input).unwrap()[&(3, Parameter::Amount)], 42.);
        graph.node_mut(value).unwrap().bindings.push((Parameter::Value, value));
        assert!(parameter_values(&graph, &input).is_err());
    }
    #[test]
    fn rough_is_deterministic_and_bypass_preserves_original() {
        let input = layers();
        let mut graph = Graph::rough();
        let a = evaluate(&graph, &input).unwrap();
        assert_eq!(a, evaluate(&graph, &input).unwrap());
        assert_ne!(a, input);
        graph.node_mut(3).unwrap().seed += 1;
        assert_ne!(a, evaluate(&graph, &input).unwrap());
        graph.node_mut(3).unwrap().bypass = true;
        assert_eq!(input, evaluate(&graph, &input).unwrap());
        graph.enabled = false;
        assert_eq!(input, evaluate(&graph, &input).unwrap());
    }
    #[test]
    fn graph_rejects_cycles_missing_nodes_and_nonfinite_parameters() {
        let mut graph = Graph::rough();
        let extra = graph.add(Kind::Fill, [0., 0.]).unwrap();
        graph.connect(3, extra, 0).unwrap();
        let before = graph.clone();
        assert!(graph.connect(extra, 3, 0).is_err());
        assert_eq!(graph, before);
        graph.node_mut(3).unwrap().inputs[0] = Some(999);
        assert!(graph.validate().is_err());
        graph.node_mut(3).unwrap().inputs[0] = Some(1);
        graph.node_mut(3).unwrap().amount = f64::NAN;
        assert!(graph.validate().is_err());
    }
    #[test]
    fn branches_stack_in_order_and_delete_disconnects() {
        let mut graph = Graph::default();
        let fill = graph.add(Kind::Fill, [0., 0.]).unwrap();
        let stack = graph.add(Kind::Stack, [0., 0.]).unwrap();
        graph.node_mut(fill).unwrap().color = [255, 0, 0];
        graph.connect(1, fill, 0).unwrap();
        graph.connect(1, stack, 0).unwrap();
        graph.connect(fill, stack, 1).unwrap();
        graph.connect(stack, 2, 0).unwrap();
        let result = evaluate(&graph, &layers()).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[1].color, [255, 0, 0]);
        graph.remove(fill);
        assert_eq!(graph.node(stack).unwrap().inputs[1], None);
        assert_eq!(evaluate(&graph, &layers()).unwrap().len(), 1);
        graph.remove(1);
        assert!(graph.node(1).is_some());
    }
    #[test]
    fn zero_rough_and_empty_input_are_safe_and_graph_roundtrips() {
        let mut graph = Graph::rough();
        graph.node_mut(3).unwrap().amount = 0.;
        assert_eq!(evaluate(&graph, &layers()).unwrap(), layers());
        assert!(evaluate(&graph, &[]).unwrap().is_empty());
        let loaded: Graph = serde_json::from_str(&serde_json::to_string(&graph).unwrap()).unwrap();
        assert_eq!(graph, loaded);
    }
    #[test]
    fn numeric_nodes_bind_parameters_reject_wrong_types_and_detect_numeric_cycles() {
        let mut graph = Graph::rough();
        let measure = graph.add(Kind::Measure, [0., 0.]).unwrap();
        graph.connect(1, measure, 0).unwrap();
        let math = graph.add(Kind::Math, [0., 0.]).unwrap();
        graph.node_mut(math).unwrap().math = MathOperation::Divide;
        graph.node_mut(math).unwrap().b = 10.;
        graph
            .connect_parameter(measure, math, Parameter::A)
            .unwrap();
        graph.connect_parameter(math, 3, Parameter::Amount).unwrap();
        let result = evaluate(&graph, &layers()).unwrap();
        let mut direct = Graph::rough();
        direct.node_mut(3).unwrap().amount = 10.;
        assert_eq!(result, evaluate(&direct, &layers()).unwrap());
        assert!(graph.connect(math, 2, 0).is_err());
        assert!(graph.connect_parameter(1, 3, Parameter::Amount).is_err());
        assert!(graph.connect(3, measure, 0).is_err());
        let value = graph.add(Kind::Value, [0., 0.]).unwrap();
        assert!(graph
            .connect_parameter(value, value, Parameter::Value)
            .is_err());
        graph.node_mut(math).unwrap().b = 0.;
        assert_eq!(evaluate(&graph, &layers()).unwrap(), layers());
    }
    #[test]
    fn layout_does_not_change_rendering_but_parameters_and_links_do() {
        let graph = Graph::rough();
        let mut moved = graph.clone();
        moved.node_mut(3).unwrap().position = [300., 400.];
        assert!(graph.same_rendering(&moved));
        moved.node_mut(3).unwrap().amount += 1.;
        assert!(!graph.same_rendering(&moved));
        let mut changed = graph.clone();
        changed.node_mut(2).unwrap().inputs[0] = Some(1);
        assert!(!graph.same_rendering(&changed));
    }
    #[test]
    fn all_visual_node_kinds_render_finite_and_roundtrip_without_mutating_source() {
        let original = layers();
        for kind in [
            Kind::Rough,
            Kind::Zigzag,
            Kind::Transform,
            Kind::Fill,
            Kind::Stack,
            Kind::Reroute,
            Kind::Pathfinder,
            Kind::Boolean,
            Kind::Offset,
            Kind::Round,
            Kind::Band,
            Kind::Pucker,
            Kind::Simplify,
            Kind::Streak,
            Kind::Dissolve,
            Kind::Halftone,
            Kind::Opacity,
            Kind::Shadow,
            Kind::Glow,
            Kind::Blur,
        ] {
            let mut graph = Graph::default();
            let node = graph.add(kind, [250., 100.]).unwrap();
            graph.connect(1, node, 0).unwrap();
            if kind.inputs() == 2 {
                graph.connect(1, node, 1).unwrap();
            }
            graph.connect(node, 2, 0).unwrap();
            let rendered = evaluate(&graph, &original).unwrap_or_else(|e| panic!("{kind:?}: {e}"));
            assert!(
                rendered
                    .iter()
                    .flat_map(|l| l.contours.iter().flatten())
                    .all(|p| p.x.is_finite() && p.y.is_finite()),
                "{kind:?}"
            );
            let loaded: Graph =
                serde_json::from_str(&serde_json::to_string(&graph).unwrap()).unwrap();
            assert_eq!(rendered, evaluate(&loaded, &original).unwrap(), "{kind:?}");
        }
        assert_eq!(original, layers());
    }
}
