//! Value decoders.
//!
//! Every decoder takes a slice of [`Message`]s and returns a [`Value`].
//! The `Decoder` enum in [`crate::command`] dispatches to these
//! functions.
//!
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::codes;
use crate::message::Message;
use crate::units::{self, UasValue, Unit};
use crate::util::{BitArray, bytes_to_hex, bytes_to_int, twos_comp};
use std::collections::BTreeMap;

/// A decoded OBD response value.
///
/// The possible decoded values: scalars, bit arrays, structured
/// objects, string lookups, etc.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// No value (the `drop` decoder, or a failed decode).
    None,
    /// Raw data bytes (`noop`).
    Bytes(Vec<u8>),
    /// A bit array (`pid`).
    BitArray(BitArray),
    /// Raw ELM response lines joined with `\n` (`raw_string`).
    RawString(String),
    /// A measured quantity with unit.
    Quantity(f64, Unit),
    /// The diagnostic status object (Mode 01 PID 01).
    Status(Status),
    /// A single string lookup (air status, OBD compliance, fuel type).
    String(Option<String>),
    /// Fuel system status: `(system_1, system_2)`.
    FuelStatus(Option<(String, String)>),
    /// A single DTC: `(code, description)`.
    Dtc(Option<Dtc>),
    /// A list of DTCs.
    Dtcs(Vec<Dtc>),
    /// Mode 06 monitor test results.
    Monitor(Monitor),
    /// A boolean flag (auxiliary input status).
    Bool(bool),
    /// Oxygen sensor presence by bank (bank 0 is always empty).
    Banks(Vec<Vec<bool>>),
    /// An encoded string (VIN, calibration ID) as raw bytes.
    EncodedBytes(Option<Vec<u8>>),
    /// Calibration verification number as a hex string.
    Cvn(Option<String>),
}

/// A DTC code with its description (empty when unknown).
pub type Dtc = (String, String);

/// Diagnostic status (Mode 01 PID 01), mirroring `OBDResponse.Status`.
#[derive(Debug, Clone, PartialEq)]
pub struct Status {
    pub mil: bool,
    pub dtc_count: u8,
    pub ignition_type: &'static str,
    /// Per-test readiness, keyed by test name.
    pub tests: BTreeMap<&'static str, StatusTest>,
}

/// Readiness of a single diagnostic test.
#[derive(Debug, Clone, PartialEq)]
pub struct StatusTest {
    pub name: String,
    pub available: bool,
    pub complete: bool,
}

impl StatusTest {
    pub fn new(name: &str, available: bool, complete: bool) -> Self {
        StatusTest {
            name: name.to_string(),
            available,
            complete,
        }
    }
}

impl std::fmt::Display for StatusTest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let a = if self.available {
            "Available"
        } else {
            "Unavailable"
        };
        let c = if self.complete {
            "Complete"
        } else {
            "Incomplete"
        };
        write!(f, "Test {}: {}, {}", self.name, a, c)
    }
}

/// Mode 06 monitor results, mirroring `OBDResponse.Monitor`.
#[derive(Debug, Clone, PartialEq)]
pub struct Monitor {
    /// Tests by TID, including null placeholders for the standard IDs.
    tests: BTreeMap<u8, MonitorTest>,
}

impl Monitor {
    pub fn new() -> Self {
        let mut tests = BTreeMap::new();
        // pre-populate with null tests so lookups of standard TIDs
        // never fail
        for (tid, _, _) in codes::TEST_IDS {
            tests.insert(tid, MonitorTest::null());
        }
        Monitor { tests }
    }

    pub fn add_test(&mut self, test: MonitorTest) {
        if let Some(tid) = test.tid {
            self.tests.insert(tid, test);
        }
    }

    /// The tests that have real data.
    pub fn tests(&self) -> Vec<&MonitorTest> {
        self.tests.values().filter(|t| !t.is_null()).collect()
    }

    pub fn len(&self) -> usize {
        self.tests().len()
    }

    pub fn is_empty(&self) -> bool {
        self.tests().is_empty()
    }

    /// Looks up a test by TID; returns a null test when unknown.
    pub fn get(&self, tid: u8) -> &MonitorTest {
        self.tests.get(&tid).unwrap_or(&MonitorTest::NULL)
    }

    /// Looks up a test by property name (e.g. `RTL_THRESHOLD_VOLTAGE`).
    pub fn get_by_name(&self, name: &str) -> Option<&MonitorTest> {
        self.tests
            .values()
            .find(|t| t.name.as_deref() == Some(name))
    }
}

impl Default for Monitor {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for Monitor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let tests = self.tests();
        if tests.is_empty() {
            write!(f, "No tests to report")
        } else {
            let lines: Vec<String> = tests.iter().map(|t| t.to_string()).collect();
            write!(f, "{}", lines.join("\n"))
        }
    }
}

