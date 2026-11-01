//! Allocation-free OBD-II / ELM327 primitives.
//!
//! This crate contains the parts of `obd2-rs` that need neither `std` nor a
//! serial port: reply framing, Mode 1 and DTC parsing, supported-PID bitmaps
//! and protocol identification. `obd2-rs` re-exports it.
//!
//! | Module        | Contents                                      |
//! |---------------|-----------------------------------------------|
//! | [`reply`]     | sans-io `ElmResponse` and the AT init sequence |
//! | [`parse`]     | reply lines, hex bytes, Mode 1 data, voltage  |
//! | [`dtc`]       | `DtcCode` and Mode 03/07/0A decoding          |
//! | [`pids`]      | supported-PID bitmaps                         |
//! | [`protocol`]  | `ProtocolId` and CAN/legacy `Framing`         |

#![no_std]
#![forbid(unsafe_code)]

pub mod dtc;
pub mod parse;
pub mod pids;
pub mod protocol;
pub mod reply;

pub use dtc::DtcCode;
pub use protocol::{Framing, ProtocolId};
pub use reply::{ElmResponse, Feed, InitCommand, InitOptions, Overflow};
