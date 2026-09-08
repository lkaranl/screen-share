use crate::capture::VideoCodec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NalFrameType {
    Aud,
    SequenceStart, // VPS (HEVC) ou SPS (H.264)
    VclSlice,      // Fatias de vídeo (I-frame, P-frame, etc.)
    Other,         // PPS, SEI, etc.
}

pub struct NalExtractor {
    codec: VideoCodec,
    buffer: Vec<u8>,
}

impl NalExtractor {
    pub fn new(codec: VideoCodec) -> Self {
        Self {
            codec,
            buffer: Vec::with_capacity(131072),
        }
    }

    /// Adiciona novos bytes do stream Annex-B e extrai quadros de vídeo completos (Access Units)
    pub fn push_bytes(&mut self, data: &[u8]) -> Vec<Vec<u8>> {
        self.buffer.extend_from_slice(data);
        let mut ready_frames = Vec::new();

        let start_codes = self.find_start_codes();
        if start_codes.len() < 2 {
            return ready_frames;
        }

        let mut current_frame_start = start_codes[0].0;
        let mut current_has_vcl = false;
        let mut last_emitted_offset = 0;

        for i in 0..start_codes.len() {
            let (code_offset, code_len) = start_codes[i];
            let nal_header_idx = code_offset + code_len;
            if nal_header_idx >= self.buffer.len() {
                break;
            }

            let nal_byte = self.buffer[nal_header_idx];
            let nal_type = self.classify_nal(nal_byte);

            // Um novo quadro (Access Unit) começa se:
            // 1. Encontramos um AUD
            // 2. Encontramos um SequenceStart (VPS/SPS) e o quadro atual já possui fatia de vídeo
            // 3. Encontramos uma fatia de vídeo (VCL) e o quadro atual já possui fatia de vídeo
            let is_boundary = match nal_type {
                NalFrameType::Aud => true,
                NalFrameType::SequenceStart => current_has_vcl,
                NalFrameType::VclSlice => current_has_vcl,
                NalFrameType::Other => false,
            };

            if is_boundary && code_offset > current_frame_start {
                let frame_bytes = &self.buffer[current_frame_start..code_offset];
                if !frame_bytes.is_empty() {
                    ready_frames.push(frame_bytes.to_vec());
                    last_emitted_offset = code_offset;
                }
                current_frame_start = code_offset;
                current_has_vcl = false;
            }

            if nal_type == NalFrameType::VclSlice {
                current_has_vcl = true;
            }
        }

        if last_emitted_offset > 0 {
            self.buffer.drain(0..last_emitted_offset);
        }

        ready_frames
    }

    /// Classifica a NAL unit de acordo com o codec ativo
    fn classify_nal(&self, first_byte: u8) -> NalFrameType {
        match self.codec {
            VideoCodec::H264 => {
                let nal_type = first_byte & 0x1F;
                match nal_type {
                    9 => NalFrameType::Aud,
                    7 => NalFrameType::SequenceStart, // SPS
                    1..=5 => NalFrameType::VclSlice,
                    _ => NalFrameType::Other, // PPS (8), SEI (6), etc.
                }
            }
            VideoCodec::HEVC => {
                let nal_type = (first_byte >> 1) & 0x3F;
                match nal_type {
                    35 => NalFrameType::Aud,
                    32 | 33 => NalFrameType::SequenceStart, // VPS (32), SPS (33)
                    0..=31 => NalFrameType::VclSlice,
                    _ => NalFrameType::Other, // PPS (34), SEI (39/40), etc.
                }
            }
            VideoCodec::AV1 => NalFrameType::Other,
        }
    }