/// A single Mode 06 test result.
#[derive(Debug, Clone, PartialEq)]
pub struct MonitorTest {
    pub tid: Option<u8>,
    pub name: Option<String>,
    pub desc: Option<String>,
    pub value: Option<f64>,
    pub min: Option<f64>,
    pub max: Option<f64>,
}

impl MonitorTest {
    /// A null test: no data yet.
    pub fn null() -> Self {
        MonitorTest {
            tid: None,
            name: None,
            desc: None,
            value: None,
            min: None,
            max: None,
        }
    }

    pub const NULL: MonitorTest = MonitorTest {
        tid: None,
        name: None,
        desc: None,
        value: None,
        min: None,
        max: None,
    };

    pub fn is_null(&self) -> bool {
        self.tid.is_none() || self.value.is_none() || self.min.is_none() || self.max.is_none()
    }

    pub fn passed(&self) -> bool {
        if self.is_null() {
            false
        } else {
            let (v, lo, hi) = (self.value.unwrap(), self.min.unwrap(), self.max.unwrap());
            (v >= lo) && (v <= hi)
        }
    }
}

impl std::fmt::Display for MonitorTest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let desc = self.desc.as_deref().unwrap_or("Unknown");
        let value = self
            .value
            .map_or_else(|| "None".to_string(), |v| v.to_string());
        let verdict = if self.passed() { "PASSED" } else { "FAILED" };
        write!(f, "{} : {} [{}]", desc, value, verdict)
    }
}

// Simple decoders

/// Drops all messages, returning `None`.
pub fn drop(_messages: &[Message]) -> Value {
    Value::None
}

/// Data in, data out: the raw message data bytes.
pub fn noop(messages: &[Message]) -> Value {
    Value::Bytes(messages[0].data.clone())
}

/// The message data as a bit array (chops mode and PID bytes).
pub fn pid(messages: &[Message]) -> Value {
    Value::BitArray(BitArray::new(&messages[0].data[2..]))
}

/// The raw ELM response lines, one per message.
pub fn raw_string(messages: &[Message]) -> Value {
    let lines: Vec<String> = messages.iter().map(|m| m.raw()).collect();
    Value::RawString(lines.join("\n"))
}

/// Applies the SAE J1979 scaling conversion to the message data.
pub fn decode_uas(messages: &[Message], id: u8) -> Value {
    let d = &messages[0].data[2..]; // chop mode and PID bytes
    match units::uas(id).map(|u| u.apply(d)) {
        Some(UasValue::Quantity { value, unit }) => Value::Quantity(value, unit),
        Some(UasValue::Bool(b)) => Value::Bool(b),
        None => Value::None,
    }
}

// Sensor decoders (scaled quantities)

/// A dimensionless count.
pub fn count(messages: &[Message]) -> Value {
    let d = &messages[0].data[2..];
    Value::Quantity(bytes_to_int(d) as f64, Unit::Count)
}

/// 0 to 100 %.
pub fn percent(messages: &[Message]) -> Value {
    let d = &messages[0].data[2..];
    Value::Quantity(d[0] as f64 * 100.0 / 255.0, Unit::Percent)
}

/// -100 to 100 %.
pub fn percent_centered(messages: &[Message]) -> Value {
    let d = &messages[0].data[2..];
    Value::Quantity((d[0] as f64 - 128.0) * 100.0 / 128.0, Unit::Percent)
}

/// -40 to 215 C.
pub fn temp(messages: &[Message]) -> Value {
    let d = &messages[0].data[2..];
    Value::Quantity(bytes_to_int(d) as f64 - 40.0, Unit::Celsius)
}

/// -128 to 128 mA.
pub fn current_centered(messages: &[Message]) -> Value {
    let d = &messages[0].data[2..];
    let v = bytes_to_int(&d[2..4]) as f64 / 256.0 - 128.0;
    Value::Quantity(v, Unit::Milliampere)
}

/// 0 to 1.275 volts.
pub fn sensor_voltage(messages: &[Message]) -> Value {
    let d = &messages[0].data[2..];
    Value::Quantity(d[0] as f64 / 200.0, Unit::Volt)
}

/// 0 to 8 volts.
pub fn sensor_voltage_big(messages: &[Message]) -> Value {
    let d = &messages[0].data[2..];
    let v = bytes_to_int(&d[2..4]) as f64 * 8.0 / 65535.0;
    Value::Quantity(v, Unit::Volt)
}

/// 0 to 765 kPa.
pub fn fuel_pressure(messages: &[Message]) -> Value {
    let d = &messages[0].data[2..];
    Value::Quantity(d[0] as f64 * 3.0, Unit::Kilopascal)
}

