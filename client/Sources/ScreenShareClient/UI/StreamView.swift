import SwiftUI
import AVFoundation
import AppKit
import Network

final class PixelBufferDisplayView: NSView {
    var inputManager: InputManager?
    private var trackingArea: NSTrackingArea?
    private var keyEventMonitor: Any?

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        wantsLayer = true
        layer?.contentsGravity = .resizeAspect
    }

    required init?(coder: NSCoder) {
        super.init(coder: coder)
        wantsLayer = true
        layer?.contentsGravity = .resizeAspect
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        window?.acceptsMouseMovedEvents = true
        window?.makeFirstResponder(self)

        if window != nil && keyEventMonitor == nil {
            keyEventMonitor = NSEvent.addLocalMonitorForEvents(matching: [.keyDown, .keyUp, .flagsChanged]) { [weak self] event in
                guard let self = self, let win = self.window, event.window == win else {
                    return event
                }
                switch event.type {
                case .keyDown:
                    self.inputManager?.handleKeyDown(keyCode: event.keyCode, modifierFlags: event.modifierFlags)
                    return nil
                case .keyUp:
                    self.inputManager?.handleKeyUp(keyCode: event.keyCode, modifierFlags: event.modifierFlags)
                    return nil
                case .flagsChanged:
                    self.inputManager?.handleFlagsChanged(modifierFlags: event.modifierFlags, keyCode: event.keyCode)
                    return nil
                default:
                    return event
                }
            }
        } else if window == nil, let monitor = keyEventMonitor {
            NSEvent.removeMonitor(monitor)
            keyEventMonitor = nil
        }
    }

    deinit {
        if let monitor = keyEventMonitor {
            NSEvent.removeMonitor(monitor)
        }
    }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let area = trackingArea {
            removeTrackingArea(area)
        }
        let area = NSTrackingArea(
            rect: bounds,
            options: [.activeAlways, .mouseMoved, .mouseEnteredAndExited, .inVisibleRect],
            owner: self,
            userInfo: nil
        )
        addTrackingArea(area)
        self.trackingArea = area
    }

    override func resetCursorRects() {
        super.resetCursorRects()
        addCursorRect(bounds, cursor: .arrow)
    }

    override var acceptsFirstResponder: Bool { true }
    override var canBecomeKeyView: Bool { true }
    override func becomeFirstResponder() -> Bool { true }
    override func resignFirstResponder() -> Bool { true }

    override func flagsChanged(with event: NSEvent) {
        inputManager?.handleFlagsChanged(modifierFlags: event.modifierFlags, keyCode: event.keyCode)
    }

    override func mouseMoved(with event: NSEvent) {
        let location = convert(event.locationInWindow, from: nil)
        inputManager?.handleMouseMoved(location: location, in: bounds)
    }

    override func mouseDragged(with event: NSEvent) {
        let location = convert(event.locationInWindow, from: nil)
        inputManager?.handleMouseMoved(location: location, in: bounds)
    }

    override func rightMouseDragged(with event: NSEvent) {
        let location = convert(event.locationInWindow, from: nil)
        inputManager?.handleMouseMoved(location: location, in: bounds)
    }

    override func otherMouseDragged(with event: NSEvent) {
        let location = convert(event.locationInWindow, from: nil)
        inputManager?.handleMouseMoved(location: location, in: bounds)
    }

    override func mouseDown(with event: NSEvent) {
        window?.makeFirstResponder(self)
        let location = convert(event.locationInWindow, from: nil)
        inputManager?.handleMouseMoved(location: location, in: bounds)
        inputManager?.handleMouseDown(button: 0)
    }

    override func mouseUp(with event: NSEvent) {
        inputManager?.handleMouseUp(button: 0)
    }

    override func rightMouseDown(with event: NSEvent) {
        window?.makeFirstResponder(self)
        let location = convert(event.locationInWindow, from: nil)
        inputManager?.handleMouseMoved(location: location, in: bounds)
        inputManager?.handleMouseDown(button: 2)
    }

    override func rightMouseUp(with event: NSEvent) {
        inputManager?.handleMouseUp(button: 2)
    }

    override func otherMouseDown(with event: NSEvent) {
        window?.makeFirstResponder(self)
        let location = convert(event.locationInWindow, from: nil)
        inputManager?.handleMouseMoved(location: location, in: bounds)
        let btn: UInt8 = event.buttonNumber == 2 ? 1 : (event.buttonNumber == 3 ? 3 : 4)
        inputManager?.handleMouseDown(button: btn)
    }

    override func otherMouseUp(with event: NSEvent) {
        let btn: UInt8 = event.buttonNumber == 2 ? 1 : (event.buttonNumber == 3 ? 3 : 4)
        inputManager?.handleMouseUp(button: btn)
    }

    override func scrollWheel(with event: NSEvent) {
        inputManager?.handleScroll(deltaY: event.scrollingDeltaY)
    }

    override func keyDown(with event: NSEvent) {
        inputManager?.handleKeyDown(keyCode: event.keyCode, modifierFlags: event.modifierFlags)
    }

    override func keyUp(with event: NSEvent) {
        inputManager?.handleKeyUp(keyCode: event.keyCode, modifierFlags: event.modifierFlags)
    }
}

struct StreamVideoViewRepresentable: NSViewRepresentable {
    let videoReceiver: UdpVideoReceiver
    let inputManager: InputManager
    @Binding var fpsCount: Int

