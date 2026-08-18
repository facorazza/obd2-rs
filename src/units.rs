//! Unit and scaling table (UAS), derived from the SAE J1979 scaling
//! definitions for each OBD PID.
//!
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::util::{bytes_to_int, twos_comp};

/// Units that appear in the UAS table and decoders.
///
/// The unit set is a closed enum; `as_str()` returns the unit string
/// used for display and MQTT payloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Count,
    Rpm,
    Kph,
    Millivolt,
    Volt,
    Milliampere,
    Ampere,
    Millisecond,
    Second,
    Milliohm,
    Ohm,
    Kiloohm,
    Celsius,
    Kilopascal,
    Degree,
    Ratio,
    Millihertz,
    Hertz,
    Kilohertz,
    Kilometer,
    MillivoltPerMillisecond,
    GramsPerSecond,
    LitersPerHour,
    PascalPerSecond,
    KilogramPerHour,
    Gram,
    Milligram,
    Percent,
    Liter,
    Inch,
    Minute,
    Microsecond,
    SquareMillimeter,
    Ppm,
    Microampere,
    Pascal,
    MillivoltPerSecond,
}

impl Unit {
    /// The pint-compatible unit string.
    pub fn as_str(self) -> &'static str {
        match self {
            Unit::Count => "count",
            Unit::Rpm => "rpm",
            Unit::Kph => "kph",
            Unit::Millivolt => "millivolt",
            Unit::Volt => "volt",
            Unit::Milliampere => "milliampere",
            Unit::Ampere => "ampere",
            Unit::Millisecond => "millisecond",
            Unit::Second => "second",
            Unit::Milliohm => "milliohm",
            Unit::Ohm => "ohm",
            Unit::Kiloohm => "kiloohm",
            Unit::Celsius => "celsius",
            Unit::Kilopascal => "kilopascal",
            Unit::Degree => "degree",
            Unit::Ratio => "ratio",
            Unit::Millihertz => "millihertz",
            Unit::Hertz => "hertz",
            Unit::Kilohertz => "kilohertz",
            Unit::Kilometer => "kilometer",
            Unit::MillivoltPerMillisecond => "millivolt / millisecond",
            Unit::GramsPerSecond => "grams_per_second",
            Unit::LitersPerHour => "liters_per_hour",
            Unit::PascalPerSecond => "pascal / second",
            Unit::KilogramPerHour => "kilogram / hour",
            Unit::Gram => "gram",
            Unit::Milligram => "milligram",
            Unit::Percent => "percent",
            Unit::Liter => "liter",
            Unit::Inch => "inch",
            Unit::Minute => "minute",
            Unit::Microsecond => "microsecond",
            Unit::SquareMillimeter => "millimeter ** 2",
            Unit::Ppm => "ppm",
            Unit::Microampere => "microampere",
            Unit::Pascal => "pascal",
            Unit::MillivoltPerSecond => "millivolt / second",
        }
    }
}

/// A Unit and Scale conversion, used in decoding Mode 06 monitor responses.
#[derive(Debug, Clone, Copy)]
pub enum Uas {
    Scaled {
        signed: bool,
        scale: f64,
        unit: Unit,
        offset: f64,
    },
    /// UAS ID 0x2E: true when any byte is non-zero.
    Any,
}

impl Uas {
    pub const fn scaled(signed: bool, scale: f64, unit: Unit) -> Self {
        Uas::Scaled {
            signed,
            scale,
            unit,
            offset: 0.0,
        }
    }

    pub const fn scaled_offset(signed: bool, scale: f64, unit: Unit, offset: f64) -> Self {
        Uas::Scaled {
            signed,
            scale,
            unit,
            offset,
        }
    }

    /// Applies the conversion to a byte slice, mirroring `UAS.__call__`.
    pub fn apply(&self, bytes: &[u8]) -> UasValue {
        match *self {
            Uas::Any => UasValue::Bool(bytes.iter().any(|b| *b != 0)),
            Uas::Scaled {
                signed,
                scale,
                unit,
                offset,
            } => {
                let raw = bytes_to_int(bytes) as i64;
                let value = if signed {
                    twos_comp(raw, (bytes.len() * 8) as u32)
                } else {
                    raw
                };
                UasValue::Quantity {
                    value: value as f64 * scale + offset,
                    unit,
                }
            }
        }
    }
}

