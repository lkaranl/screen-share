pub struct NalExtractor {
    buffer: Vec<u8>,
}

impl NalExtractor {
    pub fn new() -> Self {
        Self {
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

        // Identifica os limites de cada quadro completo (Access Unit)
        // Um novo quadro começa quando encontramos VPS/SPS/AUD ou quando já temos um slice no quadro atual e encontramos outro slice
        let mut current_frame_start = start_codes[0].0;
        let mut current_has_vcl_slice = false;
        let mut last_emitted_offset = 0;

        for i in 0..start_codes.len() {
            let (code_offset, code_len) = start_codes[i];
            let nal_header_idx = code_offset + code_len;
            if nal_header_idx >= self.buffer.len() {
                break;
            }

            let nal_byte = self.buffer[nal_header_idx];
            let is_vcl_slice = Self::is_vcl_slice(nal_byte);
            let is_new_header = Self::is_sequence_header(nal_byte);

            // Se já temos dados do quadro atual e detectamos o início do próximo quadro
            if (is_new_header || (is_vcl_slice && current_has_vcl_slice)) && code_offset > current_frame_start {
                let frame_bytes = &self.buffer[current_frame_start..code_offset];
                if !frame_bytes.is_empty() {
                    ready_frames.push(frame_bytes.to_vec());
                    last_emitted_offset = code_offset;
                }
                current_frame_start = code_offset;
                current_has_vcl_slice = false;
            }

            if is_vcl_slice {
                current_has_vcl_slice = true;
            }
        }

        if last_emitted_offset > 0 {
            self.buffer.drain(0..last_emitted_offset);
        }

        ready_frames
    }

    /// Determina se a NAL unit é um slice de vídeo (H.264 ou HEVC)
    fn is_vcl_slice(first_byte: u8) -> bool {
        // HEVC: bits 1..6 (0..31 são VCL)
        let hevc_type = (first_byte >> 1) & 0x3F;
        // H.264: bits 0..4 (1..5 são VCL)
        let h264_type = first_byte & 0x1F;

        hevc_type <= 31 || (h264_type >= 1 && h264_type <= 5)
    }

    /// Determina se a NAL unit é cabeçalho de sequência (VPS, SPS, PPS, AUD)
    fn is_sequence_header(first_byte: u8) -> bool {
        let hevc_type = (first_byte >> 1) & 0x3F;
        let h264_type = first_byte & 0x1F;

        // HEVC: 32 (VPS), 33 (SPS), 34 (PPS), 35 (AUD)
        // H.264: 7 (SPS), 8 (PPS), 9 (AUD)
        (hevc_type >= 32 && hevc_type <= 35) || (h264_type == 7 || h264_type == 8 || h264_type == 9)
    }

    /// Encontra todas as posições de start code (offset, length) no buffer
    fn find_start_codes(&self) -> Vec<(usize, usize)> {
        let mut codes = Vec::new();
        let len = self.buffer.len();
        if len < 4 {
            return codes;
        }

        let mut i = 0;
        while i + 3 < len {
            if self.buffer[i] == 0 && self.buffer[i + 1] == 0 {
                if self.buffer[i + 2] == 1 {
                    codes.push((i, 3));
                    i += 3;
                    continue;
                } else if i + 4 <= len && self.buffer[i + 2] == 0 && self.buffer[i + 3] == 1 {
                    codes.push((i, 4));
                    i += 4;
                    continue;
                }
            }
            i += 1;
        }

        codes
    }
}
