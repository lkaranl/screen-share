import Foundation
import CoreMedia

enum VideoCodecType {
    case h264
    case hevc
}

enum VideoResolutionType: UInt8, CaseIterable, Identifiable {
    case fhd = 0 // 1080p (1920x1080)
    case qhd = 1 // 2K (2560x1440)
    case uhd = 2 // 4K (3840x2160)

    var id: UInt8 { rawValue }

    var displayName: String {
        switch self {
        case .fhd: return "1080p"
        case .qhd: return "2K (1440p)"
        case .uhd: return "4K (2160p)"
        }
    }

    var shortName: String {
        switch self {
        case .fhd: return "1080p"
        case .qhd: return "2K"
        case .uhd: return "4K"
        }
    }

    var dimensions: (width: Int, height: Int) {
        switch self {
        case .fhd: return (1920, 1080)
        case .qhd: return (2560, 1440)
        case .uhd: return (3840, 2160)
        }
    }

    static func fromString(_ str: String) -> VideoResolutionType {
        let lower = str.lowercased()
        if lower.contains("4k") || lower.contains("2160") || lower.contains("uhd") {
            return .uhd
        } else if lower.contains("2k") || lower.contains("1440") || lower.contains("qhd") {
            return .qhd
        } else {
            return .fhd
        }
    }
}

struct NALUnit {
    let data: Data
    let type: UInt8
}

final class NALUnitParser {
    private var buffer = Data()
    private var codec: VideoCodecType
    private var totalNALsParsed = 0

    init(codec: VideoCodecType) {
        self.codec = codec
    }

    func switchCodec(_ newCodec: VideoCodecType) {
        self.codec = newCodec
        self.buffer.removeAll(keepingCapacity: true)
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
