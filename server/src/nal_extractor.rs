pub struct NalExtractor {
    buffer: Vec<u8>,
    codec_id: u8, // 0 = H.264, 1 = HEVC
}

impl NalExtractor {
    pub fn new(codec_id: u8) -> Self {
        Self {
            buffer: Vec::with_capacity(65536),
            codec_id,
        }
    }

    /// Adiciona novos bytes e retorna quaisquer frames NAL completos prontos para transmissão
    pub fn push_bytes(&mut self, data: &[u8]) -> Vec<Vec<u8>> {
        self.buffer.extend_from_slice(data);
        let mut ready_frames = Vec::new();

        let start_codes = self.find_start_codes();
        if start_codes.len() < 2 {
            return ready_frames;
        }

        // Se tivermos múltiplos start codes, podemos extrair as NAL units
        let mut last_processed_offset = 0;

        for i in 0..(start_codes.len() - 1) {
            let current = start_codes[i];
            let next = start_codes[i + 1];

            let nal_start = current.0 + current.1;
            let nal_end = next.0;

            if nal_end > nal_start {
                let nal_bytes = &self.buffer[current.0..nal_end];
                ready_frames.push(nal_bytes.to_vec());
                last_processed_offset = next.0;
            }
        }

        if last_processed_offset > 0 {
            self.buffer.drain(0..last_processed_offset);
        }

        ready_frames
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