/// 0 to 255 kPa.
pub fn pressure(messages: &[Message]) -> Value {
    let d = &messages[0].data[2..];
    Value::Quantity(d[0] as f64, Unit::Kilopascal)
}

/// -8192 to 8192 Pa (two's-complement bytes).
pub fn evap_pressure(messages: &[Message]) -> Value {
    let d = &messages[0].data[2..];
    let a = twos_comp(d[0] as i64, 8) as f64;
    let b = twos_comp(d[1] as i64, 8) as f64;
    Value::Quantity((a * 256.0 + b) / 4.0, Unit::Pascal)
}

/// 0 to 327.675 kPa.
pub fn abs_evap_pressure(messages: &[Message]) -> Value {
    let d = &messages[0].data[2..];
    Value::Quantity(bytes_to_int(d) as f64 / 200.0, Unit::Kilopascal)
}

/// -32767 to 32768 Pa.
pub fn evap_pressure_alt(messages: &[Message]) -> Value {
    let d = &messages[0].data[2..];
    Value::Quantity(bytes_to_int(d) as f64 - 32767.0, Unit::Pascal)
}

/// -64 to 63.5 degrees.
pub fn timing_advance(messages: &[Message]) -> Value {
    let d = &messages[0].data[2..];
    Value::Quantity((d[0] as f64 - 128.0) / 2.0, Unit::Degree)
}

/// -210 to 301 degrees.
pub fn inject_timing(messages: &[Message]) -> Value {
    let d = &messages[0].data[2..];
    Value::Quantity((bytes_to_int(d) as f64 - 26880.0) / 128.0, Unit::Degree)
}

/// 0 to 2550 grams/sec.
pub fn max_maf(messages: &[Message]) -> Value {
    let d = &messages[0].data[2..];
    Value::Quantity(d[0] as f64 * 10.0, Unit::GramsPerSecond)
}

/// 0 to 3212 Liters/hour.
pub fn fuel_rate(messages: &[Message]) -> Value {
    let d = &messages[0].data[2..];
    Value::Quantity(bytes_to_int(d) as f64 * 0.05, Unit::LitersPerHour)
}

/// 0 to 25700 %.
pub fn absolute_load(messages: &[Message]) -> Value {
    let d = &messages[0].data[2..];
    Value::Quantity(bytes_to_int(d) as f64 * 100.0 / 255.0, Unit::Percent)
}

/// Special bit encoding for PID 13: sensor presence by bank.
pub fn o2_sensors(messages: &[Message]) -> Value {
    let d = &messages[0].data[2..];
    let bits = BitArray::new(d);
    Value::Banks(vec![
        Vec::new(),       // bank 0 is invalid
        bits.slice(0..4), // bank 1
        bits.slice(4..8), // bank 2
    ])
}

/// Special bit encoding for PID 1D: sensor presence by bank.
pub fn o2_sensors_alt(messages: &[Message]) -> Value {
    let d = &messages[0].data[2..];
    let bits = BitArray::new(d);
    Value::Banks(vec![
        Vec::new(),       // bank 0 is invalid
        bits.slice(0..2), // bank 1
        bits.slice(2..4), // bank 2
        bits.slice(4..6), // bank 3
        bits.slice(6..8), // bank 4
    ])
}

/// First bit indicates PTO status.
pub fn aux_input_status(messages: &[Message]) -> Value {
    let d = &messages[0].data[2..];
    Value::Bool(((d[0] >> 7) & 1) == 1)
}

/// ELM supply voltage, parsed from the raw `AT RV` response.
pub fn elm_voltage(messages: &[Message]) -> Value {
    let raw = &messages[0].frames[0].raw;
    let v = raw.to_lowercase().replace('v', "");
    match v.trim().parse::<f64>() {
        Ok(v) => Value::Quantity(v, Unit::Volt),
        Err(_) => {
            tracing::warn!("Failed to parse ELM voltage");
            Value::None
        }
    }
}

// Special decoders (structured values)

/// Diagnostic status from Mode 01 PID 01.
pub fn status(messages: &[Message]) -> Value {
    let d = &messages[0].data[2..];
    let bits = BitArray::new(d);

    //  ┌MIL      ||||||||┌Misfire supported
    //  |         |||||||||
    //  10000011 00000111 11111111 00000000
    //   [# DTC] X        [supprt] [~ready]
    let mut output = Status {
        mil: bits.get(0),
        dtc_count: bits.value(1, 8) as u8,
        ignition_type: codes::IGNITION_TYPE[usize::from(bits.get(12))],
        tests: BTreeMap::new(),
    };

    // the 3 base tests are always present
    for (i, name) in codes::BASE_TESTS.iter().rev().enumerate() {
        let t = StatusTest::new(name, bits.get(13 + i), !bits.get(9 + i));
        output.tests.insert(name, t);
    }

    // different tests for different ignition types
    let tests: &[Option<&str>] = if bits.get(12) {
        &codes::COMPRESSION_TESTS
    } else {
        &codes::SPARK_TESTS
    };
    for (i, name) in tests.iter().rev().enumerate() {
        if let Some(name) = name {
            let t = StatusTest::new(name, bits.get(16 + i), !bits.get(24 + i));
            output.tests.insert(name, t);
        }
    }

    Value::Status(output)
}

