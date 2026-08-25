import Foundation
import CoreMedia
import VideoToolbox
import CoreVideo

final class HardwareDecoder {
    private let codec: VideoCodecType
    private var formatDescription: CMVideoFormatDescription?
    private var decompressionSession: VTDecompressionSession?

    // H.264 Parameter Sets
    private var spsData: Data?
    private var ppsData: Data?

    // HEVC Parameter Sets
    private var vpsData: Data?

    var onPixelBufferReady: ((CVPixelBuffer) -> Void)?

    private var totalFramesDecoded = 0

    init(codec: VideoCodecType) {
        self.codec = codec
    }

    func decode(nalUnit: NALUnit) {
        switch codec {
        case .h264:
            handleH264(nalUnit: nalUnit)
        case .hevc:
            handleHEVC(nalUnit: nalUnit)
        }
    }

    private func handleH264(nalUnit: NALUnit) {
        switch nalUnit.type {
        case 7: // SPS
            if spsData != nalUnit.data {
                spsData = nalUnit.data
                createH264FormatDescription()
            }
        case 8: // PPS
            if ppsData != nalUnit.data {
                ppsData = nalUnit.data
                createH264FormatDescription()
            }
        case 1...5: // Non-IDR Slice, Partition Slices, IDR Slice
            decodeSlice(from: nalUnit.data)
        default:
            break
        }
    }

    private func handleHEVC(nalUnit: NALUnit) {
        switch nalUnit.type {
        case 32: // VPS
            if vpsData != nalUnit.data {
                vpsData = nalUnit.data
                createHEVCFormatDescription()
            }
        case 33: // SPS
            if spsData != nalUnit.data {
                spsData = nalUnit.data
                createHEVCFormatDescription()
            }
        case 34: // PPS
            if ppsData != nalUnit.data {
                ppsData = nalUnit.data
                createHEVCFormatDescription()
            }
        case 0...31: // Todos os VCL Slices (TRAIL_N, TRAIL_R, TSA, STSA, RADL, RASL, BLA, IDR, CRA_NUT 21)
            decodeSlice(from: nalUnit.data)
        default:
            break
        }
    }

    private func createH264FormatDescription() {
        guard let sps = spsData, let pps = ppsData else { return }

        sps.withUnsafeBytes { spsPtr in
            pps.withUnsafeBytes { ppsPtr in
                guard let spsBase = spsPtr.baseAddress, let ppsBase = ppsPtr.baseAddress else { return }
                let parameterSets = [spsBase.assumingMemoryBound(to: UInt8.self), ppsBase.assumingMemoryBound(to: UInt8.self)]
                let parameterSetSizes = [sps.count, pps.count]

                var newFormatDesc: CMFormatDescription?
                let status = CMVideoFormatDescriptionCreateFromH264ParameterSets(
                    allocator: kCFAllocatorDefault,
                    parameterSetCount: 2,
                    parameterSetPointers: parameterSets,
                    parameterSetSizes: parameterSetSizes,
                    nalUnitHeaderLength: 4,
                    formatDescriptionOut: &newFormatDesc
                )

                if status == noErr, let format = newFormatDesc {
                    self.formatDescription = format
                    let dim = CMVideoFormatDescriptionGetDimensions(format)
                    print("✅ Formato H.264 detectado por Hardware: \(dim.width)x\(dim.height)")
                    self.setupDecompressionSession(format: format)
                }
            }
        }
    }

    private func createHEVCFormatDescription() {
        guard let vps = vpsData, let sps = spsData, let pps = ppsData else { return }

        vps.withUnsafeBytes { vpsPtr in
            sps.withUnsafeBytes { spsPtr in
                pps.withUnsafeBytes { ppsPtr in
                    guard let vpsBase = vpsPtr.baseAddress, let spsBase = spsPtr.baseAddress, let ppsBase = ppsPtr.baseAddress else { return }
                    let parameterSets = [
                        vpsBase.assumingMemoryBound(to: UInt8.self),
                        spsBase.assumingMemoryBound(to: UInt8.self),
                        ppsBase.assumingMemoryBound(to: UInt8.self)
                    ]
                    let parameterSetSizes = [vps.count, sps.count, pps.count]

                    var newFormatDesc: CMFormatDescription?
                    let status = CMVideoFormatDescriptionCreateFromHEVCParameterSets(
                        allocator: kCFAllocatorDefault,
                        parameterSetCount: 3,
                        parameterSetPointers: parameterSets,
                        parameterSetSizes: parameterSetSizes,
                        nalUnitHeaderLength: 4,
                        extensions: nil,
                        formatDescriptionOut: &newFormatDesc
                    )

                    if status == noErr, let format = newFormatDesc {
                        self.formatDescription = format
                        let dim = CMVideoFormatDescriptionGetDimensions(format)
                        print("✅ Formato HEVC/H.265 detectado por Hardware: \(dim.width)x\(dim.height)")
                        self.setupDecompressionSession(format: format)
                    }
                }
            }
        }
    }

