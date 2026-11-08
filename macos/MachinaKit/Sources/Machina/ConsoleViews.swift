import SwiftUI
import AppKit
import MachinaKit

// MARK: VNC

@MainActor @Observable
final class VNCConsoleModel {
    enum State: Equatable { case idle, connecting, connected, failed(String) }
    var state: State = .idle
    var image: CGImage?
    var size = CGSize.zero
    var desktopName = ""
    private var rfb: RFBClient?
    private var task: Task<Void, Never>?
    private var dirty = false
    private var rendering = false

    func connect(_ vm: VmInfo, api: MachinaClient) {
        disconnect()
        state = .connecting
        task = Task { [weak self] in
            do {
                let transport = try await api.openConsole(vm, kind: .vnc)
                let r = RFBClient(transport: transport)
                self?.rfb = r
                try await r.connect()
                guard let self else { return }
                self.desktopName = await r.desktopName
                self.size = CGSize(width: await r.width, height: await r.height)
                self.state = .connected
                Task { await r.run() }
                for await ev in r.events {
                    switch ev {
                    case .frame: self.markDirty(r)
                    case .resized(let w, let h): self.size = CGSize(width: w, height: h)
                    case .clipboard(let t): NSPasteboard.general.clearContents(); NSPasteboard.general.setString(t, forType: .string)
                    case .bell: NSSound.beep()
                    case .closed(let why): if self.state == .connected || self.state == .connecting { self.state = .failed(why ?? "Connection closed") }
                    }
                }
            } catch {
                self?.state = .failed(error.localizedDescription)
            }
        }
    }

    /// Redraw at most every ~30 ms, with a trailing redraw so the last frame is never left stale.
    private func markDirty(_ r: RFBClient) {
        dirty = true
        guard !rendering else { return }
        rendering = true
        Task { [weak self] in
            while let self, self.dirty {
                self.dirty = false
                self.image = await r.snapshot()
                try? await Task.sleep(nanoseconds: 30_000_000)
            }
            self?.rendering = false
        }
    }

    func disconnect() {
        task?.cancel(); task = nil
        if let r = rfb { Task { await r.close() } }
        rfb = nil; image = nil
        if state != .idle { state = .idle }
    }

    func key(_ sym: UInt32, down: Bool) { if let r = rfb { Task { await r.key(sym, down: down) } } }
    func pointer(_ x: Int, _ y: Int, _ mask: UInt8) { if let r = rfb { Task { await r.pointer(x: x, y: y, buttonMask: mask) } } }
    func scroll(_ x: Int, _ y: Int, up: Bool) { if let r = rfb { Task { await r.scroll(x: x, y: y, up: up) } } }
    func paste(_ text: String) { if let r = rfb { Task { await r.sendClipboard(text) } } }

    func ctrlAltDel() {
        let keys: [UInt32] = [0xffe3, 0xffe9, 0xffff]
        keys.forEach { key($0, down: true) }
        keys.reversed().forEach { key($0, down: false) }
    }
}

/// Draws the framebuffer aspect-fit and turns keyboard and mouse events into RFB input.
final class VNCNSView: NSView {
    var onKey: ((UInt32, Bool) -> Void)?
    var onPointer: ((Int, Int, UInt8) -> Void)?
    var onScroll: ((Int, Int, Bool) -> Void)?
    var framebuffer = CGSize.zero
    private var pressed: [UInt16: UInt32] = [:]
    private var buttons: UInt8 = 0
    private var area: NSTrackingArea?

    override var isFlipped: Bool { true }
    override var acceptsFirstResponder: Bool { true }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

