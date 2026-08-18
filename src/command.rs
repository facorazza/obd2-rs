//! OBDCommand: a single OBD query definition, and OBDResponse: the
//! result of running one.
//!
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::decoder::{self, Value};
use crate::message::Message;
use crate::util::is_hex;

/// Decoder functions used by commands, mirroring `OBDCommand.decode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decoder {
    Drop,
    Noop,
    Pid,
    RawString,
    Count,
    Percent,
    PercentCentered,
    Temp,
    CurrentCentered,
    SensorVoltage,
    SensorVoltageBig,
    FuelPressure,
    Pressure,
    EvapPressure,
    AbsEvapPressure,
    EvapPressureAlt,
    TimingAdvance,
    InjectTiming,
    MaxMaf,
    FuelRate,
    O2Sensors,
    AuxInputStatus,
    O2SensorsAlt,
    AbsoluteLoad,
    ElmVoltage,
    Status,
    FuelStatus,
    AirStatus,
    ObdCompliance,
    FuelType,
    SingleDtc,
    Dtc,
    Monitor,
    Cvn,
    /// A unit-and-scaling lookup by table id (`uas(0x1B)`).
    Uas(u8),
    /// An encoded string of fixed length (`encoded_string(17)`).
    EncodedString(usize),
}

impl Decoder {
    /// Runs the decoder over the given messages.
    pub fn decode(&self, messages: &[Message]) -> Value {
        match self {
            Decoder::Drop => decoder::drop(messages),
            Decoder::Noop => decoder::noop(messages),
            Decoder::Pid => decoder::pid(messages),
            Decoder::RawString => decoder::raw_string(messages),
            Decoder::Count => decoder::count(messages),
            Decoder::Percent => decoder::percent(messages),
            Decoder::PercentCentered => decoder::percent_centered(messages),
            Decoder::Temp => decoder::temp(messages),
            Decoder::CurrentCentered => decoder::current_centered(messages),
            Decoder::SensorVoltage => decoder::sensor_voltage(messages),
            Decoder::SensorVoltageBig => decoder::sensor_voltage_big(messages),
            Decoder::FuelPressure => decoder::fuel_pressure(messages),
            Decoder::Pressure => decoder::pressure(messages),
            Decoder::EvapPressure => decoder::evap_pressure(messages),
            Decoder::AbsEvapPressure => decoder::abs_evap_pressure(messages),
            Decoder::EvapPressureAlt => decoder::evap_pressure_alt(messages),
            Decoder::TimingAdvance => decoder::timing_advance(messages),
            Decoder::InjectTiming => decoder::inject_timing(messages),
            Decoder::MaxMaf => decoder::max_maf(messages),
            Decoder::FuelRate => decoder::fuel_rate(messages),
            Decoder::O2Sensors => decoder::o2_sensors(messages),
            Decoder::AuxInputStatus => decoder::aux_input_status(messages),
            Decoder::O2SensorsAlt => decoder::o2_sensors_alt(messages),
            Decoder::AbsoluteLoad => decoder::absolute_load(messages),
            Decoder::ElmVoltage => decoder::elm_voltage(messages),
            Decoder::Status => decoder::status(messages),
            Decoder::FuelStatus => decoder::fuel_status(messages),
            Decoder::AirStatus => decoder::air_status(messages),
            Decoder::ObdCompliance => decoder::obd_compliance(messages),
            Decoder::FuelType => decoder::fuel_type(messages),
            Decoder::SingleDtc => decoder::single_dtc(messages),
            Decoder::Dtc => decoder::dtc(messages),
            Decoder::Monitor => decoder::monitor(messages),
            Decoder::Cvn => decoder::cvn(messages),
            Decoder::Uas(id) => decoder::decode_uas(messages, *id),
            Decoder::EncodedString(len) => decoder::decode_encoded_string(messages, *len),
        }
    }
}

/// A single OBD command definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OBDCommand {
    // NOTE: `Hash` is implemented manually below (header + command only).
    /// Human readable name (also used as key in the commands dict).
    pub name: &'static str,
    /// Human readable description.
    pub desc: &'static str,
    /// The command bytes (e.g. `b"0100"`).
    pub command: &'static [u8],
    /// Number of bytes expected in the return.
    pub bytes: u32,
    /// The decoding function.
    pub decoder: Decoder,
    /// ECU bitmask from which this command expects messages.
    pub ecu: u8,
    /// Can an extra digit be appended to make the ELM return early?
    pub fast: bool,
    /// ECU header used for the queries (e.g. `b"7E0"`).
    pub header: &'static [u8],
}

impl OBDCommand {
    /// The OBD mode (first two hex digits), or `None` for non-hex
    /// commands such as `ATI`.
    pub fn mode(&self) -> Option<u8> {
        if self.command.len() >= 2 && is_hex(self.ascii()) {
            hex_pair(self.command[0], self.command[1])
        } else {
            None
        }
    }

    /// The PID (hex digits after the mode), or `None` for non-hex
    /// commands such as `ATI`.
    pub fn pid(&self) -> Option<u16> {
        if self.command.len() > 2 && is_hex(self.ascii()) {
            parse_hex(&self.command[2..])
        } else {
            None
        }
    }

