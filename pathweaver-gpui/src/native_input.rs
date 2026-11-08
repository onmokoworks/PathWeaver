//! App-local macOS input that GPUI 0.2.2 does not expose.
//!
//! Magnification is delivered through a channel so AppKit never re-enters GPUI.
//! Numeric scrubbing temporarily disconnects cursor position from mouse motion;
//! Apple's deltaX continues past display edges. No event tap, input-monitoring
//! permission, cursor warp, or modification of GPUI's registry source is used.
use crate::app::{Drag, Workspace};
use gpui::{Context, Pixels, Point, Window};

fn magnification_factor(delta: f64) -> Option<f32> {
    (delta.is_finite() && delta > -1.).then(|| (1. + delta).clamp(0.01, 100.) as f32)
}

impl Workspace {
    pub(crate) fn magnify(
        &mut self,
        position: Point<Pixels>,
        delta: f64,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        if !window.is_window_active()
            || self.editing.is_some()
            || (!matches!(self.drag, Drag::None) && !self.node_move_modal)
        {
            return;
        }
        let Some(factor) = magnification_factor(delta) else {
            return;
        };
        let nodes = self.nodes_open && self.node_bounds.get().contains(&position);
        let bounds = if nodes {
            self.node_bounds.get()
        } else if self.art_bounds.get().contains(&position) {
            self.art_bounds.get()
        } else {
            return;
        };
        let view = if nodes {
            &mut self.node_view
        } else {
            &mut self.art_view
        };
        let before = view.world(self.cursor, bounds);
        view.zoom_at(position, bounds, factor);
        if nodes && self.node_move_modal {
            self.drag
                .rebase_nodes(view.world(self.cursor, bounds) - before);
        }
        cx.notify();
    }
}

