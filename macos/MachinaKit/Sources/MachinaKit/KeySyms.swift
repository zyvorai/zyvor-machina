import Foundation

/// macOS key events to X11 keysyms (VNC) and to terminal bytes (serial).
public enum KeySyms {
    /// Keys that carry no usable character, by macOS virtual key code.
    public static func special(keyCode: UInt16) -> UInt32? {
        switch keyCode {
        case 36, 76: 0xff0d        // Return, keypad Enter
        case 48: 0xff09            // Tab
        case 51: 0xff08            // BackSpace
        case 53: 0xff1b            // Escape
        case 117: 0xffff           // Delete (forward)
        case 115: 0xff50; case 119: 0xff57   // Home, End
        case 116: 0xff55; case 121: 0xff56   // Page Up, Page Down
        case 123: 0xff51; case 126: 0xff52; case 124: 0xff53; case 125: 0xff54   // Left, Up, Right, Down
        case 122: 0xffbe; case 120: 0xffbf; case 99: 0xffc0; case 118: 0xffc1    // F1-F4
        case 96: 0xffc2; case 97: 0xffc3; case 98: 0xffc4; case 100: 0xffc5      // F5-F8
        case 101: 0xffc6; case 109: 0xffc7; case 103: 0xffc8; case 111: 0xffc9   // F9-F12
        default: nil
        }
    }

    /// Modifier keys by key code (for flagsChanged events): (keysym, isOptionOrCommandOrControlOrShift side).
    public static func modifier(keyCode: UInt16) -> UInt32? {
        switch keyCode {
        case 56: 0xffe1; case 60: 0xffe2    // Shift L/R
        case 59: 0xffe3; case 62: 0xffe4    // Control L/R
        case 58: 0xffe9; case 61: 0xffea    // Alt (Option) L/R
        case 55: 0xffeb; case 54: 0xffec    // Super (Command) L/R
        case 57: 0xffe5                     // Caps Lock
        default: nil
        }
    }

    /// X11 keysym for a typed character: Latin-1 maps to itself, everything else to the Unicode range.
    public static func keysym(forCharacter s: Unicode.Scalar) -> UInt32 {
        switch s.value {
        case 0x20...0x7e, 0xa0...0xff: return s.value
        default: return 0x0100_0000 + s.value
        }
    }

    /// Keysym for a key event: special keys first, then the typed character, else nil.
    public static func keysym(keyCode: UInt16, characters: String?) -> UInt32? {
        if let sp = special(keyCode: keyCode) { return sp }
        guard let c = characters?.unicodeScalars.first, c.value >= 0x20, c.value != 0x7f else { return nil }
        return keysym(forCharacter: c)
    }
}

public enum TerminalKeys {
    /// Bytes a serial console expects for a key press. `characters` is what the key types (with modifiers applied).
    public static func bytes(keyCode: UInt16, characters: String?, control: Bool) -> [UInt8]? {
        switch keyCode {
        case 36, 76: return [0x0d]
        case 48: return [0x09]
        case 51: return [0x7f]
        case 53: return [0x1b]
        case 117: return Array("\u{1b}[3~".utf8)
        case 115: return Array("\u{1b}[H".utf8); case 119: return Array("\u{1b}[F".utf8)
        case 116: return Array("\u{1b}[5~".utf8); case 121: return Array("\u{1b}[6~".utf8)
        case 126: return Array("\u{1b}[A".utf8); case 125: return Array("\u{1b}[B".utf8)
        case 124: return Array("\u{1b}[C".utf8); case 123: return Array("\u{1b}[D".utf8)
        default: break
        }
        guard let chars = characters, let first = chars.unicodeScalars.first else { return nil }
        if control, first.isASCII, let a = UInt8(exactly: first.value) {
            let lower = a | 0x20
            if lower >= 0x61 && lower <= 0x7a { return [lower - 0x60] }      // Ctrl-A ... Ctrl-Z
            if a == 0x5b { return [0x1b]; }; if a == 0x5c { return [0x1c] }; if a == 0x5d { return [0x1d] }
        }
        guard first.value >= 0x20, first.value != 0x7f else { return nil }
        return Array(chars.utf8)
    }
}
