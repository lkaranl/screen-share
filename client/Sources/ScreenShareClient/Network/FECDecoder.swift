import Foundation

// MARK: - Aritmética de Campo de Galois GF(2^8) compatível com a crate Rust reed-solomon-erasure
final class GaloisField256 {
    static let shared = GaloisField256()

    private var expTable = [UInt8](repeating: 0, count: 512)
    private var logTable = [UInt8](repeating: 0, count: 256)

    init() {
        var x: UInt32 = 1
        for i in 0..<255 {
            expTable[i] = UInt8(x)
            expTable[i + 255] = UInt8(x)
            logTable[Int(x)] = UInt8(i)
            x <<= 1
            if (x & 0x100) != 0 {
                x ^= 0x11d // Polinômio irredutível padrão (x^8 + x^4 + x^3 + x^2 + 1)
            }
        }
        logTable[0] = 0
    }

    @inline(__always)
    func add(_ a: UInt8, _ b: UInt8) -> UInt8 {
        return a ^ b
    }

    @inline(__always)
    func multiply(_ a: UInt8, _ b: UInt8) -> UInt8 {
        if a == 0 || b == 0 { return 0 }
        let logA = Int(logTable[Int(a)])
        let logB = Int(logTable[Int(b)])
        return expTable[logA + logB]
    }

    @inline(__always)
    func divide(_ a: UInt8, _ b: UInt8) -> UInt8 {
        if a == 0 { return 0 }
        if b == 0 { fatalError("Divisão por zero em GF(256)") }
        let logA = Int(logTable[Int(a)])
        let logB = Int(logTable[Int(b)])
        var diff = logA - logB
        if diff < 0 { diff += 255 }
        return expTable[diff]
    }

    @inline(__always)
    func inverse(_ a: UInt8) -> UInt8 {
        if a == 0 { fatalError("Inverso de zero em GF(256)") }
        let logA = Int(logTable[Int(a)])
        return expTable[255 - logA]
    }
}

// MARK: - Estrutura de Montagem do Frame
private final class FrameCollector {
    let frameIndex: UInt32
    let totalDataShards: Int
    let totalParityShards: Int
    let codec: UInt8

    var receivedShards: [Int: Data] = [:]
    var payloadSizes: [Int: Int] = [:]
    let createdAt = Date()

    init(header: VideoRtpHeader) {
        self.frameIndex = header.frameIndex
        self.totalDataShards = Int(header.totalDataShards)
        self.totalParityShards = Int(header.totalParityShards)
        self.codec = header.codec
    }

    func addShard(header: VideoRtpHeader, payload: Data) {
        let idx = Int(header.packetIndex)
        if receivedShards[idx] == nil {
            receivedShards[idx] = payload
            payloadSizes[idx] = Int(header.payloadSize)
        }
    }

    var isComplete: Bool {
        // Se temos todos os shards de dados originais
        for i in 0..<totalDataShards {
            if receivedShards[i] == nil {
                // Se faltar algum de dados, podemos recuperar se o total recebido >= totalDataShards
                return receivedShards.count >= totalDataShards
            }
        }
        return true
    }

    func reconstructPayload() -> Data? {
        // Caso 1: Temos todos os dados sem nenhuma perda
        var allDataPresent = true
        for i in 0..<totalDataShards {
            if receivedShards[i] == nil {
                allDataPresent = false
                break
            }
        }

        if allDataPresent {
            var fullData = Data()
            for i in 0..<totalDataShards {
                guard let shard = receivedShards[i] else { return nil }
                let actualSize = payloadSizes[i] ?? shard.count
                fullData.append(shard.prefix(actualSize))
            }
            return fullData
        }

        // Caso 2: Houve perda de dados, mas temos shards de paridade suficientes para recuperação
        guard receivedShards.count >= totalDataShards else {
            return nil
        }

        return recoverWithParity()
    }

