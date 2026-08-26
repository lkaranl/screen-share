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
