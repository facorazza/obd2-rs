//! Unknown protocol parser.
//!
//! Used when a connection to the ELM has been made, but the car hasn't
//! responded.
//!
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::message::{Frame, Message, ProtocolParser};

/// Passes everything through unparsed.
#[derive(Debug, Clone, Copy)]
pub struct UnknownProtocol;

impl ProtocolParser for UnknownProtocol {
    fn elm_name(&self) -> &'static str {
        ""
    }

    fn elm_id(&self) -> &'static str {
        ""
    }

    fn tx_id_engine(&self) -> Option<u32> {
        None
    }

    fn tx_id_transmission(&self) -> Option<u32> {
        None
    }

    fn parse_frame(&self, _frame: &mut Frame) -> bool {
        true // pass everything
    }

    fn parse_message(&self, _message: &mut Message) -> bool {
        true // pass everything
    }
}