    private func setupDecompressionSession(format: CMVideoFormatDescription) {
        if let session = decompressionSession {
            VTDecompressionSessionInvalidate(session)
            self.decompressionSession = nil
        }

        let destinationImageBufferAttributes: [CFString: Any] = [
            kCVPixelBufferPixelFormatTypeKey: kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
            kCVPixelBufferMetalCompatibilityKey: true,
            kCVPixelBufferOpenGLCompatibilityKey: true
        ]

        var callbackRecord = VTDecompressionOutputCallbackRecord(
            decompressionOutputCallback: { decompressionOutputRefCon, sourceFrameRefCon, status, infoFlags, imageBuffer, presentationTimeStamp, presentationDuration in
                guard let refCon = decompressionOutputRefCon else { return }
                let decoder = Unmanaged<HardwareDecoder>.fromOpaque(refCon).takeUnretainedValue()

                if status == noErr, let pixelBuffer = imageBuffer {
                    decoder.totalFramesDecoded += 1
                    decoder.onPixelBufferReady?(pixelBuffer)
                }
            },
            decompressionOutputRefCon: Unmanaged.passUnretained(self).toOpaque()
        )

        var newSession: VTDecompressionSession?
        let status = VTDecompressionSessionCreate(
            allocator: kCFAllocatorDefault,
            formatDescription: format,
            decoderSpecification: [
                kVTVideoDecoderSpecification_EnableHardwareAcceleratedVideoDecoder: true
            ] as CFDictionary,
            imageBufferAttributes: destinationImageBufferAttributes as CFDictionary,
            outputCallback: &callbackRecord,
            decompressionSessionOut: &newSession
        )

        if status == noErr, let session = newSession {
            VTSessionSetProperty(session, key: kVTDecompressionPropertyKey_RealTime, value: kCFBooleanTrue)
            self.decompressionSession = session
            print("🚀 VTDecompressionSession de Hardware (Real-Time Zero-Copy) inicializada!")
        } else {
            print("⚠️ Falha ao criar VTDecompressionSession: status \(status)")
        }
    }

    private func decodeSlice(from nalData: Data) {
        guard let formatDesc = formatDescription, let session = decompressionSession else { return }

        var blockBuffer: CMBlockBuffer?
        let totalLength = nalData.count + 4
        var status = CMBlockBufferCreateWithMemoryBlock(
            allocator: kCFAllocatorDefault,
            memoryBlock: nil,
            blockLength: totalLength,
            blockAllocator: kCFAllocatorDefault,
            customBlockSource: nil,
            offsetToData: 0,
            dataLength: totalLength,
            flags: 0,
            blockBufferOut: &blockBuffer
        )

        guard status == noErr, let buffer = blockBuffer else { return }

        // Prefixo de 4 bytes com o tamanho do NAL em Big-Endian (Formato AVCC)
        var lengthBigEndian = UInt32(nalData.count).bigEndian
        status = CMBlockBufferReplaceDataBytes(
            with: &lengthBigEndian,
            blockBuffer: buffer,
            offsetIntoDestination: 0,
            dataLength: 4
        )
        guard status == noErr else { return }

        nalData.withUnsafeBytes { rawPtr in
            if let baseAddress = rawPtr.baseAddress {
                _ = CMBlockBufferReplaceDataBytes(
                    with: baseAddress,
                    blockBuffer: buffer,
                    offsetIntoDestination: 4,
                    dataLength: nalData.count
                )
            }
        }

        var sampleBuffer: CMSampleBuffer?
        var timingInfo = CMSampleTimingInfo(
            duration: CMTime.invalid,
            presentationTimeStamp: CMClockGetTime(CMClockGetHostTimeClock()),
            decodeTimeStamp: CMTime.invalid
        )

        status = CMSampleBufferCreate(
            allocator: kCFAllocatorDefault,
            dataBuffer: buffer,
            dataReady: true,
            makeDataReadyCallback: nil,
            refcon: nil,
            formatDescription: formatDesc,
            sampleCount: 1,
            sampleTimingEntryCount: 1,
            sampleTimingArray: &timingInfo,
            sampleSizeEntryCount: 0,
            sampleSizeArray: nil,
            sampleBufferOut: &sampleBuffer
        )

        if status == noErr, let sample = sampleBuffer {
            var flagsOut: VTDecodeInfoFlags = []
            VTDecompressionSessionDecodeFrame(
                session,
                sampleBuffer: sample,
                flags: [._EnableAsynchronousDecompression],
                frameRefcon: nil,
                infoFlagsOut: &flagsOut
            )
        }
    }

    deinit {
        if let session = decompressionSession {
            VTDecompressionSessionInvalidate(session)
        }
    }
}
