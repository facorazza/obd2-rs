//! Supported-PID bitmaps.

use crate::parse::data_bytes;

/// The bitmaps reported by `01 00`, `01 20`, `01 40`, `01 60` and `01 80`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SupportedPids {
    bitmaps: [u32; 5],
}

impl SupportedPids {
    pub const fn new() -> Self {
        Self { bitmaps: [0; 5] }
    }

    /// Stores the bitmap for `base` (`0x00`, `0x20`, `0x40`, `0x60` or `0x80`).
    pub fn set(&mut self, base: u8, bitmap: u32) {
        match base {
            0x00 => self.bitmaps[0] = bitmap,
            0x20 => self.bitmaps[1] = bitmap,
            0x40 => self.bitmaps[2] = bitmap,
            0x60 => self.bitmaps[3] = bitmap,
            0x80 => self.bitmaps[4] = bitmap,
            _ => {}
        }
    }

    pub fn get(&self, base: u8) -> Option<u32> {
        match base {
            0x00 => Some(self.bitmaps[0]),
            0x20 => Some(self.bitmaps[1]),
            0x40 => Some(self.bitmaps[2]),
            0x60 => Some(self.bitmaps[3]),
            0x80 => Some(self.bitmaps[4]),
            _ => None,
        }
    }

    /// Reads the bitmap for `base` from a reply and stores it.
    pub fn update(&mut self, base: u8, response: &str) -> bool {
        let mut bytes = [0u8; 16];
        let Some(data) = data_bytes(response, base, &mut bytes) else {
            return false;
        };
        if data.len() < 4 {
            return false;
        }
        self.set(
            base,
            u32::from_be_bytes([data[0], data[1], data[2], data[3]]),
        );
        true
    }

    /// True when the ECU reported support for `pid`.
    ///
    /// Bit 31 of the `01 00` bitmap is PID 1; bit 0 is PID 0x20.
    pub fn contains(&self, pid: u8) -> bool {
        let (bitmap, offset) = match pid {
            0x01..=0x20 => (self.bitmaps[0], pid),
            0x21..=0x40 => (self.bitmaps[1], pid - 0x20),
            0x41..=0x60 => (self.bitmaps[2], pid - 0x40),
            0x61..=0x80 => (self.bitmaps[3], pid - 0x60),
            0x81..=0xA0 => (self.bitmaps[4], pid - 0x80),
            _ => return false,
        };
        bitmap & (1u32 << (32 - offset as u32)) != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bit_order_is_big_endian() {
        let mut pids = SupportedPids::new();
        // 0xBE1FB810: PIDs 1, 3, 4, 5, 6, ... as reported by a real ECU.
        pids.set(0x00, 0xBE1F_B810);
        assert!(pids.contains(0x01));
        assert!(pids.contains(0x04));
        assert!(pids.contains(0x0C));
        assert!(!pids.contains(0x02));
        assert!(!pids.contains(0x61));
    }

    #[test]
    fn diesel_blocks_are_discovered() {
        let mut pids = SupportedPids::new();
        // 0x61 (bit 31) and 0x78 (bit 8) in the 0x61-0x80 block.
        pids.set(0x60, 0x8000_0100);
        assert!(pids.contains(0x61));
        assert!(pids.contains(0x78));
        assert!(!pids.contains(0x62));
        // 0x83 (bit 29) in the 0x81-0xA0 block.
        pids.set(0x80, 0x2000_0000);
        assert!(pids.contains(0x83));
        assert!(!pids.contains(0x81));
    }

    #[test]
    fn update_reads_reply() {
        let mut pids = SupportedPids::new();
        assert!(pids.update(0x00, "SEARCHING...4100D83B8013\r\r>"));
        assert_eq!(pids.get(0x00), Some(0xD83B_8013));
        assert!(!pids.update(0x20, "NO DATA\r\r>"));
    }
}
