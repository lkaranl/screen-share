import Foundation
import Network
import AppKit

enum InputCommand: Encodable {
    case mouseMove(x: Int32, y: Int32)
    case mouseButton(button: UInt8, pressed: Bool)
    case mouseScroll(dy: Int32)
    case key(code: UInt16, pressed: Bool)
    case clipboardPaste(text: String)
    case clipboardRequest
    case ping(timestamp: UInt64)

    enum CodingKeys: String, CodingKey {
        case MouseMove
        case MouseButton
        case MouseScroll
        case Key
        case ClipboardPaste
        case ClipboardRequest
        case Ping
    }

    struct MouseMovePayload: Encodable {
        let x: Int32
        let y: Int32
    }

    struct MouseButtonPayload: Encodable {
        let button: UInt8
        let pressed: Bool
    }

    struct MouseScrollPayload: Encodable {
        let dy: Int32
    }

    struct KeyPayload: Encodable {
        let code: UInt16
        let pressed: Bool
    }

    struct ClipboardPastePayload: Encodable {
        let text: String
    }

    struct PingPayload: Encodable {
        let timestamp: UInt64
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case .mouseMove(let x, let y):
            try container.encode(MouseMovePayload(x: x, y: y), forKey: .MouseMove)
        case .mouseButton(let button, let pressed):
            try container.encode(MouseButtonPayload(button: button, pressed: pressed), forKey: .MouseButton)
        case .mouseScroll(let dy):
            try container.encode(MouseScrollPayload(dy: dy), forKey: .MouseScroll)
        case .key(let code, let pressed):
            try container.encode(KeyPayload(code: code, pressed: pressed), forKey: .Key)
        case .clipboardPaste(let text):
            try container.encode(ClipboardPastePayload(text: text), forKey: .ClipboardPaste)
        case .clipboardRequest:
            try container.encodeNil(forKey: .ClipboardRequest)
        case .ping(let timestamp):
            try container.encode(PingPayload(timestamp: timestamp), forKey: .Ping)
        }
    }
}

enum ControlResponse: Decodable {
    case clipboardSync(text: String)
    case pong(timestamp: UInt64)

    enum CodingKeys: String, CodingKey {
        case ClipboardSync
        case Pong
    }

    struct ClipboardSyncPayload: Decodable {
        let text: String
    }

    struct PongPayload: Decodable {
        let timestamp: UInt64
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        if container.contains(.ClipboardSync) {
            let payload = try container.decode(ClipboardSyncPayload.self, forKey: .ClipboardSync)
            self = .clipboardSync(text: payload.text)
        } else if container.contains(.Pong) {
            let payload = try container.decode(PongPayload.self, forKey: .Pong)
            self = .pong(timestamp: payload.timestamp)
        } else {
            throw DecodingError.dataCorrupted(DecodingError.Context(codingPath: decoder.codingPath, debugDescription: "Unknown response"))
        }
    }
}

final class ControlClient {
    private var connection: NWConnection?
    private let queue = DispatchQueue(label: "screenshare.control.queue", qos: .userInteractive)
    private var isRunning = false
    private var pingTimer: DispatchSourceTimer?

    var onLatencyUpdated: ((UInt32) -> Void)?
    var onClipboardReceived: ((String) -> Void)?

    func connect(host: String, port: UInt16 = 5001) {
        let tcpOptions = NWProtocolTCP.Options()
        tcpOptions.noDelay = true
        tcpOptions.enableFastOpen = true

        let params = NWParameters(tls: nil, tcp: tcpOptions)
        let endpoint = NWEndpoint.hostPort(host: NWEndpoint.Host(host), port: NWEndpoint.Port(integerLiteral: port))

        let conn = NWConnection(to: endpoint, using: params)
        self.connection = conn

        conn.stateUpdateHandler = { [weak self] state in
            switch state {
            case .ready:
                print("🎮 Conectado com sucesso ao canal de controle TCP :\(port)!")
                self?.isRunning = true
                self?.startReceiving()
                self?.startPingLoop()
            case .waiting(let error):
                print("⏳ Canal de Controle: Tentando alcançar \(host):\(port)... (\(error.localizedDescription))")
            case .preparing:
                print("🔄 Canal de Controle: Estabelecendo handshake TCP com \(host):\(port)...")
            case .failed(let error):
                print("❌ Falha no canal de controle: \(error)")
                self?.disconnect()
            case .cancelled:
                self?.isRunning = false
            default:
                break
            }
        }

        conn.start(queue: queue)
    }

    func send(_ cmd: InputCommand) {
        guard isRunning, let conn = connection else { return }
        do {
            var data = try JSONEncoder().encode(cmd)
            data.append(0x0A) // '\n'
            conn.send(content: data, completion: .idempotent)
        } catch {
            print("Erro ao serializar comando: \(error)")
        }
    }

    private func startReceiving() {
        guard isRunning, let conn = connection else { return }

        conn.receive(minimumIncompleteLength: 1, maximumLength: 65536) { [weak self] data, _, isComplete, error in
            guard let self = self else { return }

            if let data = data, !data.isEmpty {
                self.processIncomingData(data)
            }

            if isComplete || error != nil {
                self.disconnect()
            } else if self.isRunning {
                self.startReceiving()
            }
        }
    }

    private var incomingBuffer = Data()

    private func processIncomingData(_ data: Data) {
        incomingBuffer.append(data)

        while let newlineIndex = incomingBuffer.firstIndex(of: 0x0A) {
            let lineData = incomingBuffer.subdata(in: incomingBuffer.startIndex..<newlineIndex)
            incomingBuffer.removeSubrange(incomingBuffer.startIndex...newlineIndex)

            if !lineData.isEmpty {
                do {
                    let resp = try JSONDecoder().decode(ControlResponse.self, from: lineData)
                    switch resp {
                    case .clipboardSync(let text):
                        print("📋 Controle: Clipboard recebido do servidor (\(text.count) caracteres)")
                        DispatchQueue.main.async {
                            self.onClipboardReceived?(text)
                        }
                    case .pong(let timestamp):
                        let now = UInt64(Date().timeIntervalSince1970 * 1000.0)
                        let rtt = UInt32(now > timestamp ? (now - timestamp) : 0)
                        DispatchQueue.main.async {
                            self.onLatencyUpdated?(rtt)
                        }
                    }
                } catch {
                    // Ignora linhas mal formatadas
                }
            }
        }
    }

    private func startPingLoop() {
        pingTimer?.cancel()
        let timer = DispatchSource.makeTimerSource(queue: queue)
        timer.schedule(deadline: .now() + .milliseconds(500), repeating: .milliseconds(500))
        timer.setEventHandler { [weak self] in
            let now = UInt64(Date().timeIntervalSince1970 * 1000.0)
            self?.send(.ping(timestamp: now))
        }
        timer.resume()
        self.pingTimer = timer
    }

    func disconnect() {
        isRunning = false
        pingTimer?.cancel()
        pingTimer = nil
        connection?.cancel()
        connection = nil
    }

    deinit {
        disconnect()
    }
}
