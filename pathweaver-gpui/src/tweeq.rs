//! Actual Tweeq Vue controls hosted in the existing native editor window.
use crate::{app::Workspace, tweeq_host::TweeqHost};
use gpui::{prelude::*, *};

pub(crate) struct TweeqPanels {
    pub path: Option<TweeqHost>,
    pub node: Option<TweeqHost>,
}

impl TweeqPanels {
    pub fn new(window: &mut Window, cx: &mut Context<Workspace>) -> Self {
        let (send, receive) = async_channel::unbounded::<(String, String)>();
        let html = include_str!("../../tweeq-ui/dist/index.html");
        let panels = Self {
            path: TweeqHost::new(window, html, "path", send.clone()),
            node: TweeqHost::new(window, html, "node", send),
        };
        cx.spawn_in(window, async move |weak, cx| {
            while let Ok((panel, body)) = receive.recv().await {
                if weak
                    .update_in(cx, |s, w, cx| {
                        let target = if panel == "path" && s.inspector_tab == 1 {
                            "grid"
                        } else {
                            panel.as_str()
                        };
                        let message = serde_json::from_str::<serde_json::Value>(&body).ok();
                        if message.as_ref().is_some_and(|v| v["type"] == "error") {
                            let detail = message
                                .as_ref()
                                .and_then(|v| v["message"].as_str())
                                .unwrap_or("入力欄を再読み込みしてください");
                            s.status =
                                format!("Tweeq: {}", detail.chars().take(200).collect::<String>());
                            cx.notify();
                            return;
                        }
                        if serde_json::from_str::<serde_json::Value>(&body)
                            .ok()
                            .and_then(|v| v["type"].as_str().map(str::to_owned))
                            .as_deref()
                            == Some("ready")
                        {
                            let snapshot = s.tweeq_snapshot(target);
                            let host = if panel == "node" {
                                &mut s.tweeq_panels.node
                            } else {
                                &mut s.tweeq_panels.path
                            };
                            if let Some(host) = host {
                                host.refresh(&snapshot);
                            }
                        } else {
                            s.tweeq_message(target, &body, w, cx);
                            // A rejected confirm can leave identical state. Still
                            // acknowledge it so the web editor can restore its
                            // pending transaction and support Escape/correction.
                            let snapshot = s.tweeq_snapshot(target);
                            let host = if panel == "node" {
                                &mut s.tweeq_panels.node
                            } else {
                                &mut s.tweeq_panels.path
                            };
                            if let Some(host) = host {
                                host.refresh(&snapshot);
                            }
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        panels
    }

    pub fn focused(&self) -> bool {
        self.path.as_ref().is_some_and(TweeqHost::is_focused)
            || self.node.as_ref().is_some_and(TweeqHost::is_focused)
    }

    pub fn release_focus(&self) {
        for host in [&self.path, &self.node].into_iter().flatten() {
            if host.is_focused() {
                host.evaluate("window.pathweaverCommand?.('blur')");
                host.focus_canvas();
            }
        }
    }

    pub fn command(&self, command: &str) {
        let encoded = serde_json::to_string(command).expect("command string");
        for host in [&self.path, &self.node].into_iter().flatten() {
            if host.is_focused() {
                if matches!(command, "cut" | "copy" | "paste") && host.native_edit(command) {
                    return;
                }
                host.evaluate(&format!("window.pathweaverCommand?.({encoded})"));
            }
        }
    }
}

impl Workspace {
    pub(crate) fn tweeq_panel(&self, panel: &'static str, cx: &mut Context<Self>) -> AnyElement {
        let view = cx.entity();
        div()
            .id(SharedString::from(format!("tweeq-{panel}")))
            .flex_1()
            .min_h_0()
            .w_full()
            .h_full()
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, _, cx| {
                        view.update(cx, |s, _| {
                            let snapshot = s.tweeq_snapshot(panel);
                            let hidden = s.add_open || s.dialog_open;
                            let host = if panel == "node" {
                                &mut s.tweeq_panels.node
                            } else {
                                &mut s.tweeq_panels.path
                            };
                            if let Some(host) = host {
                                host.sync((!hidden).then_some(bounds), &snapshot);
                            }
                        });
                    },
                )
                .size_full(),
            )
            .into_any_element()
    }
}
