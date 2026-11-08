//! Transient editor views are independent of document edits and Undo.
use crate::drawing::View;
use std::collections::HashMap;

#[derive(Default)]
pub struct NodeViews {
    owner: Option<u64>,
    saved: HashMap<u64, View>,
}

impl NodeViews {
    pub fn switch(&mut self, owner: Option<u64>, current: View, ready: bool) -> Option<View> {
        if ready && let Some(previous) = self.owner {
            self.saved.insert(previous, current);
        }
        self.owner = owner;
        owner.and_then(|id| self.saved.get(&id).copied())
    }
}

pub fn dock_height(preferred: f32, viewport_height: f32) -> f32 {
    preferred.clamp(170., (viewport_height * 0.48).max(170.))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn switching_paths_restores_views_even_through_deselection() {
        let mut views = NodeViews::default();
        let a = View {
            pan: [-500., 70.],
            zoom: 0.5,
        };
        let b = View {
            pan: [800., -20.],
            zoom: 2.,
        };
        assert!(views.switch(Some(1), View::default(), true).is_none());
        assert!(views.switch(Some(2), a, true).is_none());
        let restored = views.switch(Some(1), b, true).unwrap();
        assert_eq!(restored.pan, a.pan);
        assert_eq!(restored.zoom, a.zoom);
        assert!(views.switch(None, a, true).is_none());
        let restored = views.switch(Some(2), View::default(), true).unwrap();
        assert_eq!(restored.pan, b.pan);
        assert_eq!(restored.zoom, b.zoom);
        assert!(NodeViews::default().switch(Some(1), a, true).is_none());
    }
    #[test]
    fn paths_visited_while_dock_is_closed_still_need_initial_fit() {
        let mut views = NodeViews::default();
        assert!(views.switch(Some(1), View::default(), false).is_none());
        assert!(views.switch(Some(2), View::default(), false).is_none());
        assert!(views.switch(Some(1), View::default(), false).is_none());
    }
    #[test]
    fn small_window_does_not_destroy_preferred_dock_height() {
        let preferred = 500.;
        assert!((dock_height(preferred, 640.) - 307.2).abs() < 0.001);
        assert_eq!(dock_height(preferred, 1200.), preferred);
        assert_eq!(dock_height(10., 640.), 170.);
    }
}
