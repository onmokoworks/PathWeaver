//! A local WKWebView child for the actual Tweeq Vue components.
//!
//! AppKit callbacks only enqueue messages. The owner drains the channel through
//! GPUI's executor, so WebKit never re-enters a borrowed Workspace. All objects
//! here are created, used and released on the window's main thread.
#[cfg(target_os = "macos")]
#[allow(deprecated, unexpected_cfgs)]
mod macos {
    use async_channel::Sender;
    use block::Block;
    use cocoa::foundation::{NSPoint, NSRect, NSSize, NSString};
    use gpui::{Bounds, Pixels, Window};
    use objc::{
        class,
        declare::ClassDecl,
        msg_send,
        runtime::{Class, Object, Protocol, Sel},
        sel, sel_impl,
    };
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use std::{
        cell::Cell,
        ffi::{CStr, c_void},
        marker::PhantomData,
        ptr,
        rc::Rc,
        sync::OnceLock,
    };

    #[link(name = "WebKit", kind = "framework")]
    unsafe extern "C" {}

    const STATE: &str = "pathweaverState";
    const BASE_URL: &str = "https://pathweaver.local/";
    type MessageSender = Sender<(String, String)>;
    struct HandlerState {
        panel: &'static str,
        send: MessageSender,
        initial_navigation: Cell<bool>,
    }

    unsafe fn string(value: *mut Object) -> Option<String> {
        if value.is_null() {
            return None;
        }
        let bytes: *const std::ffi::c_char = unsafe { msg_send![value, UTF8String] };
        (!bytes.is_null()).then(|| {
            unsafe { CStr::from_ptr(bytes) }
                .to_string_lossy()
                .into_owned()
        })
    }

    extern "C" fn receive(this: &Object, _: Sel, _: *mut Object, message: *mut Object) {
        unsafe {
            let state = *this.get_ivar::<*mut c_void>(STATE) as *mut HandlerState;
            if state.is_null() {
                return;
            }
            let frame: *mut Object = msg_send![message, frameInfo];
            let main_frame: bool = msg_send![frame, isMainFrame];
            if !main_frame {
                return;
            }
            let body: *mut Object = msg_send![message, body];
            let is_string: bool = msg_send![body, isKindOfClass:class!(NSString)];
            let body = if is_string {
                string(body)
            } else {
                let valid: bool = msg_send![class!(NSJSONSerialization), isValidJSONObject:body];
                if !valid {
                    return;
                }
                let data: *mut Object = msg_send![class!(NSJSONSerialization), dataWithJSONObject:body options:0usize error:ptr::null_mut::<*mut Object>()];
                if data.is_null() {
                    return;
                }
                let value: *mut Object = msg_send![class!(NSString), alloc];
                let value: *mut Object = msg_send![value, initWithData:data encoding:4usize];
                let text = string(value);
                let _: () = msg_send![value, release];
                text
            };
            if let Some(body) = body {
                // Bound IPC allocation; the inspector never needs large payloads.
                if body.len() <= 64 * 1024 {
                    let state = &*state;
                    let _ = state.send.try_send((state.panel.into(), body));
                }
            }
        }
    }

    extern "C" fn navigation(
        this: &Object,
        _: Sel,
        _: *mut Object,
        action: *mut Object,
        decision: *mut c_void,
    ) {
        unsafe {
            let request: *mut Object = msg_send![action, request];
            let url: *mut Object = msg_send![request, URL];
            let absolute: *mut Object = msg_send![url, absoluteString];
            let state = *this.get_ivar::<*mut c_void>(STATE) as *mut HandlerState;
            let allowed = !state.is_null()
                && (*state).initial_navigation.get()
                && string(absolute).is_some_and(|url| url == BASE_URL || url == "about:blank");
            if allowed {
                (*state).initial_navigation.set(false);
            }
            (*(decision as *const Block<(isize,), ()>)).call((isize::from(allowed),));
        }
    }

    extern "C" fn dealloc(this: &mut Object, _: Sel) {
        unsafe {
            let state = *this.get_ivar::<*mut c_void>(STATE) as *mut HandlerState;
            this.set_ivar(STATE, ptr::null_mut::<c_void>());
            if !state.is_null() {
                drop(Box::from_raw(state));
            }
            let _: () = msg_send![super(this, class!(NSObject)), dealloc];
        }
    }

