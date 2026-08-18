//! Basic data models shared by all protocols: frames and messages.
//!
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::BTreeMap;

use crate::util::{BitArray, is_hex};

/// Values for the ECU headers.
pub mod ecu_header {
    /// The engine ECU header used by OBD commands.
    pub const ENGINE: &str = "7E0";
}

/// Constant flags used for marking and filtering messages.
pub mod ecu {
    /// Accept messages from any ECU.
    pub const ALL: u8 = 0b1111_1111;
    /// Ignore unknown ECUs, since this lib probably can't handle them.
    pub const ALL_KNOWN: u8 = 0b1111_1110;
    /// Unknowns get their own bit, since they need to be accepted by ALL.
    pub const UNKNOWN: u8 = 0b0000_0001;
    pub const ENGINE: u8 = 0b0000_0010;
    pub const TRANSMISSION: u8 = 0b0000_0100;
}

/// CAN PCI frame types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameType {
    /// Single frame.
    Sf,
    /// First frame of a multi-frame message.
    Ff,
    /// Consecutive frame of a multi-frame message.
    Cf,
}

/// Represents a single parsed line of OBD output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub raw: String,
    pub data: Vec<u8>,
    pub priority: Option<u8>,
    pub addr_mode: Option<u8>,
    pub rx_id: Option<u32>,
    pub tx_id: Option<u32>,
    pub frame_type: Option<FrameType>,
    /// Only used when `frame_type == Cf`.
    pub seq_index: u8,
    pub data_len: Option<usize>,
}

impl Frame {
    pub fn new(raw: impl Into<String>) -> Self {
        Frame {
            raw: raw.into(),
            data: Vec::new(),
            priority: None,
            addr_mode: None,
            rx_id: None,
            tx_id: None,
            frame_type: None,
            seq_index: 0,
            data_len: None,
        }
    }
}

/// Represents a fully parsed OBD message of one or more frames.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub frames: Vec<Frame>,
    pub ecu: u8,
    pub data: Vec<u8>,
}

impl Message {
    pub fn new(frames: Vec<Frame>) -> Self {
        Message {
            frames,
            ecu: ecu::UNKNOWN,
            data: Vec::new(),
        }
    }

    /// The tx_id of the first frame, if any.
    pub fn tx_id(&self) -> Option<u32> {
        self.frames.first().and_then(|f| f.tx_id)
    }

    /// Hex string of the message data.
    pub fn hex(&self) -> String {
        crate::util::bytes_to_hex(&self.data)
    }

    /// The original raw input lines from the adapter.
    pub fn raw(&self) -> String {
        self.frames
            .iter()
            .map(|f| f.raw.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Whether this message was successfully parsed.
    pub fn parsed(&self) -> bool {
        !self.data.is_empty()
    }
}

/// Stateless parsing logic for a protocol.
///
/// The stateful part (the ECU map) lives in [`Protocol`].
pub trait ProtocolParser {
    /// The ELM's name for this protocol (e.g. "SAE J1939 (CAN 29/250)").
    fn elm_name(&self) -> &'static str;
    /// The ELM's ID for this protocol (e.g. "A").
    fn elm_id(&self) -> &'static str;
    /// The TX_ID of the engine ECU, if known.
    fn tx_id_engine(&self) -> Option<u32>;
    /// The TX_ID of the transmission ECU, if known.
    fn tx_id_transmission(&self) -> Option<u32>;

    /// Parses a single raw line into a `Frame`.
    ///
    /// Returns `false` to drop the frame on fatal errors.
    fn parse_frame(&self, frame: &mut Frame) -> bool;

    /// Assembles frames into a whole `Message`.
    ///
    /// Returns `false` to drop the message on fatal errors.
    fn parse_message(&self, message: &mut Message) -> bool;
}

/// A protocol object: a parser plus the ECU tagging map.
///
/// Initialized by passing the response to an "0100" command.
pub struct Protocol {
    parser: Box<dyn ProtocolParser>,
    ecu_map: BTreeMap<u32, u8>,
}

impl Protocol {
    /// Constructs a protocol object from a parser and the raw lines
    /// returned by the car for the "0100" command.
    pub fn new(parser: Box<dyn ProtocolParser>, lines_0100: &[String]) -> Self {
        let mut protocol = Protocol {
            parser,
            ecu_map: BTreeMap::new(),
        };

        // create the default, empty map
        if let Some(tx_id) = protocol.parser.tx_id_engine() {
            protocol.ecu_map.insert(tx_id, ecu::ENGINE);
        }
        if let Some(tx_id) = protocol.parser.tx_id_transmission() {
            protocol.ecu_map.insert(tx_id, ecu::TRANSMISSION);
        }

        // parse the 0100 data into messages and assemble the map
        let messages = protocol.call(lines_0100);
        protocol.populate_ecu_map(&messages);

        protocol
    }

    pub fn ecu_map(&self) -> &BTreeMap<u32, u8> {
        &self.ecu_map
    }

    pub fn elm_name(&self) -> &'static str {
        self.parser.elm_name()
    }

    pub fn elm_id(&self) -> &'static str {
        self.parser.elm_id()
    }

