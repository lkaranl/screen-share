import Foundation
import AppKit

final class InputManager {
    private weak var controlClient: ControlClient?
    private var lastMouseSend = Date()
    private var pendingMouse: (x: Int32, y: Int32)?

    init(controlClient: ControlClient) {
        self.controlClient = controlClient
    }

    func handleMouseMoved(location: CGPoint, in viewBounds: CGRect) {
        guard viewBounds.width > 0 && viewBounds.height > 0 else { return }

        let normX = Int32((location.x / viewBounds.width) * 32767.0)
        // No macOS o eixo Y é invertido (de baixo para cima), normalizamos para cima -> baixo
        let normY = Int32(((viewBounds.height - location.y) / viewBounds.height) * 32767.0)

        controlClient?.send(.mouseMove(x: normX, y: normY))
    }

    func handleMouseDown(button: UInt8) {
        controlClient?.send(.mouseButton(button: button, pressed: true))
    }

    func handleMouseUp(button: UInt8) {
        controlClient?.send(.mouseButton(button: button, pressed: false))
    }

    func handleScroll(deltaY: CGFloat) {
        let dy = Int32(deltaY * 5.0)
        if dy != 0 {
            controlClient?.send(.mouseScroll(dy: dy))
        }
    }

    func handleKeyDown(keyCode: UInt16, modifierFlags: NSEvent.ModifierFlags) {
        let isCmd = modifierFlags.contains(.command)
        let isCtrl = modifierFlags.contains(.control)

        // Cmd+V / Ctrl+V (Colar da área de transferência)
        if (isCmd || isCtrl) && keyCode == 0x09 { // 'V'
            if let pasteboardText = NSPasteboard.general.string(forType: .string) {
                controlClient?.send(.clipboardPaste(text: pasteboardText))
                return
            }
        }

        // Cmd+C / Ctrl+C (Copiar)
        if (isCmd || isCtrl) && keyCode == 0x08 { // 'C'
            controlClient?.send(.key(code: 29, pressed: true))  // Ctrl down
            controlClient?.send(.key(code: 46, pressed: true))  // C down
            controlClient?.send(.key(code: 46, pressed: false)) // C up
            controlClient?.send(.key(code: 29, pressed: false)) // Ctrl up

            DispatchQueue.main.asyncAfter(deadline: .now() + 0.15) { [weak self] in
                self?.controlClient?.send(.clipboardRequest)
            }
            return
        }

        let linuxCode = LinuxKeyCodes.mapMacKeyToLinux(keyCode)
        if linuxCode > 0 {
            controlClient?.send(.key(code: linuxCode, pressed: true))
        }
    }

    func handleKeyUp(keyCode: UInt16, modifierFlags: NSEvent.ModifierFlags) {
        let isCmd = modifierFlags.contains(.command)
        let isCtrl = modifierFlags.contains(.control)

        if (isCmd || isCtrl) && (keyCode == 0x09 || keyCode == 0x08) {
            return
        }

        let linuxCode = LinuxKeyCodes.mapMacKeyToLinux(keyCode)
        if linuxCode > 0 {
            controlClient?.send(.key(code: linuxCode, pressed: false))
        }
    }

    private func flushPendingMouse() {
        if let pos = pendingMouse {
            controlClient?.send(.mouseMove(x: pos.x, y: pos.y))
            pendingMouse = nil
            lastMouseSend = Date()
        }
    }
}
