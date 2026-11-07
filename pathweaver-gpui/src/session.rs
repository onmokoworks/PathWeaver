use kurbo::{Affine, Point, Rect, Vec2};
use pathweaver_core::{
    appearance::Graph,
    model::{Document, Node, Stroke},
    outline,
};
use std::path::{Path, PathBuf};

pub struct Session {
    pub doc: Document,
    pub selected: Vec<u64>,
    pub path: Option<PathBuf>,
    saved: Document,
    undo: Vec<Document>,
    redo: Vec<Document>,
    pending: Option<Document>,
}
impl Default for Session {
    fn default() -> Self {
        let doc = Document::default();
        Self {
            saved: doc.clone(),
            doc,
            selected: vec![],
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
    pub fn active(&self) -> Option<&Stroke> {
        self.selected
            .last()
            .and_then(|id| self.doc.strokes.iter().find(|s| s.id == *id))
    }
    pub fn active_mut(&mut self) -> Option<&mut Stroke> {
        let id = *self.selected.last()?;
        self.doc.strokes.iter_mut().find(|s| s.id == id)
    }
    pub fn graph(&self) -> Option<&Graph> {
        self.active()?.appearance.as_ref()
    }
    pub fn graph_mut(&mut self) -> Option<&mut Graph> {
        self.active_mut()?.appearance.as_mut()
    }
    pub fn begin(&mut self) {
        if self.pending.is_none() {
            self.pending = Some(self.doc.clone());
        }
    }
    pub fn commit(&mut self) {
        if let Some(before) = self.pending.take().filter(|before| *before != self.doc) {
            self.undo.push(before);
            self.undo.drain(..self.undo.len().saturating_sub(100));
            self.redo.clear();
        }
    }
    pub fn cancel(&mut self) {
        if let Some(before) = self.pending.take() {
            self.doc = before;
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
        if let Some(d) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.doc, d));
        }
        self.reconcile();
    }
    pub fn redo(&mut self) {
        if let Some(d) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.doc, d));
        }
        self.reconcile();
    }
    fn reconcile(&mut self) {
        self.selected
            .retain(|id| self.doc.strokes.iter().any(|s| s.id == *id));
    }
    pub fn select(&mut self, id: u64, add: bool) {
        if add {
            if self.selected.contains(&id) {
                self.selected.retain(|s| *s != id);
            } else {
                self.selected.push(id);
            }
        } else if !self.selected.contains(&id) {
            self.selected = vec![id];
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
                .doc
                .strokes
                .iter()
                .filter(|p| s.selected.contains(&p.id))
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
