use anyhow::{Context, Result};
use reed_solomon_erasure::galois_8::ReedSolomon;

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
