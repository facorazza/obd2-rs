//! Supported-PID bitmaps.

use crate::parse::data_bytes;

/// The bitmaps reported by `01 00`, `01 20` and `01 40`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SupportedPids {
    bitmaps: [u32; 3],
}

impl SupportedPids {
    pub const fn new() -> Self {
        Self { bitmaps: [0; 3] }
    }

    /// Stores the bitmap for `base` (`0x00`, `0x20` or `0x40`).
    pub fn set(&mut self, base: u8, bitmap: u32) {
        match base {
            0x00 => self.bitmaps[0] = bitmap,
            0x20 => self.bitmaps[1] = bitmap,
            0x40 => self.bitmaps[2] = bitmap,
            _ => {}
        }
    }

    pub fn get(&self, base: u8) -> Option<u32> {
        match base {
            0x00 => Some(self.bitmaps[0]),
            0x20 => Some(self.bitmaps[1]),
            0x40 => Some(self.bitmaps[2]),
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
    fn update_reads_reply() {
        let mut pids = SupportedPids::new();
        assert!(pids.update(0x00, "SEARCHING...4100D83B8013\r\r>"));
        assert_eq!(pids.get(0x00), Some(0xD83B_8013));
        assert!(!pids.update(0x20, "NO DATA\r\r>"));
    }
}
