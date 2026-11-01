//! Sans-io ELM327 reply framing and the AT init sequence.

use core::fmt;

use heapless::String;

use crate::protocol::ProtocolId;

/// A reply exceeded the fixed buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Overflow;

impl fmt::Display for Overflow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ELM327 reply did not fit")
    }
}

/// Result of feeding one notification chunk into an [`ElmResponse`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feed {
    /// The prompt has not arrived yet.
    Pending,
    /// The prompt was seen; the reply is complete.
    Complete,
}

/// Accumulates one prompt-terminated ELM327 reply.
///
/// The transport writes the command, then passes every notification chunk to
/// [`feed`](Self::feed) until it returns [`Feed::Complete`]. CR/LF pairs are
/// normalised to one `\n` per line so multi-frame replies stay distinguishable,
/// and non-printable bytes are dropped.
pub struct ElmResponse<const N: usize> {
    raw: String<N>,
}

impl<const N: usize> Default for ElmResponse<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> ElmResponse<N> {
    pub const fn new() -> Self {
        Self { raw: String::new() }
    }

    pub fn clear(&mut self) {
        self.raw.clear();
    }

    /// Appends one chunk, stopping at `end` (the prompt, `>`).
    pub fn feed(&mut self, chunk: &[u8], end: u8) -> Result<Feed, Overflow> {
        for &byte in chunk {
            if byte == end {
                return Ok(Feed::Complete);
            }
            match byte {
                b'\r' | b'\n' => {
                    if !self.raw.ends_with('\n') {
                        self.raw.push('\n').map_err(|_| Overflow)?;
                    }
                }
                0x20..=0x7e => self.raw.push(byte as char).map_err(|_| Overflow)?,
                _ => {}
            }
        }
        Ok(Feed::Pending)
    }

    pub fn raw(&self) -> &str {
        &self.raw
    }

    /// The reply with the echoed `command` and outer whitespace removed.
    pub fn reply(&self, command: &str) -> &str {
        self.raw.strip_prefix(command).unwrap_or(&self.raw).trim()
    }

    /// Non-empty, trimmed reply lines.
    pub fn lines(&self) -> impl Iterator<Item = &str> {
        self.raw
            .split('\n')
            .map(str::trim)
            .filter(|line| !line.is_empty())
    }
}

/// One command in the adapter bring-up sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitCommand {
    Reset,
    Echo(bool),
    Linefeeds(bool),
    Spaces(bool),
    Headers(bool),
    /// `None` leaves protocol selection to the adapter (`ATSP0`).
    Protocol(Option<ProtocolId>),
}

impl InitCommand {
    /// Renders the command, for example `ATE0` or `ATSP6`.
    pub fn render(self) -> String<8> {
        let mut out = String::new();
        match self {
            Self::Reset => out.push_str("ATZ"),
            Self::Echo(on) => out.push_str(if on { "ATE1" } else { "ATE0" }),
            Self::Linefeeds(on) => out.push_str(if on { "ATL1" } else { "ATL0" }),
            Self::Spaces(on) => out.push_str(if on { "ATS1" } else { "ATS0" }),
            Self::Headers(on) => out.push_str(if on { "ATH1" } else { "ATH0" }),
            Self::Protocol(None) => out.push_str("ATSP0"),
            Self::Protocol(Some(id)) => {
                let _ = out.push_str("ATSP");
                out.push_str(id.elm_id())
            }
        }
        .ok();
        out
    }

    /// Resetting or searching for a protocol takes noticeably longer than an
    /// `AT` switch.
    pub fn needs_delay(self) -> bool {
        matches!(self, Self::Reset | Self::Protocol(_))
    }

    /// Some commands do not answer `OK` on every clone, so the caller should
    /// not treat a different reply as a failure.
    pub fn is_lenient(self) -> bool {
        matches!(self, Self::Reset | Self::Protocol(_))
    }
}

/// Adapter bring-up settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct InitOptions {
    /// Send `ATZ`. BLE clones often drop the link on reset, so it is off by
    /// default.
    pub reset: bool,
    pub echo: bool,
    pub linefeeds: bool,
    pub spaces: bool,
    pub headers: bool,
    /// `None` selects `ATSP0` (automatic).
    pub protocol: Option<ProtocolId>,
}

impl InitOptions {
    /// The commands to send, in order.
    pub fn commands(&self) -> impl Iterator<Item = InitCommand> {
        [
            self.reset.then_some(InitCommand::Reset),
            Some(InitCommand::Echo(self.echo)),
            Some(InitCommand::Linefeeds(self.linefeeds)),
            Some(InitCommand::Spaces(self.spaces)),
            Some(InitCommand::Headers(self.headers)),
            Some(InitCommand::Protocol(self.protocol)),
        ]
        .into_iter()
        .flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feed_normalises_lines_and_stops_at_prompt() {
        let mut response = ElmResponse::<64>::new();
        assert_eq!(response.feed(b"41 0C\r\r", b'>'), Ok(Feed::Pending));
        assert_eq!(response.feed(b">junk", b'>'), Ok(Feed::Complete));
        assert_eq!(response.raw(), "41 0C\n");
        let lines: heapless::Vec<_, 2> = response.lines().collect();
        assert_eq!(lines.as_slice(), &["41 0C"]);
    }

    #[test]
    fn reply_strips_echo() {
        let mut response = ElmResponse::<64>::new();
        response.feed(b"ATRV\r12.6V\r\r>", b'>').unwrap();
        assert_eq!(response.reply("ATRV"), "12.6V");
        assert_eq!(response.reply("0100"), "ATRV\n12.6V");
    }

    #[test]
    fn feed_reports_overflow() {
        let mut response = ElmResponse::<4>::new();
        assert_eq!(response.feed(b"12345", b'>'), Err(Overflow));
    }

    #[test]
    fn init_commands_render() {
        let options = InitOptions {
            reset: true,
            echo: false,
            linefeeds: false,
            spaces: true,
            headers: true,
            protocol: Some(ProtocolId::ISO_15765_4_11BIT_500K),
        };
        let rendered: heapless::Vec<_, 6> = options.commands().map(InitCommand::render).collect();
        let rendered: heapless::Vec<_, 6> = rendered.iter().map(|c| c.as_str()).collect();
        assert_eq!(
            rendered.as_slice(),
            &["ATZ", "ATE0", "ATL0", "ATS1", "ATH1", "ATSP6"]
        );
    }

    #[test]
    fn init_commands_skip_reset_and_default_to_auto() {
        let rendered: heapless::Vec<_, 6> = InitOptions::default()
            .commands()
            .map(InitCommand::render)
            .collect();
        let rendered: heapless::Vec<_, 6> = rendered.iter().map(|c| c.as_str()).collect();
        assert_eq!(
            rendered.as_slice(),
            &["ATE0", "ATL0", "ATS0", "ATH0", "ATSP0"]
        );
    }
}
