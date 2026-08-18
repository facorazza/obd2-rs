#![doc = include_str!("../README.md")]

//! Module layout:
//!
//! | Module      | Contents                          |
//! |-------------|-----------------------------------|
//! | [`util`]    | byte helpers, `BitArray`, `OBDStatus` |
//! | [`units`]   | unit model and UAS table          |
//! | [`protocol`]| CAN / legacy / unknown parsers    |
//! | [`message`] | `Frame` and `Message` types       |
//! | [`elm327`]  | serial driver and AT session      |
//! | [`codes`]   | DTC and test-ID tables            |
//! | [`decoder`] | value decoders                    |
//! | [`dtc_table`] | generated DTC table             |

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
