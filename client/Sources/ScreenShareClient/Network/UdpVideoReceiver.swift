import Foundation
import Network
import CoreVideo

final class UdpVideoReceiver {
    private var udpConnection: NWConnection?
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
        let udpParams = NWParameters.udp
        udpParams.allowLocalEndpointReuse = true
        udpParams.serviceClass = .interactiveVideo

        let endpoint = NWEndpoint.hostPort(host: NWEndpoint.Host(host), port: NWEndpoint.Port(integerLiteral: port))
        let conn = NWConnection(to: endpoint, using: udpParams)
        self.udpConnection = conn

        conn.stateUpdateHandler = { [weak self, weak conn] state in
            guard let self = self else { return }
            switch state {
            case .ready:
                print("📡 Canal UDP de Vídeo conectado ao servidor (\(host):\(port))")
                // Envia pacote HELO para registrar IP/porta no servidor
                let heloData = "RS_HELO".data(using: .utf8)!
                conn?.send(content: heloData, completion: .contentProcessed({ _ in }))
                if let conn = conn {
                    self.receivePackets(on: conn)
                }
            case .failed(let err):
                print("⚠️ Erro no canal UDP de vídeo: \(err)")
            default:
                break
            }
        }

        conn.start(queue: queue)
    }

    private func receivePackets(on connection: NWConnection) {
        connection.receiveMessage { [weak self, weak connection] content, _, isComplete, error in
            guard let self = self else { return }

            if let data = content, !data.isEmpty {
                self.fecDecoder.process(packetData: data)
            }

            if error == nil, let conn = connection {
                self.receivePackets(on: conn)
            }
        }
    }

    func stop() {
        udpConnection?.cancel()
        udpConnection = nil
    }

    deinit {
        stop()
    }
}