/// Fuel system status (Mode 01 PID 03).
pub fn fuel_status(messages: &[Message]) -> Value {
    let d = &messages[0].data[2..];
    let bits = BitArray::new(d);

    let decode_byte = |bits: &[bool]| -> Option<String> {
        if bits.iter().filter(|b| **b).count() != 1 {
            tracing::debug!("Invalid response for fuel status (multiple/no bits set)");
            return None;
        }
        let idx = 7 - first_true(bits)?;
        match codes::FUEL_STATUS.get(idx) {
            Some(s) => Some(s.to_string()),
            None => {
                tracing::debug!("Invalid response for fuel status (high bits set)");
                None
            }
        }
    };

    let status_1 = decode_byte(&bits.slice(0..8));
    let status_2 = decode_byte(&bits.slice(8..16));

    match (status_1, status_2) {
        (None, None) => Value::FuelStatus(None),
        (s1, s2) => Value::FuelStatus(Some((s1.unwrap_or_default(), s2.unwrap_or_default()))),
    }
}

/// Secondary air status (Mode 01 PID 12).
pub fn air_status(messages: &[Message]) -> Value {
    let d = &messages[0].data[2..];
    let bits = BitArray::new(d);

    let status = if bits.num_set() == 1 {
        match first_true(&bits.slice(0..8)) {
            Some(idx) => codes::AIR_STATUS.get(7 - idx).map(|s| s.to_string()),
            None => None,
        }
    } else {
        tracing::debug!("Invalid response for fuel status (multiple/no bits set)");
        None
    };

    Value::String(status)
}

/// OBD compliance (Mode 01 PID 1C).
pub fn obd_compliance(messages: &[Message]) -> Value {
    let d = &messages[0].data[2..];
    let v = codes::OBD_COMPLIANCE
        .get(d[0] as usize)
        .map(|s| s.to_string());
    if v.is_none() {
        tracing::debug!("Invalid response for OBD compliance (no table entry)");
    }
    Value::String(v)
}

/// Fuel type (Mode 01 PID 51).
pub fn fuel_type(messages: &[Message]) -> Value {
    let d = &messages[0].data[2..];
    let v = codes::FUEL_TYPES.get(d[0] as usize).map(|s| s.to_string());
    if v.is_none() {
        tracing::debug!("Invalid response for fuel type (no table entry)");
    }
    Value::String(v)
}

/// Converts 2 bytes into a DTC code, mirroring `parse_dtc`.
///
/// Returns `None` for invalid codes (also ignores the ELM's padding).
pub fn parse_dtc(bytes: &[u8]) -> Option<Dtc> {
    if bytes.len() != 2 || (bytes[0] == 0 && bytes[1] == 0) {
        return None;
    }

    //  BYTES: (16,      35      )
    //  HEX:    4   1    2   3
    //  BIN:    01000001 00100011
    //          [][][  in hex   ]
    //          | / /
    //  DTC:    C0123
    let family = ["P", "C", "B", "U"][usize::from(bytes[0] >> 6)];
    let sub = (bytes[0] >> 4) & 0b0011;
    let hex = bytes_to_hex(bytes);
    let code = format!("{family}{sub}{}", &hex[1..4]);

    let desc = crate::dtc_table::lookup(&code).unwrap_or("");
    Some((code, desc.to_string()))
}

/// A single DTC from a message.
pub fn single_dtc(messages: &[Message]) -> Value {
    let d = &messages[0].data[2..];
    Value::Dtc(parse_dtc(d))
}

/// Converts a frame of 2-byte DTCs into a list of DTCs.
pub fn dtc(messages: &[Message]) -> Value {
    let mut d: Vec<u8> = Vec::new();
    for message in messages {
        d.extend_from_slice(&message.data[2..]); // remove mode and DTC_count bytes
    }

    let mut codes = Vec::new();
    // look at data in pairs of bytes; loop through ENDING indices to
    // avoid odd (invalid) code lengths
    for n in (1..d.len()).step_by(2) {
        if let Some(dtc) = parse_dtc(&[d[n - 1], d[n]]) {
            codes.push(dtc);
        }
    }
    Value::Dtcs(codes)
}