    fn handler_class() -> &'static Class {
        static CLASS: OnceLock<&Class> = OnceLock::new();
        CLASS.get_or_init(|| {
            let mut decl = ClassDecl::new("PathWeaverTweeqBridge", class!(NSObject))
                .expect("unique Tweeq bridge Objective-C class");
            decl.add_ivar::<*mut c_void>(STATE);
            if let Some(protocol) = Protocol::get("WKScriptMessageHandler") {
                decl.add_protocol(protocol);
            }
            if let Some(protocol) = Protocol::get("WKNavigationDelegate") {
                decl.add_protocol(protocol);
            }
            unsafe {
                decl.add_method(
                    sel!(userContentController:didReceiveScriptMessage:),
                    receive as extern "C" fn(&Object, Sel, *mut Object, *mut Object),
                );
                decl.add_method(
                    sel!(webView:decidePolicyForNavigationAction:decisionHandler:),
                    navigation
                        as extern "C" fn(&Object, Sel, *mut Object, *mut Object, *mut c_void),
                );
                decl.add_method(sel!(dealloc), dealloc as extern "C" fn(&mut Object, Sel));
            }
            decl.register()
        })
    }

    pub struct TweeqHost {
        web: *mut Object,
        parent: *mut Object,
        handler: *mut Object,
        controller: *mut Object,
        last_snapshot: String,
        visible: bool,
        _main_thread: PhantomData<Rc<()>>,
    }

    impl TweeqHost {
        pub fn new(
            window: &Window,
            html: &str,
            panel: &'static str,
            send: MessageSender,
        ) -> Option<Self> {
            let RawWindowHandle::AppKit(handle) =
                HasWindowHandle::window_handle(window).ok()?.as_raw()
            else {
                return None;
            };
            let parent = handle.ns_view.as_ptr() as *mut Object;
            unsafe {
                let handler: *mut Object = msg_send![handler_class(), new];
                (*handler).set_ivar(
                    STATE,
                    Box::into_raw(Box::new(HandlerState {
                        panel,
                        send,
                        initial_navigation: Cell::new(true),
                    })) as *mut c_void,
                );
                let controller: *mut Object = msg_send![class!(WKUserContentController), new];
                let name = NSString::alloc(ptr::null_mut()).init_str("pathweaver");
                let _: () = msg_send![controller, addScriptMessageHandler:handler name:name];
                let _: () = msg_send![name, release];
                let configuration: *mut Object = msg_send![class!(WKWebViewConfiguration), new];
                let _: () = msg_send![configuration, setUserContentController:controller];
                let store: *mut Object =
                    msg_send![class!(WKWebsiteDataStore), nonPersistentDataStore];
                let _: () = msg_send![configuration, setWebsiteDataStore:store];
                let web: *mut Object = msg_send![class!(WKWebView), alloc];
                let web: *mut Object = msg_send![web, initWithFrame:NSRect::new(NSPoint::new(0., 0.), NSSize::new(1., 1.)) configuration:configuration];
                let _: () = msg_send![configuration, release];
                if web.is_null() {
                    let _: () = msg_send![controller, release];
                    let _: () = msg_send![handler, release];
                    return None;
                }
                let _: () = msg_send![web, setNavigationDelegate:handler];
                let _: () = msg_send![web, setHidden:true];
                let _: () = msg_send![parent, addSubview:web];
                let _: *mut Object = msg_send![parent, retain];
                let base = NSString::alloc(ptr::null_mut()).init_str(BASE_URL);
                let url: *mut Object = msg_send![class!(NSURL), URLWithString:base];
                let markup = NSString::alloc(ptr::null_mut()).init_str(html);
                let _: *mut Object = msg_send![web, loadHTMLString:markup baseURL:url];
                let _: () = msg_send![markup, release];
                let _: () = msg_send![base, release];
                Some(Self {
                    web,
                    parent,
                    handler,
                    controller,
                    last_snapshot: String::new(),
                    visible: false,
                    _main_thread: PhantomData,
                })
            }
        }

        /// Call with actual GPUI layout bounds, in logical pixels from the top left.
        pub fn sync(&mut self, bounds: Option<Bounds<Pixels>>, snapshot: &serde_json::Value) {
            self.set_bounds(bounds);
            let serialized = snapshot.to_string();
            if self.last_snapshot != serialized {
                self.last_snapshot = serialized;
                self.send_snapshot();
            }
        }

        pub fn set_bounds(&mut self, bounds: Option<Bounds<Pixels>>) {
            unsafe {
                if let Some(bounds) = bounds {
                    let native: NSRect = msg_send![self.parent, bounds];
                    let flipped: bool = msg_send![self.parent, isFlipped];
                    let x = f64::from(f32::from(bounds.origin.x));
                    let y = f64::from(f32::from(bounds.origin.y));
                    let width = f64::from(f32::from(bounds.size.width)).max(0.);
                    let height = f64::from(f32::from(bounds.size.height)).max(0.);
                    let y = if flipped {
                        y
                    } else {
                        native.size.height - y - height
                    };
                    let frame = NSRect::new(NSPoint::new(x, y), NSSize::new(width, height));
                    let _: () = msg_send![self.web, setFrame:frame];
                    self.visible = width > 0. && height > 0.;
                } else {
                    if self.is_focused() {
                        self.focus_canvas();
                    }
                    self.visible = false;
                }
                let _: () = msg_send![self.web, setHidden:!self.visible];
            }
        }

        /// Force a snapshot after the frontend sends `ready` (initial JS is async).
        pub fn refresh(&mut self, snapshot: &serde_json::Value) {
            self.last_snapshot = snapshot.to_string();
            self.send_snapshot();
        }

        fn send_snapshot(&self) {
            if !self.last_snapshot.is_empty() {
                self.evaluate(&format!(
                    "window.pathweaverUpdate && window.pathweaverUpdate({})",
                    self.last_snapshot
                ));
            }
        }

        pub fn evaluate(&self, javascript: &str) {
            unsafe {
                let script = NSString::alloc(ptr::null_mut()).init_str(javascript);
                let _: () = msg_send![self.web, evaluateJavaScript:script completionHandler:ptr::null::<Object>()];
                let _: () = msg_send![script, release];
            }
        }

        pub fn is_focused(&self) -> bool {
            unsafe {
                let window: *mut Object = msg_send![self.parent, window];
                let responder: *mut Object = msg_send![window, firstResponder];
                if responder.is_null() {
                    return false;
                }
                let view: bool = msg_send![responder, isKindOfClass:class!(NSView)];
                view && (responder == self.web || {
                    let descendant: bool = msg_send![responder, isDescendantOf:self.web];
                    descendant
                })
            }
        }

        /// Route clipboard actions directly to WebKit's native first responder.
        /// A nil target would fall back to the app menu and recurse into GPUI.
        pub fn native_edit(&self, command: &str) -> bool {
            if !self.is_focused() {
                return false;
            }
            let selector = match command {
                "cut" => sel!(cut:),
                "copy" => sel!(copy:),
                "paste" => sel!(paste:),
                "selectAll" => sel!(selectAll:),
                _ => return false,
            };
            unsafe {
                let window: *mut Object = msg_send![self.parent, window];
                let responder: *mut Object = msg_send![window, firstResponder];
                let responds: bool = msg_send![responder, respondsToSelector:selector];
                if !responds {
                    return false;
                }
                let app: *mut Object = msg_send![class!(NSApplication), sharedApplication];
                msg_send![app, sendAction:selector to:responder from:ptr::null_mut::<Object>()]
            }
        }

        pub fn focus_canvas(&self) {
            unsafe {
                let window: *mut Object = msg_send![self.parent, window];
                let _: bool = msg_send![window, makeFirstResponder:self.parent];
            }
        }
    }

    impl Drop for TweeqHost {
        fn drop(&mut self) {
            unsafe {
                if self.is_focused() {
                    self.focus_canvas();
                }
                let _: () = msg_send![self.web, stopLoading];
                let _: () = msg_send![self.web, setNavigationDelegate:ptr::null_mut::<Object>()];
                let name = NSString::alloc(ptr::null_mut()).init_str("pathweaver");
                let _: () = msg_send![self.controller, removeScriptMessageHandlerForName:name];
                let _: () = msg_send![name, release];
                let _: () = msg_send![self.web, removeFromSuperview];
                let _: () = msg_send![self.web, release];
                let _: () = msg_send![self.controller, release];
                let _: () = msg_send![self.handler, release];
                let _: () = msg_send![self.parent, release];
            }
        }
    }
}

