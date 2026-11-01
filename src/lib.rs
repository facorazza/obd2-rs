#![doc = include_str!("../README.md")]

//! Module layout:
//!
//! | Module        | Contents                                        |
//! |---------------|-------------------------------------------------|
//! | [`util`]      | byte helpers, `BitArray`, `OBDStatus`           |
//! | [`units`]     | unit model and UAS table                        |
//! | [`protocol`]  | CAN / legacy / unknown frame parsers            |
//! | [`message`]   | `Frame` and `Message` types                     |
//! | [`elm327`]    | serial driver and AT session                    |
//! | [`codes`]     | DTC and test-ID tables                          |
//! | [`decoder`]   | value decoders                                  |
//! | [`dtc_table`] | generated DTC table                             |
//!
//! The `no_std` half of the library is re-exported from `obd2-core`:
//! [`reply`], [`parse`], [`dtc`], [`pids`], [`Framing`] and [`ProtocolId`].

#![forbid(unsafe_code)]

pub mod codes;
pub mod command;
pub mod commands;
pub mod commands_table;
pub mod decoder;
pub mod dtc_table;
pub mod elm327;
pub mod message;
pub mod obd;
pub mod protocol;
pub mod units;
pub mod util;

pub use obd2_core::protocol::{Framing, ProtocolId};
pub use obd2_core::{dtc, parse, pids, reply};