/// Parses one 9-byte Mode 06 test block into a `MonitorTest`.
pub fn parse_monitor_test(d: &[u8]) -> Option<MonitorTest> {
    let mut test = MonitorTest::null();

    let tid = d[1];
    match codes::test_id(tid) {
        Some((name, desc)) => {
            test.name = Some(name.to_string());
            test.desc = Some(desc.to_string());
        }
        None => {
            tracing::debug!("Encountered unknown Test ID");
            test.name = Some("Unknown".to_string());
            test.desc = Some("Unknown".to_string());
        }
    }

    // if we can't decode the value, abort
    let uas = units::uas(d[2])?;
    let val = |bytes: &[u8]| match uas.apply(bytes) {
        UasValue::Quantity { value, .. } => Some(value),
        UasValue::Bool(b) => Some(f64::from(b)),
    };

    test.tid = Some(tid);
    test.value = val(&d[3..5]);
    test.min = val(&d[5..7]);
    test.max = val(&d[7..9]);

    Some(test)
}

/// Mode 06 monitor results.
pub fn monitor(messages: &[Message]) -> Value {
    let d = &messages[0].data[1..]; // only dispose of the mode byte

    // test that we got the right number of bytes
    let extra_bytes = d.len() % 9;
    let d = if extra_bytes != 0 {
        tracing::debug!("Encountered monitor message with non-multiple of 9 bytes. Truncating...");
        &d[..d.len() - extra_bytes]
    } else {
        d
    };

    let mut mon = Monitor::new();
    // look at data in blocks of 9 bytes (one test result)
    for block in d.chunks_exact(9) {
        if let Some(test) = parse_monitor_test(block) {
            mon.add_test(test);
        }
    }
    Value::Monitor(mon)
}

/// Extracts an encoded string (VIN, calibration ID) from multi-part
/// messages, stripping null padding.
pub fn decode_encoded_string(messages: &[Message], length: usize) -> Value {
    let d = &messages[0].data[2..];

    if d.len() < length {
        tracing::debug!("Invalid string {:?}. Discarding...", d);
        return Value::EncodedBytes(None);
    }

    // encoded strings arrive with leading null values padding the
    // string out to the next full message size; strip them (plus the
    // literal "\xNN" escape sequences)
    const STRIP: [u8; 8] = [0x00, 0x01, 0x02, b'\\', b'x', b'0', b'1', b'2'];
    let mut out = d.to_vec();
    while out
        .first()
        .is_some_and(|b| b.is_ascii_whitespace() || STRIP.contains(b))
    {
        out.remove(0);
    }
    while out
        .last()
        .is_some_and(|b| b.is_ascii_whitespace() || STRIP.contains(b))
    {
        out.pop();
    }

    Value::EncodedBytes(Some(out))
}

/// Calibration verification numbers as a hex string.
pub fn cvn(messages: &[Message]) -> Value {
    match decode_encoded_string(messages, 4) {
        Value::EncodedBytes(Some(d)) => Value::Cvn(Some(bytes_to_hex(&d))),
        _ => Value::Cvn(None),
    }
}