#[cfg(target_os = "macos")]
#[allow(deprecated, unexpected_cfgs)]
mod macos {
    use super::*;
    use block::ConcreteBlock;
    use cocoa::foundation::{NSPoint, NSRect};
    use gpui::{Task, point, px};
    use objc::{class, msg_send, runtime::Object, sel, sel_impl};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use std::{marker::PhantomData, rc::Rc};

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGAssociateMouseAndMouseCursorPosition(connected: u32) -> i32;
    }

    /// This guard is created and dropped on AppKit's main thread only.
    /// Dropping it on mouse-up, Escape, deactivation, document change, close,
    /// or Rust unwinding reconnects the mouse and balances NSCursor::hide.
    pub struct CursorCapture {
        _main_thread: PhantomData<Rc<()>>,
    }
    impl CursorCapture {
        pub fn start() -> Option<Self> {
            unsafe {
                let app: *mut Object = msg_send![class!(NSApplication), sharedApplication];
                let active: bool = msg_send![app, isActive];
                if !active || CGAssociateMouseAndMouseCursorPosition(0) != 0 {
                    return None;
                }
                let _: () = msg_send![class!(NSCursor), hide];
                Some(Self {
                    _main_thread: PhantomData,
                })
            }
        }
        pub fn delta_x(&self) -> Option<f64> {
            unsafe {
                let app: *mut Object = msg_send![class!(NSApplication), sharedApplication];
                let event: *mut Object = msg_send![app, currentEvent];
                if event.is_null() {
                    return None;
                }
                let kind: usize = msg_send![event, type];
                // Only leftMouseDragged; other currentEvent types don't own deltaX.
                if kind != 6 {
                    return None;
                }
                let delta: f64 = msg_send![event, deltaX];
                delta.is_finite().then_some(delta)
            }
        }
    }
    impl Drop for CursorCapture {
        fn drop(&mut self) {
            unsafe {
                CGAssociateMouseAndMouseCursorPosition(1);
                let _: () = msg_send![class!(NSCursor), unhide];
            }
        }
    }

    pub struct NativeInput {
        monitor: *mut Object,
        _task: Task<()>,
        _main_thread: PhantomData<Rc<()>>,
    }
    impl NativeInput {
        pub fn install(window: &Window, cx: &mut Context<Workspace>) -> Option<Self> {
            let RawWindowHandle::AppKit(handle) =
                HasWindowHandle::window_handle(window).ok()?.as_raw()
            else {
                return None;
            };
            let view = handle.ns_view.as_ptr() as *mut Object;
            let native_window: *mut Object = unsafe { msg_send![view, window] };
            if native_window.is_null() {
                return None;
            }
            let (send, receive) = async_channel::unbounded::<(f32, f32, f64)>();
            let handler = ConcreteBlock::new(move |event: *mut Object| -> *mut Object {
                unsafe {
                    let event_window: *mut Object = msg_send![event, window];
                    if event_window != native_window {
                        return event;
                    }
                    let position: NSPoint = msg_send![event, locationInWindow];
                    let in_view: NSPoint =
                        msg_send![view, convertPoint:position fromView:std::ptr::null::<Object>()];
                    let bounds: NSRect = msg_send![view, bounds];
                    let delta: f64 = msg_send![event, magnification];
                    let _ = send.try_send((
                        in_view.x as f32,
                        (bounds.size.height - in_view.y) as f32,
                        delta,
                    ));
                    // GPUI has no magnify event; consume only this window's pinch.
                    std::ptr::null_mut()
                }
            })
            .copy();
            let monitor: *mut Object = unsafe {
                msg_send![class!(NSEvent), addLocalMonitorForEventsMatchingMask:(1usize << 30) handler:&*handler]
            };
            if monitor.is_null() {
                return None;
            }
            unsafe {
                let _: *mut Object = msg_send![monitor, retain];
            }
            let task = cx.spawn_in(window, async move |weak, cx| {
                while let Ok((x, y, delta)) = receive.recv().await {
                    if weak
                        .update_in(cx, |s, w, cx| s.magnify(point(px(x), px(y)), delta, w, cx))
                        .is_err()
                    {
                        break;
                    }
                }
            });
            Some(Self {
                monitor,
                _task: task,
                _main_thread: PhantomData,
            })
        }
    }
    impl Drop for NativeInput {
        fn drop(&mut self) {
            unsafe {
                let _: () = msg_send![class!(NSEvent), removeMonitor:self.monitor];
                let _: () = msg_send![self.monitor, release];
            }
        }
    }
}
#[cfg(target_os = "macos")]
pub use macos::{CursorCapture, NativeInput};

#[cfg(not(target_os = "macos"))]
mod fallback {
    use super::*;
    pub struct CursorCapture;
    impl CursorCapture {
        pub fn start() -> Option<Self> {
            None
        }
        pub fn delta_x(&self) -> Option<f64> {
            None
        }
    }
    pub struct NativeInput;
    impl NativeInput {
        pub fn install(_: &Window, _: &mut Context<Workspace>) -> Option<Self> {
            None
        }
    }
}
#[cfg(not(target_os = "macos"))]
pub use fallback::{CursorCapture, NativeInput};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drawing::View;
    use gpui::{Bounds, point, px, size};

    #[test]
    fn pinch_factor_tracks_native_scale_and_rejects_invalid_events() {
        assert_eq!(magnification_factor(0.25), Some(1.25));
        assert_eq!(magnification_factor(-0.2), Some(0.8));
        assert_eq!(magnification_factor(f64::NAN), None);
        assert_eq!(magnification_factor(-1.), None);
        assert_eq!(magnification_factor(f64::INFINITY), None);
    }
    #[test]
    fn pinch_keeps_content_under_pointer_stationary_for_both_views() {
        let bounds = Bounds::new(point(px(45.), px(90.)), size(px(800.), px(400.)));
        let pointer = point(px(365.), px(240.));
        for zoom in [0.5, 2.] {
            let mut view = View {
                pan: [30., -40.],
                zoom,
            };
            let before = view.world(pointer, bounds);
            view.zoom_at(pointer, bounds, magnification_factor(0.2).unwrap());
            assert!((view.world(pointer, bounds) - before).hypot() < 0.0001);
        }
    }
}
