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

        // Correção de Aspect Ratio 16:9 (Letterbox / Pillarbox) estilo Moonlight
        let targetAspect: CGFloat = 16.0 / 9.0
        let viewAspect = viewBounds.width / viewBounds.height

        var videoRect = viewBounds
        if viewAspect > targetAspect {
            let videoWidth = viewBounds.height * targetAspect
            let offsetX = (viewBounds.width - videoWidth) / 2.0
            videoRect = CGRect(x: offsetX, y: 0, width: videoWidth, height: viewBounds.height)
        } else {
            let videoHeight = viewBounds.width / targetAspect
            let offsetY = (viewBounds.height - videoHeight) / 2.0
            videoRect = CGRect(x: 0, y: offsetY, width: viewBounds.width, height: videoHeight)
        }

        // Clampa a posição dentro do retângulo ativo do vídeo
        let clampedX = max(videoRect.minX, min(location.x, videoRect.maxX))
        let clampedY = max(videoRect.minY, min(location.y, videoRect.maxY))

        let normX = Int32(((clampedX - videoRect.minX) / videoRect.width) * 32767.0)
        // No macOS o eixo Y é invertido (de baixo para cima), normalizamos para cima -> baixo
        let normY = Int32(((videoRect.maxY - clampedY) / videoRect.height) * 32767.0)

        controlClient?.send(.mouseMove(x: normX, y: normY))
    }

    func handleMouseDelta(deltaX: CGFloat, deltaY: CGFloat) {
        let dx = Int32(deltaX)
        let dy = Int32(deltaY)
        if dx != 0 || dy != 0 {
            controlClient?.send(.mouseMoveRelative(dx: dx, dy: dy))
        }
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

        // Cmd+V / Ctrl+V (Colar da área de transferência com sincronização automática do Mac)
        if (isCmd || isCtrl) && keyCode == 0x09 { // 'V'
            if let pasteboardText = NSPasteboard.general.string(forType: .string), !pasteboardText.isEmpty {
                controlClient?.send(.clipboardPaste(text: pasteboardText))
                return
            } else {
                controlClient?.send(.key(code: 29, pressed: true))
                controlClient?.send(.key(code: 47, pressed: true))
                controlClient?.send(.key(code: 47, pressed: false))
                return
            }
        }

        // Cmd+C / Ctrl+C (Copiar seleção no servidor e puxar para o clipboard do Mac)
        if (isCmd || isCtrl) && keyCode == 0x08 { // 'C'
            controlClient?.send(.key(code: 29, pressed: true))  // Ctrl down
            controlClient?.send(.key(code: 46, pressed: true))  // C down
            controlClient?.send(.key(code: 46, pressed: false)) // C up

            // Aguarda a aplicação remota popular a área de transferência do Linux e sincroniza
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.15) { [weak self] in
                self?.controlClient?.send(.clipboardRequest)
            }
            return
        }

        // Cmd+X / Ctrl+X (Recortar seleção no servidor e puxar para o clipboard do Mac)
        if (isCmd || isCtrl) && keyCode == 0x07 { // 'X'
            controlClient?.send(.key(code: 29, pressed: true))  // Ctrl down
            controlClient?.send(.key(code: 45, pressed: true))  // X down
            controlClient?.send(.key(code: 45, pressed: false)) // X up

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

        if (isCmd || isCtrl) && (keyCode == 0x09 || keyCode == 0x08 || keyCode == 0x07) {
            return
        }

        let linuxCode = LinuxKeyCodes.mapMacKeyToLinux(keyCode)
        if linuxCode > 0 {
            controlClient?.send(.key(code: linuxCode, pressed: false))
        }
    }

    func handleFlagsChanged(modifierFlags: NSEvent.ModifierFlags, keyCode: UInt16) {
        let linuxCode = LinuxKeyCodes.mapMacKeyToLinux(keyCode)
        guard linuxCode > 0 else { return }

        let isPressed: Bool
        switch keyCode {
        case 0x38, 0x3C: // Left / Right Shift
            isPressed = modifierFlags.contains(.shift)
        case 0x3B, 0x3E: // Left / Right Ctrl
            isPressed = modifierFlags.contains(.control)
        case 0x3A, 0x3D: // Left / Right Option (Alt)
            isPressed = modifierFlags.contains(.option)
        case 0x37:       // Left Command (Atalhos Cmd mapeados para Ctrl no Linux)
            isPressed = modifierFlags.contains(.command)
        case 0x36:       // Right Command (Super / Windows no Linux)
            isPressed = modifierFlags.contains(.command)
        case 0x39:       // Caps Lock
            isPressed = modifierFlags.contains(.capsLock)
        default:
            return
        }

        controlClient?.send(.key(code: linuxCode, pressed: isPressed))
    }

    private func flushPendingMouse() {
        if let pos = pendingMouse {
            controlClient?.send(.mouseMove(x: pos.x, y: pos.y))
            pendingMouse = nil
            lastMouseSend = Date()
        }
    }
}