    override init(frame: NSRect) { super.init(frame: frame); wantsLayer = true; layer?.contentsGravity = .resizeAspect; layer?.backgroundColor = NSColor.black.cgColor }
    required init?(coder: NSCoder) { fatalError() }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let a = area { removeTrackingArea(a) }
        area = NSTrackingArea(rect: bounds, options: [.mouseMoved, .activeInKeyWindow, .inVisibleRect], owner: self)
        addTrackingArea(area!)
    }

    // Aspect-fit mapping from a view point to a framebuffer pixel.
    private func fbPoint(_ e: NSEvent) -> (Int, Int)? {
        guard framebuffer.width > 0, framebuffer.height > 0 else { return nil }
        let p = convert(e.locationInWindow, from: nil)
        let s = min(bounds.width / framebuffer.width, bounds.height / framebuffer.height)
        let ox = (bounds.width - framebuffer.width * s) / 2, oy = (bounds.height - framebuffer.height * s) / 2
        return (Int(((p.x - ox) / s).rounded(.down)), Int(((p.y - oy) / s).rounded(.down)))
    }

    private func send(_ e: NSEvent, _ bit: UInt8, down: Bool) {
        window?.makeFirstResponder(self)
        if down { buttons |= bit } else { buttons &= ~bit }
        if let (x, y) = fbPoint(e) { onPointer?(x, y, buttons) }
    }
    private func move(_ e: NSEvent) { if let (x, y) = fbPoint(e) { onPointer?(x, y, buttons) } }

    override func mouseDown(with e: NSEvent) { send(e, 1, down: true) }
    override func mouseUp(with e: NSEvent) { send(e, 1, down: false) }
    override func rightMouseDown(with e: NSEvent) { send(e, 4, down: true) }
    override func rightMouseUp(with e: NSEvent) { send(e, 4, down: false) }
    override func otherMouseDown(with e: NSEvent) { send(e, 2, down: true) }
    override func otherMouseUp(with e: NSEvent) { send(e, 2, down: false) }
    override func mouseMoved(with e: NSEvent) { move(e) }
    override func mouseDragged(with e: NSEvent) { move(e) }
    override func rightMouseDragged(with e: NSEvent) { move(e) }
    override func otherMouseDragged(with e: NSEvent) { move(e) }
    override func scrollWheel(with e: NSEvent) {
        guard e.scrollingDeltaY != 0, let (x, y) = fbPoint(e) else { return }
        onScroll?(x, y, e.scrollingDeltaY > 0)
    }

    // Command shortcuts stay with the app (Cmd-K, Cmd-Q, ...); everything else goes to the guest.
    override func performKeyEquivalent(with e: NSEvent) -> Bool { false }

    override func keyDown(with e: NSEvent) {
        if e.isARepeat, let s = pressed[e.keyCode] { onKey?(s, false); onKey?(s, true); return }
        guard let sym = KeySyms.keysym(keyCode: e.keyCode, characters: e.characters) else { return }
        pressed[e.keyCode] = sym
        onKey?(sym, true)
    }
    override func keyUp(with e: NSEvent) { if let s = pressed.removeValue(forKey: e.keyCode) { onKey?(s, false) } }

    override func flagsChanged(with e: NSEvent) {
        guard let sym = KeySyms.modifier(keyCode: e.keyCode) else { return }
        let flag: NSEvent.ModifierFlags
        switch e.keyCode { case 56, 60: flag = .shift; case 59, 62: flag = .control; case 58, 61: flag = .option; case 57: flag = .capsLock; default: flag = .command }
        let down = e.modifierFlags.contains(flag)
        if down { pressed[e.keyCode] = sym } else { pressed.removeValue(forKey: e.keyCode) }
        onKey?(sym, down)
    }

    override func resignFirstResponder() -> Bool {
        // Release anything still held so the guest never sees a stuck key.
        for (_, s) in pressed { onKey?(s, false) }
        pressed.removeAll()
        return super.resignFirstResponder()
    }
}

struct VNCSurface: NSViewRepresentable {
    let model: VNCConsoleModel
    func makeNSView(context: Context) -> VNCNSView {
        let v = VNCNSView()
        v.onKey = { [model] s, d in MainActor.assumeIsolated { model.key(s, down: d) } }
        v.onPointer = { [model] x, y, m in MainActor.assumeIsolated { model.pointer(x, y, m) } }
        v.onScroll = { [model] x, y, up in MainActor.assumeIsolated { model.scroll(x, y, up: up) } }
        return v
    }
    func updateNSView(_ v: VNCNSView, context: Context) {
        v.framebuffer = model.size
        v.layer?.contents = model.image
    }
}

// MARK: Serial

@MainActor @Observable
final class SerialConsoleModel {
    enum State: Equatable { case idle, connecting, connected, failed(String) }
    var state: State = .idle
    var text = ""
    private var buffer = TerminalBuffer()
    private var transport: WebSocketTransport?
    private var task: Task<Void, Never>?
    private var pending = false

    func connect(_ vm: VmInfo, api: MachinaClient) { connect { try await api.openConsole(vm, kind: .serial) } }

    /// Any text WebSocket (a VM serial port, a bare-metal SOL session).
    func connect(_ open: @escaping @Sendable () async throws -> WebSocketTransport) {
        disconnect(); buffer = TerminalBuffer(); text = ""; state = .connecting
        task = Task { [weak self] in
            do {
                let t = try await open()
                self?.transport = t
                self?.state = .connected
                while !Task.isCancelled {
                    let d = try await t.receive()
                    guard let self else { return }
                    self.buffer.feed(String(decoding: d, as: UTF8.self))
                    self.schedule()
                }
            } catch { if !Task.isCancelled { self?.state = .failed(error.localizedDescription) } }
        }
    }

    private func schedule() {
        guard !pending else { return }
        pending = true
        Task { [weak self] in
            try? await Task.sleep(nanoseconds: 40_000_000)
            guard let self else { return }
            self.text = self.buffer.text; self.pending = false
        }
    }

    func send(_ bytes: [UInt8]) { if let t = transport { Task { try? await t.send(Data(bytes)) } } }
    func disconnect() { task?.cancel(); task = nil; transport?.close(); transport = nil; if state != .idle { state = .idle } }
}

final class SerialTextView: NSTextView {
    var onBytes: (([UInt8]) -> Void)?
    override var acceptsFirstResponder: Bool { true }
    override func keyDown(with e: NSEvent) {
        let ctrl = e.modifierFlags.contains(.control)
        if let b = TerminalKeys.bytes(keyCode: e.keyCode, characters: ctrl ? e.charactersIgnoringModifiers : e.characters, control: ctrl) { onBytes?(b) }
    }
    override func performKeyEquivalent(with e: NSEvent) -> Bool { false }
    override func mouseDown(with e: NSEvent) { window?.makeFirstResponder(self); super.mouseDown(with: e) }
}

