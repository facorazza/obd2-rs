//! Protocol identification.
//!
//! The ELM327 reports the negotiated protocol as a one-character id
//! (`1`..`9`, `A`..`C`). The only property the parsers need is whether the
//! protocol frames replies as CAN, because CAN adds a DTC-count byte that the
//! legacy protocols do not.

/// How an adapter frames OBD replies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Framing {
    /// ISO 15765-4 or SAE J1939: a DTC count follows the response byte.
    #[default]
    Can,
    /// SAE J1850, ISO 9141-2 or ISO 14230-4: no count byte.
    Legacy,
}

impl Framing {
    /// Infers the framing from the descriptive `ATDP` reply.
    pub fn from_description(reply: &str) -> Option<Self> {
        if contains_ignore_ascii_case(reply, "CAN") || contains_ignore_ascii_case(reply, "15765") {
            Some(Self::Can)
        } else if ["J1850", "9141", "14230", "KWP"]
            .iter()
            .any(|needle| contains_ignore_ascii_case(reply, needle))
        {
            Some(Self::Legacy)
        } else {
            None
        }
    }
}

/// An ELM327 protocol id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProtocolId(u8);

impl ProtocolId {
    pub const SAE_J1850_PWM: Self = Self(b'1');
    pub const SAE_J1850_VPW: Self = Self(b'2');
    pub const ISO_9141_2: Self = Self(b'3');
    pub const ISO_14230_4_5BAUD: Self = Self(b'4');
    pub const ISO_14230_4_FAST: Self = Self(b'5');
    pub const ISO_15765_4_11BIT_500K: Self = Self(b'6');
    pub const ISO_15765_4_29BIT_500K: Self = Self(b'7');
    pub const ISO_15765_4_11BIT_250K: Self = Self(b'8');
    pub const ISO_15765_4_29BIT_250K: Self = Self(b'9');
    pub const SAE_J1939: Self = Self(b'A');
    pub const USER_CAN_11BIT_125K: Self = Self(b'B');
    pub const USER_CAN_29BIT_125K: Self = Self(b'C');

    /// Parses a single-character protocol id such as `"6"` or `"A"`.
    pub fn parse(id: &str) -> Option<Self> {
        match id.as_bytes() {
            [byte] => Self::from_byte(*byte),
            _ => None,
        }
    }

    /// Parses the id returned by `ATDPN`, which may carry an `A` prefix when
    /// the adapter selected the protocol automatically (`A6`).
    pub fn from_atdpn(reply: &str) -> Option<Self> {
        Self::from_byte(*reply.trim().as_bytes().last()?)
    }

    pub const fn from_byte(byte: u8) -> Option<Self> {
        let id = byte.to_ascii_uppercase();
        if matches!(id, b'1'..=b'9' | b'A'..=b'C') {
            Some(Self(id))
        } else {
            None
        }
    }

    pub const fn framing(self) -> Framing {
        match self.0 {
            b'6'..=b'9' | b'A'..=b'C' => Framing::Can,
            _ => Framing::Legacy,
        }
    }

    pub const fn elm_id(self) -> &'static str {
        match self.0 {
            b'1' => "1",
            b'2' => "2",
            b'3' => "3",
            b'4' => "4",
            b'5' => "5",
            b'6' => "6",
            b'7' => "7",
            b'8' => "8",
            b'9' => "9",
            b'A' => "A",
            b'B' => "B",
            b'C' => "C",
            _ => "",
        }
    }

    pub const fn elm_name(self) -> &'static str {
        match self.0 {
            b'1' => "SAE J1850 PWM",
            b'2' => "SAE J1850 VPW",
            b'3' => "ISO 9141-2",
            b'4' => "ISO 14230-4 (KWP 5BAUD)",
            b'5' => "ISO 14230-4 (KWP FAST)",
            b'6' => "ISO 15765-4 (CAN 11/500)",
            b'7' => "ISO 15765-4 (CAN 29/500)",
            b'8' => "ISO 15765-4 (CAN 11/250)",
            b'9' => "ISO 15765-4 (CAN 29/250)",
            b'A' => "SAE J1939 (CAN 29/250)",
            b'B' => "User CAN (11/125)",
            b'C' => "User CAN (29/125)",
            _ => "Unknown",
        }
    }
}

fn contains_ignore_ascii_case(haystack: &str, needle: &str) -> bool {
    haystack
        .as_bytes()
        .windows(needle.len())
        .any(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atdpn_accepts_automatic_prefix() {
        assert_eq!(
            ProtocolId::from_atdpn("A6\r"),
            Some(ProtocolId::ISO_15765_4_11BIT_500K)
        );
        assert_eq!(ProtocolId::from_atdpn("3"), Some(ProtocolId::ISO_9141_2));
        assert_eq!(ProtocolId::from_atdpn("AUTO"), None);
        assert_eq!(ProtocolId::from_atdpn(""), None);
    }

    #[test]
    fn framing_classifies_ids() {
        assert_eq!(ProtocolId::ISO_9141_2.framing(), Framing::Legacy);
        assert_eq!(ProtocolId::ISO_15765_4_11BIT_500K.framing(), Framing::Can);
        assert_eq!(ProtocolId::SAE_J1939.framing(), Framing::Can);
        assert_eq!(ProtocolId::USER_CAN_29BIT_125K.framing(), Framing::Can);
    }

    #[test]
    fn description_fallback() {
        assert_eq!(
            Framing::from_description("ISO 15765-4 (CAN 11/500)"),
            Some(Framing::Can)
        );
        assert_eq!(
            Framing::from_description("ISO 9141-2"),
            Some(Framing::Legacy)
        );
        assert_eq!(Framing::from_description("AUTO"), None);
    }

    #[test]
    fn names_match_ids() {
        assert_eq!(ProtocolId::ISO_15765_4_11BIT_500K.elm_id(), "6");
        assert_eq!(ProtocolId::SAE_J1939.elm_name(), "SAE J1939 (CAN 29/250)");
        assert_eq!(
            ProtocolId::parse("6"),
            Some(ProtocolId::ISO_15765_4_11BIT_500K)
        );
        assert_eq!(ProtocolId::parse("?"), None);
        assert_eq!(ProtocolId::parse("10"), None);
    }
}
