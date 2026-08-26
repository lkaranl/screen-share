import SwiftUI
import AVFoundation
import AppKit
import Network

final class PixelBufferDisplayView: NSView {
    override var wantsUpdateLayer: Bool { true }

    var inputManager: InputManager?
    private var trackingArea: NSTrackingArea?

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
        inputManager?.handleMouseDown(button: 0)
    }

    override func mouseUp(with event: NSEvent) {
        inputManager?.handleMouseUp(button: 0)
    }

    override func rightMouseDown(with event: NSEvent) {
        inputManager?.handleMouseDown(button: 2)
    }

    override func rightMouseUp(with event: NSEvent) {
        inputManager?.handleMouseUp(button: 2)
    }

    override func otherMouseDown(with event: NSEvent) {
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

    let videoReceiver: UdpVideoReceiver
    let controlClient: ControlClient
    let inputManager: InputManager

    @Published var fps: Int = 60
    @Published var latency: UInt32 = 0

    init(host: String, codec: VideoCodecType) {
        self.host = host
        self.codec = codec

        let receiver = UdpVideoReceiver(codec: codec)
        let control = ControlClient()
        let input = InputManager(controlClient: control)

        self.videoReceiver = receiver
        self.controlClient = control
        self.inputManager = input

        print("🚀 StreamSession criada para \(host) com codec \(codec) via RTP/UDP + FEC")
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

    /// Abre uma conexão TCP na porta 5000 e envia 2 bytes com a porta UDP de escuta (50000),
    /// informando ao servidor para onde enviar os frames de vídeo.
    private func sendVideoHandshake(host: String) {
        let tcpOptions = NWProtocolTCP.Options()
        tcpOptions.noDelay = true
        tcpOptions.enableFastOpen = true
        let params = NWParameters(tls: nil, tcp: tcpOptions)
        let conn = NWConnection(
            to: NWEndpoint.hostPort(host: NWEndpoint.Host(host), port: 5000),
            using: params
        )

        conn.stateUpdateHandler = { [weak conn] state in
            switch state {
            case .ready:
                print("🔗 Canal de Handshake de Vídeo TCP conectado — informando porta UDP \(UdpVideoReceiver.videoListenPort)")
                // Envia a porta UDP como 2 bytes big-endian
                let port = UdpVideoReceiver.videoListenPort
                let portBytes = withUnsafeBytes(of: port.bigEndian) { Data($0) }
                conn?.send(content: portBytes, completion: .contentProcessed({ _ in
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

    init(host: String, codec: VideoCodecType, onDisconnect: @escaping () -> Void) {
        _session = StateObject(wrappedValue: StreamSession(host: host, codec: codec))
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

            HUDOverlayView(fps: session.fps, latency: session.latency)
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
