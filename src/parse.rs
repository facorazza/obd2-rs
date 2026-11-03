//! Reply-line and Mode 1 parsing.

use heapless::String;

/// Splits a reply into non-empty, trimmed lines.
pub fn lines(response: &str) -> impl Iterator<Item = &str> {
    response
        .split(['\r', '\n'])
        .map(str::trim)
        .filter(|line| !line.is_empty())
}

/// Removes an ELM327 multi-frame line prefix (`0:`, `1:`, ...).
pub fn strip_frame_prefix(line: &str) -> &str {
    let line = line.trim_start();
    if let Some(index) = line.find(':') {
        let prefix = &line[..index];
        if !prefix.is_empty()
            && prefix.len() <= 2
            && prefix.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return &line[index + 1..];
        }
    }
    line
}

/// Decodes the hex runs of one reply line into `out`.
///
/// With spaces off the adapter glues status text to the payload
/// (`SEARCHING...4100D83B8013`), so each token is split into runs of hex
/// digits. A CAN header is the only legal odd-length run and is skipped.
/// Returns `None` when the line does not fit.
pub fn line_bytes(line: &str, out: &mut [u8]) -> Option<usize> {
    let mut len = 0;
    for token in line.split_whitespace() {
        for run in token.split(|c: char| !c.is_ascii_hexdigit()) {
            if run.len() % 2 != 0 {
                continue;
            }
            if len + run.len() / 2 > out.len() {
                return None;
            }
            let mut nibbles = run.bytes().map(hex_nibble);
            while let (Some(high), Some(low)) = (nibbles.next(), nibbles.next()) {
                out[len] = (high << 4) | low;
                len += 1;
            }
        }
    }
    Some(len)
}

/// Extracts the data bytes from a `41 <pid> ...` reply.
///
/// Several ECUs may answer a functional request, one frame per line and in
/// varying order. The engine ECU's frame wins so a PID does not flip between
/// modules; any other header is only a fallback.
///
/// Payloads longer than one CAN frame (the compression-ignition PIDs above
/// `0x5F` carry up to 13 data bytes) arrive as indexed continuation lines
/// (`0:`, `1:`, ...) when the adapter is configured without headers, and are
/// joined back together in order.
pub fn data_bytes<'a>(response: &str, pid: u8, out: &'a mut [u8]) -> Option<&'a [u8]> {
    let (index, line) =
        find_line(response, pid, true).or_else(|| find_line(response, pid, false))?;
    let len = line_bytes(line, out)?;
    let start = out[..len].windows(2).position(|pair| pair == [0x41, pid])?;

    let mut used = len - start - 2;
    out.copy_within(start + 2..len, 0);

    let Some(first) = frame_index(line) else {
        return Some(&out[..used]);
    };

    // Only a strictly increasing run of frame indices belongs to this reply;
    // another ECU's message restarts numbering at zero.
    let mut expected = first.wrapping_add(1);
    for next in lines(response).skip(index + 1) {
        if frame_index(next) != Some(expected) {
            break;
        }
        let mut bytes = [0u8; 16];
        let count = line_bytes(next, &mut bytes)?;
        if used + count > out.len() {
            return None;
        }
        out[used..used + count].copy_from_slice(&bytes[..count]);
        used += count;
        expected = expected.wrapping_add(1);
    }

    Some(&out[..used])
}

/// Reads the adapter supply voltage from an `ATRV` reply (`12.6V`).
pub fn voltage(response: &str) -> Option<f32> {
    let mut digits = String::<16>::new();
    for c in response.chars() {
        if c.is_ascii_digit() || c == '.' {
            digits.push(c).ok()?;
        }
    }
    digits.parse().ok()
}

fn find_line(response: &str, pid: u8, engine_only: bool) -> Option<(usize, &str)> {
    lines(response).enumerate().find(|(_, line)| {
        if engine_only && !is_engine_ecu(line) {
            return false;
        }
        let mut bytes = [0u8; 16];
        line_bytes(line, &mut bytes)
            .is_some_and(|len| bytes[..len].windows(2).any(|pair| pair == [0x41, pid]))
    })
}

/// The ELM327 multi-frame line number (`0:`), if the line has one.
fn frame_index(line: &str) -> Option<u8> {
    let prefix = line.trim_start().split_once(':')?.0;
    if prefix.is_empty() || prefix.len() > 2 {
        return None;
    }
    u8::from_str_radix(prefix, 16).ok()
}

