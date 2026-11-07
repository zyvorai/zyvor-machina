// Generates macos/Resources/Machina.icns: a blue rounded square with a white "m". macOS only.
// Usage: swiftc -o /tmp/make-icon macos/scripts/make-icon.swift && /tmp/make-icon macos/Resources
import AppKit

func render(_ px: Int) -> Data {
    let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: px, pixelsHigh: px, bitsPerSample: 8, samplesPerPixel: 4,
                               hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
    let s = CGFloat(px), inset = s * 0.08, rect = NSRect(x: inset, y: inset, width: s - 2 * inset, height: s - 2 * inset)
    let path = NSBezierPath(roundedRect: rect, xRadius: s * 0.22, yRadius: s * 0.22)
    NSGradient(colors: [NSColor(red: 0.16, green: 0.60, blue: 1.0, alpha: 1), NSColor(red: 0.0, green: 0.44, blue: 0.89, alpha: 1)])!.draw(in: path, angle: -90)
    let font = NSFont.systemFont(ofSize: s * 0.56, weight: .semibold)
    let str = NSAttributedString(string: "m", attributes: [.font: font, .foregroundColor: NSColor.white])
    let size = str.size()
    str.draw(at: NSPoint(x: (s - size.width) / 2, y: (s - size.height) / 2 - s * 0.02))
    NSGraphicsContext.restoreGraphicsState()
    return rep.representation(using: .png, properties: [:])!
}

let out = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "."
let set = out + "/Machina.iconset"
try? FileManager.default.removeItem(atPath: set)
try! FileManager.default.createDirectory(atPath: set, withIntermediateDirectories: true)
for (name, px) in [("icon_16x16", 16), ("icon_16x16@2x", 32), ("icon_32x32", 32), ("icon_32x32@2x", 64), ("icon_128x128", 128), ("icon_128x128@2x", 256),
                   ("icon_256x256", 256), ("icon_256x256@2x", 512), ("icon_512x512", 512), ("icon_512x512@2x", 1024)] {
    try! render(px).write(to: URL(fileURLWithPath: "\(set)/\(name).png"))
}
let p = Process(); p.executableURL = URL(fileURLWithPath: "/usr/bin/iconutil")
p.arguments = ["-c", "icns", set, "-o", out + "/Machina.icns"]
try! p.run(); p.waitUntilExit()
try? FileManager.default.removeItem(atPath: set)
print(p.terminationStatus == 0 ? "wrote \(out)/Machina.icns" : "iconutil failed")
