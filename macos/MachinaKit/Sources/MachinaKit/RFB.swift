import Foundation
import CoreGraphics

public enum RFBError: Error, LocalizedError, Sendable {
    case badVersion(String), unusableSecurity([UInt8]), authFailed(String), protocolError(String), closed
    public var errorDescription: String? {
        switch self {
        case .badVersion(let v): return "Not a VNC server (got \(v.debugDescription))"
        case .unusableSecurity(let t): return t.contains(2) ? "This display is password protected (VNC authentication is not supported yet)" : "No supported VNC security type: \(t)"
        case .authFailed(let m): return "VNC refused the connection: \(m)"
        case .protocolError(let m): return "VNC protocol error: \(m)"
        case .closed: return "Console connection closed"
        }
    }
}

public enum RFBEvent: Sendable {
    case frame           // framebuffer changed; call snapshot()
    case resized(Int, Int)
    case bell
    case clipboard(String)
    case closed(String?)
}

/// 32-bit BGRA framebuffer (matches CGImage little-endian premultiplied-first, so no per-pixel swizzle).
struct Framebuffer {
    var width: Int, height: Int
    var pixels: [UInt8]
    init(width: Int, height: Int) { self.width = width; self.height = height; pixels = [UInt8](repeating: 0, count: width * height * 4) }

    mutating func resize(_ w: Int, _ h: Int) { self = Framebuffer(width: w, height: h) }

    private func ok(_ x: Int, _ y: Int, _ w: Int, _ h: Int) -> Bool { x >= 0 && y >= 0 && w >= 0 && h >= 0 && x + w <= width && y + h <= height }

    /// `src` holds w*h pixels as the server sends them: 4 bytes B,G,R,X.
    mutating func putRaw(_ x: Int, _ y: Int, _ w: Int, _ h: Int, _ src: ArraySlice<UInt8>) {
        guard ok(x, y, w, h), src.count >= w * h * 4 else { return }
        var s = src.startIndex
        for row in 0..<h {
            let d = ((y + row) * width + x) * 4
            for i in 0..<(w * 4) { pixels[d + i] = src[s + i] }
            for i in stride(from: 3, to: w * 4, by: 4) { pixels[d + i] = 255 }
            s += w * 4
        }
    }

    mutating func fill(_ x: Int, _ y: Int, _ w: Int, _ h: Int, _ b: UInt8, _ g: UInt8, _ r: UInt8) {
        guard ok(x, y, w, h) else { return }
        for row in 0..<h { for col in 0..<w { let d = ((y + row) * width + x + col) * 4; pixels[d] = b; pixels[d + 1] = g; pixels[d + 2] = r; pixels[d + 3] = 255 } }
    }

    mutating func copy(from sx: Int, _ sy: Int, to x: Int, _ y: Int, _ w: Int, _ h: Int) {
        guard ok(sx, sy, w, h), ok(x, y, w, h) else { return }
        var tmp = [UInt8](); tmp.reserveCapacity(w * h * 4)
        for row in 0..<h { let s = ((sy + row) * width + sx) * 4; tmp += pixels[s..<(s + w * 4)] }
        for row in 0..<h { let d = ((y + row) * width + x) * 4; pixels.replaceSubrange(d..<(d + w * 4), with: tmp[(row * w * 4)..<((row + 1) * w * 4)]) }
    }

    func image() -> CGImage? {
        guard width > 0, height > 0 else { return nil }
        let data = Data(pixels) as CFData
        guard let provider = CGDataProvider(data: data) else { return nil }
        return CGImage(width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: width * 4,
                       space: CGColorSpaceCreateDeviceRGB(),
                       bitmapInfo: CGBitmapInfo(rawValue: CGBitmapInfo.byteOrder32Little.rawValue | CGImageAlphaInfo.premultipliedFirst.rawValue),
                       provider: provider, decode: nil, shouldInterpolate: true, intent: .defaultIntent)
    }
}

