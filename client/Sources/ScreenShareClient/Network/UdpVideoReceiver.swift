import Foundation
import Network
import CoreVideo

final class UdpVideoReceiver {
    /// Porta local onde escutamos os frames UDP do servidor (fixa e conhecida)
    static let videoListenPort: UInt16 = 50000

    private var listener: NWListener?
    private var activeConnection: NWConnection?
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
            self.decoder.decodeFrame(nalUnits: nalUnits)
        }
    }

    /// Inicia o listener UDP na porta 50000 para receber frames do servidor.
    /// Retorna a porta local confirmada (sempre 50000).
    func start() {
        let udpParams = NWParameters.udp
        udpParams.allowLocalEndpointReuse = true
        udpParams.serviceClass = .interactiveVideo

        guard let listener = try? NWListener(using: udpParams, on: NWEndpoint.Port(rawValue: Self.videoListenPort)!) else {
            print("❌ Falha ao criar NWListener na porta \(Self.videoListenPort)")
            return
        }
        self.listener = listener

        listener.stateUpdateHandler = { state in
            switch state {
            case .ready:
                print("📡 NWListener UDP de Vídeo pronto na porta \(Self.videoListenPort)")
            case .failed(let err):
                print("❌ NWListener UDP falhou: \(err)")
            default:
                break
            }
        }

        listener.newConnectionHandler = { [weak self] conn in
            guard let self = self else { return }
            print("📡 Servidor conectou ao canal UDP de vídeo")
            // Cancela conexão anterior se existir
            self.activeConnection?.cancel()
            self.activeConnection = conn
            conn.start(queue: self.queue)
            self.receivePackets(on: conn)
        }

        listener.start(queue: queue)
    }

    private func receivePackets(on connection: NWConnection) {
        connection.receiveMessage { [weak self, weak connection] content, _, _, error in
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
        activeConnection?.cancel()
        activeConnection = nil
        listener?.cancel()
        listener = nil
    }

    deinit {
        stop()
    }
}
