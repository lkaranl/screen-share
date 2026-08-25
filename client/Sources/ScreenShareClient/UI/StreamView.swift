import SwiftUI
import AVFoundation
import AppKit

final class SampleBufferDisplayView: NSView {
    override var wantsUpdateLayer: Bool { true }

    override func makeBackingLayer() -> CALayer {
        let displayLayer = AVSampleBufferDisplayLayer()
        displayLayer.videoGravity = .resizeAspect
        displayLayer.preventsDisplaySleepDuringVideoPlayback = true
        // Sem controlTimebase restritivo: cada frame com DisplayImmediately é renderizado no instante em que chega
        return displayLayer
    }

    var displayLayer: AVSampleBufferDisplayLayer {
        layer as! AVSampleBufferDisplayLayer
    }

    var inputManager: InputManager?
    private var trackingArea: NSTrackingArea?

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        wantsLayer = true
    }

    required init?(coder: NSCoder) {
        super.init(coder: coder)
        wantsLayer = true
    }

    override func layout() {
        super.layout()
        displayLayer.frame = bounds
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
    let videoReceiver: VideoReceiver
    let inputManager: InputManager
    @Binding var fpsCount: Int

    func makeNSView(context: Context) -> SampleBufferDisplayView {
        let view = SampleBufferDisplayView()
        view.inputManager = inputManager

        var frameCounter = 0
        var lastFPSTime = Date()

        var hasLoggedFirstFrame = false

        videoReceiver.onSampleBuffer = { [weak view] sampleBuffer in
            guard let view = view else { return }

            DispatchQueue.main.async {
                if !hasLoggedFirstFrame {
                    print("🖥️ StreamView: Primeiro frame enfileirado na tela (Status da Layer: \(view.displayLayer.status.rawValue))")
                    hasLoggedFirstFrame = true
                }

                if view.displayLayer.status == .failed {
                    print("⚠️ AVSampleBufferDisplayLayer falhou com erro: \(String(describing: view.displayLayer.error))")
                    view.displayLayer.flush()
                }

                view.displayLayer.enqueue(sampleBuffer)
            }

            frameCounter += 1
            if Date().timeIntervalSince(lastFPSTime) >= 1.0 {
                let current = frameCounter
                frameCounter = 0
                lastFPSTime = Date()
                DispatchQueue.main.async {
                    self.fpsCount = current
                }
            }
        }

        return view
    }

    func updateNSView(_ nsView: SampleBufferDisplayView, context: Context) {
        nsView.inputManager = inputManager
    }
}

final class StreamSession: ObservableObject {
    let host: String
    let codec: VideoCodecType

    let videoReceiver: VideoReceiver
    let controlClient: ControlClient
    let inputManager: InputManager

    @Published var fps: Int = 60
    @Published var latency: UInt32 = 0

    init(host: String, codec: VideoCodecType) {
        self.host = host
        self.codec = codec

        let receiver = VideoReceiver(codec: codec)
        let control = ControlClient()
        let input = InputManager(controlClient: control)

        self.videoReceiver = receiver
        self.controlClient = control
        self.inputManager = input

        print("🚀 StreamSession criada para \(host) com codec \(codec)")
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

        controlClient.connect(host: host)
        videoReceiver.start(host: host)
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