/// Index of the first `true` bit.
fn first_true(bits: &[bool]) -> Option<usize> {
    bits.iter().position(|b| *b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::Frame;
    use crate::units::Unit;

    fn msg(data: &[u8]) -> Message {
        let mut m = Message::new(vec![Frame::new("")]);
        m.data = data.to_vec();
        m
    }

    fn msgs(data: &[u8]) -> Vec<Message> {
        vec![msg(data)]
    }

    fn assert_qty(value: Value, expected: f64, unit: Unit) {
        match value {
            Value::Quantity(v, u) => {
                assert!((v - expected).abs() < 1e-9, "value {v} != {expected}");
                assert_eq!(u, unit);
            }
            other => panic!("expected Quantity, got {other:?}"),
        }
    }

    #[test]
    fn drop_decoder() {
        assert_eq!(drop(&msgs(&[0x41, 0x00])), Value::None);
    }

    #[test]
    fn noop_decoder() {
        assert_eq!(
            noop(&msgs(&[0x41, 0x00, 0xBE])),
            Value::Bytes(vec![0x41, 0x00, 0xBE])
        );
    }

    #[test]
    fn pid_decoder_returns_bits() {
        // data[2:] = 0xBE 0x7F -> 10111110 01111111
        let v = pid(&msgs(&[0x41, 0x00, 0xBE, 0x7F]));
        match v {
            Value::BitArray(bits) => {
                assert_eq!(bits.len(), 16);
                assert!(bits.get(0));
                assert!(bits.get(6));
                assert!(!bits.get(7));
                assert!(!bits.get(8));
            }
            other => panic!("expected BitArray, got {other:?}"),
        }
    }

    #[test]
    fn raw_string_decoder() {
        let mut m1 = Message::new(vec![Frame::new("7E8 06 41 00 BE 7F B8 13")]);
        m1.data = vec![0x41];
        let v = raw_string(&[m1]);
        assert_eq!(v, Value::RawString("7E8 06 41 00 BE 7F B8 13".to_string()));
    }

    #[test]
    fn decode_uas_rpm() {
        // UAS 0x07: RPM = 0.25 * raw
        let v = decode_uas(&msgs(&[0x41, 0x0C, 0x0F, 0xA0]), 0x07);
        assert_qty(v, 1000.0, Unit::Rpm);
    }

    #[test]
    fn count_decoder() {
        assert_qty(count(&msgs(&[0x41, 0x00, 0x03])), 3.0, Unit::Count);
    }

    #[test]
    fn percent_decoder() {
        // 51 = 81/255 * 100 = 31.7647%
        assert_qty(
            percent(&msgs(&[0x41, 0x04, 0x51])),
            81.0 * 100.0 / 255.0,
            Unit::Percent,
        );
    }

    #[test]
    fn percent_centered_decoder() {
        // 0x80 -> 0%, 0x00 -> -100%, 0xFF -> 99.2%
        assert_qty(
            percent_centered(&msgs(&[0x41, 0x06, 0x80])),
            0.0,
            Unit::Percent,
        );
        assert_qty(
            percent_centered(&msgs(&[0x41, 0x06, 0x00])),
            -100.0,
            Unit::Percent,
        );
    }

    #[test]
    fn temp_decoder() {
        assert_qty(temp(&msgs(&[0x41, 0x05, 0x46])), 30.0, Unit::Celsius);
    }

    #[test]
    fn current_centered_decoder() {
        // 4 data bytes; d[2..4] = 0x8000 -> 128 - 128 = 0 mA
        assert_qty(
            current_centered(&msgs(&[0x41, 0x34, 0x00, 0x00, 0x80, 0x00])),
            0.0,
            Unit::Milliampere,
        );
    }

    #[test]
    fn sensor_voltage_decoder() {
        assert_qty(sensor_voltage(&msgs(&[0x41, 0x14, 0x80])), 0.64, Unit::Volt);
    }

    #[test]
    fn fuel_pressure_decoder() {
        assert_qty(
            fuel_pressure(&msgs(&[0x41, 0x0A, 0x10])),
            48.0,
            Unit::Kilopascal,
        );
    }

    #[test]
    fn pressure_decoder() {
        assert_qty(pressure(&msgs(&[0x41, 0x0B, 0x5A])), 90.0, Unit::Kilopascal);
    }

    #[test]
    fn evap_pressure_decoder() {
        // bytes 0x06 0x01: (6*256 + 1)/4 = 384.25 Pa
        assert_qty(
            evap_pressure(&msgs(&[0x41, 0x2C, 0x06, 0x01])),
            384.25,
            Unit::Pascal,
        );
    }

    #[test]
    fn timing_advance_decoder() {
        assert_qty(
            timing_advance(&msgs(&[0x41, 0x0E, 0x80])),
            0.0,
            Unit::Degree,
        );
        assert_qty(
            timing_advance(&msgs(&[0x41, 0x0E, 0x64])),
            -14.0,
            Unit::Degree,
        );
    }

    #[test]
    fn max_maf_decoder() {
        assert_qty(
            max_maf(&msgs(&[0x41, 0x10, 0x32])),
            500.0,
            Unit::GramsPerSecond,
        );
    }

    #[test]
    fn fuel_rate_decoder() {
        // 0x03E8 = 1000 * 0.05 = 50 L/h
        assert_qty(
            fuel_rate(&msgs(&[0x41, 0x5E, 0x03, 0xE8])),
            50.0,
            Unit::LitersPerHour,
        );
    }

    #[test]
    fn absolute_load_decoder() {
        assert_qty(
            absolute_load(&msgs(&[0x41, 0x43, 0x00, 0xFF])),
            100.0,
            Unit::Percent,
        );
    }

    #[test]
    fn o2_sensors_banks() {
        // 0xFF: all sensors present in banks 1 and 2
        let v = o2_sensors(&msgs(&[0x41, 0x0D, 0xFF]));
        assert_eq!(v, Value::Banks(vec![vec![], vec![true; 4], vec![true; 4]]));
    }

    #[test]
    fn aux_input_status_pto() {
        // bit 7 set -> PTO active
        assert_eq!(
            aux_input_status(&msgs(&[0x41, 0x1E, 0x80])),
            Value::Bool(true)
        );
        assert_eq!(
            aux_input_status(&msgs(&[0x41, 0x1E, 0x7F])),
            Value::Bool(false)
        );
    }

    #[test]
    fn elm_voltage_parses() {
        let m = Message::new(vec![Frame::new("12.5V")]);
        let v = elm_voltage(&[m]);
        assert_qty(v, 12.5, Unit::Volt);
    }

    #[test]
    fn elm_voltage_bad_input() {
        let m = Message::new(vec![Frame::new("junk")]);
        assert_eq!(elm_voltage(&[m]), Value::None);
    }

    #[test]
    fn status_decoder_full() {
        // MIL on, 3 DTCs, spark ignition, all tests available & complete
        let v = status(&msgs(&[0x41, 0x01, 0x83, 0x07, 0xFF, 0x00]));
        match v {
            Value::Status(s) => {
                assert!(s.mil);
                assert_eq!(s.dtc_count, 3);
                assert_eq!(s.ignition_type, "spark");
                assert_eq!(
                    s.tests["MISFIRE_MONITORING"],
                    StatusTest::new("MISFIRE_MONITORING", true, true)
                );
                assert_eq!(
                    s.tests["CATALYST_MONITORING"],
                    StatusTest::new("CATALYST_MONITORING", true, true)
                );
            }
            other => panic!("expected Status, got {other:?}"),
        }
    }

    #[test]
    fn status_decoder_compression() {
        // bit 12 set (0x08 in the second byte) -> compression ignition
        let v = status(&msgs(&[0x41, 0x01, 0x00, 0x08, 0xFF, 0x00]));
        match v {
            Value::Status(s) => {
                assert_eq!(s.ignition_type, "compression");
                assert!(s.tests.contains_key("NMHC_CATALYST_MONITORING"));
                assert!(!s.tests.contains_key("CATALYST_MONITORING"));
            }
            other => panic!("expected Status, got {other:?}"),
        }
    }

    #[test]
    fn fuel_status_decoder() {
        // 0x01 = open loop (insufficient engine temperature), 0x00 = invalid
        let v = fuel_status(&msgs(&[0x41, 0x03, 0x01, 0x00]));
        assert_eq!(
            v,
            Value::FuelStatus(Some((
                "Open loop due to insufficient engine temperature".to_string(),
                String::new(),
            )))
        );
    }

    #[test]
    fn fuel_status_decoder_invalid() {
        // multiple bits set in first byte -> invalid
        let v = fuel_status(&msgs(&[0x41, 0x03, 0x03, 0x00]));
        assert_eq!(v, Value::FuelStatus(None));
    }

    #[test]
    fn air_status_decoder() {
        // 0x01 -> bit 7 set -> index 0 -> "Upstream"
        let v = air_status(&msgs(&[0x41, 0x12, 0x01]));
        assert_eq!(v, Value::String(Some("Upstream".to_string())));
    }

    #[test]
    fn obd_compliance_decoder() {
        let v = obd_compliance(&msgs(&[0x41, 0x1C, 0x01]));
        assert_eq!(
            v,
            Value::String(Some("OBD-II as defined by the CARB".to_string()))
        );
    }

    #[test]
    fn fuel_type_decoder() {
        let v = fuel_type(&msgs(&[0x41, 0x51, 0x01]));
        assert_eq!(v, Value::String(Some("Gasoline".to_string())));
    }

    #[test]
    fn parse_dtc_codes() {
        // P0101
        assert_eq!(parse_dtc(&[0x01, 0x01]).unwrap().0, "P0101");
        // C0123 (bytes 0x41 0x23)
        assert_eq!(parse_dtc(&[0x41, 0x23]).unwrap().0, "C0123");
        // B0134
        assert_eq!(parse_dtc(&[0x81, 0x34]).unwrap().0, "B0134");
        // U0001
        assert_eq!(parse_dtc(&[0xC0, 0x01]).unwrap().0, "U0001");
    }

    #[test]
    fn parse_dtc_invalid() {
        assert_eq!(parse_dtc(&[0x00, 0x00]), None);
        assert_eq!(parse_dtc(&[0x01]), None);
        assert_eq!(parse_dtc(&[0x01, 0x01, 0x02]), None);
    }

    #[test]
    fn parse_dtc_known_description() {
        // P0101 exists in the DTC table
        let (code, desc) = parse_dtc(&[0x01, 0x01]).unwrap();
        assert_eq!(code, "P0101");
        assert!(!desc.is_empty(), "expected a description for P0101");
    }

    #[test]
    fn single_dtc_decoder() {
        // P0101 with its table description
        let v = single_dtc(&msgs(&[0x41, 0x02, 0x01, 0x01]));
        match v {
            Value::Dtc(Some((code, desc))) => {
                assert_eq!(code, "P0101");
                assert_eq!(
                    desc,
                    "Mass or Volume Air Flow Sensor A Circuit Range/Performance"
                );
            }
            other => panic!("expected Dtc, got {other:?}"),
        }
    }

    #[test]
    fn dtc_decoder_multiple() {
        // two messages, each with 2 DTCs
        let mut m1 = msg(&[0x43, 0x02, 0x01, 0x01, 0x01, 0x02]);
        let mut m2 = msg(&[0x43, 0x01, 0x02, 0x03]);
        let mut v = dtc(&[m1.clone(), m2.clone()]);
        match &mut v {
            Value::Dtcs(codes) => {
                assert_eq!(codes.len(), 3);
                assert_eq!(codes[0].0, "P0101");
                assert_eq!(codes[1].0, "P0102");
                assert_eq!(codes[2].0, "P0203");
            }
            other => panic!("expected Dtcs, got {other:?}"),
        }
        let _ = (&mut m1, &mut m2);
    }

    #[test]
    fn dtc_decoder_stops_at_padding() {
        // trailing 00 00 pairs are ignored
        let v = dtc(&msgs(&[0x43, 0x02, 0x01, 0x01, 0x00, 0x00]));
        match v {
            Value::Dtcs(codes) => assert_eq!(codes.len(), 1),
            other => panic!("expected Dtcs, got {other:?}"),
        }
    }

    #[test]
    fn monitor_decoder() {
        // one 9-byte block: 0x01(MID) 0x01(TID) 0x84(UAS) 02 02 00 00 00 00
        // UAS 0x84: signed 0.001x -> value 0.514, min 0, max 0
        let v = monitor(&msgs(&[
            0x46, 0x01, 0x01, 0x84, 0x02, 0x02, 0x00, 0x00, 0x00, 0x00,
        ]));
        match v {
            Value::Monitor(mon) => {
                let t = mon.get(0x01);
                assert_eq!(t.name.as_deref(), Some("RTL_THRESHOLD_VOLTAGE"));
                assert_eq!(
                    t.desc.as_deref(),
                    Some("Rich to lean sensor threshold voltage")
                );
                assert!(!t.is_null());
            }
            other => panic!("expected Monitor, got {other:?}"),
        }
    }

    #[test]
    fn monitor_decoder_truncates_bad_length() {
        // 10 bytes -> one extra byte truncated
        let v = monitor(&msgs(&[
            0x46, 0x01, 0x01, 0x84, 0x02, 0x02, 0x00, 0x00, 0x00, 0x00, 0xFF,
        ]));
        match v {
            Value::Monitor(mon) => assert_eq!(mon.len(), 1),
            other => panic!("expected Monitor, got {other:?}"),
        }
    }

    #[test]
    fn monitor_null_test_lookup() {
        let mon = Monitor::new();
        assert!(mon.get(0x01).is_null());
        assert!(mon.get(0xFF).is_null());
    }

    #[test]
    fn monitor_test_passed() {
        let t = MonitorTest {
            tid: Some(1),
            name: None,
            desc: Some("test".to_string()),
            value: Some(5.0),
            min: Some(0.0),
            max: Some(10.0),
        };
        assert!(t.passed());
        let t2 = MonitorTest {
            value: Some(11.0),
            ..t.clone()
        };
        assert!(!t2.passed());
    }

    #[test]
    fn encoded_string_vins() {
        // (starts with 'W' to avoid the strip-set quirk: leading
        // bytes 0x00-0x02 and '0','1','2','x','\\' are stripped from
        // both ends)
        let v = decode_encoded_string(
            &msgs(&[
                0x49, 0x02, 0x57, 0x50, 0x30, 0x5A, 0x5A, 0x5A, 0x39, 0x39, 0x5A, 0x54, 0x53, 0x33,
                0x39, 0x32, 0x31, 0x32, 0x34,
            ]),
            17,
        );
        match v {
            Value::EncodedBytes(Some(b)) => {
                assert_eq!(String::from_utf8_lossy(&b), "WP0ZZZ99ZTS392124");
            }
            other => panic!("expected EncodedBytes, got {other:?}"),
        }
    }

    #[test]
    fn encoded_string_too_short() {
        let v = decode_encoded_string(&msgs(&[0x49, 0x02, 0x31]), 17);
        assert_eq!(v, Value::EncodedBytes(None));
    }

    #[test]
    fn cvn_decoder() {
        // bytes_to_hex produces lowercase hex
        let v = cvn(&msgs(&[0x49, 0x06, 0x00, 0x00, 0xAB, 0xCD, 0x12, 0x34]));
        assert_eq!(v, Value::Cvn(Some("abcd1234".to_string())));
    }
}
