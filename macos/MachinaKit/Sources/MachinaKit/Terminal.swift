import Foundation

/// A small line-oriented terminal for serial consoles: it renders text, carriage returns, backspaces and
/// "erase to end of line", and swallows the other escape sequences (colours, cursor movement). Full-screen
/// programs (vi, top) will not draw correctly; login prompts, boot logs and shells do.
public struct TerminalBuffer: Sendable {
    public private(set) var lines: [[Character]] = [[]]
    private var col = 0
    private enum Mode { case text, esc, csi, osc, oscEsc }
    private var mode: Mode = .text
    private var csiParams = ""
    public let maxLines: Int

    public init(maxLines: Int = 5000) { self.maxLines = maxLines }

    public var text: String { lines.map { String($0) }.joined(separator: "\n") }

    public mutating func feed(_ s: String) {
        for ch in s.unicodeScalars {
            switch mode {
            case .text: text(ch)
            case .esc:
                if ch == "[" { mode = .csi; csiParams = "" }
                else if ch == "]" { mode = .osc }
                else { mode = .text }                       // two-character escape (ESC c, ESC 7, ...)
            case .csi:
                if ch.value >= 0x40 && ch.value <= 0x7e { csi(Character(ch)); mode = .text } else { csiParams.unicodeScalars.append(ch) }
            case .osc:
                if ch.value == 7 { mode = .text } else if ch.value == 0x1b { mode = .oscEsc }
            case .oscEsc: mode = .text
            }
        }
    }

    private mutating func text(_ ch: Unicode.Scalar) {
        switch ch.value {
        case 0x1b: mode = .esc
        case 0x0d: col = 0
        case 0x0a: newline()
        case 0x08, 0x7f: col = max(0, col - 1)
        case 0x07, 0x00: break
        case 0x09: let n = 8 - col % 8; for _ in 0..<n { put(" ") }
        default: if ch.value >= 0x20 { put(Character(ch)) }
        }
    }

    private mutating func put(_ c: Character) {
        var line = lines[lines.count - 1]
        while line.count < col { line.append(" ") }
        if col < line.count { line[col] = c } else { line.append(c) }
        lines[lines.count - 1] = line
        col += 1
    }

    private mutating func newline() {
        lines.append([]); col = 0
        if lines.count > maxLines { lines.removeFirst(lines.count - maxLines) }
    }

    private mutating func csi(_ final: Character) {
        let n = Int(csiParams.split(separator: ";").first.flatMap { Substring($0).filter(\.isNumber) } ?? "") ?? 0
        switch final {
        case "K":                                           // erase in line: 0 cursor→end, 1 start→cursor, 2 whole line
            var line = lines[lines.count - 1]
            if n == 0 { if col < line.count { line.removeSubrange(col...) } }
            else if n == 1 { for i in 0..<min(col + 1, line.count) { line[i] = " " } }
            else { line.removeAll() }
            lines[lines.count - 1] = line
        case "C": col += max(n, 1)
        case "D": col = max(0, col - max(n, 1))
        case "G": col = max(0, n - 1)
        default: break                                      // colours (m), cursor moves, mode changes: ignored
        }
    }
}
