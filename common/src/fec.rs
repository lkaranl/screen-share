use anyhow::{Context, Result};
use reed_solomon_erasure::galois_8::ReedSolomon;
use std::collections::BTreeMap;

use crate::rtp::{
    VideoRtpHeader, FLAG_EOF, FLAG_PARITY, FLAG_SOF, RTP_HEADER_SIZE, RTP_PAYLOAD_MAX_SIZE,
};

pub struct FecEncoder {
    fec_percentage: usize,
}

impl FecEncoder {
    pub fn new(fec_percentage: usize) -> Self {
        Self {
            fec_percentage: fec_percentage.clamp(5, 50),
        }
    }

    /// Codifica um frame de vídeo em uma lista de pacotes RTP (Dados + Paridade FEC)
    pub fn encode_frame(
        &self,
        frame_index: u32,
        codec_id: u8,
        frame_data: &[u8],
    ) -> Result<Vec<Vec<u8>>> {
        let total_bytes = frame_data.len();
        if total_bytes == 0 {
            return Ok(Vec::new());
        }

        let shard_size = RTP_PAYLOAD_MAX_SIZE;
        let data_shards = (total_bytes + shard_size - 1) / shard_size;
        let parity_shards = ((data_shards * self.fec_percentage + 99) / 100).max(1);

        // Instancia o encoder Reed-Solomon SIMD
        let rs = ReedSolomon::new(data_shards, parity_shards)
            .context("Falha ao inicializar encoder Reed-Solomon")?;

        // Aloca os shards (Data + Parity)
        let total_shards = data_shards + parity_shards;
        let mut shards: Vec<Vec<u8>> = Vec::with_capacity(total_shards);

        // Preenche os shards de dados
        for i in 0..data_shards {
            let start = i * shard_size;
            let end = (start + shard_size).min(total_bytes);
            let mut shard = vec![0u8; shard_size];
            shard[..(end - start)].copy_from_slice(&frame_data[start..end]);
            shards.push(shard);
        }

        // Aloca os shards de paridade inicializados com zeros
        for _ in 0..parity_shards {
            shards.push(vec![0u8; shard_size]);
        }

        // Calcula a paridade Reed-Solomon em memória
        rs.encode(&mut shards)
            .context("Falha ao calcular paridade Reed-Solomon")?;

        // Empacota os shards com os headers RTP de 16 bytes
        let mut packets = Vec::with_capacity(total_shards);

        for (packet_idx, shard) in shards.into_iter().enumerate() {
            let is_parity = packet_idx >= data_shards;
            let payload_size = if !is_parity {
                let start = packet_idx * shard_size;
                let end = (start + shard_size).min(total_bytes);
                (end - start) as u16
            } else {
                shard_size as u16
            };

            let mut flags = 0u8;
            if packet_idx == 0 {
                flags |= FLAG_SOF;
            }
            if packet_idx == data_shards - 1 {
                flags |= FLAG_EOF;
            }
            if is_parity {
                flags |= FLAG_PARITY;
            }

            let header = VideoRtpHeader {
                frame_index,
                packet_index: packet_idx as u16,
                total_data_shards: data_shards as u16,
                total_parity_shards: parity_shards as u16,
                payload_size,
                flags,
                codec: codec_id,
            };

            let mut packet = vec![0u8; RTP_HEADER_SIZE + shard_size];
            header.serialize(&mut packet[0..RTP_HEADER_SIZE]);
            packet[RTP_HEADER_SIZE..].copy_from_slice(&shard);

            packets.push(packet);
        }

        Ok(packets)
    }
}

pub struct DecodedFrame {
    pub frame_index: u32,
    pub codec: u8,
    pub data: Vec<u8>,
}

struct PendingFrame {
    total_data_shards: usize,
    total_parity_shards: usize,
    shard_size: usize,
    codec: u8,
    payload_sizes: Vec<usize>,
    shards: Vec<Option<Vec<u8>>>,
    received_count: usize,
}

pub struct FecDecoder {
    pending_frames: BTreeMap<u32, PendingFrame>,
    max_active_frames: usize,
    last_emitted_frame: Option<u32>,
}

impl FecDecoder {
    pub fn new(max_active_frames: usize) -> Self {
        Self {
            pending_frames: BTreeMap::new(),
            max_active_frames: max_active_frames.max(4),
            last_emitted_frame: None,
        }
    }

