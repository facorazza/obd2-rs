#![doc = include_str!("../README.md")]

//! Module layout:
//!
//! | Module        | Contents                                        |
//! |---------------|-------------------------------------------------|
//! | [`parse`]     | reply lines, hex bytes, Mode 1 data, voltage    |
//! | [`reply`]     | sans-io `ElmResponse` and the AT init sequence  |
//! | [`dtc`]       | `DtcCode` and Mode 03/07/0A decoding            |
//! | [`pids`]      | supported-PID bitmaps                           |
//! | [`protocol`]  | `ProtocolId`, `Framing` and frame parsers       |
//!
//! Everything above is allocation-free and available without `std`; the
//! modules below it need the serial port or the standard library and are
//! only compiled with the default `std` feature.
//!
//! | Module        | Contents                                        |
//! |---------------|-------------------------------------------------|
//! | [`util`]      | byte helpers, `BitArray`, `OBDStatus`           |
//! | [`units`]     | unit model and UAS table                        |
//! | [`message`]   | `Frame` and `Message` types                     |
//! | [`elm327`]    | serial driver and AT session                    |
//! | [`codes`]     | DTC and test-ID tables                          |
//! | [`decoder`]   | value decoders                                  |
//! | [`dtc_table`] | generated DTC table                             |

#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]

pub mod dtc;
pub mod parse;
pub mod pids;
pub mod protocol;
pub mod reply;

#[cfg(feature = "std")]
pub mod codes;
#[cfg(feature = "std")]
pub mod command;
#[cfg(feature = "std")]
pub mod commands;
#[cfg(feature = "std")]
pub mod commands_table;
#[cfg(feature = "std")]
pub mod decoder;
#[cfg(feature = "std")]
pub mod dtc_table;
#[cfg(feature = "std")]
pub mod elm327;
#[cfg(feature = "std")]
pub mod message;
#[cfg(feature = "std")]
pub mod obd;
#[cfg(feature = "std")]
pub mod units;
#[cfg(feature = "std")]
pub mod util;

pub use dtc::DtcCode;
pub use protocol::{Framing, ProtocolId};
pub use reply::{ElmResponse, Feed, InitCommand, InitOptions, Overflow};