    func makeNSView(context: Context) -> PixelBufferDisplayView {
        let view = PixelBufferDisplayView()
        view.inputManager = inputManager

        var frameCounter = 0
        var lastFPSTime = Date()

        var hasLoggedFirstFrame = false

        videoReceiver.onPixelBuffer = { [weak view] pixelBuffer in
            guard let view = view else { return }

            DispatchQueue.main.async {
                if !hasLoggedFirstFrame {
                    print("🖥️ StreamView: Renderização direta de GPU (Zero-Copy RTP/UDP) ativa!")
                    hasLoggedFirstFrame = true
                }

                CATransaction.begin()
                CATransaction.setDisableActions(true)
                view.layer?.contents = pixelBuffer
                CATransaction.commit()
            }

            frameCounter += 1
            let elapsed = Date().timeIntervalSince(lastFPSTime)
            if elapsed >= 0.5 {
                let current = Int(round(Double(frameCounter) / elapsed))
                frameCounter = 0
                lastFPSTime = Date()
                DispatchQueue.main.async {
                    self.fpsCount = current
                }
            }
        }

        return view
    }

    func updateNSView(_ nsView: PixelBufferDisplayView, context: Context) {
        nsView.inputManager = inputManager
    }
}

final class StreamSession: ObservableObject {
    let host: String
    let codec: VideoCodecType
    let resolution: VideoResolutionType

    let videoReceiver: UdpVideoReceiver
    let controlClient: ControlClient
    let inputManager: InputManager

    @Published var fps: Int = 60
    @Published var latency: UInt32 = 0

    init(host: String, codec: VideoCodecType, resolution: VideoResolutionType) {
        self.host = host
        self.codec = codec
        self.resolution = resolution

        let receiver = UdpVideoReceiver(codec: codec)
        let control = ControlClient()
        let input = InputManager(controlClient: control)

        self.videoReceiver = receiver
        self.controlClient = control
        self.inputManager = input

        print("🚀 StreamSession criada para \(host) com codec \(codec) e resolução \(resolution.displayName) via RTP/UDP + FEC")
    }

    func start() {
        print("▶️ Iniciando conexões de rede e decodificador...")
        controlClient.onLatencyUpdated = { [weak self] rtt in
            DispatchQueue.main.async {
                self?.latency = rtt
            }
        }
        controlClient.onClipboardReceived = { text in
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(text, forType: .string)
        }

        // 1. Inicia o listener UDP na porta 50000 ANTES do handshake TCP
        videoReceiver.start()

        // 2. Conecta controle e envia handshake de vídeo em paralelo
        controlClient.connect(host: host)
        sendVideoHandshake(host: host)
    }

    /// Abre uma conexão TCP na porta 5000 e envia 4 bytes com a porta UDP de escuta (50000),
    /// o codec selecionado (0 = H.264, 1 = HEVC) e a resolução (0 = 1080p, 1 = 2K, 2 = 4K),
    /// informando ao servidor como inicializar o stream.
    private func sendVideoHandshake(host: String) {
        let tcpOptions = NWProtocolTCP.Options()
        tcpOptions.noDelay = true
        tcpOptions.enableFastOpen = true
        let params = NWParameters(tls: nil, tcp: tcpOptions)
        let conn = NWConnection(
            to: NWEndpoint.hostPort(host: NWEndpoint.Host(host), port: 5000),
            using: params
        )

        let selectedCodec = self.codec
        let selectedResolution = self.resolution
        conn.stateUpdateHandler = { [weak conn] state in
            switch state {
            case .ready:
                print("🔗 Canal de Handshake de Vídeo TCP conectado — informando porta UDP \(UdpVideoReceiver.videoListenPort), codec \(selectedCodec) e resolução \(selectedResolution.displayName)")
                // Envia a porta UDP como 2 bytes big-endian + 1 byte de codec (0 = H264, 1 = HEVC) + 1 byte de resolução (0 = FHD, 1 = QHD, 2 = UHD)
                var handshakeBytes = Data()
                var port = UdpVideoReceiver.videoListenPort.bigEndian
                handshakeBytes.append(Data(bytes: &port, count: 2))
                let codecByte: UInt8 = (selectedCodec == .hevc) ? 1 : 0
                handshakeBytes.append(codecByte)
                handshakeBytes.append(selectedResolution.rawValue)

                conn?.send(content: handshakeBytes, completion: .contentProcessed({ _ in
                    conn?.cancel()
                }))
            case .failed(let err):
                print("⚠️ Falha no handshake TCP de vídeo: \(err)")
            default:
                break
            }
        }

        let q = DispatchQueue(label: "screenshare.tcp.video.handshake", qos: .userInitiated)
        conn.start(queue: q)
    }

    func stop() {
        print("⏹️ Encerrando StreamSession...")
        videoReceiver.stop()
        controlClient.disconnect()
    }
}

struct StreamView: View {
    @StateObject private var session: StreamSession
    let onDisconnect: () -> Void

    init(host: String, codec: VideoCodecType, resolution: VideoResolutionType, onDisconnect: @escaping () -> Void) {
        _session = StateObject(wrappedValue: StreamSession(host: host, codec: codec, resolution: resolution))
        self.onDisconnect = onDisconnect
    }

    var body: some View {
        ZStack(alignment: .topTrailing) {
            Color.black.ignoresSafeArea()

            StreamVideoViewRepresentable(
                videoReceiver: session.videoReceiver,
                inputManager: session.inputManager,
                fpsCount: $session.fps
            )
            .ignoresSafeArea()

            HUDOverlayView(fps: session.fps, latency: session.latency, resolution: session.resolution)
                .padding(.top, 14)
                .padding(.trailing, 14)
        }
        .onAppear {
            session.start()
        }
        .onDisappear {
            session.stop()
        }
    }
}
