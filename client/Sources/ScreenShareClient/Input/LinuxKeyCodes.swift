import Foundation
import AppKit

/// Mapeamento de KeyCodes do macOS (Carbon / IOKit) para KeyCodes do Kernel Linux (uinput / evdev)
enum LinuxKeyCodes {
    static func mapMacKeyToLinux(_ keyCode: UInt16) -> UInt16 {
        switch keyCode {
        // Letras
        case 0x00: return 30 // A
        case 0x0B: return 48 // B
        case 0x08: return 46 // C
        case 0x02: return 32 // D
        case 0x0E: return 18 // E
        case 0x03: return 33 // F
        case 0x05: return 34 // G
        case 0x04: return 35 // H
        case 0x22: return 23 // I
        case 0x26: return 36 // J
        case 0x28: return 37 // K
        case 0x25: return 38 // L
        case 0x2E: return 50 // M
        case 0x2D: return 49 // N
        case 0x1F: return 24 // O
        case 0x23: return 25 // P
        case 0x0C: return 16 // Q
        case 0x0F: return 19 // R
        case 0x01: return 31 // S
        case 0x11: return 20 // T
        case 0x20: return 22 // U
        case 0x09: return 47 // V
        case 0x0D: return 17 // W
        case 0x07: return 45 // X
        case 0x10: return 21 // Y
        case 0x06: return 44 // Z

        // Números (linha superior)
        case 0x12: return 2  // 1
        case 0x13: return 3  // 2
        case 0x14: return 4  // 3
        case 0x15: return 5  // 4
        case 0x17: return 6  // 5
        case 0x16: return 7  // 6
        case 0x1A: return 8  // 7
        case 0x1C: return 9  // 8
        case 0x19: return 10 // 9
        case 0x1D: return 11 // 0

        // Teclas especiais
        case 0x24: return 28 // Return / Enter
        case 0x35: return 1  // Escape
        case 0x33: return 14 // Backspace / Delete
        case 0x30: return 15 // Tab
        case 0x31: return 57 // Space

        // Modificadores
        case 0x38: return 42  // Left Shift
        case 0x3C: return 54  // Right Shift
        case 0x3B: return 29  // Left Ctrl
        case 0x3E: return 97  // Right Ctrl
        case 0x3A: return 56  // Left Option / Alt
        case 0x3D: return 100 // Right Option / Alt
        case 0x37: return 125 // Left Command (GUI)
        case 0x36: return 126 // Right Command (GUI)
        case 0x39: return 58  // Caps Lock

        // Pontuação e Símbolos
        case 0x1B: return 12 // Minus (-)
        case 0x18: return 13 // Equals (=)
        case 0x21: return 26 // Left Bracket ([)
        case 0x1E: return 27 // Right Bracket (])
        case 0x29: return 39 // Semicolon (;)
        case 0x27: return 40 // Quote (')
        case 0x32: return 41 // Backquote (`)
        case 0x2A: return 43 // Backslash (\)
        case 0x2B: return 51 // Comma (,)
        case 0x2F: return 52 // Period (.)
        case 0x2C: return 53 // Slash (/)

        // Setas direcionais
        case 0x7E: return 103 // Up Arrow
        case 0x7D: return 108 // Down Arrow
        case 0x7B: return 105 // Left Arrow
        case 0x7C: return 106 // Right Arrow

        // Navegação e Edição
        case 0x73: return 102 // Home
        case 0x77: return 107 // End
        case 0x74: return 104 // Page Up
        case 0x79: return 109 // Page Down
        case 0x75: return 111 // Forward Delete

        // Teclas de Função
        case 0x7A: return 59  // F1
        case 0x78: return 60  // F2
        case 0x63: return 61  // F3
        case 0x76: return 62  // F4
        case 0x60: return 63  // F5
        case 0x61: return 64  // F6
        case 0x62: return 65  // F7
        case 0x64: return 66  // F8
        case 0x65: return 67  // F9
        case 0x6D: return 68  // F10
        case 0x67: return 87  // F11
        case 0x6F: return 88  // F12

        default:
            return 0
        }
    }
}
