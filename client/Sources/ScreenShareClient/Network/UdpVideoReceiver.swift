import Foundation
import Network
import CoreVideo

final class UdpVideoReceiver {
    private var tcpHandshakeConnection: NWConnection?
    private var udpListener: NWListener?
    private let queue = DispatchQueue(label: "screenshare.udp.video.queue", qos: .userInteractive)

    private let fecDecoder = FECDecoder()
    private let parser: NALUnitParser
    private let decoder: HardwareDecoder

    var onPixelBuffer: ((CVPixelBuffer) -> Void)?

    init(codec: VideoCodecType) {
        self.parser = NALUnitParser(codec: codec)
        self.decoder = HardwareDecoder(codec: codec)

        self.decoder.onPixelBufferReady = { [weak self] pixelBuffer in
            self?.onPixelBuffer?(pixelBuffer)
        }

        self.fecDecoder.onFrameReconstructed = { [weak self] (frameData: Data, _: UInt8) in
            guard let self = self else { return }
            let nalUnits = self.parser.parse(data: frameData)
            for nal in nalUnits {
                self.decoder.decode(nalUnit: nal)
            }
        }
    }

    func start(host: String, port: UInt16 = 5000) {
        setupUdpListener(port: port)
        setupTcpHandshake(host: host, port: port)
    }

    private func setupUdpListener(port: UInt16) {
        do {
            let udpParams = NWParameters.udp
            udpParams.allowLocalEndpointReuse = true

            let listener = try NWListener(using: udpParams, on: NWEndpoint.Port(integerLiteral: port))
            self.udpListener = listener

            listener.newConnectionHandler = { [weak self] connection in
                guard let self = self else { return }
                connection.start(queue: self.queue)
                self.receiveUdpPackets(on: connection)
            }

            listener.stateUpdateHandler = { state in
                switch state {
                case .ready:
                    print("📡 Receptor UDP de Vídeo pronto e escutando na porta \(port) (Buffer de Alta Performance)")
                case .failed(let err):
                    print("⚠️ Falha no listener UDP: \(err)")
                default:
                    break
                }
            }

            listener.start(queue: queue)
        } catch {
            print("❌ Erro ao criar NWListener UDP: \(error)")
        }
    }

    private func receiveUdpPackets(on connection: NWConnection) {
        connection.receiveMessage { [weak self, weak connection] content, _, isComplete, error in
            guard let self = self else { return }

            if let data = content, !data.isEmpty {
                self.fecDecoder.process(packetData: data)
            }

            if error == nil, let conn = connection {
                self.receiveUdpPackets(on: conn)
            }
        }
    }

    private func setupTcpHandshake(host: String, port: UInt16) {
        let tcpOptions = NWProtocolTCP.Options()
        tcpOptions.noDelay = true

        let params = NWParameters(tls: nil, tcp: tcpOptions)
        let endpoint = NWEndpoint.hostPort(host: NWEndpoint.Host(host), port: NWEndpoint.Port(integerLiteral: port))

        let conn = NWConnection(to: endpoint, using: params)
        self.tcpHandshakeConnection = conn

        conn.stateUpdateHandler = { state in
            switch state {
            case .ready:
                print("🔗 Handshake TCP estabelecido com o servidor de vídeo (\(host):\(port))")
            case .failed(let err):
                print("⚠️ Erro no handshake TCP com servidor de vídeo: \(err)")
            default:
                break
            }
        }

        conn.start(queue: queue)
    }

    func stop() {
        udpListener?.cancel()
        udpListener = nil
        tcpHandshakeConnection?.cancel()
        tcpHandshakeConnection = nil
    }

    deinit {
        stop()
    }
}
