//! Legacy (non-CAN) protocol parser: SAE J1850 PWM/VPW, ISO 9141-2 and
//! ISO 14230-4.
//!
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::message::{Frame, Message, ProtocolParser};
use crate::protocol::decode_hex;
use crate::util::contiguous;

/// Legacy protocol variants.
#[derive(Debug, Clone, Copy)]
pub enum LegacyProtocol {
    SaeJ1850Pwm,
    SaeJ1850Vpw,
    Iso91412,
    Iso1423045baud,
    Iso142304Fast,
}

impl ProtocolParser for LegacyProtocol {
    fn elm_name(&self) -> &'static str {
        match self {
            LegacyProtocol::SaeJ1850Pwm => "SAE J1850 PWM",
            LegacyProtocol::SaeJ1850Vpw => "SAE J1850 VPW",
            LegacyProtocol::Iso91412 => "ISO 9141-2",
            LegacyProtocol::Iso1423045baud => "ISO 14230-4 (KWP 5BAUD)",
            LegacyProtocol::Iso142304Fast => "ISO 14230-4 (KWP FAST)",
        }
    }

    fn elm_id(&self) -> &'static str {
        match self {
            LegacyProtocol::SaeJ1850Pwm => "1",
            LegacyProtocol::SaeJ1850Vpw => "2",
            LegacyProtocol::Iso91412 => "3",
            LegacyProtocol::Iso1423045baud => "4",
            LegacyProtocol::Iso142304Fast => "5",
        }
    }

    fn tx_id_engine(&self) -> Option<u32> {
        Some(0x10)
    }

    fn tx_id_transmission(&self) -> Option<u32> {
        None
    }

    fn parse_frame(&self, frame: &mut Frame) -> bool {
        let raw = &frame.raw;

        // handle odd size frames and drop
        if raw.len() & 1 == 1 {
            tracing::debug!("Dropping frame for being odd");
            return false;
        }

        let Some(raw_bytes) = decode_hex(raw) else {
            return false;
        };

        if raw_bytes.len() < 6 {
            tracing::debug!("Dropped frame for being too short");
            return false;
        }
        if raw_bytes.len() > 11 {
            tracing::debug!("Dropped frame for being too long");
            return false;
        }

        // 48 6B 10 41 00 BE 7F B8 13 ck
        // ck = checksum byte (handled by ELM adapter)

        // exclude header and trailing checksum
        frame.data = raw_bytes[3..raw_bytes.len() - 1].to_vec();

        // read header information
        frame.priority = Some(raw_bytes[0]);
        frame.rx_id = Some(u32::from(raw_bytes[1]));
        frame.tx_id = Some(u32::from(raw_bytes[2]));

        true
    }

    fn parse_message(&self, message: &mut Message) -> bool {
        // len(frames) will always be >= 1 (guaranteed by the caller)
        let mode = message.frames[0].data[0];

        // test that all frames are responses to the same Mode (SID)
        if message.frames.len() > 1 && !message.frames[1..].iter().all(|f| f.data[0] == mode) {
            tracing::debug!("Recieved frames from multiple commands");
            return false;
        }

        // legacy protocols have different re-assembly procedures for
        // different Modes. NOTE: there are hacks here to make some output
        // compatible with CAN, since CAN is the standard.

        if mode == 0x43 {
            // GET_DTC requests return frames with no PID or order bytes.
            // Forge the mode byte and CAN's DTC_count byte.
            message.data = vec![0x43, 0x00];
            for f in &message.frames {
                message.data.extend_from_slice(&f.data[1..]);
            }
        } else if message.frames.len() == 1 {
            // return data, excluding the mode/pid bytes
            message.data = message.frames[0].data.clone();
        } else {
            // generic multiline requests carry an order byte; sort by it
            let mut order: Vec<usize> = (0..message.frames.len()).collect();
            order.sort_by_key(|&i| message.frames[i].data[2]);

            // check contiguity
            let indices: Vec<i64> = order
                .iter()
                .map(|&i| i64::from(message.frames[i].data[2]))
                .collect();
            if !contiguous(&indices, 1, message.frames.len() as i64) {
                tracing::debug!("Recieved multiline response with missing frames");
                return false;
            }

            // preserve the first frame's mode and PID bytes (for consistency
            // with CAN); remove the sequence byte
            message.frames[order[0]].data.remove(2);
            message.data = message.frames[order[0]].data.clone();

            // add the data from the remaining frames
            for &i in &order[1..] {
                message.data.extend_from_slice(&message.frames[i].data[3..]);
            }
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{Protocol, ecu};

    const LEGACY: [LegacyProtocol; 5] = [
        LegacyProtocol::SaeJ1850Pwm,
        LegacyProtocol::SaeJ1850Vpw,
        LegacyProtocol::Iso91412,
        LegacyProtocol::Iso1423045baud,
        LegacyProtocol::Iso142304Fast,
    ];

    fn check_message(m: &Message, n_frames: usize, tx_id: u32, data: &[u8]) {
        assert_eq!(m.frames.len(), n_frames);
        assert_eq!(m.tx_id(), Some(tx_id));
        assert_eq!(m.data, data);
    }

    #[test]
    fn single_frame() {
        for p in LEGACY {
            let p = Protocol::new(Box::new(p), &[]);

            // minimum valid length
            let r = p.call(&["48 6B 10 41 00 FF".to_string()]);
            assert_eq!(r.len(), 1);
            check_message(&r[0], 1, 0x10, &[0x41, 0x00]);

            // maximum valid length
            let r = p.call(&["48 6B 10 41 00 00 01 02 03 04 FF".to_string()]);
            assert_eq!(r.len(), 1);
            check_message(&r[0], 1, 0x10, &[0x41, 0x00, 0x00, 0x01, 0x02, 0x03, 0x04]);

            // too short
            let r = p.call(&["48 6B 10 41 FF".to_string()]);
            assert_eq!(r.len(), 0);

            // too long
            let r = p.call(&["48 6B 10 41 00 00 01 02 03 04 05 FF".to_string()]);
            assert_eq!(r.len(), 0);

            // odd (invalid)
            let r = p.call(&["48 6B 10 41 00 00 F".to_string()]);
            assert_eq!(r.len(), 0);
        }
    }

    #[test]
    fn hex_straining() {
        // non-hex values should be marked as ECU.UNKNOWN
        for p in LEGACY {
            let p = Protocol::new(Box::new(p), &[]);

            // single non-hex message
            let r = p.call(&["12.8 Volts".to_string()]);
            assert_eq!(r.len(), 1);
            assert_eq!(r[0].ecu, ecu::UNKNOWN);
            assert_eq!(r[0].frames.len(), 1);

            // multiple non-hex messages
            let r = p.call(&["12.8 Volts".to_string(), "NO DATA".to_string()]);
            assert_eq!(r.len(), 2);
            for m in &r {
                assert_eq!(m.ecu, ecu::UNKNOWN);
                assert_eq!(m.frames.len(), 1);
            }

            // mixed hex and non-hex
            let r = p.call(&[
                "NO DATA".to_string(),
                "48 6B 10 41 00 00 01 02 03 FF".to_string(),
            ]);
            assert_eq!(r.len(), 2);

            // first message should be the valid, parsable hex message
            check_message(&r[0], 1, 0x10, &[0x41, 0x00, 0x00, 0x01, 0x02, 0x03]);

            // second message: invalid, non-parsable non-hex
            assert_eq!(r[1].ecu, ecu::UNKNOWN);
            assert_eq!(r[1].frames.len(), 1);
            assert_eq!(r[1].data.len(), 0); // no data
        }
    }

    #[test]
    fn multi_ecu() {
        for p in LEGACY {
            let p = Protocol::new(Box::new(p), &[]);

            let test_case = [
                "48 6B 13 41 00 00 01 02 03 FF",
                "48 6B 10 41 00 00 01 02 03 FF",
                "48 6B 11 41 00 00 01 02 03 FF",
            ];
            let correct_data = [0x41, 0x00, 0x00, 0x01, 0x02, 0x03];

            // separate ECUs, single frames each
            let r = p.call(&test_case.map(String::from));
            assert_eq!(r.len(), test_case.len());

            // messages are returned in ECU order
            check_message(&r[0], 1, 0x10, &correct_data);
            check_message(&r[1], 1, 0x11, &correct_data);
            check_message(&r[2], 1, 0x13, &correct_data);
        }
    }

    #[test]
    fn multi_line() {
        // valid multiline messages are recombined into single messages
        for p in LEGACY {
            let p = Protocol::new(Box::new(p), &[]);

            let test_case = [
                "48 6B 10 49 02 01 00 01 02 03 FF",
                "48 6B 10 49 02 02 04 05 06 07 FF",
                "48 6B 10 49 02 03 08 09 0A 0B FF",
            ];
            let correct_data: Vec<u8> = [vec![0x49, 0x02], (0..12).collect()].concat();

            // in-order
            let r = p.call(&test_case.map(String::from));
            assert_eq!(r.len(), 1);
            check_message(&r[0], test_case.len(), 0x10, &correct_data);

            // test a few out-of-order cases
            let mut shuffled = test_case.map(String::from);
            for _ in 0..4 {
                shuffled.rotate_left(1);
                let r = p.call(&shuffled);
                assert_eq!(r.len(), 1);
                check_message(&r[0], test_case.len(), 0x10, &correct_data);
            }
        }
    }

    #[test]
    fn multi_line_missing_frames() {
        // missing frames in a multi-frame message should drop the message
        for p in LEGACY {
            let p = Protocol::new(Box::new(p), &[]);

            let test_case = [
                "48 6B 10 49 02 01 00 01 02 03 FF",
                "48 6B 10 49 02 02 04 05 06 07 FF",
                "48 6B 10 49 02 03 08 09 0A 0B FF",
            ];

            for n in 0..test_case.len() - 1 {
                let mut sub_test: Vec<String> = test_case.map(String::from).to_vec();
                sub_test.remove(n);
                let r = p.call(&sub_test);
                assert_eq!(r.len(), 0);
            }
        }
    }

    #[test]
    fn multi_line_mode_03() {
        // mode 03: an extra byte is fudged in to make the output look like CAN
        for p in LEGACY {
            let p = Protocol::new(Box::new(p), &[]);

            let test_case = [
                "48 6B 10 43 00 01 02 03 04 05 FF",
                "48 6B 10 43 06 07 08 09 0A 0B FF",
            ];
            // data is stitched in order received; 0x00 is an arbitrary value
            let correct_data: Vec<u8> = [vec![0x43, 0x00], (0..12).collect()].concat();

            let r = p.call(&test_case.map(String::from));
            assert_eq!(r.len(), 1);
            check_message(&r[0], test_case.len(), 0x10, &correct_data);
        }
    }

    #[test]
    fn elm_names_and_ids() {
        assert_eq!(LegacyProtocol::SaeJ1850Pwm.elm_name(), "SAE J1850 PWM");
        assert_eq!(LegacyProtocol::SaeJ1850Pwm.elm_id(), "1");
        assert_eq!(
            LegacyProtocol::Iso142304Fast.elm_name(),
            "ISO 14230-4 (KWP FAST)"
        );
        assert_eq!(LegacyProtocol::Iso142304Fast.elm_id(), "5");
    }
}
