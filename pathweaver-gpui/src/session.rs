use kurbo::{Affine, Point, Rect, Vec2};
use pathweaver_core::{
    appearance::Graph,
    editing::Anchor,
    model::{Document, Node, Stroke},
    outline,
};
use std::path::{Path, PathBuf};

// History restores the editing target as well as the document. Selection-only
// changes do not create history entries or make the document dirty.
#[derive(Clone)]
struct Snapshot {
    doc: Document,
    selected: Vec<u64>,
    anchors: Vec<Anchor>,
    segment: Option<Anchor>,
}

pub struct Session {
    pub doc: Document,
    pub selected: Vec<u64>,
    pub anchors: Vec<Anchor>,
    pub segment: Option<Anchor>,
    pub path: Option<PathBuf>,
    saved: Document,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    pending: Option<Snapshot>,
}
impl Default for Session {
    fn default() -> Self {
        let doc = Document::default();
        Self {
            saved: doc.clone(),
            doc,
            selected: vec![],
            anchors: vec![],
            segment: None,
            path: None,
            undo: vec![],
            redo: vec![],
            pending: None,
        }
    }
}
impl Session {
    pub fn dirty(&self) -> bool {
        self.doc != self.saved
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
            || self
                .pending
                .as_ref()
                .is_some_and(|before| before.doc != self.doc)
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
            && !self
                .pending
                .as_ref()
                .is_some_and(|before| before.doc != self.doc)
    }
    pub fn active(&self) -> Option<&Stroke> {
        self.selected
            .last()
            .and_then(|id| self.doc.strokes.iter().find(|s| s.id == *id && s.visible))
    }
    pub fn active_mut(&mut self) -> Option<&mut Stroke> {
        let id = *self.selected.last()?;
        self.doc
            .strokes
            .iter_mut()
            .find(|s| s.id == id && s.visible)
    }
    pub fn graph(&self) -> Option<&Graph> {
        self.active()?.appearance.as_ref()
    }
    pub fn graph_mut(&mut self) -> Option<&mut Graph> {
        self.active_mut()?.appearance.as_mut()
    }
    fn snapshot(&self) -> Snapshot {
        Snapshot {
            doc: self.doc.clone(),
            selected: self.selected.clone(),
            anchors: self.anchors.clone(),
            segment: self.segment,
        }
    }
    fn restore(&mut self, snapshot: Snapshot) {
        self.doc = snapshot.doc;
        self.selected = snapshot.selected;
        self.anchors = snapshot.anchors;
        self.segment = snapshot.segment;
        self.reconcile();
    }
    pub fn begin(&mut self) {
        self.reconcile();
        if self.pending.is_none() {
            self.pending = Some(self.snapshot());
        }
    }
    pub fn commit(&mut self) {
        self.reconcile();
        if let Some(before) = self.pending.take().filter(|before| before.doc != self.doc) {
            self.undo.push(before);
            self.undo.drain(..self.undo.len().saturating_sub(100));
            self.redo.clear();
        }
    }
    pub fn cancel(&mut self) {
        if let Some(before) = self.pending.take() {
            self.restore(before);
        }
        self.reconcile();
    }
    pub fn edit(&mut self, f: impl FnOnce(&mut Self)) {
        self.begin();
        f(self);
        self.commit();
    }
    pub fn undo(&mut self) {
        self.commit();
        if let Some(before) = self.undo.pop() {
            self.redo.push(self.snapshot());
            self.restore(before);
        }
    }
    pub fn redo(&mut self) {
        // A new gesture invalidates the old redo branch before restoring history.
        self.commit();
        if let Some(after) = self.redo.pop() {
            self.undo.push(self.snapshot());
            self.restore(after);
        }
    }
    pub fn reconcile(&mut self) {
        self.selected
            .retain(|id| self.doc.strokes.iter().any(|s| s.id == *id && s.visible));
        self.anchors.retain(|a| {
            self.doc.strokes.iter().any(|s| {
                s.id == a.stroke
                    && s.visible
                    && self.selected.contains(&s.id)
                    && s.nodes.iter().any(|n| n.id == a.node)
            })
        });
        if self.segment.is_some_and(|a| {
            !self.doc.strokes.iter().any(|s| {
                s.id == a.stroke
                    && s.visible
                    && self.selected.contains(&s.id)
                    && s.segment_index(a.node).is_some()
            })
        }) {
            self.segment = None;
        }
    }
    pub fn select(&mut self, id: u64, add: bool) {
        if !self.doc.strokes.iter().any(|s| s.id == id && s.visible) {
            return;
        }
        self.anchors.clear();
        self.segment = None;
        if add {
            if self.selected.contains(&id) {
                self.selected.retain(|s| *s != id);
            } else {
                self.selected.push(id);
            }
        } else {
            if !self.selected.contains(&id) {
                self.selected.clear();
            }
            self.selected.retain(|s| *s != id);
            self.selected.push(id);
        }
    }
    pub fn select_anchor(&mut self, at: Anchor, add: bool) {
        if !self
            .doc
            .strokes
            .iter()
            .any(|s| s.id == at.stroke && s.visible && s.nodes.iter().any(|n| n.id == at.node))
        {
            return;
        }
        self.segment = None;
        if add && self.anchors.contains(&at) {
            self.anchors.retain(|a| *a != at);
        } else {
            if !add && !self.anchors.contains(&at) {
                self.anchors.clear();
            }
            self.anchors.retain(|a| *a != at);
            self.anchors.push(at);
        }
        self.selected = self
            .anchors
            .iter()
            .map(|a| a.stroke)
            .fold(Vec::new(), |mut ids, id| {
                ids.retain(|v| *v != id);
                ids.push(id);
                ids
            });
    }
    pub fn anchor_position(&self) -> Option<Point> {
        let at = self.anchors.last()?;
        Some(
            self.doc
                .strokes
                .iter()
                .find(|s| s.id == at.stroke)?
                .nodes
                .iter()
                .find(|n| n.id == at.node)?
                .p,
        )
    }
    pub fn move_anchors(&mut self, delta: Vec2) {
        for s in &mut self.doc.strokes {
            for n in &mut s.nodes {
                if self.anchors.contains(&Anchor {
                    stroke: s.id,
                    node: n.id,
                }) {
                    n.p += delta;
                }
            }
        }
    }
    pub fn remove(&mut self) {
        self.edit(|s| {
            s.doc.strokes.retain(|p| !s.selected.contains(&p.id));
            s.selected.clear();
        });
    }
    pub fn new_path(&mut self, p: Point) -> u64 {
        let id = self.doc.id();
        let mut stroke = Stroke::new(id, format!("パス {}", self.doc.strokes.len() + 1));
        stroke.set_line_width(24.);
        stroke.nodes.push(node(self.doc.id(), p));
        self.doc.strokes.push(stroke);
        self.selected = vec![id];
        id
    }
    pub fn add_point(&mut self, p: Point) -> Option<u64> {
        let id = self.doc.id();
        self.active_mut()?.nodes.push(node(id, p));
        Some(id)
    }
    pub fn transform(&mut self, matrix: Affine) {
        for stroke in &mut self.doc.strokes {
            if self.selected.contains(&stroke.id) {
                *stroke = stroke.transformed(matrix, true);
            }
        }
    }
    pub fn bounds(&self, selected: bool) -> Option<Rect> {
        self.doc
            .strokes
            .iter()
            .filter(|s| s.visible && (!selected || self.selected.contains(&s.id)))
            .filter_map(stroke_bounds)
            .reduce(|a, b| a.union(b))
    }
    pub fn duplicate(&mut self) {
        self.edit(|s| {
            let originals: Vec<_> = s
                .selected
                .iter()
                .filter_map(|id| s.doc.strokes.iter().find(|p| p.id == *id && p.visible))
                .cloned()
                .collect();
            s.selected.clear();
            for mut p in originals {
                p.id = s.doc.id();
                p.name.push_str(" コピー");
                for n in &mut p.nodes {
                    let old = n.id;
                    n.id = s.doc.id();
                    for k in &mut p.widths {
                        if k.segment == old {
                            k.segment = n.id;
                        }
                    }
                    for c in &mut p.corners {
                        if c.node == old {
                            c.node = n.id;
                        }
                    }
                }
                for k in &mut p.widths {
                    k.id = s.doc.id();
                }
                p = p.transformed(Affine::translate((20., 20.)), false);
                s.selected.push(p.id);
                s.doc.strokes.push(p);
            }
        });
    }
    pub fn load(&mut self, path: &Path) -> Result<(), String> {
        if std::fs::metadata(path).map_err(|e| e.to_string())?.len() > 32 * 1024 * 1024 {
            return Err("ファイルは32MBまでです".into());
        }
        let mut doc: Document =
            serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        doc.upgrade()?;
        doc.validate()?;
        *self = Self {
            saved: doc.clone(),
            doc,
            path: Some(path.into()),
            ..Default::default()
        };
        Ok(())
    }
    pub fn save(&mut self, path: &Path) -> Result<(), String> {
        self.doc.validate()?;
        let text = serde_json::to_vec_pretty(&self.doc).map_err(|e| e.to_string())?;
        atomic_write(path, &text)?;
        self.path = Some(path.into());
        self.saved = self.doc.clone();
        Ok(())
    }
}
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    // Same-directory rename keeps a failed write from truncating an existing document.
    let temp = path.with_file_name(format!(
        ".{}.{}.tmp",
        path.file_name().unwrap_or_default().to_string_lossy(),
        std::process::id()
    ));
    let result = (|| {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        std::fs::rename(&temp, path)
    })()
    .map_err(|e: std::io::Error| e.to_string());
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}
pub fn node(id: u64, p: Point) -> Node {
    Node {
        id,
        p,
        incoming: Vec2::ZERO,
        outgoing: Vec2::ZERO,
        smooth: false,
        exact_incoming: false,
        exact_outgoing: false,
    }
}
pub fn stroke_bounds(stroke: &Stroke) -> Option<Rect> {
    let points: Vec<_> = if let Ok(out) = outline::paint(stroke) {
        out.contours.into_iter().flatten().collect()
    } else {
        vec![]
    };
    points
        .into_iter()
        .chain(stroke.nodes.iter().map(|n| n.p))
        .map(|p| Rect::from_points(p, p))
        .reduce(|a, b| a.union(b))
}
pub fn inside(p: Point, contours: &[Vec<Point>]) -> bool {
    let mut winding = 0;
    for c in contours {
        for (a, b) in c.iter().zip(c.iter().cycle().skip(1)).take(c.len()) {
            let cross = (b.x - a.x) * (p.y - a.y) - (p.x - a.x) * (b.y - a.y);
            if a.y <= p.y && b.y > p.y && cross > 0. {
                winding += 1;
            } else if a.y > p.y && b.y <= p.y && cross < 0. {
                winding -= 1;
            }
        }
    }
    winding != 0
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn anchor_selection_coordinates_topology_and_undo() {
        use pathweaver_core::editing;
        let mut s = Session::default();
        s.new_path(Point::ZERO);
        s.add_point(Point::new(50., 30.));
        s.add_point(Point::new(100., 0.));
        let a = Anchor {
            stroke: s.doc.strokes[0].id,
            node: s.doc.strokes[0].nodes[0].id,
        };
        let b = Anchor {
            stroke: a.stroke,
            node: s.doc.strokes[0].nodes[1].id,
        };
        s.select_anchor(a, false);
        s.select_anchor(b, true);
        assert_eq!(s.anchor_position(), Some(Point::new(50., 30.)));
        s.edit(|s| s.move_anchors(Vec2::new(20., -10.)));
        assert_eq!(s.doc.strokes[0].nodes[0].p, Point::new(20., -10.));
        assert_eq!(s.doc.strokes[0].nodes[2].p, Point::new(100., 0.));
        s.undo();
        assert_eq!(s.anchor_position(), Some(Point::new(50., 30.)));
        let before = s.doc.clone();
        s.edit(|s| {
            let parts = editing::separate(&mut s.doc, b).unwrap();
            s.anchors.clear();
            for at in parts {
                s.select_anchor(at, true);
            }
        });
        assert_eq!(s.selected.len(), 2);
        s.undo();
        s.doc.validate().unwrap();
        assert_eq!(s.doc, before);
        assert_eq!(s.anchors, vec![a, b]);
        s.redo();
        assert_eq!(s.doc.strokes.len(), 2);
        s.select(a.stroke, false);
        assert!(s.anchors.is_empty());
    }
    #[test]
    fn grouped_edit_undo_and_cancel() {
        let mut s = Session::default();
        s.edit(|s| {
            s.new_path(Point::new(5., 5.));
            s.add_point(Point::new(50., 50.));
        });
        let original = s.doc.clone();
        s.begin();
        s.transform(Affine::translate((10., 20.)));
        s.transform(Affine::translate((4., 3.)));
        s.commit();
        assert_ne!(s.doc, original);
        s.undo();
        assert_eq!(s.doc, original);
        s.redo();
        assert_ne!(s.doc, original);
        let before_cancel = s.doc.clone();
        s.begin();
        s.transform(Affine::translate((100., 100.)));
        s.cancel();
        assert_eq!(s.doc, before_cancel);
    }
    #[test]
    fn duplicates_remap_width_ids_and_graphs() {
        let mut s = Session {
            doc: Document::example(),
            ..Default::default()
        };
        s.selected = vec![s.doc.strokes[0].id];
        s.active_mut().unwrap().appearance = Some(Graph::rough());
        s.duplicate();
        s.doc.validate().unwrap();
        assert_eq!(s.doc.strokes.len(), 2);
        s.graph_mut().unwrap().node_mut(3).unwrap().amount = 99.;
        assert_eq!(
            s.doc.strokes[0]
                .appearance
                .as_ref()
                .unwrap()
                .node(3)
                .unwrap()
                .amount,
            3.
        );
    }
    #[test]
    fn redo_does_not_leave_a_pending_snapshot_from_another_history_branch() {
        let mut s = Session::default();
        s.edit(|s| {
            s.new_path(Point::ZERO);
        });
        s.edit(|s| s.transform(Affine::translate((10., 0.))));
        s.undo();
        s.begin();
        s.transform(Affine::translate((30., 0.)));
        s.redo();
        assert_eq!(s.active().unwrap().nodes[0].p.x, 30.);
        s.cancel();
        assert_eq!(s.active().unwrap().nodes[0].p.x, 30.);
        s.undo();
        assert_eq!(s.active().unwrap().nodes[0].p.x, 0.);
    }
    #[test]
    fn history_availability_accounts_for_pending_edits() {
        let mut s = Session::default();
        assert!(!s.can_undo());
        assert!(!s.can_redo());
        s.begin();
        s.new_path(Point::ZERO);
        assert!(s.can_undo());
        assert!(!s.can_redo());
        s.commit();
        s.undo();
        assert!(!s.can_undo());
        assert!(s.can_redo());
        s.begin();
        assert!(s.can_redo());
        s.new_path(Point::new(10., 20.));
        assert!(s.can_undo());
        assert!(!s.can_redo());
        s.cancel();
        assert!(!s.can_undo());
        assert!(s.can_redo());
    }

    #[test]
    fn hide_clears_edit_targets_and_undo_restores_them() {
        let mut s = Session::default();
        let first = s.new_path(Point::ZERO);
        s.add_point(Point::new(50., 0.));
        let at = Anchor {
            stroke: first,
            node: s.doc.strokes[0].nodes[0].id,
        };
        s.select_anchor(at, false);
        s.segment = Some(at);
        s.edit(|s| s.doc.strokes[0].visible = false);
        assert!(s.selected.is_empty());
        assert!(s.anchors.is_empty());
        assert!(s.segment.is_none());
        assert!(s.active().is_none());
        s.select(first, false);
        s.select_anchor(at, false);
        assert!(s.selected.is_empty());
        assert!(s.anchors.is_empty());
        s.undo();
        assert!(s.doc.strokes[0].visible);
        assert_eq!(s.selected, vec![first]);
        assert_eq!(s.anchors, vec![at]);
        assert_eq!(s.segment, Some(at));
        s.redo();
        assert!(!s.doc.strokes[0].visible);
        assert!(s.selected.is_empty());
        assert!(s.anchors.is_empty());
        assert!(s.segment.is_none());
    }

    #[test]
    fn delete_undo_restores_selection_order_and_cancel_restores_targets() {
        let mut s = Session::default();
        let first = s.new_path(Point::ZERO);
        let second = s.new_path(Point::new(100., 0.));
        s.select(first, true);
        assert_eq!(s.selected, vec![second, first]);
        s.remove();
        assert!(s.doc.strokes.is_empty());
        assert!(s.selected.is_empty());
        s.undo();
        assert_eq!(s.selected, vec![second, first]);
        assert_eq!(s.active().unwrap().id, first);
        s.redo();
        assert!(s.doc.strokes.is_empty());
        assert!(s.selected.is_empty());
        s.undo();
        let before = s.doc.clone();
        s.begin();
        s.doc.strokes.clear();
        s.selected.clear();
        s.cancel();
        assert_eq!(s.doc, before);
        assert_eq!(s.selected, vec![second, first]);
    }

    #[test]
    fn clicking_selected_path_changes_active_without_losing_multiselection() {
        let mut s = Session::default();
        let first = s.new_path(Point::ZERO);
        let second = s.new_path(Point::new(100., 0.));
        s.select(first, true);
        assert_eq!(s.selected, vec![second, first]);
        s.select(second, false);
        assert_eq!(s.selected, vec![first, second]);
        assert_eq!(s.active().unwrap().id, second);
        s.select(first, false);
        assert_eq!(s.selected, vec![second, first]);
        s.duplicate();
        assert_eq!(s.active().unwrap().nodes[0].p, Point::new(20., 20.));
        s.undo();
        assert_eq!(s.selected, vec![second, first]);
        // Clicking and dragging without changing geometry does not add undo steps.
        let undo_len = s.undo.len();
        s.begin();
        s.select(second, false);
        s.commit();
        assert_eq!(s.undo.len(), undo_len);
        s.select(second, true);
        assert_eq!(s.selected, vec![first]);
    }

    #[test]
    fn saved_document_roundtrips_and_failed_load_keeps_work() {
        let dir = std::env::temp_dir().join(format!("pathweaver-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("doc.pathweaver");
        let mut s = Session {
            doc: Document::example(),
            ..Default::default()
        };
        s.doc.appearance_presets.push(
            pathweaver_core::appearance_assets::Preset::new("紙", "質感", &Graph::rough()).unwrap(),
        );
        s.save(&path).unwrap();
        assert!(!s.dirty());
        let mut loaded = Session::default();
        loaded.load(&path).unwrap();
        assert_eq!(s.doc, loaded.doc);
        std::fs::write(&path, "invalid").unwrap();
        assert!(loaded.load(&path).is_err());
        assert_eq!(s.doc, loaded.doc);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