/// Result of applying a UAS conversion to a byte slice.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UasValue {
    Quantity { value: f64, unit: Unit },
    Bool(bool),
}

/// Looks up a standardized UAS ID, returning `None` for unknown IDs.
///
/// Table derived from the SAE J1979 scaling tables.
pub fn uas(id: u8) -> Option<Uas> {
    use Unit::*;
    Some(match id {
        0x01 => Uas::scaled(false, 1.0, Count),
        0x02 => Uas::scaled(false, 0.1, Count),
        0x03 => Uas::scaled(false, 0.01, Count),
        0x04 => Uas::scaled(false, 0.001, Count),
        0x05 => Uas::scaled(false, 0.0000305, Count),
        0x06 => Uas::scaled(false, 0.000305, Count),
        0x07 => Uas::scaled(false, 0.25, Rpm),
        0x08 => Uas::scaled(false, 0.01, Kph),
        0x09 => Uas::scaled(false, 1.0, Kph),
        0x0A => Uas::scaled(false, 0.122, Millivolt),
        0x0B => Uas::scaled(false, 0.001, Volt),
        0x0C => Uas::scaled(false, 0.01, Volt),
        0x0D => Uas::scaled(false, 0.00390625, Milliampere),
        0x0E => Uas::scaled(false, 0.001, Ampere),
        0x0F => Uas::scaled(false, 0.01, Ampere),
        0x10 => Uas::scaled(false, 1.0, Millisecond),
        0x11 => Uas::scaled(false, 100.0, Millisecond),
        0x12 => Uas::scaled(false, 1.0, Second),
        0x13 => Uas::scaled(false, 1.0, Milliohm),
        0x14 => Uas::scaled(false, 1.0, Ohm),
        0x15 => Uas::scaled(false, 1.0, Kiloohm),
        0x16 => Uas::scaled_offset(false, 0.1, Celsius, -40.0),
        0x17 => Uas::scaled(false, 0.01, Kilopascal),
        0x18 => Uas::scaled(false, 0.0117, Kilopascal),
        0x19 => Uas::scaled(false, 0.079, Kilopascal),
        0x1A => Uas::scaled(false, 1.0, Kilopascal),
        0x1B => Uas::scaled(false, 10.0, Kilopascal),
        0x1C => Uas::scaled(false, 0.01, Degree),
        0x1D => Uas::scaled(false, 0.5, Degree),
        0x1E => Uas::scaled(false, 0.0000305, Ratio),
        0x1F => Uas::scaled(false, 0.05, Ratio),
        0x20 => Uas::scaled(false, 0.00390625, Ratio),
        0x21 => Uas::scaled(false, 1.0, Millihertz),
        0x22 => Uas::scaled(false, 1.0, Hertz),
        0x23 => Uas::scaled(false, 1.0, Kilohertz),
        0x24 => Uas::scaled(false, 1.0, Count),
        0x25 => Uas::scaled(false, 1.0, Kilometer),
        0x26 => Uas::scaled(false, 0.1, MillivoltPerMillisecond),
        0x27 => Uas::scaled(false, 0.01, GramsPerSecond),
        0x28 => Uas::scaled(false, 1.0, GramsPerSecond),
        0x29 => Uas::scaled(false, 0.25, PascalPerSecond),
        0x2A => Uas::scaled(false, 0.001, KilogramPerHour),
        0x2B => Uas::scaled(false, 1.0, Count),
        0x2C => Uas::scaled(false, 0.01, Gram),
        0x2D => Uas::scaled(false, 0.01, Milligram),
        0x2E => Uas::Any,
        0x2F => Uas::scaled(false, 0.01, Percent),
        0x30 => Uas::scaled(false, 0.001526, Percent),
        0x31 => Uas::scaled(false, 0.001, Liter),
        0x32 => Uas::scaled(false, 0.0000305, Inch),
        0x33 => Uas::scaled(false, 0.00024414, Ratio),
        0x34 => Uas::scaled(false, 1.0, Minute),
        0x35 => Uas::scaled(false, 10.0, Millisecond),
        0x36 => Uas::scaled(false, 0.01, Gram),
        0x37 => Uas::scaled(false, 0.1, Gram),
        0x38 => Uas::scaled(false, 1.0, Gram),
        0x39 => Uas::scaled_offset(false, 0.01, Percent, -327.68),
        0x3A => Uas::scaled(false, 0.001, Gram),
        0x3B => Uas::scaled(false, 0.0001, Gram),
        0x3C => Uas::scaled(false, 0.1, Microsecond),
        0x3D => Uas::scaled(false, 0.01, Milliampere),
        0x3E => Uas::scaled(false, 0.00006103516, SquareMillimeter),
        0x3F => Uas::scaled(false, 0.01, Liter),
        0x40 => Uas::scaled(false, 1.0, Ppm),
        0x41 => Uas::scaled(false, 0.01, Microampere),
        // signed
        0x81 => Uas::scaled(true, 1.0, Count),
        0x82 => Uas::scaled(true, 0.1, Count),
        0x83 => Uas::scaled(true, 0.01, Count),
        0x84 => Uas::scaled(true, 0.001, Count),
        0x85 => Uas::scaled(true, 0.0000305, Count),
        0x86 => Uas::scaled(true, 0.000305, Count),
        0x87 => Uas::scaled(true, 1.0, Ppm),
        0x8A => Uas::scaled(true, 0.122, Millivolt),
        0x8B => Uas::scaled(true, 0.001, Volt),
        0x8C => Uas::scaled(true, 0.01, Volt),
        0x8D => Uas::scaled(true, 0.00390625, Milliampere),
        0x8E => Uas::scaled(true, 0.001, Ampere),
        0x90 => Uas::scaled(true, 1.0, Millisecond),
        0x96 => Uas::scaled(true, 0.1, Celsius),
        0x99 => Uas::scaled(true, 0.1, Kilopascal),
        0x9C => Uas::scaled(true, 0.01, Degree),
        0x9D => Uas::scaled(true, 0.5, Degree),
        0xA8 => Uas::scaled(true, 1.0, GramsPerSecond),
        0xA9 => Uas::scaled(true, 0.25, PascalPerSecond),
        0xAD => Uas::scaled(true, 0.01, Milligram),
        0xAE => Uas::scaled(true, 0.1, Milligram),
        0xAF => Uas::scaled(true, 0.01, Percent),
        0xB0 => Uas::scaled(true, 0.003052, Percent),
        0xB1 => Uas::scaled(true, 2.0, MillivoltPerSecond),
        0xFC => Uas::scaled(true, 0.01, Kilopascal),
        0xFD => Uas::scaled(true, 0.001, Kilopascal),
        0xFE => Uas::scaled(true, 0.25, Pascal),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Relative tolerance for float comparisons in tests.
    const TOLERANCE: f64 = 0.025;

    fn b(hex: &str) -> Vec<u8> {
        let hex: String = hex.chars().filter(|c| *c != ' ').collect();
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect()
    }

    fn assert_quantity(id: u8, hex: &str, expected: f64, unit: Unit) {
        let value = uas(id).unwrap().apply(&b(hex));
        match value {
            UasValue::Quantity { value, unit: u } => {
                assert_eq!(u, unit, "UAS 0x{id:02X} unit mismatch");
                assert!(
                    (value - expected).abs() < TOLERANCE,
                    "UAS 0x{id:02X} value mismatch: got {value}, expected {expected}"
                );
            }
            UasValue::Bool(_) => panic!("UAS 0x{id:02X} returned Bool, expected Quantity"),
        }
    }

    #[test]
    fn unsigned_counts() {
        assert_quantity(0x01, "0000", 0.0, Unit::Count);
        assert_quantity(0x01, "0001", 1.0, Unit::Count);
        assert_quantity(0x01, "FFFF", 65535.0, Unit::Count);
        assert_quantity(0x02, "0001", 0.1, Unit::Count);
        assert_quantity(0x02, "FFFF", 6553.5, Unit::Count);
        assert_quantity(0x03, "0001", 0.01, Unit::Count);
        assert_quantity(0x03, "FFFF", 655.35, Unit::Count);
        assert_quantity(0x04, "0001", 0.001, Unit::Count);
        assert_quantity(0x04, "FFFF", 65.535, Unit::Count);
        assert_quantity(0x05, "0001", 0.0000305, Unit::Count);
        assert_quantity(0x05, "FFFF", 1.9999, Unit::Count);
        assert_quantity(0x06, "0001", 0.000305, Unit::Count);
        assert_quantity(0x06, "FFFF", 19.988, Unit::Count);
    }

    #[test]
    fn unsigned_rates_and_units() {
        assert_quantity(0x07, "0001", 0.25, Unit::Rpm);
        assert_quantity(0x07, "FFFF", 16383.75, Unit::Rpm);
        assert_quantity(0x08, "0001", 0.01, Unit::Kph);
        assert_quantity(0x08, "FFFF", 655.35, Unit::Kph);
        assert_quantity(0x09, "0001", 1.0, Unit::Kph);
        assert_quantity(0x09, "FFFF", 65535.0, Unit::Kph);
        assert_quantity(0x0A, "0001", 0.122, Unit::Millivolt);
        assert_quantity(0x0A, "FFFF", 7995.27, Unit::Millivolt);
        assert_quantity(0x0B, "0001", 0.001, Unit::Volt);
        assert_quantity(0x0B, "FFFF", 65.535, Unit::Volt);
        assert_quantity(0x0C, "0001", 0.01, Unit::Volt);
        assert_quantity(0x0C, "FFFF", 655.35, Unit::Volt);
        assert_quantity(0x0D, "0001", 0.00390625, Unit::Milliampere);
        assert_quantity(0x0D, "FFFF", 255.996, Unit::Milliampere);
        assert_quantity(0x0E, "0001", 0.001, Unit::Ampere);
        assert_quantity(0x0E, "FFFF", 65.535, Unit::Ampere);
        assert_quantity(0x0F, "0001", 0.01, Unit::Ampere);
        assert_quantity(0x0F, "FFFF", 655.35, Unit::Ampere);
    }

    #[test]
    fn unsigned_time_and_resistance() {
        assert_quantity(0x10, "0001", 1.0, Unit::Millisecond);
        assert_quantity(0x10, "FFFF", 65535.0, Unit::Millisecond);
        assert_quantity(0x11, "0001", 100.0, Unit::Millisecond);
        assert_quantity(0x11, "FFFF", 6553500.0, Unit::Millisecond);
        assert_quantity(0x12, "0001", 1.0, Unit::Second);
        assert_quantity(0x12, "FFFF", 65535.0, Unit::Second);
        assert_quantity(0x13, "0001", 1.0, Unit::Milliohm);
        assert_quantity(0x13, "FFFF", 65535.0, Unit::Milliohm);
        assert_quantity(0x14, "0001", 1.0, Unit::Ohm);
        assert_quantity(0x14, "FFFF", 65535.0, Unit::Ohm);
        assert_quantity(0x15, "0001", 1.0, Unit::Kiloohm);
        assert_quantity(0x15, "FFFF", 65535.0, Unit::Kiloohm);
    }

    #[test]
    fn unsigned_temperature_and_pressure() {
        assert_quantity(0x16, "0000", -40.0, Unit::Celsius);
        assert_quantity(0x16, "0001", -39.9, Unit::Celsius);
        assert_quantity(0x16, "FFFF", 6513.5, Unit::Celsius);
        assert_quantity(0x17, "0001", 0.01, Unit::Kilopascal);
        assert_quantity(0x17, "FFFF", 655.35, Unit::Kilopascal);
        assert_quantity(0x18, "0001", 0.0117, Unit::Kilopascal);
        assert_quantity(0x18, "FFFF", 766.7595, Unit::Kilopascal);
        assert_quantity(0x19, "0001", 0.079, Unit::Kilopascal);
        assert_quantity(0x19, "FFFF", 5177.265, Unit::Kilopascal);
        assert_quantity(0x1A, "0001", 1.0, Unit::Kilopascal);
        assert_quantity(0x1A, "FFFF", 65535.0, Unit::Kilopascal);
        assert_quantity(0x1B, "0001", 10.0, Unit::Kilopascal);
        assert_quantity(0x1B, "FFFF", 655350.0, Unit::Kilopascal);
    }

    #[test]
    fn unsigned_angles_and_ratios() {
        assert_quantity(0x1C, "0001", 0.01, Unit::Degree);
        assert_quantity(0x1C, "FFFF", 655.35, Unit::Degree);
        assert_quantity(0x1D, "0001", 0.5, Unit::Degree);
        assert_quantity(0x1D, "FFFF", 32767.5, Unit::Degree);
        assert_quantity(0x1E, "0001", 0.0000305, Unit::Ratio);
        assert_quantity(0x1E, "FFFF", 1.9999, Unit::Ratio);
        assert_quantity(0x1F, "0001", 0.05, Unit::Ratio);
        assert_quantity(0x1F, "FFFF", 3276.75, Unit::Ratio);
        assert_quantity(0x20, "0001", 0.00390625, Unit::Ratio);
        assert_quantity(0x20, "FFFF", 255.996, Unit::Ratio);
    }

    #[test]
    fn unsigned_frequency_and_distance() {
        assert_quantity(0x21, "0001", 1.0, Unit::Millihertz);
        assert_quantity(0x21, "FFFF", 65535.0, Unit::Millihertz);
        assert_quantity(0x22, "0001", 1.0, Unit::Hertz);
        assert_quantity(0x22, "FFFF", 65535.0, Unit::Hertz);
        assert_quantity(0x23, "0001", 1.0, Unit::Kilohertz);
        assert_quantity(0x23, "FFFF", 65535.0, Unit::Kilohertz);
        assert_quantity(0x24, "0001", 1.0, Unit::Count);
        assert_quantity(0x24, "FFFF", 65535.0, Unit::Count);
        assert_quantity(0x25, "0001", 1.0, Unit::Kilometer);
        assert_quantity(0x25, "FFFF", 65535.0, Unit::Kilometer);
    }

    #[test]
    fn unsigned_flow_and_mass() {
        assert_quantity(0x26, "0001", 0.1, Unit::MillivoltPerMillisecond);
        assert_quantity(0x26, "FFFF", 6553.5, Unit::MillivoltPerMillisecond);
        assert_quantity(0x27, "0001", 0.01, Unit::GramsPerSecond);
        assert_quantity(0x27, "FFFF", 655.35, Unit::GramsPerSecond);
        assert_quantity(0x28, "0001", 1.0, Unit::GramsPerSecond);
        assert_quantity(0x28, "FFFF", 65535.0, Unit::GramsPerSecond);
        assert_quantity(0x29, "0001", 0.25, Unit::PascalPerSecond);
        assert_quantity(0x29, "FFFF", 16383.75, Unit::PascalPerSecond);
        assert_quantity(0x2A, "0001", 0.001, Unit::KilogramPerHour);
        assert_quantity(0x2A, "FFFF", 65.535, Unit::KilogramPerHour);
        assert_quantity(0x2B, "0001", 1.0, Unit::Count);
        assert_quantity(0x2B, "FFFF", 65535.0, Unit::Count);
        assert_quantity(0x2C, "0001", 0.01, Unit::Gram);
        assert_quantity(0x2C, "FFFF", 655.35, Unit::Gram);
        assert_quantity(0x2D, "0001", 0.01, Unit::Milligram);
        assert_quantity(0x2D, "FFFF", 655.35, Unit::Milligram);
    }

    #[test]
    fn unsigned_any_and_percent() {
        // 0x2E is the "any byte set" lambda
        assert_eq!(uas(0x2E).unwrap().apply(&b("0000")), UasValue::Bool(false));
        assert_eq!(uas(0x2E).unwrap().apply(&b("0001")), UasValue::Bool(true));
        assert_eq!(uas(0x2E).unwrap().apply(&b("0100")), UasValue::Bool(true));
        assert_quantity(0x2F, "0001", 0.01, Unit::Percent);
        assert_quantity(0x2F, "FFFF", 655.35, Unit::Percent);
        assert_quantity(0x30, "0001", 0.001526, Unit::Percent);
        assert_quantity(0x30, "FFFF", 100.006, Unit::Percent);
    }

    #[test]
    fn unsigned_volume_and_length() {
        assert_quantity(0x31, "0001", 0.001, Unit::Liter);
        assert_quantity(0x31, "FFFF", 65.535, Unit::Liter);
        assert_quantity(0x32, "0001", 0.0000305, Unit::Inch);
        assert_quantity(0x32, "FFFF", 1.9999, Unit::Inch);
        assert_quantity(0x33, "0001", 0.00024414, Unit::Ratio);
        assert_quantity(0x33, "FFFF", 15.999, Unit::Ratio);
        assert_quantity(0x34, "0001", 1.0, Unit::Minute);
        assert_quantity(0x34, "FFFF", 65535.0, Unit::Minute);
        assert_quantity(0x35, "0001", 10.0, Unit::Millisecond);
        assert_quantity(0x35, "FFFF", 655350.0, Unit::Millisecond);
    }

    #[test]
    fn unsigned_mass_and_misc() {
        assert_quantity(0x36, "0001", 0.01, Unit::Gram);
        assert_quantity(0x36, "FFFF", 655.35, Unit::Gram);
        assert_quantity(0x37, "0001", 0.1, Unit::Gram);
        assert_quantity(0x37, "FFFF", 6553.5, Unit::Gram);
        assert_quantity(0x38, "0001", 1.0, Unit::Gram);
        assert_quantity(0x38, "FFFF", 65535.0, Unit::Gram);
        assert_quantity(0x39, "0000", -327.68, Unit::Percent);
        assert_quantity(0x39, "0001", -327.67, Unit::Percent);
        assert_quantity(0x39, "FFFF", 327.67, Unit::Percent);
        assert_quantity(0x3A, "0001", 0.001, Unit::Gram);
        assert_quantity(0x3A, "FFFF", 65.535, Unit::Gram);
        assert_quantity(0x3B, "0001", 0.0001, Unit::Gram);
        assert_quantity(0x3B, "FFFF", 6.5535, Unit::Gram);
        assert_quantity(0x3C, "0001", 0.1, Unit::Microsecond);
        assert_quantity(0x3C, "FFFF", 6553.5, Unit::Microsecond);
        assert_quantity(0x3D, "0001", 0.01, Unit::Milliampere);
        assert_quantity(0x3D, "FFFF", 655.35, Unit::Milliampere);
        assert_quantity(0x3E, "0001", 0.00006103516, Unit::SquareMillimeter);
        assert_quantity(0x3E, "FFFF", 3.9999, Unit::SquareMillimeter);
        assert_quantity(0x3F, "0001", 0.01, Unit::Liter);
        assert_quantity(0x3F, "FFFF", 655.35, Unit::Liter);
        assert_quantity(0x40, "0001", 1.0, Unit::Ppm);
        assert_quantity(0x40, "FFFF", 65535.0, Unit::Ppm);
        assert_quantity(0x41, "0001", 0.01, Unit::Microampere);
        assert_quantity(0x41, "FFFF", 655.35, Unit::Microampere);
    }

    #[test]
    fn signed_counts() {
        assert_quantity(0x81, "0000", 0.0, Unit::Count);
        assert_quantity(0x81, "0001", 1.0, Unit::Count);
        assert_quantity(0x81, "FFFF", -1.0, Unit::Count);
        assert_quantity(0x82, "0001", 0.1, Unit::Count);
        assert_quantity(0x82, "FFFF", -0.1, Unit::Count);
        assert_quantity(0x83, "0001", 0.01, Unit::Count);
        assert_quantity(0x83, "FFFF", -0.01, Unit::Count);
        assert_quantity(0x84, "0001", 0.001, Unit::Count);
        assert_quantity(0x84, "FFFF", -0.001, Unit::Count);
        assert_quantity(0x85, "0001", 0.0000305, Unit::Count);
        assert_quantity(0x85, "FFFF", -0.0000305, Unit::Count);
        assert_quantity(0x86, "0001", 0.000305, Unit::Count);
        assert_quantity(0x86, "FFFF", -0.000305, Unit::Count);
        assert_quantity(0x87, "0001", 1.0, Unit::Ppm);
        assert_quantity(0x87, "FFFF", -1.0, Unit::Ppm);
    }

    #[test]
    fn signed_electrical() {
        assert_quantity(0x8A, "0001", 0.122, Unit::Millivolt);
        assert_quantity(0x8A, "FFFF", -0.122, Unit::Millivolt);
        assert_quantity(0x8B, "0001", 0.001, Unit::Volt);
        assert_quantity(0x8B, "FFFF", -0.001, Unit::Volt);
        assert_quantity(0x8C, "0001", 0.01, Unit::Volt);
        assert_quantity(0x8C, "FFFF", -0.01, Unit::Volt);
        assert_quantity(0x8D, "0001", 0.00390625, Unit::Milliampere);
        assert_quantity(0x8D, "FFFF", -0.00390625, Unit::Milliampere);
        assert_quantity(0x8E, "0001", 0.001, Unit::Ampere);
        assert_quantity(0x8E, "FFFF", -0.001, Unit::Ampere);
        assert_quantity(0x90, "0001", 1.0, Unit::Millisecond);
        assert_quantity(0x90, "FFFF", -1.0, Unit::Millisecond);
    }

    #[test]
    fn signed_temperature_pressure_angle() {
        assert_quantity(0x96, "0001", 0.1, Unit::Celsius);
        assert_quantity(0x96, "FFFF", -0.1, Unit::Celsius);
        assert_quantity(0x99, "0001", 0.1, Unit::Kilopascal);
        assert_quantity(0x99, "FFFF", -0.1, Unit::Kilopascal);
        assert_quantity(0x9C, "0001", 0.01, Unit::Degree);
        assert_quantity(0x9C, "FFFF", -0.01, Unit::Degree);
        assert_quantity(0x9D, "0001", 0.5, Unit::Degree);
        assert_quantity(0x9D, "FFFF", -0.5, Unit::Degree);
    }

    #[test]
    fn signed_flow_and_misc() {
        assert_quantity(0xA8, "0001", 1.0, Unit::GramsPerSecond);
        assert_quantity(0xA8, "FFFF", -1.0, Unit::GramsPerSecond);
        assert_quantity(0xA9, "0001", 0.25, Unit::PascalPerSecond);
        assert_quantity(0xA9, "FFFF", -0.25, Unit::PascalPerSecond);
        assert_quantity(0xAD, "0001", 0.01, Unit::Milligram);
        assert_quantity(0xAD, "FFFF", -0.01, Unit::Milligram);
        assert_quantity(0xAE, "0001", 0.1, Unit::Milligram);
        assert_quantity(0xAE, "FFFF", -0.1, Unit::Milligram);
        assert_quantity(0xAF, "0001", 0.01, Unit::Percent);
        assert_quantity(0xAF, "FFFF", -0.01, Unit::Percent);
        assert_quantity(0xB0, "0001", 0.003052, Unit::Percent);
        assert_quantity(0xB0, "FFFF", -0.003052, Unit::Percent);
        assert_quantity(0xB1, "0001", 2.0, Unit::MillivoltPerSecond);
        assert_quantity(0xB1, "FFFF", -2.0, Unit::MillivoltPerSecond);
        assert_quantity(0xFC, "0001", 0.01, Unit::Kilopascal);
        assert_quantity(0xFC, "FFFF", -0.01, Unit::Kilopascal);
        assert_quantity(0xFD, "0001", 0.001, Unit::Kilopascal);
        assert_quantity(0xFD, "FFFF", -0.001, Unit::Kilopascal);
        assert_quantity(0xFE, "0001", 0.25, Unit::Pascal);
        assert_quantity(0xFE, "FFFF", -0.25, Unit::Pascal);
    }

    #[test]
    fn unknown_ids_return_none() {
        assert!(uas(0x00).is_none());
        assert!(uas(0x42).is_none());
        assert!(uas(0x80).is_none());
        assert!(uas(0x88).is_none());
        assert!(uas(0xFF).is_none());
    }

    #[test]
    fn unit_strings() {
        assert_eq!(Unit::Count.as_str(), "count");
        assert_eq!(Unit::Rpm.as_str(), "rpm");
        assert_eq!(Unit::Celsius.as_str(), "celsius");
        assert_eq!(Unit::SquareMillimeter.as_str(), "millimeter ** 2");
        assert_eq!(Unit::Ppm.as_str(), "ppm");
    }
}