    /// Processa um pacote RTP bruto. Se um frame for completado, retorna `Some(DecodedFrame)`.
    pub fn process_packet(&mut self, packet_bytes: &[u8]) -> Result<Option<DecodedFrame>> {
        let header = match VideoRtpHeader::deserialize(packet_bytes) {
            Some(h) => h,
            None => return Ok(None),
        };

        if packet_bytes.len() < RTP_HEADER_SIZE {
            return Ok(None);
        }

        let payload = &packet_bytes[RTP_HEADER_SIZE..];
        let frame_index = header.frame_index;

        if let Some(last_emitted) = self.last_emitted_frame {
            if frame_index <= last_emitted && last_emitted - frame_index < 500 {
                return Ok(None); // Frame antigo já entregue
            }
        }

        let total_data = header.total_data_shards as usize;
        let total_parity = header.total_parity_shards as usize;
        let total_shards = total_data + total_parity;
        let packet_idx = header.packet_index as usize;

        if packet_idx >= total_shards || total_data == 0 {
            return Ok(None);
        }

        let shard_size = payload.len();

        let pending = self.pending_frames.entry(frame_index).or_insert_with(|| {
            PendingFrame {
                total_data_shards: total_data,
                total_parity_shards: total_parity,
                shard_size,
                codec: header.codec,
                payload_sizes: vec![0; total_data],
                shards: vec![None; total_shards],
                received_count: 0,
            }
        });

        if pending.shards[packet_idx].is_none() {
            let mut shard_vec = vec![0u8; pending.shard_size];
            let copy_len = payload.len().min(pending.shard_size);
            shard_vec[..copy_len].copy_from_slice(&payload[..copy_len]);

            if packet_idx < total_data {
                pending.payload_sizes[packet_idx] = header.payload_size as usize;
            }

            pending.shards[packet_idx] = Some(shard_vec);
            pending.received_count += 1;
        }

        // Verifica se temos shards suficientes para reconstruir
        if pending.received_count >= pending.total_data_shards {
            let frame = self.reconstruct_frame(frame_index)?;
            self.last_emitted_frame = Some(frame_index);

            // Limpa frames antigos da memória
            while self.pending_frames.len() > self.max_active_frames {
                if let Some(&oldest_key) = self.pending_frames.keys().next() {
                    if oldest_key <= frame_index {
                        self.pending_frames.remove(&oldest_key);
                    } else {
                        break;
                    }
                }
            }

            return Ok(Some(frame));
        }

        // Limita o número de frames pendentes para evitar vazamento de memória
        while self.pending_frames.len() > self.max_active_frames * 2 {
            if let Some(&oldest_key) = self.pending_frames.keys().next() {
                self.pending_frames.remove(&oldest_key);
            }
        }

        Ok(None)
    }

    fn reconstruct_frame(&mut self, frame_index: u32) -> Result<DecodedFrame> {
        let mut pending = self.pending_frames.remove(&frame_index)
            .context("Frame não encontrado para reconstrução")?;

        let data_shards_len = pending.total_data_shards;
        let parity_shards_len = pending.total_parity_shards;
        let shard_size = pending.shard_size;

        // Verifica se todos os shards de dados já estão presentes sem necessidade de FEC
        let all_data_present = pending.shards[..data_shards_len].iter().all(|s| s.is_some());

        if !all_data_present {
            // Reconstrói usando Reed-Solomon
            let rs = ReedSolomon::new(data_shards_len, parity_shards_len)
                .context("Falha ao inicializar ReedSolomon para decodificação")?;

            rs.reconstruct(&mut pending.shards)
                .context("Falha ao reconstruir shards de dados com Reed-Solomon")?;
        }

        // Remonta o frame concatenando os shards de dados
        let mut complete_data = Vec::new();
        for i in 0..data_shards_len {
            if let Some(ref shard) = pending.shards[i] {
                let psize = pending.payload_sizes[i];
                let take_len = if psize > 0 && psize <= shard.len() {
                    psize
                } else if i == data_shards_len - 1 {
                    shard.len()
                } else {
                    shard_size
                };
                complete_data.extend_from_slice(&shard[..take_len]);
            }
        }

        Ok(DecodedFrame {
            frame_index,
            codec: pending.codec,
            data: complete_data,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fec_encode_decode_no_loss() {
        let encoder = FecEncoder::new(20);
        let mut decoder = FecDecoder::new(8);

        let test_payload = b"Hello, Screen Share Remote Video Stream! Ultra low latency test.";
        let packets = encoder.encode_frame(1, 0, test_payload).expect("Codificação falhou");
        assert!(!packets.is_empty());

        let mut decoded = None;
        for pkt in packets {
            if let Some(frame) = decoder.process_packet(&pkt).expect("Processamento falhou") {
                decoded = Some(frame);
            }
        }

        let frame = decoded.expect("Frame deveria ter sido reconstruído");
        assert_eq!(frame.frame_index, 1);
        assert_eq!(frame.codec, 0);
        assert_eq!(frame.data, test_payload);
    }

    #[test]
    fn test_fec_encode_decode_with_packet_loss() {
        let encoder = FecEncoder::new(25);
        let mut decoder = FecDecoder::new(8);

        // Payload grande para gerar múltiplos shards
        let test_payload = vec![42u8; 4000];
        let packets = encoder.encode_frame(2, 1, &test_payload).expect("Codificação falhou");
        assert!(packets.len() > 3);

        // Removemos propositalmente 1 pacote de dados (simulando perda de rede)
        let mut simulated_packets = packets;
        simulated_packets.remove(0); // descarta o primeiro pacote de dados

        let mut decoded = None;
        for pkt in simulated_packets {
            if let Some(frame) = decoder.process_packet(&pkt).expect("Processamento falhou") {
                decoded = Some(frame);
            }
        }

        let frame = decoded.expect("Frame deveria ter sido reconstruído via paridade FEC");
        assert_eq!(frame.frame_index, 2);
        assert_eq!(frame.codec, 1);
        assert_eq!(frame.data, test_payload);
    }
}
