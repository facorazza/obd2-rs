//! Utility helpers: byte conversion, bit arrays and serial port scan.
//!
// SPDX-License-Identifier: GPL-3.0-or-later

use std::fmt;
use std::ops::Range;

/// Connection status flags.
///
/// The variant order is meaningful: `CarConnected >= ElmConnected`
/// etc. hold (a connected ELM implies an open port, etc.).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum OBDStatus {
    NotConnected,
    ElmConnected,
    ObdConnected,
    CarConnected,
}

impl OBDStatus {
    /// The string value of this status.
    pub fn as_str(self) -> &'static str {
        match self {
            OBDStatus::NotConnected => "Not Connected",
            OBDStatus::ElmConnected => "ELM Connected",
            OBDStatus::ObdConnected => "OBD Connected",
            OBDStatus::CarConnected => "Car Connected",
        }
    }
}

impl fmt::Display for OBDStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Converts a big-endian byte slice into a single integer.
pub fn bytes_to_int(bs: &[u8]) -> u64 {
    bs.iter().fold(0u64, |v, b| (v << 8) | u64::from(*b))
}

/// Converts a byte slice into a lowercase hex string.
pub fn bytes_to_hex(bs: &[u8]) -> String {
    let mut s = String::with_capacity(bs.len() * 2);
    for b in bs {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// Computes the two's complement of `val` interpreted as a `num_bits`-wide
/// signed integer.
pub fn twos_comp(val: i64, num_bits: u32) -> i64 {
    if (val & (1 << (num_bits - 1))) != 0 {
        val - (1 << num_bits)
    } else {
        val
    }
}

/// Returns true when every character of `s` is a hex digit.
///
/// Note: an empty string is considered hex.
pub fn is_hex(s: &str) -> bool {
    s.chars().all(|c| c.is_ascii_hexdigit())
}

/// Checks that a list of integers are consecutive, starting at `start` and
/// ending at `end`.
pub fn contiguous(l: &[i64], start: i64, end: i64) -> bool {
    if l.is_empty() {
        return false;
    }
    if l[0] != start {
        return false;
    }
    if l[l.len() - 1] != end {
        return false;
    }
    l.windows(2).all(|w| w[0] + 1 == w[1])
}

/// A (deliberately inefficient) bit array.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BitArray {
    bits: Vec<bool>,
}

impl BitArray {
    pub fn new(bytes: &[u8]) -> Self {
        let mut bits = Vec::with_capacity(bytes.len() * 8);
        for b in bytes {
            for i in (0..8).rev() {
                bits.push((b >> i) & 1 == 1);
            }
        }
        Self { bits }
    }

    /// Returns the bit at `index`, or `false` when out of range.
    pub fn get(&self, index: usize) -> bool {
        self.bits.get(index).copied().unwrap_or(false)
    }

    /// Returns the bits in `range` (clamped).
    pub fn slice(&self, range: Range<usize>) -> Vec<bool> {
        self.bits.get(range).unwrap_or(&[]).to_vec()
    }

    pub fn num_set(&self) -> usize {
        self.bits.iter().filter(|b| **b).count()
    }

    pub fn num_cleared(&self) -> usize {
        self.bits.len() - self.num_set()
    }

    /// Interprets `bits[start..stop]` as a big-endian binary number.
    pub fn value(&self, start: usize, stop: usize) -> u64 {
        if start >= self.bits.len() {
            return 0;
        }
        let stop = stop.min(self.bits.len());
        if start >= stop {
            return 0;
        }
        self.bits[start..stop]
            .iter()
            .fold(0u64, |v, b| (v << 1) | u64::from(*b))
    }

    pub fn len(&self) -> usize {
        self.bits.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bits.is_empty()
    }
}

impl fmt::Display for BitArray {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for b in &self.bits {
            f.write_str(if *b { "1" } else { "0" })?;
        }
        Ok(())
    }
}

impl IntoIterator for BitArray {
    type Item = bool;
    type IntoIter = std::vec::IntoIter<bool>;

    fn into_iter(self) -> Self::IntoIter {
        self.bits.into_iter()
    }
}

/// Returns whether a serial port can be opened.
pub fn try_port(port_name: &str) -> bool {
    // the port closes on drop
    serialport::new(port_name, 9600).open().is_ok()
}