#[cfg(target_os = "macos")]
pub use macos::TweeqHost;

#[cfg(not(target_os = "macos"))]
mod fallback {
    use gpui::{Bounds, Pixels, Window};
    pub struct TweeqHost;
    impl TweeqHost {
        pub fn new(
            _: &Window,
            _: &str,
            _: &'static str,
            _: async_channel::Sender<(String, String)>,
        ) -> Option<Self> {
            None
        }
        pub fn sync(&mut self, _: Option<Bounds<Pixels>>, _: &serde_json::Value) {}
        pub fn set_bounds(&mut self, _: Option<Bounds<Pixels>>) {}
        pub fn refresh(&mut self, _: &serde_json::Value) {}
        pub fn evaluate(&self, _: &str) {}
        pub fn is_focused(&self) -> bool {
            false
        }
        /// Route clipboard actions directly to WebKit's native first responder.
        /// A nil target would fall back to the app menu and recurse into GPUI.
        pub fn native_edit(&self, command: &str) -> bool {
            if !self.is_focused() {
                return false;
            }
            let selector = match command {
                "cut" => sel!(cut:),
                "copy" => sel!(copy:),
                "paste" => sel!(paste:),
                "selectAll" => sel!(selectAll:),
                _ => return false,
            };
            unsafe {
                let window: *mut Object = msg_send![self.parent, window];
                let responder: *mut Object = msg_send![window, firstResponder];
                let responds: bool = msg_send![responder, respondsToSelector:selector];
                if !responds {
                    return false;
                }
                let app: *mut Object = msg_send![class!(NSApplication), sharedApplication];
                msg_send![app, sendAction:selector to:responder from:ptr::null_mut::<Object>()]
            }
        }

        pub fn native_edit(&self, _: &str) -> bool {
            false
        }
        pub fn focus_canvas(&self) {}
    }
}
#[cfg(not(target_os = "macos"))]
pub use fallback::TweeqHost;