/// RFB (VNC) 3.3/3.7/3.8 client: security "None", Raw / CopyRect / Hextile / DesktopSize, keyboard, pointer, clipboard.
/// Not supported yet: VNC password auth, ZRLE/Tight.
public actor RFBClient {
    public private(set) var width = 0
    public private(set) var height = 0
    public private(set) var desktopName = ""
    public nonisolated let events: AsyncStream<RFBEvent>
    private let continuation: AsyncStream<RFBEvent>.Continuation
    private let transport: ConsoleTransport
    private var fb = Framebuffer(width: 0, height: 0)
    private var buf = [UInt8]()
    private var head = 0
    private var buttons: UInt8 = 0

    public init(transport: ConsoleTransport) {
        self.transport = transport
        var c: AsyncStream<RFBEvent>.Continuation!
        events = AsyncStream { c = $0 }
        continuation = c
    }

    // MARK: byte reader

    private func need(_ n: Int) async throws {
        while buf.count - head < n {
            let chunk = try await transport.receive()
            if chunk.isEmpty { continue }
            if head > 65536 { buf.removeFirst(head); head = 0 }
            buf += chunk
        }
    }
    private func read(_ n: Int) async throws -> ArraySlice<UInt8> {
        try await need(n); defer { head += n }; return buf[head..<(head + n)]
    }
    private func u8() async throws -> Int { Int(try await read(1).first!) }
    private func u16() async throws -> Int { let s = try await read(2); return Int(s[s.startIndex]) << 8 | Int(s[s.startIndex + 1]) }
    private func u32() async throws -> UInt32 { try await read(4).reduce(0) { $0 << 8 | UInt32($1) } }

    private func send(_ bytes: [UInt8]) async throws { try await transport.send(Data(bytes)) }
    private func be16(_ v: Int) -> [UInt8] { [UInt8(v >> 8 & 0xff), UInt8(v & 0xff)] }
    private func be32(_ v: UInt32) -> [UInt8] { [UInt8(v >> 24 & 0xff), UInt8(v >> 16 & 0xff), UInt8(v >> 8 & 0xff), UInt8(v & 0xff)] }

    // MARK: handshake

    /// Version, security, init, our pixel format and encodings, and the first full update request.
    public func connect() async throws {
        let v = String(decoding: try await read(12), as: UTF8.self)
        guard v.hasPrefix("RFB "), v.hasSuffix("\n"), let minor = Int(v.dropFirst(8).prefix(3)) else { throw RFBError.badVersion(v) }
        let ours = minor >= 8 ? "RFB 003.008\n" : minor >= 7 ? "RFB 003.007\n" : "RFB 003.003\n"
        try await send(Array(ours.utf8))

        if minor >= 7 {
            let n = try await u8()
            if n == 0 { let len = Int(try await u32()); throw RFBError.authFailed(String(decoding: try await read(len), as: UTF8.self)) }
            let types = Array(try await read(n))
            guard types.contains(1) else { throw RFBError.unusableSecurity(types) }
            try await send([1])
        } else {
            let t = try await u32()
            guard t == 1 else { throw t == 0 ? RFBError.authFailed("connection failed") : RFBError.unusableSecurity([UInt8(truncatingIfNeeded: t)]) }
        }
        if minor >= 8 {   // 3.3 and 3.7 send no SecurityResult for "None"
            if try await u32() != 0 {
                let len = Int(try await u32()); throw RFBError.authFailed(String(decoding: try await read(len), as: UTF8.self))
            }
        }
        try await send([1])                                   // ClientInit: shared
        width = try await u16(); height = try await u16()
        _ = try await read(16)                                // server pixel format (we replace it)
        let nameLen = Int(try await u32())
        desktopName = String(decoding: try await read(nameLen), as: UTF8.self)
        fb = Framebuffer(width: width, height: height)

        // SetPixelFormat: 32 bpp, depth 24, little endian, true colour, 8 bits each, shifts R16 G8 B0 (memory order B,G,R,X).
        try await send([0, 0, 0, 0, 32, 24, 0, 1] + be16(255) + be16(255) + be16(255) + [16, 8, 0, 0, 0, 0])
        // SetEncodings: Hextile, CopyRect, Raw, DesktopSize pseudo-encoding.
        try await send([2, 0] + be16(4) + be32(5) + be32(1) + be32(0) + be32(UInt32(bitPattern: -223)))
        try await requestUpdate(incremental: false)
    }

    private func requestUpdate(incremental: Bool) async throws {
        try await send([3, incremental ? 1 : 0] + be16(0) + be16(0) + be16(width) + be16(height))
    }

    // MARK: message loop

    /// Runs until the connection ends. Emits events; the last one is `.closed`.
    public func run() async {
        do {
            while true {
                switch try await u8() {
                case 0: try await framebufferUpdate()
                case 1:                                         // SetColourMapEntries (unused with true colour)
                    _ = try await read(2); let first = try await u16(); _ = first; let n = try await u16(); _ = try await read(n * 6)
                case 2: continuation.yield(.bell)
                case 3:
                    _ = try await read(3); let len = Int(try await u32())
                    continuation.yield(.clipboard(String(try await read(len).map { Character(Unicode.Scalar($0)) })))   // Latin-1 per RFC 6143
                case let t: throw RFBError.protocolError("unknown server message \(t)")
                }
            }
        } catch {
            continuation.yield(.closed((error as? RFBError).map { $0.localizedDescription } ?? error.localizedDescription))
        }
        continuation.finish()
    }

    private func framebufferUpdate() async throws {
        _ = try await read(1)
        let n = try await u16()
        var resized = false
        for _ in 0..<n {
            let x = try await u16(), y = try await u16(), w = try await u16(), h = try await u16()
            let enc = Int32(bitPattern: try await u32())
            switch enc {
            case 0: fb.putRaw(x, y, w, h, try await read(w * h * 4))
            case 1: let sx = try await u16(), sy = try await u16(); fb.copy(from: sx, sy, to: x, y, w, h)
            case 5: try await hextile(x, y, w, h)
            case -223: width = w; height = h; fb.resize(w, h); resized = true
            default: throw RFBError.protocolError("unsupported encoding \(enc)")
            }
        }
        if resized { continuation.yield(.resized(width, height)) }
        continuation.yield(.frame)
        try await requestUpdate(incremental: true)
    }

    private func hextile(_ rx: Int, _ ry: Int, _ rw: Int, _ rh: Int) async throws {
        var bg: [UInt8] = [0, 0, 0, 0], fg: [UInt8] = [255, 255, 255, 255]
        for ty in stride(from: 0, to: rh, by: 16) {
            for tx in stride(from: 0, to: rw, by: 16) {
                let tw = min(16, rw - tx), th = min(16, rh - ty), x = rx + tx, y = ry + ty
                let sub = try await u8()
                if sub & 1 != 0 { fb.putRaw(x, y, tw, th, try await read(tw * th * 4)); continue }
                if sub & 2 != 0 { bg = Array(try await read(4)) }
                if sub & 4 != 0 { fg = Array(try await read(4)) }
                fb.fill(x, y, tw, th, bg[0], bg[1], bg[2])
                guard sub & 8 != 0 else { continue }
                let count = try await u8()
                for _ in 0..<count {
                    var c = fg
                    if sub & 16 != 0 { c = Array(try await read(4)) }
                    let xy = try await u8(), wh = try await u8()
                    fb.fill(x + (xy >> 4), y + (xy & 15), (wh >> 4) + 1, (wh & 15) + 1, c[0], c[1], c[2])
                }
            }
        }
    }

    // MARK: output

    public func snapshot() -> CGImage? { fb.image() }
    public func rawPixels() -> (width: Int, height: Int, bgra: [UInt8]) { (fb.width, fb.height, fb.pixels) }

    public func key(_ keysym: UInt32, down: Bool) async {
        try? await send([4, down ? 1 : 0, 0, 0] + be32(keysym))
    }

    /// `buttonMask`: bit0 left, bit1 middle, bit2 right, bit3/4 wheel up/down. Coordinates are framebuffer pixels.
    public func pointer(x: Int, y: Int, buttonMask: UInt8) async {
        buttons = buttonMask
        try? await send([5, buttonMask] + be16(max(0, min(x, width - 1))) + be16(max(0, min(y, height - 1))))
    }

    public func scroll(x: Int, y: Int, up: Bool) async {
        await pointer(x: x, y: y, buttonMask: buttons | (up ? 8 : 16))
        await pointer(x: x, y: y, buttonMask: buttons & ~(up ? 8 : 16))
    }

    public func sendClipboard(_ text: String) async {
        let bytes = Array(text.unicodeScalars.filter { $0.value < 256 }.map { UInt8($0.value) })
        try? await send([6, 0, 0, 0] + be32(UInt32(bytes.count)) + bytes)
    }

    public func close() { transport.close(); continuation.finish() }
}
