// PathWeaver macOS host: one window, one WKWebView running the Vue + Tweeq UI
// (with pathweaver-core as wasm). The host only provides what a web page
// cannot: native menus, open/save panels, file writes and the close prompt.
import AppKit
import UniformTypeIdentifiers
import WebKit

/// The page's top bar acts as the title bar: the page reports when the
/// pointer is over an empty part of it, and presses there move the window
/// (drag) or zoom it to fill the screen (double-click), like a native title bar.
final class HostWebView: WKWebView {
    var dragRegion = false
    override func mouseDown(with event: NSEvent) {
        guard dragRegion, let window else { return super.mouseDown(with: event) }
        guard event.clickCount >= 2 else { return window.performDrag(with: event) }
        // Same as a native title bar: follow System Settings › Desktop & Dock ›
        // "Double-click a window's title bar to" (zoom by default, not full screen).
        switch UserDefaults.standard.string(forKey: "AppleActionOnDoubleClick") {
        case "Minimize": window.performMiniaturize(nil)
        case "None": break
        default: window.zoom(nil)
        }
    }
}

final class Host: NSObject, NSApplicationDelegate, NSWindowDelegate, WKScriptMessageHandler, WKNavigationDelegate, WKUIDelegate, NSMenuItemValidation {
    var window: NSWindow!
    var web: HostWebView!
    var dirty = false
    var path: String?
    var title = "名称未設定"
    var canUndo = false, canRedo = false, hasSelection = false
    var snap: [String: Bool] = [:]
    var closeAfterSave = false
    var ready = false
    var pendingOpen: String?