struct SerialSurface: NSViewRepresentable {
    let model: SerialConsoleModel
    func makeNSView(context: Context) -> NSScrollView {
        let scroll = NSScrollView(); scroll.hasVerticalScroller = true
        let tv = SerialTextView(); tv.isEditable = false; tv.isSelectable = true
        tv.font = .monospacedSystemFont(ofSize: 12, weight: .regular)
        tv.backgroundColor = .black; tv.textColor = NSColor(white: 0.85, alpha: 1)
        tv.autoresizingMask = [.width]; tv.isVerticallyResizable = true
        tv.onBytes = { [model] b in MainActor.assumeIsolated { model.send(b) } }
        scroll.documentView = tv
        return scroll
    }
    func updateNSView(_ scroll: NSScrollView, context: Context) {
        guard let tv = scroll.documentView as? NSTextView, tv.string != model.text else { return }
        tv.string = model.text
        tv.scrollToEndOfDocument(nil)
    }
}

// MARK: Tab

struct VMConsoleTab: View {
    /// Off on purpose: on the test host, closing the serial console WebSocket was followed by the daemon exiting and being
    /// restarted by systemd (suspected cause: the daemon opens the VM's PTY without O_NOCTTY). Turn this on once the
    /// daemon is fixed; the serial code path is otherwise complete.
    static let serialEnabled = false

    @Environment(AppModel.self) private var model
    let vm: VmInfo
    enum Kind: String, CaseIterable, Identifiable { case graphics = "Graphics (VNC)", serial = "Serial"; var id: String { rawValue } }
    @State private var kind: Kind = .graphics
    @State private var vnc = VNCConsoleModel()
    @State private var serial = SerialConsoleModel()

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                if Self.serialEnabled {
                    Picker("", selection: $kind) { ForEach(Kind.allCases) { Text($0.rawValue).tag($0) } }
                        .pickerStyle(.segmented).labelsHidden().frame(width: 280)
                        .onChange(of: kind) { _, _ in vnc.disconnect(); serial.disconnect() }
                } else {
                    Label("Graphics (VNC)", systemImage: "display").font(.headline)
                }
                Spacer()
                if kind == .graphics, vnc.state == .connected {
                    Button("Ctrl-Alt-Del") { vnc.ctrlAltDel() }
                    Button("Paste") { if let s = NSPasteboard.general.string(forType: .string) { vnc.paste(s) } }
                }
                if connected { Button("Disconnect") { vnc.disconnect(); serial.disconnect() } }
                else { Button("Connect") { connect() }.buttonStyle(.borderedProminent).disabled(!vm.isRunning || isConnecting) }
            }
            if !vm.isRunning { Text("Start the VM to open its console.").foregroundStyle(.secondary) }
            ZStack {
                switch kind {
                case .graphics: VNCSurface(model: vnc)
                case .serial: SerialSurface(model: serial)
                }
                overlay
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity).background(.black).clipShape(.rect(cornerRadius: 8))
            Text(kind == .graphics ? "Click the display to capture the keyboard. Command shortcuts stay with the app."
                                   : "Click the console to type. Full-screen programs (vi, top) do not render in this simple terminal.")
                .font(.caption).foregroundStyle(.secondary)
            if !Self.serialEnabled {
                Text("The serial console is turned off: on the tested daemon, closing it made the daemon exit and restart.")
                    .font(.caption).foregroundStyle(.secondary)
            }
        }
        .padding(24)
        .onDisappear { vnc.disconnect(); serial.disconnect() }
    }

    private var connected: Bool { kind == .graphics ? vnc.state == .connected : serial.state == .connected }
    private var isConnecting: Bool { kind == .graphics ? vnc.state == .connecting : serial.state == .connecting }

    @ViewBuilder private var overlay: some View {
        let s: String? = {
            switch kind == .graphics ? vnc.state.description : serial.state.description {
            case "connecting": return "Connecting…"
            case let f where f.hasPrefix("failed:"): return String(f.dropFirst(7))
            case "idle": return vm.isRunning ? "Not connected" : nil
            default: return nil
            }
        }()
        if let s { Text(s).foregroundStyle(.white.opacity(0.8)).multilineTextAlignment(.center).padding() }
    }

    private func connect() {
        guard let api = try? model.api() else { return }
        if kind == .graphics || !Self.serialEnabled { vnc.connect(vm, api: api) } else { serial.connect(vm, api: api) }
    }
}

private extension VNCConsoleModel.State {
    var description: String { switch self { case .idle: "idle"; case .connecting: "connecting"; case .connected: "connected"; case .failed(let m): "failed:" + m } }
}
private extension SerialConsoleModel.State {
    var description: String { switch self { case .idle: "idle"; case .connecting: "connecting"; case .connected: "connected"; case .failed(let m): "failed:" + m } }
}
