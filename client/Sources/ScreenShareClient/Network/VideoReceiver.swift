import Foundation
import Network
import CoreMedia

final class VideoReceiver {
    private var connection: NWConnection?
    private let queue = DispatchQueue(label: "screenshare.video.queue", qos: .userInteractive)
    private var isRunning = false

    private let parser: NALUnitParser
    private let decoder: HardwareDecoder

    var onPixelBuffer: ((CVPixelBuffer) -> Void)?

    init(codec: VideoCodecType) {
        self.parser = NALUnitParser(codec: codec)
        self.decoder = HardwareDecoder(codec: codec)

        self.decoder.onPixelBufferReady = { [weak self] pixelBuffer in
            self?.onPixelBuffer?(pixelBuffer)
        }
    }

    func start(host: String, port: UInt16 = 5000) {
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
                print("🎥 Conectado com sucesso ao canal de vídeo TCP :\(port)!")
                self?.isRunning = true
                self?.receiveLoop()
            case .waiting(let error):
                print("⏳ Canal de Vídeo: Tentando alcançar \(host):\(port)... (\(error.localizedDescription))")
            case .preparing:
                print("🔄 Canal de Vídeo: Estabelecendo handshake TCP com \(host):\(port)...")
            case .failed(let error):
                print("❌ Falha no canal de vídeo: \(error)")
                self?.stop()
            case .cancelled:
                self?.isRunning = false
            default:
                break
            }
        }

        conn.start(queue: queue)
    }

    private var totalBytesReceived = 0
    private var lastLogTime = Date()

    private func receiveLoop() {
        guard isRunning, let conn = connection else { return }

        conn.receive(minimumIncompleteLength: 1, maximumLength: 131072) { [weak self] data, _, isComplete, error in
            guard let self = self else { return }

            if let data = data, !data.isEmpty {
                self.totalBytesReceived += data.count
                if Date().timeIntervalSince(self.lastLogTime) >= 2.0 {
                    print("📡 Rede de Vídeo: Recebidos \(self.totalBytesReceived / 1024) KB totais do servidor...")
                    self.lastLogTime = Date()
                }

                let nalUnits = self.parser.parse(data: data)
                for nal in nalUnits {
                    self.decoder.decode(nalUnit: nal)
                }
            }

            if let err = error {
                print("❌ Erro no socket de vídeo TCP: \(err)")
                self.stop()
            } else if isComplete {
                print("⚠️ Conexão de vídeo TCP fechada pelo servidor")
                self.stop()
            } else if self.isRunning {
                self.receiveLoop()
            }
        }
    }

    func stop() {
        isRunning = false
        connection?.cancel()
        connection = nil
    }

    deinit {
        stop()
    }
}