    private func recoverWithParity() -> Data? {
        let gf = GaloisField256.shared
        let k = totalDataShards
        guard let firstShard = receivedShards.values.first else { return nil }
        let shardSize = firstShard.count

        // Coleta os primeiros k shards disponíveis
        var presentIndices: [Int] = []
        var presentShards: [Data] = []

        for (idx, shard) in receivedShards.sorted(by: { $0.key < $1.key }) {
            if presentIndices.count < k {
                presentIndices.append(idx)
                presentShards.append(shard)
            }
        }

        guard presentIndices.count == k else { return nil }

        // Matriz de codificação geradora Vandermonde
        // Linha i, coluna j: (j + 1)^i
        var matrix = [[UInt8]](repeating: [UInt8](repeating: 0, count: k), count: k)
        for r in 0..<k {
            let rowIdx = presentIndices[r]
            for c in 0..<k {
                if rowIdx < k {
                    // Linhas de dados são matriz identidade
                    matrix[r][c] = (rowIdx == c) ? 1 : 0
                } else {
                    // Linhas de paridade são potências de Vandermonde
                    let pIdx = rowIdx - k
                    let base = UInt8(c + 1)
                    var val: UInt8 = 1
                    for _ in 0...pIdx {
                        val = gf.multiply(val, base)
                    }
                    matrix[r][c] = val
                }
            }
        }

        // Inversão da matriz (Eliminação de Gauss-Jordan em GF(256))
        var inv = [[UInt8]](repeating: [UInt8](repeating: 0, count: k), count: k)
        for i in 0..<k { inv[i][i] = 1 }

        for col in 0..<k {
            var pivotRow = -1
            for row in col..<k {
                if matrix[row][col] != 0 {
                    pivotRow = row
                    break
                }
            }
            guard pivotRow != -1 else { return nil }

            if pivotRow != col {
                matrix.swapAt(col, pivotRow)
                inv.swapAt(col, pivotRow)
            }

            let pivotVal = matrix[col][col]
            let invPivot = gf.inverse(pivotVal)
            for j in 0..<k {
                matrix[col][j] = gf.multiply(matrix[col][j], invPivot)
                inv[col][j] = gf.multiply(inv[col][j], invPivot)
            }

            for row in 0..<k {
                if row != col && matrix[row][col] != 0 {
                    let factor = matrix[row][col]
                    for j in 0..<k {
                        matrix[row][j] = gf.add(matrix[row][j], gf.multiply(factor, matrix[col][j]))
                        inv[row][j] = gf.add(inv[row][j], gf.multiply(factor, inv[col][j]))
                    }
                }
            }
        }

        // Reconstrói os k shards de dados originais
        var recoveredData = Data()

        for dataIdx in 0..<k {
            var recoveredShard = [UInt8](repeating: 0, count: shardSize)

            for bytePos in 0..<shardSize {
                var sum: UInt8 = 0
                for r in 0..<k {
                    let coeff = inv[dataIdx][r]
                    let byteVal = presentShards[r][bytePos]
                    sum = gf.add(sum, gf.multiply(coeff, byteVal))
                }
                recoveredShard[bytePos] = sum
            }

            let actualSize = payloadSizes[dataIdx] ?? shardSize
            recoveredData.append(Data(recoveredShard.prefix(actualSize)))
        }

        return recoveredData
    }
}

// MARK: - Decodificador FEC Público
final class FECDecoder {
    private var frames: [UInt32: FrameCollector] = [:]
    private var lastEmittedFrame: UInt32 = 0
    private let queue = DispatchQueue(label: "screenshare.fec.decoder", qos: .userInteractive)

    var onFrameReconstructed: ((Data, UInt8) -> Void)?

    func process(packetData: Data) {
        queue.async { [weak self] in
            guard let self = self else { return }
            guard let (header, payload) = VideoRtpHeader.parse(from: packetData) else { return }

            // Descarta frames muito antigos
            if self.lastEmittedFrame > 0 && header.frameIndex < self.lastEmittedFrame && (self.lastEmittedFrame - header.frameIndex) < 1000 {
                return
            }

            let collector: FrameCollector
            if let existing = self.frames[header.frameIndex] {
                collector = existing
            } else {
                collector = FrameCollector(header: header)
                self.frames[header.frameIndex] = collector
            }

            collector.addShard(header: header, payload: payload)

            // Limpa frames antigos que expiraram (> 1 segundo)
            let now = Date()
            self.frames = self.frames.filter { now.timeIntervalSince($0.value.createdAt) < 1.0 }

            if collector.isComplete {
                if let fullFrameData = collector.reconstructPayload() {
                    self.lastEmittedFrame = collector.frameIndex
                    self.frames.removeValue(forKey: collector.frameIndex)
                    self.onFrameReconstructed?(fullFrameData, collector.codec)
                }
            }
        }
    }
}