/// True for a line answered by the engine ECU on 11-bit CAN (`7E8`).
fn is_engine_ecu(line: &str) -> bool {
    line.trim_start()
        .get(..3)
        .is_some_and(|header| header.eq_ignore_ascii_case("7E8"))
}

fn hex_nibble(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        _ => byte - b'A' + 10,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(response: &str, pid: u8) -> heapless::Vec<u8, 16> {
        let mut bytes = [0u8; 16];
        data_bytes(response, pid, &mut bytes)
            .map(|data| heapless::Vec::from_slice(data).unwrap())
            .unwrap_or_default()
    }

    #[test]
    fn decodes_with_spaces_and_headers() {
        assert_eq!(
            data("7E8 04 41 0C 1A F8\r\r>", 0x0C).as_slice(),
            &[0x1A, 0xF8]
        );
    }

    #[test]
    fn decodes_status_glued_to_payload() {
        assert_eq!(
            data("SEARCHING...410C1AF8\r\r>", 0x0C).as_slice(),
            &[0x1A, 0xF8]
        );
        let bitmap = data("SEARCHING...4100D83B80134100981A8013", 0x00);
        assert_eq!(&bitmap[..4], &[0xD8, 0x3B, 0x80, 0x13]);
    }

    #[test]
    fn prefers_engine_ecu() {
        // Two modules answer, and their order is not stable.
        assert_eq!(
            data("7E9 04 41 0C 0C B7\r7E8 04 41 0C 1A F8\r", 0x0C).as_slice(),
            &[0x1A, 0xF8]
        );
        // With spaces off the header is glued to the payload.
        assert_eq!(
            data("7E904 410C0CB7\r7E804 410C1AF8\r", 0x0C).as_slice(),
            &[0x1A, 0xF8]
        );
    }

    #[test]
    fn rejects_no_data() {
        assert!(data("NO DATA\r\r>", 0x0C).is_empty());
        assert!(data("UNABLE TO CONNECT\r\r>", 0x0C).is_empty());
    }

    #[test]
    fn joins_multiframe_payloads() {
        // 01 78 carries 9 data bytes, so the adapter splits it into two
        // indexed lines with headers off.
        assert_eq!(
            data("0: 41 78 0F 01 F4 02 8A\r1: 03 20 03 B6\r\r>", 0x78).as_slice(),
            &[0x0F, 0x01, 0xF4, 0x02, 0x8A, 0x03, 0x20, 0x03, 0xB6]
        );
        // The same with spaces off.
        assert_eq!(
            data("0:41780F01F4028A\r1:032003B6\r\r>", 0x78).as_slice(),
            &[0x0F, 0x01, 0xF4, 0x02, 0x8A, 0x03, 0x20, 0x03, 0xB6]
        );
    }

    #[test]
    fn stops_at_a_broken_frame_sequence() {
        // A missing frame or another ECU restarting at 0 must not be glued
        // onto the payload.
        assert_eq!(
            data("0: 41 78 0F 01 F4 02 8A\r2: 03 20 03 B6\r", 0x78).as_slice(),
            &[0x0F, 0x01, 0xF4, 0x02, 0x8A]
        );
        assert_eq!(
            data("0: 41 78 0F 01 F4 02 8A\r0: 03 20 03 B6\r", 0x78).as_slice(),
            &[0x0F, 0x01, 0xF4, 0x02, 0x8A]
        );
    }

    #[test]
    fn strips_multiframe_prefixes() {
        assert_eq!(strip_frame_prefix("0: 43 02"), " 43 02");
        assert_eq!(strip_frame_prefix("A:"), "");
        assert_eq!(strip_frame_prefix("NO DATA"), "NO DATA");
        assert_eq!(strip_frame_prefix("41:00"), "00");
    }

    #[test]
    fn line_bytes_skips_odd_runs() {
        let mut bytes = [0u8; 8];
        assert_eq!(line_bytes("7E8 04 41 0C 1A F8", &mut bytes), Some(5));
        assert_eq!(&bytes[..5], &[0x04, 0x41, 0x0C, 0x1A, 0xF8]);
        assert_eq!(line_bytes("7E8 04 41", &mut [0u8; 1]), None);
    }

    #[test]
    fn parses_voltage() {
        assert_eq!(voltage("12.6V"), Some(12.6));
        assert_eq!(voltage("ATRV\r12.6V\r\r>"), Some(12.6));
        assert_eq!(voltage("NO DATA"), None);
    }
}
