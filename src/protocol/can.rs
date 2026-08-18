//! CAN protocol parser (ISO 15765-4, SAE J1939).
//!
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::message::{Frame, FrameType, Message, ProtocolParser};
use crate::protocol::decode_hex;
use crate::util::contiguous;

/// CAN protocol variants.
#[derive(Debug, Clone, Copy)]
pub enum CanProtocol {
    Iso15765411bit500k,
    Iso15765429bit500k,
    Iso15765411bit250k,
    Iso15765429bit250k,
    SaeJ1939,
}

impl CanProtocol {
    fn id_bits(self) -> u8 {
        match self {
            CanProtocol::Iso15765411bit500k | CanProtocol::Iso15765411bit250k => 11,
            CanProtocol::Iso15765429bit500k
            | CanProtocol::Iso15765429bit250k
            | CanProtocol::SaeJ1939 => 29,
        }
    }
}

impl ProtocolParser for CanProtocol {
    fn elm_name(&self) -> &'static str {
        match self {
            CanProtocol::Iso15765411bit500k => "ISO 15765-4 (CAN 11/500)",
            CanProtocol::Iso15765429bit500k => "ISO 15765-4 (CAN 29/500)",
            CanProtocol::Iso15765411bit250k => "ISO 15765-4 (CAN 11/250)",
            CanProtocol::Iso15765429bit250k => "ISO 15765-4 (CAN 29/250)",
            CanProtocol::SaeJ1939 => "SAE J1939 (CAN 29/250)",
        }
    }

    fn elm_id(&self) -> &'static str {
        match self {
            CanProtocol::Iso15765411bit500k => "6",
            CanProtocol::Iso15765429bit500k => "7",
            CanProtocol::Iso15765411bit250k => "8",
            CanProtocol::Iso15765429bit250k => "9",
            CanProtocol::SaeJ1939 => "A",
        }
    }

    fn tx_id_engine(&self) -> Option<u32> {
        Some(0)
    }

    fn tx_id_transmission(&self) -> Option<u32> {
        Some(1)
    }

    fn parse_frame(&self, frame: &mut Frame) -> bool {
        // pad 11-bit CAN headers out to 32 bits for consistency,
        // since ELM already does this for 29-bit CAN headers
        let mut raw = frame.raw.clone();
        if self.id_bits() == 11 {
            raw = format!("00000{raw}");
        }

        // handle odd size frames and drop
        if raw.len() & 1 == 1 {
            tracing::debug!("Dropping frame for being odd");
            return false;
        }

        let Some(raw_bytes) = decode_hex(&raw) else {
            return false;
        };

        // check for valid size: at least a PCI byte and one following byte
        // (for FF frames with 12-bit length codes, or 1 byte of data)
        if raw_bytes.len() < 6 {
            tracing::debug!("Dropped frame for being too short");
            return false;
        }
        if raw_bytes.len() > 12 {
            tracing::debug!("Dropped frame for being too long");
            return false;
        }

        // read header information
        if self.id_bits() == 11 {
            // 00 00 07 E8 06 41 00 BE 7F B8 13
            frame.priority = Some(raw_bytes[2] & 0x0F); // always 7
            frame.addr_mode = Some(raw_bytes[3] & 0xF0); // 0xD0 functional, 0xE0 physical

            if frame.addr_mode == Some(0xD0) {
                // untested: 11-bit functional request from tester
                frame.rx_id = Some(u32::from(raw_bytes[3] & 0x0F)); // usually 0x0F for broadcast
                frame.tx_id = Some(0xF1); // made-up to mimic all other protocols
            } else if raw_bytes[3] & 0x08 != 0 {
                frame.rx_id = Some(0xF1); // made-up to mimic all other protocols
                frame.tx_id = Some(u32::from(raw_bytes[3] & 0x07));
            } else {
                // untested: 11-bit message header from tester
                frame.tx_id = Some(0xF1); // made-up to mimic all other protocols
                frame.rx_id = Some(u32::from(raw_bytes[3] & 0x07));
            }
        } else {
            // 29-bit: 18 DA 33 F1 ...
            frame.priority = Some(raw_bytes[0]); // usually 0x18
            frame.addr_mode = Some(raw_bytes[1]); // DB functional, DA physical
            frame.rx_id = Some(u32::from(raw_bytes[2])); // 0x33 broadcast
            frame.tx_id = Some(u32::from(raw_bytes[3])); // 0xF1 tester ID
        }

        // extract the frame data
        frame.data = raw_bytes[4..].to_vec();

        // read PCI byte (always first byte in the data section)
        let pci = frame.data[0];
        frame.frame_type = match pci & 0xF0 {
            0x00 => Some(FrameType::Sf),
            0x10 => Some(FrameType::Ff),
            0x20 => Some(FrameType::Cf),
            _ => {
                tracing::debug!("Dropping frame carrying unknown PCI frame type");
                return false;
            }
        };

        match frame.frame_type {
            Some(FrameType::Sf) => {
                // single frames have 4 bit length codes
                frame.data_len = Some(usize::from(pci & 0x0F));
                // drop frames with no data
                if frame.data_len == Some(0) {
                    return false;
                }
            }
            Some(FrameType::Ff) => {
                // first frames have 12 bit length codes
                frame.data_len = Some((usize::from(pci & 0x0F) << 8) + usize::from(frame.data[1]));
                // drop frames with no data
                if frame.data_len == Some(0) {
                    return false;
                }
            }
            Some(FrameType::Cf) => {
                // consecutive frames have 4 bit sequence indices
                frame.seq_index = pci & 0x0F;
            }
            None => unreachable!(),
        }

        true
    }

    fn parse_message(&self, message: &mut Message) -> bool {
        if message.frames.len() == 1 {
            let frame = &message.frames[0];

            if frame.frame_type != Some(FrameType::Sf) {
                tracing::debug!("Recieved lone frame not marked as single frame");
                return false;
            }

            // extract data, ignore PCI byte and anything after the marked length
            let data_len = frame.data_len.unwrap_or(0);
            message.data = frame.data[1..1 + data_len].to_vec();
        } else {
            // sort FF and CF into their own lists
            let mut ff: Vec<usize> = Vec::new();
            let mut cf: Vec<usize> = Vec::new();

            for (i, f) in message.frames.iter().enumerate() {
                match f.frame_type {
                    Some(FrameType::Ff) => ff.push(i),
                    Some(FrameType::Cf) => cf.push(i),
                    _ => {
                        tracing::debug!(
                            "Dropping frame in multi-frame response not marked as FF or CF"
                        )
                    }
                }
            }

            // check that we captured only one first-frame
            if ff.len() > 1 {
                tracing::debug!("Recieved multiple frames marked FF");
                return false;
            } else if ff.is_empty() {
                tracing::debug!("Never received frame marked FF");
                return false;
            }

            // check that there was at least one consecutive-frame
            if cf.is_empty() {
                tracing::debug!("Never received frame marked CF");
                return false;
            }

            // calculate proper sequence indices from the lower 4 bits given
            for i in 1..cf.len() {
                let prev = message.frames[cf[i - 1]].seq_index;
                let curr = message.frames[cf[i]].seq_index;
                // 1) take the high order bits from the last_sn and low order bits from the frame
                let mut seq = (prev & !0x0F) + curr;
                // 2) if this is more than 7 frames away, we probably just wrapped
                if (seq as i16) < (prev as i16) - 7 {
                    seq += 0x10;
                }
                message.frames[cf[i]].seq_index = seq;
            }

            // sort the sequence indices
            cf.sort_by_key(|&i| message.frames[i].seq_index);

            // check contiguity, and that we aren't missing any frames
            let indices: Vec<i64> = cf
                .iter()
                .map(|&i| i64::from(message.frames[i].seq_index))
                .collect();
            if !contiguous(&indices, 1, cf.len() as i64) {
                tracing::debug!("Recieved multiline response with missing frames");
                return false;
            }

            // on the first frame, skip PCI byte AND length code
            let ff_idx = ff[0];
            message.data = message.frames[ff_idx].data[2..].to_vec();

            // now that they're in order, load/accumulate the data from each CF frame
            for &i in &cf {
                message.data.extend_from_slice(&message.frames[i].data[1..]);
            }

            // chop to the correct size (as specified in the first frame)
            let data_len = message.frames[ff_idx].data_len.unwrap_or(0);
            message.data.truncate(data_len);
        }

        // trim DTC requests based on DTC count
        // this ISN'T in the decoder because the legacy protocols
        // don't provide a DTC_count byte, and instead insert a 0x00
        if message.data.first() == Some(&0x43) {
            let num_dtc_bytes = usize::from(message.data[1]) * 2; // each DTC is 2 bytes
            message.data.truncate(num_dtc_bytes + 2); // +2 for mode/DTC_count bytes
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::Protocol;

    const CAN_11: [CanProtocol; 2] = [
        CanProtocol::Iso15765411bit500k,
        CanProtocol::Iso15765411bit250k,
    ];
    const CAN_29: [CanProtocol; 3] = [
        CanProtocol::Iso15765429bit500k,
        CanProtocol::Iso15765429bit250k,
        CanProtocol::SaeJ1939,
    ];

    fn check_message(m: &Message, num_frames: usize, tx_id: u32, data: &[u8]) {
        assert_eq!(m.frames.len(), num_frames);
        assert_eq!(m.tx_id(), Some(tx_id));
        assert_eq!(m.data, data);
    }

    #[test]
    fn single_frame() {
        for p in CAN_11 {
            let p = Protocol::new(Box::new(p), &[]);

            let r = p.call(&["7E8 06 41 00 00 01 02 03".to_string()]);
            assert_eq!(r.len(), 1);
            check_message(&r[0], 1, 0x0, &[0x41, 0x00, 0x00, 0x01, 0x02, 0x03]);

            // minimum valid length
            let r = p.call(&["7E8 01 41".to_string()]);
            assert_eq!(r.len(), 1);
            check_message(&r[0], 1, 0x0, &[0x41]);

            // maximum valid length
            let r = p.call(&["7E8 07 41 00 00 01 02 03 04".to_string()]);
            assert_eq!(r.len(), 1);
            check_message(&r[0], 1, 0x0, &[0x41, 0x00, 0x00, 0x01, 0x02, 0x03, 0x04]);

            // too short
            let r = p.call(&["7E8 01".to_string()]);
            assert_eq!(r.len(), 0);

            // too long
            let r = p.call(&["7E8 08 41 00 00 01 02 03 04 05".to_string()]);
            assert_eq!(r.len(), 0);

            // drop frames with zero data
            let r = p.call(&["7E8 00".to_string()]);
            assert_eq!(r.len(), 0);

            // drop odd-sized frames (post padding)
            let r = p.call(&["7E8 08 41 00 00 01 02 03 04 0".to_string()]);
            assert_eq!(r.len(), 0);
        }
    }

    #[test]
    fn hex_straining() {
        // non-hex values should be marked as ECU.UNKNOWN
        for p in CAN_11 {
            let p = Protocol::new(Box::new(p), &[]);

            // single non-hex message
            let r = p.call(&["12.8 Volts".to_string()]);
            assert_eq!(r.len(), 1);
            assert_eq!(r[0].ecu, crate::message::ecu::UNKNOWN);
            assert_eq!(r[0].frames.len(), 1);

            // multiple non-hex messages
            let r = p.call(&["12.8 Volts".to_string(), "NO DATA".to_string()]);
            assert_eq!(r.len(), 2);
            for m in &r {
                assert_eq!(m.ecu, crate::message::ecu::UNKNOWN);
                assert_eq!(m.frames.len(), 1);
            }

            // mixed hex and non-hex
            let r = p.call(&[
                "NO DATA".to_string(),
                "7E8 06 41 00 00 01 02 03".to_string(),
            ]);
            assert_eq!(r.len(), 2);

            // first message should be the valid, parsable hex message
            check_message(&r[0], 1, 0x0, &[0x41, 0x00, 0x00, 0x01, 0x02, 0x03]);

            // second message: invalid, non-parsable non-hex
            assert_eq!(r[1].ecu, crate::message::ecu::UNKNOWN);
            assert_eq!(r[1].frames.len(), 1);
            assert_eq!(r[1].data.len(), 0); // no data
        }
    }

    #[test]
    fn multi_ecu() {
        for p in CAN_11 {
            let p = Protocol::new(Box::new(p), &[]);

            let test_case = [
                "7E8 06 41 00 00 01 02 03",
                "7EB 06 41 00 00 01 02 03",
                "7EA 06 41 00 00 01 02 03",
            ];
            let correct_data = [0x41, 0x00, 0x00, 0x01, 0x02, 0x03];

            // separate ECUs, single frames each
            let r = p.call(&test_case.map(String::from));
            assert_eq!(r.len(), 3);

            // messages are returned in ECU order
            check_message(&r[0], 1, 0x0, &correct_data);
            check_message(&r[1], 1, 0x2, &correct_data);
            check_message(&r[2], 1, 0x3, &correct_data);
        }
    }

    #[test]
    fn multi_line() {
        // valid multiline messages are recombined into single messages
        for p in CAN_11 {
            let p = Protocol::new(Box::new(p), &[]);

            let test_case = [
                "7E8 10 20 49 04 00 01 02 03",
                "7E8 21 04 05 06 07 08 09 0A",
                "7E8 22 0B 0C 0D 0E 0F 10 11",
                "7E8 23 12 13 14 15 16 17 18",
            ];
            let correct_data: Vec<u8> = [vec![0x49, 0x04], (0..25).collect()].concat();

            // in-order
            let r = p.call(&test_case.map(String::from));
            assert_eq!(r.len(), 1);
            check_message(&r[0], test_case.len(), 0x0, &correct_data);

            // test a few out-of-order cases
            let mut shuffled = test_case.map(String::from);
            for _ in 0..4 {
                // rotate deterministically instead of random.shuffle
                shuffled.rotate_left(1);
                let r = p.call(&shuffled);
                assert_eq!(r.len(), 1);
                check_message(&r[0], test_case.len(), 0x0, &correct_data);
            }
        }
    }

    #[test]
    fn multi_line_missing_frames() {
        // missing frames in a multi-frame message should drop the message
        for p in CAN_11 {
            let p = Protocol::new(Box::new(p), &[]);

            let test_case = [
                "7E8 10 20 49 04 00 01 02 03",
                "7E8 21 04 05 06 07 08 09 0A",
                "7E8 22 0B 0C 0D 0E 0F 10 11",
                "7E8 23 12 13 14 15 16 17 18",
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
        // mode 03 commands have a DTC count byte accounted for in the protocol layer
        for p in CAN_11 {
            let p = Protocol::new(Box::new(p), &[]);

            let test_case = ["7E8 10 20 43 04 00 01 02 03", "7E8 21 04 05 06 07 08 09 0A"];
            let correct_data: Vec<u8> = [vec![0x43, 0x04], (0..8).collect()].concat();

            let r = p.call(&test_case.map(String::from));
            assert_eq!(r.len(), 1);
            check_message(&r[0], test_case.len(), 0, &correct_data);
        }
    }

    #[test]
    fn can_29_basic() {
        // 29-bit CAN: 18 DA 33 F1 06 41 00 00 01 02 03
        for p in CAN_29 {
            let p = Protocol::new(Box::new(p), &[]);
            let r = p.call(&["18 DA 33 F1 06 41 00 00 01 02 03".to_string()]);
            assert_eq!(r.len(), 1);
            check_message(&r[0], 1, 0xF1, &[0x41, 0x00, 0x00, 0x01, 0x02, 0x03]);
        }
    }

    #[test]
    fn elm_names_and_ids() {
        assert_eq!(
            CanProtocol::Iso15765411bit500k.elm_name(),
            "ISO 15765-4 (CAN 11/500)"
        );
        assert_eq!(CanProtocol::Iso15765411bit500k.elm_id(), "6");
        assert_eq!(CanProtocol::SaeJ1939.elm_name(), "SAE J1939 (CAN 29/250)");
        assert_eq!(CanProtocol::SaeJ1939.elm_id(), "A");
    }
}