    /// Encontra todas as posições de start code (offset, length) no buffer
    fn find_start_codes(&self) -> Vec<(usize, usize)> {
        let mut codes = Vec::new();
        let len = self.buffer.len();
        if len < 3 {
            return codes;
        }

        let mut i = 0;
        while i + 2 < len {
            if self.buffer[i] == 0 && self.buffer[i + 1] == 0 {
                // Checa 4 bytes: 00 00 00 01
                if i + 3 < len && self.buffer[i + 2] == 0 && self.buffer[i + 3] == 1 {
                    codes.push((i, 4));
                    i += 4;
                    continue;
                }
                // Checa 3 bytes: 00 00 01
                if self.buffer[i + 2] == 1 {
                    codes.push((i, 3));
                    i += 3;
                    continue;
                }
            }
            i += 1;
        }

        codes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_h264_atomic_keyframe_extraction() {
        let mut extractor = NalExtractor::new(VideoCodec::H264);

        // Frame 1: Keyframe [AUD (9)] [SPS (7)] [PPS (8)] [IDR Slice (5)]
        // Frame 2: P-frame [AUD (9)] [P Slice (1)]
        let mut stream = Vec::new();
        // AUD
        stream.extend_from_slice(&[0, 0, 0, 1, 0x09, 0xF0]);
        // SPS
        stream.extend_from_slice(&[0, 0, 0, 1, 0x67, 0x42, 0x00, 0x28]);
        // PPS
        stream.extend_from_slice(&[0, 0, 0, 1, 0x68, 0xCE, 0x38, 0x80]);
        // IDR Slice
        stream.extend_from_slice(&[0, 0, 0, 1, 0x65, 0x88, 0x84, 0x00]);

        // Frame 2 AUD
        stream.extend_from_slice(&[0, 0, 0, 1, 0x09, 0xF0]);
        // Frame 2 P-Slice
        stream.extend_from_slice(&[0, 0, 0, 1, 0x41, 0x9A]);

        // Frame 3 AUD (para forçar emissão do Frame 2)
        stream.extend_from_slice(&[0, 0, 0, 1, 0x09, 0xF0]);

        let frames = extractor.push_bytes(&stream);
        assert_eq!(frames.len(), 2, "Deveria emitir exatamente 2 quadros completos");

        // Quadro 1 deve conter AUD + SPS + PPS + IDR Slice
        assert!(frames[0].starts_with(&[0, 0, 0, 1, 0x09]));
        assert!(frames[0].contains(&0x67), "Quadro 1 deve conter SPS");
        assert!(frames[0].contains(&0x68), "Quadro 1 deve conter PPS");
        assert!(frames[0].contains(&0x65), "Quadro 1 deve conter IDR Slice");

        // Quadro 2 deve conter AUD + P-Slice
        assert!(frames[1].starts_with(&[0, 0, 0, 1, 0x09]));
        assert!(frames[1].contains(&0x41), "Quadro 2 deve conter P Slice");
    }

    #[test]
    fn test_hevc_atomic_keyframe_extraction() {
        let mut extractor = NalExtractor::new(VideoCodec::HEVC);

        let mut stream = Vec::new();
        // AUD (35 -> 0x46)
        stream.extend_from_slice(&[0, 0, 0, 1, 0x46, 0x01]);
        // VPS (32 -> 0x40)
        stream.extend_from_slice(&[0, 0, 0, 1, 0x40, 0x01, 0x0C]);
        // SPS (33 -> 0x42)
        stream.extend_from_slice(&[0, 0, 0, 1, 0x42, 0x01, 0x01]);
        // PPS (34 -> 0x44)
        stream.extend_from_slice(&[0, 0, 0, 1, 0x44, 0x01, 0xC0]);
        // IDR Slice (19 -> 0x26)
        stream.extend_from_slice(&[0, 0, 0, 1, 0x26, 0x01, 0xAF]);

        // Frame 2 AUD
        stream.extend_from_slice(&[0, 0, 0, 1, 0x46, 0x01]);
        // Frame 2 TRAIL Slice (1 -> 0x02)
        stream.extend_from_slice(&[0, 0, 0, 1, 0x02, 0x01, 0xD0]);

        // Frame 3 AUD
        stream.extend_from_slice(&[0, 0, 0, 1, 0x46, 0x01]);

        let frames = extractor.push_bytes(&stream);
        assert_eq!(frames.len(), 2, "Deveria emitir exatamente 2 quadros HEVC completos");

        // Quadro 1 deve conter VPS, SPS, PPS e IDR Slice juntos
        assert!(frames[0].contains(&0x40), "Quadro 1 deve conter VPS");
        assert!(frames[0].contains(&0x42), "Quadro 1 deve conter SPS");
        assert!(frames[0].contains(&0x44), "Quadro 1 deve conter PPS");
        assert!(frames[0].contains(&0x26), "Quadro 1 deve conter IDR Slice");
    }
}
