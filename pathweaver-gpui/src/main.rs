mod app;
mod assets;
mod color_picker;
mod controls;
mod drawing;
mod input;
mod interaction;
mod menus;
mod native_input;
mod nodes;
mod path_edit;
mod session;
mod theme;
mod tweeq;
mod tweeq_bridge;
mod tweeq_host;
mod viewport;
use gpui::{prelude::*, *};
fn main() {
    theme::initialize();
    Application::new().with_assets(assets::Assets).run(|cx| {
        input::bind_keys(cx);
        menus::initialize(cx);
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        let bounds = Bounds::centered(None, size(px(1380.), px(900.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(980.), px(640.))),
                titlebar: Some(TitlebarOptions {
                    title: Some("PathWeaver".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |window, cx| {
                let view = cx.new(|cx| app::Workspace::new(window, cx));
                let weak = view.downgrade();
                window.on_window_should_close(cx, move |window, cx| {
                    weak.update(cx, |s, cx| {
                        if !s.session.dirty() && s.editing.is_none() {
                            true
                        } else {
                            s.confirm_discard(window, cx, |_, w, _| w.remove_window());
                            false
                        }
                    })
                    .unwrap_or(true)
                });
                view
            },
        )
        .expect("PathWeaver window");
        cx.activate(true);
    });
}