/// Scans for available serial ports, returning a list of port names.
///
/// Uses the `serialport` crate's cross-platform enumeration, then
/// verifies each candidate can actually be opened.
pub fn scan_serial() -> Vec<String> {
    let mut available = Vec::new();
    if let Ok(ports) = serialport::available_ports() {
        for port in ports {
            if try_port(&port.port_name) {
                available.push(port.port_name);
            }
        }
    }
    available
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_to_int_big_endian() {
        assert_eq!(bytes_to_int(&[]), 0);
        assert_eq!(bytes_to_int(&[0x00]), 0);
        assert_eq!(bytes_to_int(&[0x01]), 1);
        assert_eq!(bytes_to_int(&[0x00, 0x01]), 1);
        assert_eq!(bytes_to_int(&[0x01, 0x00]), 256);
        assert_eq!(bytes_to_int(&[0xFF, 0xFF]), 65535);
        assert_eq!(bytes_to_int(&[0x01, 0x02, 0x03, 0x04]), 0x01020304);
    }

    #[test]
    fn bytes_to_hex_lowercase() {
        assert_eq!(bytes_to_hex(&[]), "");
        assert_eq!(bytes_to_hex(&[0x00, 0x01, 0xAB, 0xFF]), "0001abff");
    }

    #[test]
    fn twos_comp_positive() {
        assert_eq!(twos_comp(0, 8), 0);
        assert_eq!(twos_comp(127, 8), 127);
        assert_eq!(twos_comp(0x7FFF, 16), 32767);
    }

    #[test]
    fn twos_comp_negative() {
        assert_eq!(twos_comp(0xFF, 8), -1);
        assert_eq!(twos_comp(0x80, 8), -128);
        assert_eq!(twos_comp(0xFFFF, 16), -1);
        assert_eq!(twos_comp(0x8000, 16), -32768);
    }

    #[test]
    fn is_hex_accepts_digits_and_letters() {
        assert!(is_hex(""));
        assert!(is_hex("0123456789abcdefABCDEF"));
        assert!(is_hex("7E8"));
        assert!(!is_hex("12.8 Volts"));
        assert!(!is_hex("NO DATA"));
        assert!(!is_hex("7E8 06"));
    }

    #[test]
    fn contiguous_checks_consecutive() {
        assert!(!contiguous(&[], 1, 0));
        assert!(!contiguous(&[1, 2], 1, 3));
        assert!(!contiguous(&[2, 3], 1, 2));
        assert!(contiguous(&[1], 1, 1));
        assert!(contiguous(&[1, 2, 3], 1, 3));
        assert!(!contiguous(&[1, 3, 4], 1, 4));
    }

    #[test]
    fn bit_array_basic() {
        let ba = BitArray::new(&[0b1010_0000]);
        assert_eq!(ba.len(), 8);
        assert!(ba.get(0));
        assert!(!ba.get(1));
        assert!(ba.get(2));
        assert!(!ba.get(7));
        // out of range reads are false
        assert!(!ba.get(8));
        assert!(!ba.get(100));
        assert_eq!(ba.num_set(), 2);
        assert_eq!(ba.num_cleared(), 6);
        assert_eq!(ba.to_string(), "10100000");
    }

    #[test]
    fn bit_array_value() {
        let ba = BitArray::new(&[0b0000_1111, 0b0000_0001]);
        assert_eq!(ba.value(0, 4), 0b0000);
        assert_eq!(ba.value(4, 8), 0b1111);
        assert_eq!(ba.value(12, 16), 0b0001);
        assert_eq!(ba.value(0, 16), 0x0F01);
        // empty / out-of-range slices return 0
        assert_eq!(ba.value(16, 20), 0);
        assert_eq!(ba.value(8, 8), 0);
    }

    #[test]
    fn bit_array_slice_and_iter() {
        let ba = BitArray::new(&[0b1010_0000]);
        assert_eq!(ba.slice(0..2), vec![true, false]);
        assert_eq!(ba.slice(8..16), Vec::<bool>::new());
        let collected: Vec<bool> = ba.clone().into_iter().collect();
        assert_eq!(
            collected,
            vec![true, false, true, false, false, false, false, false]
        );
    }

    #[test]
    fn obd_status_strings() {
        assert_eq!(OBDStatus::NotConnected.as_str(), "Not Connected");
        assert_eq!(OBDStatus::ElmConnected.as_str(), "ELM Connected");
        assert_eq!(OBDStatus::ObdConnected.as_str(), "OBD Connected");
        assert_eq!(OBDStatus::CarConnected.as_str(), "Car Connected");
    }
}