    /// Runs the command over the given messages: filters by ECU,
    /// constrains the data size, and decodes.
    pub fn call(&self, messages: &[Message]) -> OBDResponse<'_> {
        let filtered: Vec<Message> = messages
            .iter()
            .filter(|m| (self.ecu & m.ecu) > 0)
            .map(|m| self.constrain_message_data(m))
            .collect();

        let value = if filtered.is_empty() {
            None
        } else {
            Some(self.decoder.decode(&filtered))
        };

        OBDResponse {
            command: self,
            messages: filtered,
            value,
        }
    }

    /// Pads or chops the message data to the size specified by this
    /// command, mirroring `OBDCommand.__constrain_message_data`.
    fn constrain_message_data(&self, message: &Message) -> Message {
        let mut data = message.data.clone();
        let len = data.len() as u32;
        if self.bytes > 0 {
            match len.cmp(&self.bytes) {
                std::cmp::Ordering::Greater => data.truncate(self.bytes as usize),
                std::cmp::Ordering::Less => data.resize(self.bytes as usize, 0),
                std::cmp::Ordering::Equal => {}
            }
        }
        Message {
            frames: message.frames.clone(),
            ecu: message.ecu,
            data,
        }
    }

    /// The command bytes as an ASCII string (for hex checks).
    fn ascii(&self) -> &str {
        std::str::from_utf8(self.command).unwrap_or("")
    }
}

/// Hashing covers header + command, so commands can be used as
/// set/dict keys.
impl std::hash::Hash for OBDCommand {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.header.hash(state);
        self.command.hash(state);
    }
}

/// Standard response object for any OBDCommand.
#[derive(Debug, Clone)]
pub struct OBDResponse<'a> {
    /// The command that produced this response.
    pub command: &'a OBDCommand,
    /// The messages that were decoded (already ECU-filtered and
    /// size-constrained).
    pub messages: Vec<Message>,
    /// The decoded value, or `None` when no messages were received.
    pub value: Option<Value>,
}

impl<'a> OBDResponse<'a> {
    /// An empty response, as returned when a query cannot be sent.
    pub fn null(command: &'a OBDCommand) -> Self {
        OBDResponse {
            command,
            messages: Vec::new(),
            value: None,
        }
    }

    /// True when no messages were received or the value is null.
    pub fn is_null(&self) -> bool {
        self.messages.is_empty() || self.value.is_none()
    }
}

/// Parses two ASCII hex digits into a byte.
fn hex_pair(hi: u8, lo: u8) -> Option<u8> {
    let hi = (hi as char).to_digit(16)? as u8;
    let lo = (lo as char).to_digit(16)? as u8;
    Some(hi * 16 + lo)
}

/// Parses ASCII hex digits into an integer.
fn parse_hex(bytes: &[u8]) -> Option<u16> {
    let s = std::str::from_utf8(bytes).ok()?;
    u16::from_str_radix(s, 16).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::ecu;

    #[test]
    fn mode_and_pid() {
        let cmd = OBDCommand {
            name: "TEST",
            desc: "test",
            command: b"0100",
            bytes: 6,
            decoder: Decoder::Pid,
            ecu: ecu::ENGINE,
            fast: true,
            header: b"7E0",
        };
        assert_eq!(cmd.mode(), Some(1));
        assert_eq!(cmd.pid(), Some(0));

        let cmd6 = OBDCommand {
            command: b"0601",
            ..cmd.clone()
        };
        assert_eq!(cmd6.mode(), Some(6));
        assert_eq!(cmd6.pid(), Some(1));

        let at = OBDCommand {
            command: b"ATI",
            ..cmd.clone()
        };
        assert_eq!(at.mode(), None);
        assert_eq!(at.pid(), None);
    }

    #[test]
    fn constrain_pads_and_chops() {
        let cmd = OBDCommand {
            name: "TEST",
            desc: "test",
            command: b"0100",
            bytes: 4,
            decoder: Decoder::Pid,
            ecu: ecu::ALL,
            fast: true,
            header: b"7E0",
        };
        let short = Message {
            frames: vec![],
            ecu: ecu::ENGINE,
            data: vec![0x41, 0x00],
        };
        let long = Message {
            frames: vec![],
            ecu: ecu::ENGINE,
            data: vec![0x41, 0x00, 0x01, 0x02, 0x03, 0x04],
        };
        let r = cmd.call(&[short, long]);
        assert_eq!(r.messages.len(), 2);
        assert_eq!(r.messages[0].data, vec![0x41, 0x00, 0x00, 0x00]);
        assert_eq!(r.messages[1].data, vec![0x41, 0x00, 0x01, 0x02]);
        assert!(r.value.is_some());
    }

    #[test]
    fn call_filters_by_ecu() {
        let cmd = OBDCommand {
            name: "TEST",
            desc: "test",
            command: b"0100",
            bytes: 0,
            decoder: Decoder::Pid,
            ecu: ecu::ENGINE,
            fast: true,
            header: b"7E0",
        };
        let other = Message {
            frames: vec![],
            ecu: ecu::TRANSMISSION,
            data: vec![0x41, 0x00],
        };
        let r = cmd.call(&[other]);
        assert!(r.messages.is_empty());
        assert!(r.is_null());
    }
}
