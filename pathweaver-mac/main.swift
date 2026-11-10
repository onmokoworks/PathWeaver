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
        case "pointerLock":
            setPointerLock(m["value"] as? Bool ?? false)
        case "libraryList":
            sendLibrary()
        case "librarySave":
            let name = safeName(m["name"] as? String ?? "外観")
            do {
                try (m["text"] as? String ?? "").write(to: libraryDir.appendingPathComponent("\(name).pwn"), atomically: true, encoding: .utf8)
            } catch {
                js("window.pathweaver.failed(\(json("ライブラリに保存できませんでした：\(error.localizedDescription)")))")
            }
            sendLibrary()
        case "libraryDelete":
            try? FileManager.default.trashItem(at: libraryDir.appendingPathComponent("\(safeName(m["name"] as? String ?? "")).pwn"), resultingItemURL: nil)
            sendLibrary()
        case "libraryReveal":
            NSWorkspace.shared.open(libraryDir)
        case "lookExport":
            let panel = NSSavePanel()
            panel.allowedContentTypes = [UTType(filenameExtension: "pwn") ?? .json]
            panel.nameFieldStringValue = "\(safeName(m["name"] as? String ?? "外観")).pwn"
            panel.beginSheetModal(for: window) { r in
                if r == .OK, let url = panel.url { try? (m["text"] as? String ?? "").write(to: url, atomically: true, encoding: .utf8) }
            }
        case "lookImport":
            let panel = NSOpenPanel()
            panel.allowedContentTypes = [UTType(filenameExtension: "pwn") ?? .json]
            panel.allowsMultipleSelection = true
            panel.beginSheetModal(for: window) { r in
                guard r == .OK else { return }
                for url in panel.urls {
                    let dest = self.libraryDir.appendingPathComponent(url.lastPathComponent)
                    try? FileManager.default.removeItem(at: dest)
                    try? FileManager.default.copyItem(at: url, to: dest)
                }
                self.sendLibrary()
            }
        case "place":
            // File → Place: SVG arrives as text, bitmaps as data URLs (formats
            // WebKit can't show, like HEIC or TIFF, are converted to PNG).
            let panel = NSOpenPanel()
            panel.allowedContentTypes = [.svg, .png, .jpeg, .gif, .webP, .heic, .tiff, .bmp].compactMap { $0 }
            panel.allowsMultipleSelection = true
            panel.beginSheetModal(for: window) { r in
                guard r == .OK else { return }
                for url in panel.urls { self.sendPlaced(url) }
            }
        case "fonts":
            let names = NSFontManager.shared.availableFontFamilies.filter { !$0.hasPrefix(".") }
            js("window.pathweaver.fonts(\(json(names)))")
        case "glyphs":
            let id = m["id"] as? Int ?? 0
            let rings = glyphOutlines(text: m["text"] as? String ?? "", family: m["font"] as? String ?? "",
                                      size: m["size"] as? Double ?? 12, lineHeight: m["lineHeight"] as? Double ?? 1.2)
            js("window.pathweaver.glyphs(\(id), \(json(rings)))")
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

    func sendPlaced(_ url: URL) {
        let name = url.lastPathComponent
        if url.pathExtension.lowercased() == "svg" {
            guard let text = try? String(contentsOf: url, encoding: .utf8) else {
                js("window.pathweaver.failed(\(json("「\(name)」を読めませんでした")))"); return
            }
            js("window.pathweaver.placed(\(json(name)), 'svg', \(json(text)))")
            return
        }
        guard let data = try? Data(contentsOf: url) else {
            js("window.pathweaver.failed(\(json("「\(name)」を読めませんでした")))"); return
        }
        let ext = url.pathExtension.lowercased()
        var mime = ["png": "image/png", "jpg": "image/jpeg", "jpeg": "image/jpeg", "gif": "image/gif", "webp": "image/webp"][ext]
        var bytes = data
        if mime == nil {
            guard let rep = NSBitmapImageRep(data: data), let png = rep.representation(using: .png, properties: [:]) else {
                js("window.pathweaver.failed(\(json("「\(name)」は画像として読めませんでした")))"); return
            }
            bytes = png
            mime = "image/png"
        }
        js("window.pathweaver.placed(\(json(name)), 'image', \(json("data:\(mime!);base64,\(bytes.base64EncodedString())")))")
    }

    /// Glyph outlines laid out like CSS (see textOutline.ts): origin = top-left
    /// of the first line box, line box = size × lineHeight, baseline placed by
    /// half-leading from the primary font's ascent/descent. CoreText picks
    /// fallback fonts (Japanese in a Latin font) per run. Rings come back
    /// flattened as [x0, y0, x1, y1, …].
    func glyphOutlines(text: String, family: String, size: Double, lineHeight: Double) -> [[Double]] {
        let base = NSFontDescriptor(fontAttributes: [.family: family])
        let font = NSFont(descriptor: base, size: size) ?? NSFont(name: family, size: size) ?? NSFont.systemFont(ofSize: size)
        let ctFont = font as CTFont
        let ascent = Double(CTFontGetAscent(ctFont)), descent = Double(CTFontGetDescent(ctFont))
        let lineBox = size * lineHeight
        var rings: [[Double]] = []
        let tolerance = max(0.02, size * 0.004)
        for (i, line) in text.components(separatedBy: "\n").enumerated() {
            if line.isEmpty { continue }
            let baseline = Double(i) * lineBox + (lineBox - (ascent + descent)) / 2 + ascent
            let attr = NSAttributedString(string: line, attributes: [.font: font, NSAttributedString.Key(kCTLigatureAttributeName as String): 1])
            let ctLine = CTLineCreateWithAttributedString(attr)
            for run in (CTLineGetGlyphRuns(ctLine) as? [CTRun]) ?? [] {
                let attrs = CTRunGetAttributes(run) as NSDictionary
                let runFont = (attrs[kCTFontAttributeName] as! CTFont?) ?? ctFont
                let n = CTRunGetGlyphCount(run)
                var glyphs = [CGGlyph](repeating: 0, count: n)
                var pos = [CGPoint](repeating: .zero, count: n)
                CTRunGetGlyphs(run, CFRange(location: 0, length: n), &glyphs)
                CTRunGetPositions(run, CFRange(location: 0, length: n), &pos)
                for k in 0..<n {
                    // Glyph space is y-up: flip into the page's y-down space.
                    var m = CGAffineTransform(a: 1, b: 0, c: 0, d: -1, tx: pos[k].x, ty: CGFloat(baseline) - pos[k].y)
                    guard let path = CTFontCreatePathForGlyph(runFont, glyphs[k], &m) else { continue }
                    rings.append(contentsOf: flattenPath(path, tolerance: tolerance))
                }
            }
        }
        return rings
    }
    func flattenPath(_ path: CGPath, tolerance: Double) -> [[Double]] {
        var rings: [[Double]] = []
        var ring: [Double] = []
        var last = CGPoint.zero, start = CGPoint.zero
        func close() { if ring.count >= 6 { rings.append(ring) }; ring = [] }
        func add(_ p: CGPoint) { ring.append(Double(p.x)); ring.append(Double(p.y)) }
        func steps(_ len: CGFloat) -> Int { max(2, min(32, Int((Double(len) / tolerance).squareRoot() / 2) + 1)) }
        path.applyWithBlock { el in
            let e = el.pointee
            switch e.type {
            case .moveToPoint:
                close(); start = e.points[0]; last = start; add(start)
            case .addLineToPoint:
                last = e.points[0]; add(last)
            case .addQuadCurveToPoint:
                let c = e.points[0], p = e.points[1]
                let n = steps(hypot(c.x - last.x, c.y - last.y) + hypot(p.x - c.x, p.y - c.y))
                for i in 1...n {
                    let t = CGFloat(i) / CGFloat(n), u = 1 - t
                    add(CGPoint(x: u * u * last.x + 2 * u * t * c.x + t * t * p.x, y: u * u * last.y + 2 * u * t * c.y + t * t * p.y))
                }
                last = p
            case .addCurveToPoint:
                let c1 = e.points[0], c2 = e.points[1], p = e.points[2]
                let n = steps(hypot(c1.x - last.x, c1.y - last.y) + hypot(c2.x - c1.x, c2.y - c1.y) + hypot(p.x - c2.x, p.y - c2.y))
                for i in 1...n {
                    let t = CGFloat(i) / CGFloat(n), u = 1 - t
                    let a = u * u * u, b = 3 * u * u * t, cc = 3 * u * t * t, d = t * t * t
                    add(CGPoint(x: a * last.x + b * c1.x + cc * c2.x + d * p.x, y: a * last.y + b * c1.y + cc * c2.y + d * p.y))
                }
                last = p
            case .closeSubpath:
                close(); last = start
            @unknown default: break
            }
        }
        close()
        return rings
    }

    /// Diagnostic run (PATHWEAVER_SELFTEST=/path/prefix): load the sample,
    /// report state and blank icons, snapshot the real WKWebView, then quit.
    func selfTest(_ out: String) {
        js("window.pathweaver.command('example')")
        // Live text through CoreText (Latin + Japanese fallback): ring count.
        js("window.__pw.textProbe('Aあ')")
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.0) {
            self.js("document.querySelector('[data-path]')?.click()")
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.8) {
                let probe = "JSON.stringify({strokes: window.__pw.ed.doc.strokes.length, selected: window.__pw.ed.selected.length, blankIcons: [...document.querySelectorAll('svg.iconify')].filter(e => !e.innerHTML.trim()).length, error: window.__pw.ed.error, textRings: window.__pwTextProbe})"
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

    // MARK: look library (like After Effects' .ffx presets): one .pwn file
    // per look in ~/Library/Application Support/PathWeaver/Looks, shared by
    // every document.
    lazy var libraryDir: URL = {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        let dir = base.appendingPathComponent("PathWeaver/Looks", isDirectory: true)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir
    }()
    func safeName(_ s: String) -> String {
        let cleaned = s.replacingOccurrences(of: "/", with: "／").replacingOccurrences(of: ":", with: "：").trimmingCharacters(in: .whitespaces)
        return cleaned.isEmpty ? "外観" : cleaned
    }
    func sendLibrary() {
        let files = (try? FileManager.default.contentsOfDirectory(at: libraryDir, includingPropertiesForKeys: nil)) ?? []
        let items: [[String: String]] = files.filter { $0.pathExtension == "pwn" }
            .sorted { $0.lastPathComponent.localizedStandardCompare($1.lastPathComponent) == .orderedAscending }
            .compactMap { url in
                guard let text = try? String(contentsOf: url, encoding: .utf8) else { return nil }
                return ["name": url.deletingPathExtension().lastPathComponent, "text": text]
            }
        guard let data = try? JSONSerialization.data(withJSONObject: items), let list = String(data: data, encoding: .utf8) else { return }
        js("window.pathweaver.library && window.pathweaver.library(\(list))")
    }

    // MARK: pointer lock (WKWebView does not grant the web API)
    /// Freeze and hide the cursor and feed raw mouse deltas to the page, so a
    /// numeric drag keeps going past the screen edge. Released on mouse up,
    /// when the page asks, or when the window loses focus.
    var lockMonitor: Any?
    func setPointerLock(_ on: Bool) {
        if on {
            guard lockMonitor == nil else { return }
            CGAssociateMouseAndMouseCursorPosition(0)
            NSCursor.hide()
            lockMonitor = NSEvent.addLocalMonitorForEvents(matching: [.leftMouseDragged, .mouseMoved, .leftMouseUp]) { [weak self] e in
                guard let self else { return e }
                if e.type == .leftMouseUp {
                    DispatchQueue.main.async { self.setPointerLock(false) }
                } else if e.deltaX != 0 || e.deltaY != 0 {
                    self.js("window.__pwPointerDelta && window.__pwPointerDelta(\(e.deltaX), \(e.deltaY))")
                }
                return e
            }
        } else {
            guard let m = lockMonitor else { return }
            NSEvent.removeMonitor(m)
            lockMonitor = nil
            CGAssociateMouseAndMouseCursorPosition(1)
            NSCursor.unhide()
            js("window.__pwPointerUnlocked && window.__pwPointerUnlocked()")
        }
    }
    func windowDidResignKey(_ notification: Notification) { setPointerLock(false) }
    func json(_ v: Any) -> String {
        let d = try! JSONSerialization.data(withJSONObject: v)
        return String(data: d, encoding: .utf8)!
    }
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
            item("配置…（SVG・画像）", "place", "p", [.command, .shift]),
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
        add("オブジェクト", [
            item("ブレンドを作成", "makeBlend", "b", [.command, .option]),
            item("ブレンドを解除", "releaseBlend", "b", [.command, .option, .shift]),
            item("ブレンドを拡張", "expandBlend"),
        ])
        add("書式", [
            item("アウトラインを作成", "outline", "o", [.command, .shift]),
        ])
        add("表示", [
            item("グリッドを表示して吸着", "snapGrid", "'"), item("ガイドに吸着", "snapGuides", ";"),
            item("接線に吸着", "snapTangent"), item("法線に吸着", "snapNormal"),
            .separator(),
            item("ガイドを作成", "makeGuides", "5"), item("ガイドを解除", "releaseGuides", "5", [.command, .option]),
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
            item("図形（M 長方形・L 楕円・\\ 直線）", "toolShape"),
            item("文字（T）", "toolText"),
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