    /// Parses a list of raw lines from the car into messages.
    pub fn call(&self, lines: &[String]) -> Vec<Message> {
        // preprocess: sort hex (OBD) lines from non-hex (ELM) lines
        let mut obd_lines = Vec::new();
        let mut non_obd_lines = Vec::new();
        for line in lines {
            let scrubbed: String = line.chars().filter(|c| *c != ' ').collect();
            if is_hex(&scrubbed) {
                obd_lines.push(scrubbed);
            } else {
                non_obd_lines.push(line.clone());
            }
        }

        // parse each frame (each line)
        let mut frames = Vec::new();
        for line in obd_lines {
            let mut frame = Frame::new(line);
            if self.parser.parse_frame(&mut frame) {
                frames.push(frame);
            }
        }

        // group frames by transmitting ECU
        let mut frames_by_ecu: BTreeMap<Option<u32>, Vec<Frame>> = BTreeMap::new();
        for frame in frames {
            frames_by_ecu.entry(frame.tx_id).or_default().push(frame);
        }

        // parse frames into whole messages
        let mut messages = Vec::new();
        for (tx_id, ecu_frames) in frames_by_ecu {
            let mut message = Message::new(ecu_frames);
            if self.parser.parse_message(&mut message) {
                message.ecu = tx_id
                    .and_then(|id| self.ecu_map.get(&id).copied())
                    .unwrap_or(ecu::UNKNOWN);
                messages.push(message);
            }
        }

        // handle invalid lines (probably from the ELM)
        for line in non_obd_lines {
            messages.push(Message::new(vec![Frame::new(line)]));
        }

        messages
    }

    /// Given a list of messages from different ECUs (in response to the
    /// 0100 PID listing command), associate each tx_id to an ECU ID
    /// constant. This is mostly concerned with finding the engine.
    fn populate_ecu_map(&mut self, messages: &[Message]) {
        // filter out messages that don't contain any data
        // this will prevent ELM responses from being mapped to ECUs
        let messages: Vec<&Message> = messages.iter().filter(|m| m.parsed()).collect();

        if messages.is_empty() {
            // pass
        } else if messages.len() == 1 {
            // if there's only one response, mark it as the engine regardless
            if let Some(tx_id) = messages[0].tx_id() {
                self.ecu_map.insert(tx_id, ecu::ENGINE);
            }
        } else {
            // the engine is important; if we can't find it, use a fallback
            let mut found_engine = false;

            // if any tx_ids are exact matches to the expected values, record them
            for m in &messages {
                let Some(tx_id) = m.tx_id() else {
                    tracing::debug!("parse_frame failed to extract TX_ID");
                    continue;
                };

                if Some(tx_id) == self.parser.tx_id_engine() {
                    self.ecu_map.insert(tx_id, ecu::ENGINE);
                    found_engine = true;
                } else if Some(tx_id) == self.parser.tx_id_transmission() {
                    self.ecu_map.insert(tx_id, ecu::TRANSMISSION);
                }
            }

            if !found_engine {
                // last resort: choose the ECU with the most bits set
                // (most PIDs supported) to be the engine
                let mut best = 0;
                let mut tx_id = None;
                for m in &messages {
                    let bits = BitArray::new(&m.data).num_set();
                    if bits > best {
                        best = bits;
                        tx_id = m.tx_id();
                    }
                }
                if let Some(tx) = tx_id {
                    self.ecu_map.insert(tx, ecu::ENGINE);
                }
            }

            // any remaining tx_ids are unknown
            for m in &messages {
                if let Some(tx_id) = m.tx_id() {
                    self.ecu_map.entry(tx_id).or_insert(ecu::UNKNOWN);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ecu_constants_do_not_overlap() {
        // make sure none of the ECU ID values overlap (excludes ALL)
        let ecus = [ecu::UNKNOWN, ecu::ENGINE, ecu::TRANSMISSION];
        for (i, e) in ecus.iter().enumerate() {
            assert!(ecu::ALL & e > 0, "ECU: {e} is not included in ECU.ALL");
            for other in &ecus[..i] {
                assert_eq!(e & other, 0, "ECU: {e} has a conflicting bit");
            }
        }
    }

    #[test]
    fn frame_constructor_defaults() {
        let frame = Frame::new("asdf");
        assert_eq!(frame.raw, "asdf");
        assert_eq!(frame.priority, None);
        assert_eq!(frame.addr_mode, None);
        assert_eq!(frame.rx_id, None);
        assert_eq!(frame.tx_id, None);
        assert_eq!(frame.frame_type, None);
        assert_eq!(frame.seq_index, 0);
        assert_eq!(frame.data_len, None);
    }

    #[test]
    fn message_constructor() {
        let mut frame = Frame::new("raw input from OBD tool");
        frame.tx_id = Some(42);
        let frames = vec![frame];

        let message = Message::new(frames.clone());
        assert_eq!(message.frames, frames);
        assert_eq!(message.ecu, ecu::UNKNOWN);
        assert_eq!(message.tx_id(), Some(42));

        // if no frames are given, then we can't report a tx_id
        assert_eq!(Message::new(vec![]).tx_id(), None);
    }

    #[test]
    fn message_hex() {
        let mut message = Message::new(vec![]);
        message.data = vec![0x00, 0x01, 0x02];
        assert_eq!(message.hex(), "000102");
        assert_eq!(u8::from_str_radix(&message.hex()[0..2], 16).unwrap(), 0x00);
        assert_eq!(u8::from_str_radix(&message.hex()[2..4], 16).unwrap(), 0x01);
        assert_eq!(u8::from_str_radix(&message.hex()[4..6], 16).unwrap(), 0x02);
        assert_eq!(u64::from_str_radix(&message.hex(), 16).unwrap(), 0x000102);
    }

    #[test]
    fn message_raw_and_parsed() {
        let mut message = Message::new(vec![Frame::new("line1"), Frame::new("line2")]);
        assert_eq!(message.raw(), "line1\nline2");
        assert!(!message.parsed());
        message.data = vec![0x41];
        assert!(message.parsed());
    }
}
