//! Shared lifetime of a gesture, independent of the editor that started it.
use crate::app::{Drag, Workspace};
use gpui::{Context, MouseButton, MouseDownEvent};
use kurbo::Vec2;

impl Drag {
    pub(crate) fn release_button(&self) -> Option<MouseButton> {
        match self {
            Self::None => None,
            Self::Pan { .. } => Some(MouseButton::Middle),
            Self::Cut { .. } | Self::Connect { lazy: true, .. } => Some(MouseButton::Right),
            _ => Some(MouseButton::Left),
        }
    }

    pub(crate) fn rebase_nodes(&mut self, delta: Vec2) {
        if let Self::Nodes { last } = self {
            *last += delta;
        }
    }
}

impl Workspace {
    pub(crate) fn finish_interaction(&mut self, commit: bool, cx: &mut Context<Self>) {
        if commit {
            // An unfinished wire is a proposal, not a disconnected input to save.
            if matches!(self.drag, Drag::Connect { .. } | Drag::ConnectInput { .. }) {
                self.session.cancel();
            } else {
                self.session.commit();
            }
        } else {
            self.session.cancel();
        }
        self.drag = Drag::None;
        self.node_move_modal = false;
        self.numeric_before = None;
        self.numeric_snap = false;
        self.snap_guide = None;
        self.snap_frames.clear();
        self.refresh(cx);
    }

    pub(crate) fn handle_drag_down(&mut self, e: &MouseDownEvent, cx: &mut Context<Self>) -> bool {
        if self.node_move_modal && matches!(e.button, MouseButton::Left | MouseButton::Right) {
            self.finish_interaction(e.button == MouseButton::Left, cx);
            self.refresh(cx);
            return true;
        }
        // A second button must not replace the active gesture or its transaction.
        !matches!(self.drag, Drag::None)
    }

    pub(crate) fn history_step(&mut self, redo: bool, cx: &mut Context<Self>) {
        self.finish_interaction(true, cx);
        self.pen_path = None;
        if redo {
            self.session.redo();
        } else {
            self.session.undo();
        }
        self.refresh(cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{point, px};
    use kurbo::Point;

    #[test]
    fn gestures_only_finish_with_the_button_that_started_them() {
        assert_eq!(Drag::None.release_button(), None);
        assert_eq!(
            Drag::Nodes { last: Point::ZERO }.release_button(),
            Some(MouseButton::Left)
        );
        assert_eq!(
            Drag::Pan {
                nodes: false,
                last: point(px(0.), px(0.))
            }
            .release_button(),
            Some(MouseButton::Middle)
        );
        assert_eq!(
            Drag::Connect {
                from: 1,
                at: point(px(0.), px(0.)),
                lazy: true
            }
            .release_button(),
            Some(MouseButton::Right)
        );
        assert_eq!(
            Drag::Connect {
                from: 1,
                at: point(px(0.), px(0.)),
                lazy: false
            }
            .release_button(),
            Some(MouseButton::Left)
        );
    }

    #[test]
    fn view_change_does_not_become_node_movement() {
        let before_cursor = Point::new(30., 40.);
        let after_cursor = Point::new(130., 240.);
        let mut drag = Drag::Nodes {
            last: before_cursor,
        };
        drag.rebase_nodes(after_cursor - before_cursor);
        let Drag::Nodes { last } = drag else {
            unreachable!()
        };
        assert_eq!(after_cursor - last, Vec2::ZERO);
        assert_eq!((after_cursor + Vec2::new(2., 3.)) - last, Vec2::new(2., 3.));
    }
}
