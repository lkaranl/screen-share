import Foundation
import CoreMedia

enum VideoCodecType {
    case h264
    case hevc
}

struct NALUnit {
    let data: Data
    let type: UInt8
}

final class NALUnitParser {
    private var buffer = Data()
    private let codec: VideoCodecType
    private var totalNALsParsed = 0

    init(codec: VideoCodecType) {
        self.codec = codec
    }

    struct StartCode {
        let offset: Int
        let length: Int
    }

    func parse(data: Data) -> [NALUnit] {
        buffer.append(data)
        var nalUnits: [NALUnit] = []

        let startCodes = findAllStartCodes(in: buffer)
        guard !startCodes.isEmpty else {
            return nalUnits
        }

        for i in 0..<startCodes.count {
            let current = startCodes[i]
            let payloadStart = current.offset + current.length
            let payloadEnd = (i + 1 < startCodes.count) ? startCodes[i + 1].offset : buffer.count

            if payloadEnd > payloadStart {
                let nalData = buffer.subdata(in: payloadStart..<payloadEnd)
                if let nalType = extractNALType(from: nalData) {
                    nalUnits.append(NALUnit(data: nalData, type: nalType))
                    totalNALsParsed += 1
                }
            }
        }

        buffer.removeAll(keepingCapacity: true)
        return nalUnits
    }

    private func findAllStartCodes(in data: Data) -> [StartCode] {
        var results: [StartCode] = []
        let count = data.count
        guard count >= 3 else { return results }

        data.withUnsafeBytes { rawBuffer in
            guard let ptr = rawBuffer.baseAddress?.assumingMemoryBound(to: UInt8.self) else { return }
            var i = 0
            while i <= count - 3 {
                if ptr[i] == 0 && ptr[i + 1] == 0 {
                    if i <= count - 4 && ptr[i + 2] == 0 && ptr[i + 3] == 1 {
                        results.append(StartCode(offset: i, length: 4))
                        i += 4
                        continue
                    } else if ptr[i + 2] == 1 {
                        results.append(StartCode(offset: i, length: 3))
                        i += 3
                        continue
                    }
                }
                i += 1
            }
        }

        return results
    }

    private func extractNALType(from data: Data) -> UInt8? {
        guard let firstByte = data.first else { return nil }
        switch codec {
        case .h264:
            return firstByte & 0x1F
        case .hevc:
            return (firstByte >> 1) & 0x3F
        }
    }
}
