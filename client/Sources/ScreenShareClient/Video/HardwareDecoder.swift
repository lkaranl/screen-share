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

    func decodeFrame(nalUnits: [NALUnit]) {
        guard !nalUnits.isEmpty else { return }

        var slices: [Data] = []

        for nal in nalUnits {
            switch codec {
            case .h264:
                switch nal.type {
                case 7: // SPS
                    if spsData != nal.data {
                        spsData = nal.data
                        createH264FormatDescription()
                    }
                case 8: // PPS
                    if ppsData != nal.data {
                        ppsData = nal.data
                        createH264FormatDescription()
                    }
                case 1...5: // VCL Slices
                    slices.append(nal.data)
                default:
                    break
                }
            case .hevc:
                switch nal.type {
                case 32: // VPS
                    if vpsData != nal.data {
                        vpsData = nal.data
                        createHEVCFormatDescription()
                    }
                case 33: // SPS
                    if spsData != nal.data {
                        spsData = nal.data
                        createHEVCFormatDescription()
                    }
                case 34: // PPS
                    if ppsData != nal.data {
                        ppsData = nal.data
                        createHEVCFormatDescription()
                    }
                case 0...31: // VCL Slices (IDR, CRA, TRAIL, etc.)
                    slices.append(nal.data)
                default:
                    break
                }
            }
        }

        if !slices.isEmpty {
            decodeSlices(slices)
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
                    if let current = self.formatDescription, CMFormatDescriptionEqual(current, otherFormatDescription: format) {
                        return
                    }
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
                        if let current = self.formatDescription, CMFormatDescriptionEqual(current, otherFormatDescription: format) {
                            return
                        }
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

        self.formatDescription = format

        let destinationImageBufferAttributes: [CFString: Any] = [
            kCVPixelBufferPixelFormatTypeKey: kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
            kCVPixelBufferMetalCompatibilityKey: true,
            kCVPixelBufferOpenGLCompatibilityKey: true
        ]

        var callbackRecord = VTDecompressionOutputCallbackRecord(
            decompressionOutputCallback: { decompressionOutputRefCon, _, status, infoFlags, imageBuffer, _, _ in
                guard let refCon = decompressionOutputRefCon else { return }
                let decoder = Unmanaged<HardwareDecoder>.fromOpaque(refCon).takeUnretainedValue()

                if status == noErr, let pixelBuffer = imageBuffer {
                    decoder.totalFramesDecoded += 1
                    if decoder.totalFramesDecoded % 60 == 1 {
                        let w = CVPixelBufferGetWidth(pixelBuffer)
                        let h = CVPixelBufferGetHeight(pixelBuffer)
                        print("🖼️ Frame decodificado com sucesso via VideoToolbox #\(decoder.totalFramesDecoded) (\(w)x\(h))")
                    }
                    decoder.onPixelBufferReady?(pixelBuffer)
                } else {
                    print("⚠️ Erro no callback de descompressão VideoToolbox: status \(status) (infoFlags: \(infoFlags.rawValue))")
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

    /// Decodifica todos os slices de um quadro juntos no mesmo CMSampleBuffer (padrão Moonlight)
    private func decodeSlices(_ slices: [Data]) {
        guard let formatDesc = formatDescription, let session = decompressionSession else {
            return
        }

        // Calcula tamanho total com prefixo de 4 bytes de tamanho para cada slice (formato AVCC/HVCC)
        let totalLength = slices.reduce(0) { $0 + $1.count + 4 }

        var blockBuffer: CMBlockBuffer?
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

        guard status == noErr, let buffer = blockBuffer else {
            return
        }

        var offset = 0
        for slice in slices {
            var lengthBigEndian = UInt32(slice.count).bigEndian
            status = CMBlockBufferReplaceDataBytes(
                with: &lengthBigEndian,
                blockBuffer: buffer,
                offsetIntoDestination: offset,
                dataLength: 4
            )
            guard status == noErr else { return }
            offset += 4

            slice.withUnsafeBytes { rawPtr in
                if let baseAddress = rawPtr.baseAddress {
                    _ = CMBlockBufferReplaceDataBytes(
                        with: baseAddress,
                        blockBuffer: buffer,
                        offsetIntoDestination: offset,
                        dataLength: slice.count
                    )
                }
            }
            offset += slice.count
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
            let decodeStatus = VTDecompressionSessionDecodeFrame(
                session,
                sampleBuffer: sample,
                flags: [._EnableAsynchronousDecompression],
                frameRefcon: nil,
                infoFlagsOut: &flagsOut
            )
            if decodeStatus != noErr {
                print("⚠️ VTDecompressionSessionDecodeFrame retornou erro: \(decodeStatus)")
            }
        }
    }

    deinit {
        if let session = decompressionSession {
            VTDecompressionSessionInvalidate(session)
        }
    }
}
