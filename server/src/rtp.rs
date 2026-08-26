use byteorder::{BigEndian, ByteOrder};

pub const RTP_MAGIC: u16 = 0x5253; // "RS" (Remote Screen)
pub const RTP_HEADER_SIZE: usize = 16;
pub const RTP_PAYLOAD_MAX_SIZE: usize = 1380;
pub const RTP_PACKET_MAX_SIZE: usize = RTP_HEADER_SIZE + RTP_PAYLOAD_MAX_SIZE; // 1396 bytes (cabe perfeitamente em MTU 1500)

pub const FLAG_SOF: u8 = 0x01; // Start of Frame
pub const FLAG_EOF: u8 = 0x02; // End of Frame
pub const FLAG_PARITY: u8 = 0x04; // Shard de Paridade FEC

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoRtpHeader {
    pub frame_index: u32,
    pub packet_index: u16,
    pub total_data_shards: u16,
    pub total_parity_shards: u16,
    pub payload_size: u16,
    pub flags: u8,
    pub codec: u8, // 0 = H.264, 1 = HEVC
}

impl VideoRtpHeader {
    pub fn serialize(&self, buf: &mut [u8]) {
        assert!(buf.len() >= RTP_HEADER_SIZE);
        BigEndian::write_u16(&mut buf[0..2], RTP_MAGIC);
        BigEndian::write_u32(&mut buf[2..6], self.frame_index);
        BigEndian::write_u16(&mut buf[6..8], self.packet_index);
        BigEndian::write_u16(&mut buf[8..10], self.total_data_shards);
        BigEndian::write_u16(&mut buf[10..12], self.total_parity_shards);
        BigEndian::write_u16(&mut buf[12..14], self.payload_size);
        buf[14] = self.flags;
        buf[15] = self.codec;
    }

    pub fn deserialize(buf: &[u8]) -> Option<Self> {
        if buf.len() < RTP_HEADER_SIZE {
            return None;
        }
        let magic = BigEndian::read_u16(&buf[0..2]);
        if magic != RTP_MAGIC {
            return None;
        }

        Some(Self {
            frame_index: BigEndian::read_u32(&buf[2..6]),
            packet_index: BigEndian::read_u16(&buf[6..8]),
            total_data_shards: BigEndian::read_u16(&buf[8..10]),
            total_parity_shards: BigEndian::read_u16(&buf[10..12]),
            payload_size: BigEndian::read_u16(&buf[12..14]),
            flags: buf[14],
            codec: buf[15],
        })
    }
}
