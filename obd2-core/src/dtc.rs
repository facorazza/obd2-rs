//! Diagnostic trouble codes (Modes 03, 07 and 0A).

use core::fmt;

use heapless::Vec;

use crate::parse::{line_bytes, lines, strip_frame_prefix};
use crate::protocol::Framing;

/// A five-character diagnostic trouble code such as `P0133`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct DtcCode([u8; 5]);

impl DtcCode {
    /// Converts two DTC bytes into a code, or `None` for the `00 00` padding.
    pub fn from_bytes(high: u8, low: u8) -> Option<Self> {
        if high == 0 && low == 0 {
            return None;
        }

        let family = match high >> 6 {
            0 => b'P',
            1 => b'C',
            2 => b'B',
            _ => b'U',
        };
        Some(Self([
            family,
            b'0' + ((high >> 4) & 0x03),
            hex_digit(high & 0x0F),
            hex_digit(low >> 4),
            hex_digit(low & 0x0F),
        ]))
    }

    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.0).expect("DTC codes are ASCII")
    }
}

impl fmt::Display for DtcCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Debug for DtcCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Decodes a Mode 03 (`stored`), 07 (`pending`) or 0A (`permanent`) reply.
///
/// CAN replies carry a DTC-count byte after the response byte (`43`, `47`,
/// `4A`); the legacy protocols do not. Multi-frame replies arrive as numbered
/// lines (`0:`, `1:`, ...), which are stripped before the bytes are collected.
/// `out` is cleared first.
pub fn decode<const N: usize>(
    response: &str,
    mode: u8,
    framing: Framing,
    out: &mut Vec<DtcCode, N>,
) {
    out.clear();
    let response_mode = mode.wrapping_add(0x40);

    match framing {
        Framing::Can => {
            let mut bytes = [0u8; 128];
            let mut len = 0;
            for line in lines(response) {
                let Some(n) = line_bytes(strip_frame_prefix(line), &mut bytes[len..]) else {
                    continue;
                };
                len += n;
            }

            let Some(start) = bytes[..len].iter().position(|byte| *byte == response_mode) else {
                return;
            };
            let Some((&count, pairs)) = bytes[start + 1..len].split_first() else {
                return;
            };
            for pair in pairs.chunks_exact(2).take(count as usize) {
                let Some(code) = DtcCode::from_bytes(pair[0], pair[1]) else {
                    continue;
                };
                if out.push(code).is_err() {
                    return;
                }
            }
        }
        Framing::Legacy => {
            // Each legacy frame repeats the response byte, so decode line by
            // line instead of concatenating the whole reply.
            for line in lines(response) {
                let mut bytes = [0u8; 32];
                let Some(n) = line_bytes(strip_frame_prefix(line), &mut bytes) else {
                    continue;
                };
                let Some(start) = bytes[..n].iter().position(|byte| *byte == response_mode) else {
                    continue;
                };
                for pair in bytes[start + 1..n].chunks_exact(2) {
                    let Some(code) = DtcCode::from_bytes(pair[0], pair[1]) else {
                        continue;
                    };
                    if out.push(code).is_err() {
                        return;
                    }
                }
            }
        }
    }
}

fn hex_digit(nibble: u8) -> u8 {
    match nibble {
        0..=9 => b'0' + nibble,
        _ => b'A' + nibble - 10,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode_codes(response: &str, mode: u8, framing: Framing) -> Vec<DtcCode, 16> {
        let mut codes = Vec::new();
        decode(response, mode, framing, &mut codes);
        codes
    }

    #[test]
    fn code_from_bytes_covers_families() {
        assert_eq!(DtcCode::from_bytes(0x01, 0x01).unwrap().as_str(), "P0101");
        assert_eq!(DtcCode::from_bytes(0x41, 0x23).unwrap().as_str(), "C0123");
        assert_eq!(DtcCode::from_bytes(0x81, 0x34).unwrap().as_str(), "B0134");
        assert_eq!(DtcCode::from_bytes(0xC0, 0x01).unwrap().as_str(), "U0001");
        assert_eq!(DtcCode::from_bytes(0x00, 0x00), None);
    }

    #[test]
    fn can_reply_uses_count_byte() {
        let codes = decode_codes("43 02 01 33 02 44 00 00", 0x03, Framing::Can);
        assert_eq!(codes.len(), 2);
        assert_eq!(codes[0].as_str(), "P0133");
        assert_eq!(codes[1].as_str(), "P0244");
    }

    #[test]
    fn can_multiframe_reply() {
        let codes = decode_codes("0: 43 04 00 01 02 03\r1: 04 05 06 07", 0x03, Framing::Can);
        let expected = ["P0001", "P0203", "P0405", "P0607"];
        assert_eq!(codes.len(), expected.len());
        for (code, expected) in codes.iter().zip(expected) {
            assert_eq!(code.as_str(), expected);
        }
    }

    #[test]
    fn legacy_reply_has_no_count_byte() {
        let codes = decode_codes("43 03 01 C1 01 00 00", 0x03, Framing::Legacy);
        assert_eq!(codes.len(), 2);
        assert_eq!(codes[0].as_str(), "P0301");
        assert_eq!(codes[1].as_str(), "U0101");
    }

    #[test]
    fn pending_and_permanent_modes() {
        let pending = decode_codes("47 01 01 01", 0x07, Framing::Can);
        assert_eq!(pending[0].as_str(), "P0101");

        let permanent = decode_codes("4A 01 C1 01", 0x0A, Framing::Can);
        assert_eq!(permanent[0].as_str(), "U0101");
    }

    #[test]
    fn empty_and_invalid_replies() {
        assert!(decode_codes("43 00", 0x03, Framing::Can).is_empty());
        assert!(decode_codes("43 00 00 00 00 00 00", 0x03, Framing::Can).is_empty());
        assert!(decode_codes("43 00", 0x03, Framing::Legacy).is_empty());
        assert!(decode_codes("NO DATA", 0x03, Framing::Can).is_empty());
    }

    #[test]
    fn stops_at_capacity() {
        let codes = decode_codes("43 04 00 01 02 03 04 05 06 07", 0x03, Framing::Can);
        let mut small = Vec::<DtcCode, 2>::new();
        decode(
            "43 04 00 01 02 03 04 05 06 07",
            0x03,
            Framing::Can,
            &mut small,
        );
        assert_eq!(codes.len(), 4);
        assert_eq!(small.len(), 2);
    }
}