    func applicationDidFinishLaunching(_ note: Notification) {
        let config = WKWebViewConfiguration()
        config.userContentController.add(self, name: "pathweaver")
        config.preferences.setValue(true, forKey: "developerExtrasEnabled")
        web = HostWebView(frame: .zero, configuration: config)
        web.navigationDelegate = self
        web.uiDelegate = self
        web.setValue(false, forKey: "drawsBackground")
        if #available(macOS 13.3, *) { web.isInspectable = true }

        window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1440, height: 900),
                          styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView],
                          backing: .buffered, defer: false)
        window.titlebarAppearsTransparent = true
        window.titleVisibility = .hidden
        window.minSize = NSSize(width: 900, height: 560)
        window.collectionBehavior.insert(.fullScreenPrimary)
        window.contentView = web
        window.delegate = self
        window.setFrameAutosaveName("PathWeaverMain")
        if !window.setFrameUsingName("PathWeaverMain") { window.center() }
        window.makeKeyAndOrderFront(nil)

        buildMenu()
        let index = Bundle.main.url(forResource: "index", withExtension: "html", subdirectory: "web")!
        web.loadFileURL(index, allowingReadAccessTo: index.deletingLastPathComponent())
        NSApp.activate(ignoringOtherApps: true)
    }

    func application(_ sender: NSApplication, openFile filename: String) -> Bool {
        if ready { openPath(filename) } else { pendingOpen = filename }
        return true
    }
    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }
    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        if !dirty { return .terminateNow }
        window.performClose(nil)
        return .terminateCancel
    }

    // MARK: JS → native
    func userContentController(_ c: WKUserContentController, didReceive message: WKScriptMessage) {
        guard let text = message.body as? String,
              let data = text.data(using: .utf8),
              let m = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let type = m["type"] as? String else { return }
        switch type {
        case "ready":
            ready = true
            if let p = pendingOpen { pendingOpen = nil; openPath(p) }
            if let out = ProcessInfo.processInfo.environment["PATHWEAVER_SELFTEST"] { selfTest(out) }
        case "error":
            FileHandle.standardError.write("JS error: \(m["message"] ?? "")\n".data(using: .utf8)!)
        case "state":
            dirty = m["dirty"] as? Bool ?? false
            title = m["title"] as? String ?? title
            path = m["path"] as? String
            window.title = title
            window.isDocumentEdited = dirty
            window.representedURL = path.map { URL(fileURLWithPath: $0) }
        case "dragRegion":
            web.dragRegion = m["value"] as? Bool ?? false
        case "menu":
            canUndo = m["undo"] as? Bool ?? false
            canRedo = m["redo"] as? Bool ?? false
            hasSelection = m["selection"] as? Bool ?? false
            snap = m["snap"] as? [String: Bool] ?? snap
        case "open":
            let panel = NSOpenPanel()
            panel.allowedContentTypes = [UTType(filenameExtension: "pathweaver") ?? .json, .json]
            panel.allowsOtherFileTypes = true
            panel.beginSheetModal(for: window) { r in
                if r == .OK, let url = panel.url { self.openPath(url.path) }
            }
        case "save":
            let body = m["text"] as? String ?? ""
            if let p = m["path"] as? String, !p.isEmpty {
                write(body, to: URL(fileURLWithPath: p), callback: "saved")
            } else {
                let panel = NSSavePanel()
                panel.allowedContentTypes = [UTType(filenameExtension: "pathweaver") ?? .json]
                panel.nameFieldStringValue = (m["name"] as? String).map { $0.hasSuffix(".pathweaver") ? $0 : "\($0).pathweaver" } ?? "名称未設定.pathweaver"
                panel.beginSheetModal(for: window) { r in
                    if r == .OK, let url = panel.url { self.write(body, to: url, callback: "saved") }
                    else { self.closeAfterSave = false }
                }
            }
        case "export":
            let panel = NSSavePanel()
            panel.allowedContentTypes = [.svg]
            panel.nameFieldStringValue = m["name"] as? String ?? "PathWeaver.svg"
            panel.beginSheetModal(for: window) { r in
                if r == .OK, let url = panel.url { self.write(m["text"] as? String ?? "", to: url, callback: "exported") }
            }
        default: break
        }
    }

    /// Diagnostic run (PATHWEAVER_SELFTEST=/path/prefix): load the sample,
    /// report state and blank icons, snapshot the real WKWebView, then quit.
    func selfTest(_ out: String) {
        js("window.pathweaver.command('example')")
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.0) {
            self.js("document.querySelector('[data-path]')?.click()")
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.8) {
                let probe = "JSON.stringify({strokes: window.__pw.ed.doc.strokes.length, selected: window.__pw.ed.selected.length, blankIcons: [...document.querySelectorAll('svg.iconify')].filter(e => !e.innerHTML.trim()).length, error: window.__pw.ed.error})"
                self.web.evaluateJavaScript(probe) { r, e in
                    print("selftest:", r ?? "nil", e.map { "\($0)" } ?? "")
                    self.web.takeSnapshot(with: nil) { image, _ in
                        if let tiff = image?.tiffRepresentation, let rep = NSBitmapImageRep(data: tiff),
                           let png = rep.representation(using: .png, properties: [:]) {
                            try? png.write(to: URL(fileURLWithPath: out + ".png"))
                        }
                        exit(0)
                    }
                }
            }
        }
    }

    func openPath(_ p: String) {
        if dirty {
            let a = NSAlert()
            a.messageText = "保存されていない変更があります"
            a.informativeText = "別の書類を開くと、現在の変更は失われます。"
            a.addButton(withTitle: "破棄して開く")
            a.addButton(withTitle: "キャンセル")
            if a.runModal() != .alertFirstButtonReturn { return }
        }
        do {
            let text = try String(contentsOfFile: p, encoding: .utf8)
            js("window.pathweaver.opened(\(json(p)), \(json(text)))")
            NSDocumentController.shared.noteNewRecentDocumentURL(URL(fileURLWithPath: p))
        } catch {
            js("window.pathweaver.failed(\(json("開けませんでした：\(error.localizedDescription)")))")
        }
    }

    /// Atomic write (temp file + rename), then notify the page.
    func write(_ text: String, to url: URL, callback: String) {
        do {
            try text.write(to: url, atomically: true, encoding: .utf8)
            js("window.pathweaver.\(callback)(\(json(url.path)))")
            if callback == "saved" {
                NSDocumentController.shared.noteNewRecentDocumentURL(url)
                if closeAfterSave {
                    closeAfterSave = false
                    dirty = false
                    DispatchQueue.main.async { self.window.close() }
                }
            }
        } catch {
            closeAfterSave = false
            js("window.pathweaver.failed(\(json("保存できませんでした：\(error.localizedDescription)")))")
        }
    }

    func js(_ code: String) { web.evaluateJavaScript(code, completionHandler: nil) }
    func json(_ s: String) -> String {
        let d = try! JSONSerialization.data(withJSONObject: [s])
        let a = String(data: d, encoding: .utf8)!
        return String(a.dropFirst().dropLast())
    }

    func windowDidEnterFullScreen(_ notification: Notification) { js("document.documentElement.classList.add('fullscreen')") }
    func windowDidExitFullScreen(_ notification: Notification) { js("document.documentElement.classList.remove('fullscreen')") }

    // MARK: page alert() / confirm() as sheets (WKWebView shows nothing without these)
    func webView(_ webView: WKWebView, runJavaScriptAlertPanelWithMessage message: String, initiatedByFrame frame: WKFrameInfo, completionHandler: @escaping () -> Void) {
        let a = NSAlert()
        a.messageText = message
        a.addButton(withTitle: "OK")
        a.beginSheetModal(for: window) { _ in completionHandler() }
    }
    func webView(_ webView: WKWebView, runJavaScriptConfirmPanelWithMessage message: String, initiatedByFrame frame: WKFrameInfo, completionHandler: @escaping (Bool) -> Void) {
        let a = NSAlert()
        a.messageText = message
        a.addButton(withTitle: "OK")
        a.addButton(withTitle: "キャンセル")
        a.beginSheetModal(for: window) { r in completionHandler(r == .alertFirstButtonReturn) }
    }

    /// <input type=file> (画像 node): the native open panel, images only.
    func webView(_ webView: WKWebView, runOpenPanelWith parameters: WKOpenPanelParameters, initiatedByFrame frame: WKFrameInfo, completionHandler: @escaping ([URL]?) -> Void) {
        let panel = NSOpenPanel()
        panel.allowsMultipleSelection = parameters.allowsMultipleSelection
        panel.canChooseDirectories = false
        panel.allowedContentTypes = [.image]
        panel.beginSheetModal(for: window) { r in completionHandler(r == .OK ? panel.urls : nil) }
    }

    // MARK: close prompt: 保存 / 保存しない / キャンセル
    func windowShouldClose(_ sender: NSWindow) -> Bool {
        if !dirty { return true }
        let a = NSAlert()
        a.messageText = "「\(title)」への変更を保存しますか？"
        a.informativeText = "保存しないと、変更は失われます。"
        a.addButton(withTitle: "保存")
        a.addButton(withTitle: "キャンセル")
        a.addButton(withTitle: "保存しない")
        a.beginSheetModal(for: window) { r in
            switch r {
            case .alertFirstButtonReturn:
                self.closeAfterSave = true
                self.js("window.pathweaver.command('save')")
            case .alertThirdButtonReturn:
                self.dirty = false
                self.window.close()
            default: break
            }
        }
        return false
    }

    // MARK: menus
    @objc func command(_ sender: NSMenuItem) {
        guard let name = sender.representedObject as? String else { return }
        js("window.pathweaver.command(\(json(name)))")
    }
    func item(_ title: String, _ name: String, _ key: String = "", _ mods: NSEvent.ModifierFlags = [.command]) -> NSMenuItem {
        let i = NSMenuItem(title: title, action: #selector(command(_:)), keyEquivalent: key)
        i.keyEquivalentModifierMask = mods
        i.representedObject = name
        i.target = self
        return i
    }
    func validateMenuItem(_ item: NSMenuItem) -> Bool {
        if let name = item.representedObject as? String, let on = snap[name] {
            item.state = on ? .on : .off
            return true
        }
        switch item.representedObject as? String {
        case "undo": return canUndo
        case "redo": return canRedo
        case "duplicate", "delete": return hasSelection
        default: return true
        }
    }
    func buildMenu() {
        let main = NSMenu()
        func add(_ title: String, _ items: [NSMenuItem]) {
            let top = NSMenuItem(title: title, action: nil, keyEquivalent: "")
            let m = NSMenu(title: title)
            items.forEach(m.addItem)
            top.submenu = m
            main.addItem(top)
        }
        add("PathWeaver", [
            NSMenuItem(title: "PathWeaverについて", action: #selector(NSApplication.orderFrontStandardAboutPanel(_:)), keyEquivalent: ""),
            .separator(),
            item("設定…", "settings", ","),
            .separator(),
            NSMenuItem(title: "PathWeaverを隠す", action: #selector(NSApplication.hide(_:)), keyEquivalent: "h"),
            .separator(),
            NSMenuItem(title: "PathWeaverを終了", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q"),
        ])
        add("ファイル", [
            item("新規", "new", "n"), item("開く…", "open", "o"), item("サンプルを開く", "example"),
            .separator(),
            NSMenuItem(title: "閉じる", action: #selector(NSWindow.performClose(_:)), keyEquivalent: "w"),
            item("保存", "save", "s"), item("別名で保存…", "saveAs", "s", [.command, .shift]),
            .separator(),
            item("SVGを書き出す…", "export", "e"),
        ])
        add("編集", [
            item("取り消す", "undo", "z"), item("やり直す", "redo", "z", [.command, .shift]),
            .separator(),
            NSMenuItem(title: "カット", action: #selector(NSText.cut(_:)), keyEquivalent: "x"),
            NSMenuItem(title: "コピー", action: #selector(NSText.copy(_:)), keyEquivalent: "c"),
            NSMenuItem(title: "ペースト", action: #selector(NSText.paste(_:)), keyEquivalent: "v"),
            item("複製", "duplicate", "d"), item("削除（Delete）", "delete"),
            .separator(),
            item("フォルダにまとめる", "group", "g"), item("フォルダを解除", "ungroup", "g", [.command, .shift]),
            .separator(),
            item("すべてを選択", "selectAll", "a"),
        ])
        add("表示", [
            item("グリッドを表示して吸着", "snapGrid", "'"), item("ガイドに吸着", "snapGuides", ";"),
            item("接線に吸着", "snapTangent"), item("法線に吸着", "snapNormal"),
            .separator(),
            item("全体を表示", "fit", "0"), item("100%", "actualSize", "1"),
            item("拡大", "zoomIn", "+"), item("縮小", "zoomOut", "-"),
            .separator(),
            item("ノードパネルを表示／隠す", "toggleNodes", "n", [.command, .option]),
            item("ライト／ダークを切り替え", "toggleTheme"),
            .separator(),
            {
                let f = NSMenuItem(title: "フルスクリーン", action: #selector(NSWindow.toggleFullScreen(_:)), keyEquivalent: "f")
                f.keyEquivalentModifierMask = [.command, .control]
                return f
            }(),
        ])
        add("ツール", [
            // Bare-key shortcuts (V, A, P, ⇧C) are handled by the page so they
            // never steal keys from text fields; the menu lists them for discovery.
            item("選択（V）", "toolSelect"), item("ダイレクト選択（A）", "toolDirect"),
            item("ペン（P）", "toolPen"), item("アンカーポイント（⇧C）", "toolConvert"),
        ])
        let windowMenu = NSMenu(title: "ウインドウ")
        windowMenu.addItem(NSMenuItem(title: "しまう", action: #selector(NSWindow.performMiniaturize(_:)), keyEquivalent: "m"))
        let w = NSMenuItem(title: "ウインドウ", action: nil, keyEquivalent: "")
        w.submenu = windowMenu
        main.addItem(w)
        NSApp.windowsMenu = windowMenu
        NSApp.mainMenu = main
    }
}

let app = NSApplication.shared
let host = Host()
app.delegate = host
app.setActivationPolicy(.regular)
app.run()
