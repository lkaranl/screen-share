import Foundation

public struct VideoRtpHeader {
    public static let magic: UInt16 = 0x5253
    public static let headerSize = 16

    public let frameIndex: UInt32
    public let packetIndex: UInt16
    public let totalDataShards: UInt16
    public let totalParityShards: UInt16
    public let payloadSize: UInt16
    public let flags: UInt8
    public let codec: UInt8

    public var isSOF: Bool { (flags & 0x01) != 0 }
    public var isEOF: Bool { (flags & 0x02) != 0 }
    public var isParity: Bool { (flags & 0x04) != 0 }

    public static func parse(from data: Data) -> (header: VideoRtpHeader, payload: Data)? {
        guard data.count >= headerSize else { return nil }

        let magic = data.withUnsafeBytes { $0.loadUnaligned(fromByteOffset: 0, as: UInt16.self).bigEndian }
        guard magic == VideoRtpHeader.magic else { return nil }

        let frameIndex = data.withUnsafeBytes { $0.loadUnaligned(fromByteOffset: 2, as: UInt32.self).bigEndian }
        let packetIndex = data.withUnsafeBytes { $0.loadUnaligned(fromByteOffset: 6, as: UInt16.self).bigEndian }
        let totalDataShards = data.withUnsafeBytes { $0.loadUnaligned(fromByteOffset: 8, as: UInt16.self).bigEndian }
        let totalParityShards = data.withUnsafeBytes { $0.loadUnaligned(fromByteOffset: 10, as: UInt16.self).bigEndian }
        let payloadSize = data.withUnsafeBytes { $0.loadUnaligned(fromByteOffset: 12, as: UInt16.self).bigEndian }
        let flags = data[14]
        let codec = data[15]

        let header = VideoRtpHeader(
            frameIndex: frameIndex,
            packetIndex: packetIndex,
            totalDataShards: totalDataShards,
            totalParityShards: totalParityShards,
            payloadSize: payloadSize,
            flags: flags,
            codec: codec
        )

        let payload = data.subdata(in: headerSize..<data.count)
        return (header, payload)
    }
}
