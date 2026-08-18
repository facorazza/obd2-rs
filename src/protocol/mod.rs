//! OBD-II protocol parsers: CAN, legacy (non-CAN) and unknown.
//!
// SPDX-License-Identifier: GPL-3.0-or-later

mod can;
mod legacy;
mod unknown;

pub use can::CanProtocol;
pub use legacy::LegacyProtocol;
pub use unknown::UnknownProtocol;

/// Decodes a hex string (no spaces) into bytes.
pub(crate) fn decode_hex(hex: &str) -> Option<Vec<u8>> {
    if hex.len() % 2 != 0 {
        return None;
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
        .collect()
}
