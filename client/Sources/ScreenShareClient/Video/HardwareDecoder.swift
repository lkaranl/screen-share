import Foundation
import CoreMedia
import VideoToolbox
import AVFoundation

final class HardwareDecoder {
    private let codec: VideoCodecType
    private var formatDescription: CMVideoFormatDescription?

    // H.264 Parameter Sets
    private var spsData: Data?
    private var ppsData: Data?

    // HEVC Parameter Sets
    private var vpsData: Data?

    var onSampleBufferReady: ((CMSampleBuffer) -> Void)?

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
            spsData = nalUnit.data
            createH264FormatDescription()
        case 8: // PPS
            ppsData = nalUnit.data
            createH264FormatDescription()
        case 1...5: // Non-IDR Slice, Partition Slices, IDR Slice
            createAndEmitSampleBuffer(from: nalUnit.data)
        default:
            break
        }
    }

    private func handleHEVC(nalUnit: NALUnit) {
        switch nalUnit.type {
        case 32: // VPS
            vpsData = nalUnit.data
            createHEVCFormatDescription()
        case 33: // SPS
            spsData = nalUnit.data
            createHEVCFormatDescription()
        case 34: // PPS
            ppsData = nalUnit.data
            createHEVCFormatDescription()
        case 0...31: // Todos os VCL Slices (TRAIL_N, TRAIL_R, TSA, STSA, RADL, RASL, BLA, IDR, CRA_NUT 21)
            createAndEmitSampleBuffer(from: nalUnit.data)
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
                } else {
                    print("⚠️ Falha ao criar CMVideoFormatDescription H.264: status \(status)")
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
                    } else {
                        print("⚠️ Falha ao criar CMVideoFormatDescription HEVC: status \(status)")
                    }
                }
            }
        }
    }

    private var totalFramesEmitted = 0
    private var hasLoggedWaiting = false

    private func createAndEmitSampleBuffer(from nalData: Data) {
        guard let formatDesc = formatDescription else {
            if !hasLoggedWaiting {
                print("⏳ Decoder: Aguardando SPS/PPS (Headers de vídeo) do servidor para inicializar hardware...")
                hasLoggedWaiting = true
            }
            return
        }

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

        guard status == noErr, let buffer = blockBuffer else {
            print("⚠️ Erro ao criar CMBlockBuffer: \(status)")
            return
        }

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
            // Anexa flag para exibição imediata
            if let attachments = CMSampleBufferGetSampleAttachmentsArray(sample, createIfNecessary: true) {
                let dict = unsafeBitCast(CFArrayGetValueAtIndex(attachments, 0), to: CFMutableDictionary.self)
                CFDictionarySetValue(dict, Unmanaged.passUnretained(kCMSampleAttachmentKey_DisplayImmediately).toOpaque(), Unmanaged.passUnretained(kCFBooleanTrue).toOpaque())
            }

            totalFramesEmitted += 1
            if totalFramesEmitted == 1 {
                print("🎉 Decoder: Primeiro frame CMSampleBuffer gerado com sucesso!")
            } else if totalFramesEmitted % 180 == 0 {
                print("🎬 Decoder: \(totalFramesEmitted) frames de vídeo gerados para renderização")
            }

            self.onSampleBufferReady?(sample)
        } else {
            print("⚠️ Erro ao criar CMSampleBuffer: \(status)")
        }
    }
}
