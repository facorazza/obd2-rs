//! ELM327 serial driver.
//!
//! Handles communication with the ELM327 adapter: port opening, baud
//! detection, the AT init sequence, protocol auto-detection and the
//! low-power state machine.
//!
// SPDX-License-Identifier: GPL-3.0-or-later

use std::time::Duration;

use crate::message::{Message, Protocol, ProtocolParser};
use crate::protocol::{CanProtocol, LegacyProtocol, UnknownProtocol};
use crate::util::OBDStatus;

/// The ELM prompt character.
pub const ELM_PROMPT: &[u8] = b">";
/// An 'OK' which indicates we are entering low power state.
pub const ELM_LP_ACTIVE: &[u8] = b"OK";

/// 38400, 9600 are the possible boot bauds (unless reprogrammed via
/// PP 0C). 19200, 38400, 57600, 115200, 230400, 500000 are listed on
/// p.46 of the ELM327 datasheet.
///
/// We check the two default baud rates first, then go fastest to
/// slowest, on the theory that anyone who's using a slow baud rate is
/// going to be less picky about the time required to detect it.
pub const TRY_BAUDS: [u32; 6] = [38400, 9600, 230400, 115200, 57600, 19200];

/// Used as a fallback, when ATSP0 doesn't cut it.
pub const TRY_PROTOCOL_ORDER: [&str; 10] = [
    "6", // ISO_15765_4_11bit_500k
    "8", // ISO_15765_4_11bit_250k
    "1", // SAE_J1850_PWM
    "7", // ISO_15765_4_29bit_500k
    "9", // ISO_15765_4_29bit_250k
    "2", // SAE_J1850_VPW
    "3", // ISO_9141_2
    "4", // ISO_14230_4_5baud
    "5", // ISO_14230_4_fast
    "A", // SAE_J1939
];

/// The ELM protocol IDs this library understands.
const SUPPORTED_PROTOCOLS: [&str; 10] = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "A"];

/// Returns the parser for an ELM protocol ID.
fn protocol_parser(elm_id: &str) -> Box<dyn ProtocolParser> {
    match elm_id {
        "1" => Box::new(LegacyProtocol::SaeJ1850Pwm),
        "2" => Box::new(LegacyProtocol::SaeJ1850Vpw),
        "3" => Box::new(LegacyProtocol::Iso91412),
        "4" => Box::new(LegacyProtocol::Iso1423045baud),
        "5" => Box::new(LegacyProtocol::Iso142304Fast),
        "6" => Box::new(CanProtocol::Iso15765411bit500k),
        "7" => Box::new(CanProtocol::Iso15765429bit500k),
        "8" => Box::new(CanProtocol::Iso15765411bit250k),
        "9" => Box::new(CanProtocol::Iso15765429bit250k),
        "A" => Box::new(CanProtocol::SaeJ1939),
        _ => Box::new(UnknownProtocol),
    }
}

/// The serial port operations the ELM327 driver needs.
///
/// This mirrors the subset of `serialport::SerialPort` used by the
/// driver, so tests can drive the ELM327 with a fake port.
pub trait ElmPort {
    fn port_name(&self) -> String;
    fn baud_rate(&self) -> u32;
    fn set_baud_rate(&mut self, baud: u32) -> Result<(), ElmError>;
    fn timeout(&self) -> Duration;
    fn set_timeout(&mut self, timeout: Duration) -> Result<(), ElmError>;
    fn clear_buffers(&mut self) -> Result<(), ElmError>;
    fn write(&mut self, data: &[u8]) -> Result<(), ElmError>;
    fn flush(&mut self) -> Result<(), ElmError>;
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, ElmError>;
    fn close(&mut self);
}

/// Errors produced by the ELM327 driver.
#[derive(Debug, thiserror::Error)]
pub enum ElmError {
    #[error("serial port error: {0}")]
    Serial(#[from] serialport::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Message(String),
}

impl ElmPort for Box<dyn serialport::SerialPort> {
    fn port_name(&self) -> String {
        self.name().unwrap_or_default()
    }

    fn baud_rate(&self) -> u32 {
        <dyn serialport::SerialPort>::baud_rate(&**self).unwrap_or(0)
    }

    fn set_baud_rate(&mut self, baud: u32) -> Result<(), ElmError> {
        <dyn serialport::SerialPort>::set_baud_rate(&mut **self, baud)?;
        Ok(())
    }

    fn timeout(&self) -> Duration {
        <dyn serialport::SerialPort>::timeout(&**self)
    }

    fn set_timeout(&mut self, timeout: Duration) -> Result<(), ElmError> {
        <dyn serialport::SerialPort>::set_timeout(&mut **self, timeout)?;
        Ok(())
    }

    fn clear_buffers(&mut self) -> Result<(), ElmError> {
        <dyn serialport::SerialPort>::clear(&**self, serialport::ClearBuffer::All)?;
        Ok(())
    }

    fn write(&mut self, data: &[u8]) -> Result<(), ElmError> {
        <dyn serialport::SerialPort>::write(&mut **self, data)?;
        Ok(())
    }

    fn flush(&mut self) -> Result<(), ElmError> {
        <dyn serialport::SerialPort>::flush(&mut **self)?;
        Ok(())
    }

    fn read(&mut self, buf: &mut [u8]) -> Result<usize, ElmError> {
        Ok(<dyn serialport::SerialPort>::read(&mut **self, buf)?)
    }

    fn close(&mut self) {
        // serialport 4.x has no close(); the port closes on drop
    }
}

/// Handles communication with the ELM327 adapter.
pub struct ELM327 {
    status: OBDStatus,
    port: Option<Box<dyn ElmPort>>,
    protocol: Protocol,
    low_power: bool,
    timeout: Duration,
}

impl ELM327 {
    /// Builds an ELM327 around an existing port (used by tests).
    #[cfg(test)]
    pub(crate) fn from_parts(
        status: OBDStatus,
        port: Box<dyn ElmPort>,
        protocol: Protocol,
        timeout: Duration,
    ) -> Self {
        ELM327 {
            status,
            port: Some(port),
            protocol,
            low_power: false,
            timeout,
        }
    }

    /// Initializes the port by resetting the device and getting
    /// supported PIDs.
    ///
    /// Mirrors `ELM327.__init__`: on failure the object is returned in
    /// the `NotConnected` state (check [`ELM327::status`]).
    pub fn new(
        portname: &str,
        baudrate: Option<u32>,
        protocol: Option<&str>,
        timeout: Duration,
        check_voltage: bool,
        start_low_power: bool,
    ) -> Self {
        tracing::info!(
            "Initializing ELM327: PORT={portname} BAUD={} PROTOCOL={}",
            baudrate.map_or("auto".to_string(), |b| b.to_string()),
            protocol.unwrap_or("auto")
        );

        let mut elm = ELM327 {
            status: OBDStatus::NotConnected,
            port: None,
            protocol: Protocol::new(Box::new(UnknownProtocol), &[]),
            low_power: false,
            timeout,
        };

        // open port
        // 8N1, 10 second timeout
        match serialport::new(portname, 38400)
            .timeout(Duration::from_secs(10))
            .open()
        {
            Ok(port) => elm.port = Some(Box::new(port)),
            Err(e) => {
                elm.error(e.to_string());
                return elm;
            }
        }

        // If we start with the IC in the low power state we need to wake it up
        if start_low_power {
            elm.write_raw(b" ");
            std::thread::sleep(Duration::from_secs(1));
        }

        // find the ELM's baud
        if !elm.set_baudrate(baudrate) {
            elm.error("Failed to set baudrate");
            return elm;
        }

        // ATZ (reset)
        // return data can be junk, so don't bother checking
        if let Err(e) = elm.send(b"ATZ", Some(Duration::from_secs(1)), ELM_PROMPT) {
            elm.error(e.to_string());
            return elm;
        }

        // ATE0 (echo OFF)
        match elm.send(b"ATE0", None, ELM_PROMPT) {
            Ok(r) if is_ok(&r, true) => {}
            _ => {
                elm.error("ATE0 did not return 'OK'");
                return elm;
            }
        }

        // ATH1 (headers ON)
        match elm.send(b"ATH1", None, ELM_PROMPT) {
            Ok(r) if is_ok(&r, false) => {}
            _ => {
                elm.error("ATH1 did not return 'OK', or echoing is still ON");
                return elm;
            }
        }

        // ATL0 (linefeeds OFF)
        match elm.send(b"ATL0", None, ELM_PROMPT) {
            Ok(r) if is_ok(&r, false) => {}
            _ => {
                elm.error("ATL0 did not return 'OK'");
                return elm;
            }
        }

        // by now, we've successfully communicated with the ELM, but not the car
        elm.status = OBDStatus::ElmConnected;

        // AT RV (read volt)
        if check_voltage {
            match elm.send(b"AT RV", None, ELM_PROMPT) {
                Ok(r) if r.len() == 1 && !r[0].is_empty() => {
                    let volts: f64 = r[0]
                        .to_lowercase()
                        .replace('v', "")
                        .trim()
                        .parse()
                        .unwrap_or(f64::NAN);
                    if volts.is_nan() {
                        elm.error("Incorrect response from 'AT RV'");
                        return elm;
                    }
                    if volts < 6.0 {
                        tracing::error!("OBD2 socket disconnected");
                        return elm;
                    }
                }
                _ => {
                    elm.error("No answer from 'AT RV'");
                    return elm;
                }
            }
            // by now, we've successfully connected to the OBD socket
            elm.status = OBDStatus::ObdConnected;
        }

        // try to communicate with the car, and load the correct protocol parser
        if elm.set_protocol(protocol) {
            elm.status = OBDStatus::CarConnected;
            tracing::info!(
                "Connected Successfully: PORT={portname} BAUD={} PROTOCOL={}",
                elm.port.as_ref().map_or(0, |p| p.baud_rate()),
                elm.protocol.elm_id()
            );
        } else if elm.status == OBDStatus::ObdConnected {
            tracing::error!("Adapter connected, but the ignition is off");
        } else {
            tracing::error!("Connected to the adapter, but failed to connect to the vehicle");
        }

        elm
    }

    /// The name of the connected port, or an empty string.
    pub fn port_name(&self) -> String {
        self.port
            .as_ref()
            .map_or_else(String::new, |p| p.port_name())
    }

    /// The current connection status.
    pub fn status(&self) -> OBDStatus {
        self.status
    }

    /// The ECU IDs discovered for the current protocol.
    pub fn ecus(&self) -> Vec<u8> {
        self.protocol.ecu_map().values().copied().collect()
    }

    /// The ELM's name for the current protocol.
    pub fn protocol_name(&self) -> &'static str {
        self.protocol.elm_name()
    }

    /// The ELM's ID for the current protocol.
    pub fn protocol_id(&self) -> &'static str {
        self.protocol.elm_id()
    }

    /// Whether the adapter is in low power mode.
    pub fn is_low_power(&self) -> bool {
        self.low_power
    }

    /// Enters low power mode.
    ///
    /// Returns the status from the ELM327; 'OK' means low power mode is
    /// going to become active.
    pub fn low_power(&mut self) -> Option<Vec<String>> {
        if self.status == OBDStatus::NotConnected {
            tracing::info!("cannot enter low power when unconnected");
            return None;
        }

        let lines = self
            .send(b"ATLP", Some(Duration::from_secs(1)), ELM_LP_ACTIVE)
            .unwrap_or_default();

        if has_message(&lines, "OK") {
            tracing::debug!("Successfully entered low power mode");
            self.low_power = true;
        } else {
            tracing::debug!("Failed to enter low power mode");
        }

        Some(lines)
    }

    /// Exits low power mode.
    ///
    /// Sends a space to trigger the RS232 to wakeup, even if we aren't
    /// in low power mode, to ensure we will be able to leave it.
    pub fn normal_power(&mut self) -> Option<Vec<String>> {
        if self.status == OBDStatus::NotConnected {
            tracing::info!("cannot exit low power when unconnected");
            return None;
        }

        let lines = self.send(b" ", None, ELM_PROMPT).unwrap_or_default();

        // assume we woke up
        tracing::debug!("Successfully exited low power mode");
        self.low_power = false;

        Some(lines)
    }

    /// Resets the device and sets all attributes to unconnected states.
    pub fn close(&mut self) {
        self.status = OBDStatus::NotConnected;
        self.protocol = Protocol::new(Box::new(UnknownProtocol), &[]);

        if self.port.is_some() {
            tracing::info!("closing port");
            self.write_raw(b"ATZ");
            if let Some(mut port) = self.port.take() {
                port.close();
            }
        }
    }

    /// Sends the given command string and parses the response lines
    /// with the protocol object.
    ///
    /// An empty command string will re-trigger the previous command.
    /// Returns `None` when unconnected.
    pub fn send_and_parse(&mut self, cmd: &[u8]) -> Option<Vec<Message>> {
        if self.status == OBDStatus::NotConnected {
            tracing::info!("cannot send_and_parse() when unconnected");
            return None;
        }

        // check if we are in low power
        if self.low_power {
            self.normal_power();
        }

        let lines = match self.send(cmd, None, ELM_PROMPT) {
            Ok(lines) => lines,
            Err(e) => {
                tracing::error!("{e}");
                return None;
            }
        };

        Some(self.protocol.call(&lines))
    }

    /// Sets the protocol, either manually or via auto-detection.
    fn set_protocol(&mut self, protocol: Option<&str>) -> bool {
        match protocol {
            Some(p) => {
                if !SUPPORTED_PROTOCOLS.contains(&p) {
                    tracing::error!("{p} is not a valid protocol. Please use \"1\" through \"A\"");
                    return false;
                }
                self.manual_protocol(p)
            }
            None => self.auto_protocol(),
        }
    }

    fn manual_protocol(&mut self, protocol: &str) -> bool {
        let _ = self.send(format!("ATTP{protocol}").as_bytes(), None, ELM_PROMPT);
        let r0100 = self.send(b"0100", None, ELM_PROMPT).unwrap_or_default();

        if !has_message(&r0100, "UNABLE TO CONNECT") {
            // success, found the protocol
            self.protocol = Protocol::new(protocol_parser(protocol), &r0100);
            return true;
        }

        false
    }

    /// Attempts communication with the car.
    ///
    /// If no protocol is specified, protocols are tried with `ATTP`.
    /// Upon success, the appropriate protocol parser is loaded.
    fn auto_protocol(&mut self) -> bool {
        // try the ELM's auto protocol mode
        let _ = self.send(b"ATSP0", Some(Duration::from_secs(1)), ELM_PROMPT);

        // 0100 (first command, SEARCH protocols)
        let r0100 = self
            .send(b"0100", Some(Duration::from_secs(1)), ELM_PROMPT)
            .unwrap_or_default();
        if has_message(&r0100, "UNABLE TO CONNECT") {
            tracing::error!("Failed to query protocol 0100: unable to connect");
            return false;
        }

        // ATDPN (list protocol number)
        let r = self.send(b"ATDPN", None, ELM_PROMPT).unwrap_or_default();
        if r.len() != 1 {
            tracing::error!("Failed to retrieve current protocol");
            return false;
        }

        let mut p = r[0].clone();
        // suppress any "automatic" prefix
        if p.len() > 1 && p.starts_with('A') {
            p = p[1..].to_string();
        }

        // check if the protocol is something we know
        if SUPPORTED_PROTOCOLS.contains(&p.as_str()) {
            // jackpot, instantiate the corresponding protocol handler
            self.protocol = Protocol::new(protocol_parser(&p), &r0100);
            return true;
        }

        // an unknown protocol; try them one-by-one
        tracing::debug!("ELM responded with unknown protocol. Trying them one-by-one");
        for p in TRY_PROTOCOL_ORDER {
            let _ = self.send(format!("ATTP{p}").as_bytes(), None, ELM_PROMPT);
            let r0100 = self.send(b"0100", None, ELM_PROMPT).unwrap_or_default();
            if !has_message(&r0100, "UNABLE TO CONNECT") {
                // success, found the protocol
                self.protocol = Protocol::new(protocol_parser(p), &r0100);
                return true;
            }
        }

        // if we've come this far, then we have failed...
        tracing::error!("Failed to determine protocol");
        false
    }

    fn set_baudrate(&mut self, baud: Option<u32>) -> bool {
        match baud {
            None => {
                // when connecting to a pseudo terminal, don't bother with auto baud
                if self.port_name().starts_with("/dev/pts") {
                    tracing::debug!("Detected pseudo terminal, skipping baudrate setup");
                    true
                } else {
                    self.auto_baudrate()
                }
            }
            Some(baud) => self
                .port
                .as_mut()
                .map(|p| p.set_baud_rate(baud).is_ok())
                .unwrap_or(false),
        }
    }

    /// Detects the baud rate at which a connected ELM32x interface is
    /// operating. Returns boolean for success.
    fn auto_baudrate(&mut self) -> bool {
        let Some(port) = self.port.as_mut() else {
            return false;
        };

        // before we change the timeout, save the "normal" value
        let timeout = port.timeout();
        // we're only talking with the ELM, so things should go quickly
        let _ = port.set_timeout(self.timeout);

        for baud in TRY_BAUDS {
            if port.set_baud_rate(baud).is_err() {
                continue;
            }
            let _ = port.clear_buffers();

            // Send a nonsense command to get a prompt back from the scanner
            // (an empty command runs the risk of repeating a dangerous
            // command). The first character might get eaten if the interface
            // was busy, so write a second one (again so that the lone CR
            // doesn't repeat the previous command).
            //
            // All commands should be terminated with carriage return
            // according to ELM327 and STN11XX specifications.
            if port.write(b"\x7F\x7F\r").is_err() {
                continue;
            }
            let _ = port.flush();

            let mut response = [0u8; 1024];
            let n = port.read(&mut response).unwrap_or(0);
            tracing::debug!("Response from baud {baud}: {:?}", &response[..n]);

            // watch for the prompt character
            if response[..n].ends_with(b">") {
                tracing::debug!("Choosing baud {baud}");
                let _ = port.set_timeout(timeout); // reinstate our original timeout
                return true;
            }
        }

        tracing::debug!("Failed to choose baud");
        let _ = port.set_timeout(timeout); // reinstate our original timeout
        false
    }

    /// Unprotected send: writes the given string, then reads until the
    /// end marker (by default, the prompt) is seen, after an optional
    /// delay.
    fn send(
        &mut self,
        cmd: &[u8],
        delay: Option<Duration>,
        end_marker: &[u8],
    ) -> Result<Vec<String>, ElmError> {
        self.write_raw(cmd);

        let mut delayed = Duration::ZERO;
        if let Some(d) = delay {
            tracing::debug!("wait: {} seconds", d.as_secs_f64());
            std::thread::sleep(d);
            delayed += d;
        }

        let mut r = self.read_until(end_marker);
        while delayed < Duration::from_secs(1) && r.is_empty() {
            let d = Duration::from_millis(100);
            tracing::debug!("no response; wait: {} seconds", d.as_secs_f64());
            std::thread::sleep(d);
            delayed += d;
            r = self.read_until(end_marker);
        }
        Ok(r)
    }

    /// "Low-level" write of a string to the port, terminated with a
    /// carriage return per ELM327/STN11XX specifications.
    fn write_raw(&mut self, cmd: &[u8]) {
        let Some(port) = self.port.as_mut() else {
            tracing::info!("cannot perform write when unconnected");
            return;
        };

        let mut buf = cmd.to_vec();
        buf.push(b'\r');
        tracing::debug!("write: {:?}", String::from_utf8_lossy(&buf));

        let result = port
            .clear_buffers() // dump everything in the input buffer
            .and_then(|_| port.write(&buf))
            .and_then(|_| port.flush()); // wait for the output buffer to finish

        if let Err(e) = result {
            tracing::debug!("write error: {e}");
            self.status = OBDStatus::NotConnected;
            self.port = None;
            tracing::error!("Device disconnected while writing");
        }
    }

    /// "Low-level" read: accumulates characters until the end marker
    /// (by default, the prompt character) is seen. Returns a list of
    /// line strings.
    fn read_until(&mut self, end_marker: &[u8]) -> Vec<String> {
        let Some(port) = self.port.as_mut() else {
            tracing::info!("cannot perform read when unconnected");
            return Vec::new();
        };

        let mut buffer: Vec<u8> = Vec::new();

        loop {
            // retrieve as much data as possible
            let mut chunk = [0u8; 1024];
            let n = match port.read(&mut chunk) {
                Ok(n) => n,
                Err(e) => {
                    tracing::debug!("read error: {e}");
                    self.status = OBDStatus::NotConnected;
                    self.port = None;
                    tracing::error!("Device disconnected while reading");
                    return Vec::new();
                }
            };

            // if nothing was received
            if n == 0 {
                tracing::warn!("Failed to read port");
                break;
            }

            buffer.extend_from_slice(&chunk[..n]);

            // end on specified end-marker sequence
            if buffer.windows(end_marker.len()).any(|w| w == end_marker) {
                break;
            }
        }

        tracing::debug!("read: {:?}", String::from_utf8_lossy(&buffer));

        // clean out any null characters
        buffer.retain(|b| *b != 0);

        // remove the prompt character
        if buffer.ends_with(ELM_PROMPT) {
            buffer.truncate(buffer.len() - 1);
        }

        // convert bytes into a standard string, split into lines while
        // removing empty lines and trailing spaces
        String::from_utf8_lossy(&buffer)
            .split(['\r', '\n'])
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect()
    }

    /// Handles fatal failures: closes the serial port and logs.
    fn error(&mut self, msg: impl std::fmt::Display) {
        self.close();
        tracing::error!("{msg}");
    }
}

/// Whether the response lines indicate an 'OK' from the ELM.
fn is_ok(lines: &[String], expect_echo: bool) -> bool {
    if lines.is_empty() {
        return false;
    }
    if expect_echo {
        // don't test for the echo itself; allow the adapter to already
        // have echo disabled
        has_message(lines, "OK")
    } else {
        lines.len() == 1 && lines[0] == "OK"
    }
}

/// Whether any line contains `text`.
fn has_message(lines: &[String], text: &str) -> bool {
    lines.iter().any(|l| l.contains(text))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    /// A scripted fake port for driving the ELM327 without hardware.
    #[derive(Clone, Default)]
    pub(crate) struct FakePort {
        /// (response bytes, delay) pairs consumed per read
        script: Arc<Mutex<VecDeque<Vec<u8>>>>,
        written: Arc<Mutex<Vec<Vec<u8>>>>,
        baud: Arc<Mutex<u32>>,
        timeout: Arc<Mutex<Duration>>,
    }

    impl FakePort {
        pub(crate) fn new(script: Vec<Vec<u8>>) -> Self {
            FakePort {
                script: Arc::new(Mutex::new(VecDeque::from(script))),
                ..Default::default()
            }
        }

        pub(crate) fn written(&self) -> Vec<Vec<u8>> {
            self.written.lock().unwrap().clone()
        }
    }

    impl ElmPort for FakePort {
        fn port_name(&self) -> String {
            "/dev/fake".to_string()
        }

        fn baud_rate(&self) -> u32 {
            *self.baud.lock().unwrap()
        }

        fn set_baud_rate(&mut self, baud: u32) -> Result<(), ElmError> {
            *self.baud.lock().unwrap() = baud;
            Ok(())
        }

        fn timeout(&self) -> Duration {
            *self.timeout.lock().unwrap()
        }

        fn set_timeout(&mut self, timeout: Duration) -> Result<(), ElmError> {
            *self.timeout.lock().unwrap() = timeout;
            Ok(())
        }

        fn clear_buffers(&mut self) -> Result<(), ElmError> {
            Ok(())
        }

        fn write(&mut self, data: &[u8]) -> Result<(), ElmError> {
            self.written.lock().unwrap().push(data.to_vec());
            Ok(())
        }

        fn flush(&mut self) -> Result<(), ElmError> {
            Ok(())
        }

        fn read(&mut self, buf: &mut [u8]) -> Result<usize, ElmError> {
            let mut script = self.script.lock().unwrap();
            match script.pop_front() {
                Some(data) => {
                    let n = data.len().min(buf.len());
                    buf[..n].copy_from_slice(&data[..n]);
                    Ok(n)
                }
                None => Ok(0),
            }
        }

        fn close(&mut self) {}
    }

    #[test]
    fn is_ok_checks() {
        assert!(!is_ok(&[], false));
        assert!(is_ok(&["OK".to_string()], false));
        assert!(!is_ok(&["OK".to_string(), "junk".to_string()], false));
        assert!(is_ok(&["junk OK junk".to_string()], true));
        assert!(is_ok(&["OK".to_string()], true));
    }

    #[test]
    fn has_message_checks() {
        assert!(has_message(
            &["UNABLE TO CONNECT".to_string()],
            "UNABLE TO CONNECT"
        ));
        assert!(!has_message(&["OK".to_string()], "UNABLE TO CONNECT"));
    }

    #[test]
    fn read_until_accumulates_lines() {
        let mut elm = ELM327 {
            status: OBDStatus::ElmConnected,
            port: Some(Box::new(FakePort::new(vec![b"OK\r\r>".to_vec()]))),
            protocol: Protocol::new(Box::new(UnknownProtocol), &[]),
            low_power: false,
            timeout: Duration::from_secs(1),
        };

        let lines = elm.read_until(ELM_PROMPT);
        assert_eq!(lines, vec!["OK".to_string()]);
    }

    #[test]
    fn read_until_strips_nulls_and_prompt() {
        let mut elm = ELM327 {
            status: OBDStatus::ElmConnected,
            port: Some(Box::new(FakePort::new(vec![b"\x00ATZ\r\r\x00>".to_vec()]))),
            protocol: Protocol::new(Box::new(UnknownProtocol), &[]),
            low_power: false,
            timeout: Duration::from_secs(1),
        };

        let lines = elm.read_until(ELM_PROMPT);
        assert_eq!(lines, vec!["ATZ".to_string()]);
    }

    #[test]
    fn write_raw_appends_carriage_return() {
        let fake = FakePort::new(vec![]);
        let mut elm = ELM327 {
            status: OBDStatus::ElmConnected,
            port: Some(Box::new(fake.clone())),
            protocol: Protocol::new(Box::new(UnknownProtocol), &[]),
            low_power: false,
            timeout: Duration::from_secs(1),
        };

        elm.write_raw(b"ATE0");
        assert_eq!(fake.written(), vec![b"ATE0\r".to_vec()]);
    }

    #[test]
    fn send_returns_lines() {
        let fake = FakePort::new(vec![b"OK\r\r>".to_vec()]);
        let mut elm = ELM327 {
            status: OBDStatus::ElmConnected,
            port: Some(Box::new(fake)),
            protocol: Protocol::new(Box::new(UnknownProtocol), &[]),
            low_power: false,
            timeout: Duration::from_secs(1),
        };

        let lines = elm.send(b"ATE0", None, ELM_PROMPT).unwrap();
        assert_eq!(lines, vec!["OK".to_string()]);
    }

    #[test]
    fn send_and_parse_returns_messages() {
        // respond to "0100" with a CAN single frame
        let fake = FakePort::new(vec![b"7E8 06 41 00 BE 7F B8 13\r\r>".to_vec()]);
        let mut elm = ELM327 {
            status: OBDStatus::CarConnected,
            port: Some(Box::new(fake)),
            protocol: Protocol::new(Box::new(CanProtocol::Iso15765411bit500k), &[]),
            low_power: false,
            timeout: Duration::from_secs(1),
        };

        let messages = elm.send_and_parse(b"0100").unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].data, vec![0x41, 0x00, 0xBE, 0x7F, 0xB8, 0x13]);
    }

    #[test]
    fn send_and_parse_none_when_unconnected() {
        let mut elm = ELM327 {
            status: OBDStatus::NotConnected,
            port: None,
            protocol: Protocol::new(Box::new(UnknownProtocol), &[]),
            low_power: false,
            timeout: Duration::from_secs(1),
        };
        assert!(elm.send_and_parse(b"0100").is_none());
    }

    #[test]
    fn low_power_roundtrip() {
        let fake = FakePort::new(vec![b"OK".to_vec(), b">".to_vec()]);
        let mut elm = ELM327 {
            status: OBDStatus::CarConnected,
            port: Some(Box::new(fake)),
            protocol: Protocol::new(Box::new(UnknownProtocol), &[]),
            low_power: false,
            timeout: Duration::from_secs(1),
        };

        let lines = elm.low_power().unwrap();
        assert!(has_message(&lines, "OK"));
        assert!(elm.is_low_power());

        elm.normal_power().unwrap();
        assert!(!elm.is_low_power());
    }

    #[test]
    fn close_resets_state() {
        let fake = FakePort::new(vec![]);
        let mut elm = ELM327 {
            status: OBDStatus::CarConnected,
            port: Some(Box::new(fake)),
            protocol: Protocol::new(Box::new(CanProtocol::Iso15765411bit500k), &[]),
            low_power: false,
            timeout: Duration::from_secs(1),
        };

        elm.close();
        assert_eq!(elm.status(), OBDStatus::NotConnected);
        assert_eq!(elm.port_name(), "");
        assert_eq!(elm.protocol_id(), "");
    }

    #[test]
    fn protocol_parser_registry() {
        assert_eq!(protocol_parser("6").elm_id(), "6");
        assert_eq!(protocol_parser("A").elm_name(), "SAE J1939 (CAN 29/250)");
        assert_eq!(protocol_parser("1").elm_name(), "SAE J1850 PWM");
        assert_eq!(protocol_parser("?").elm_id(), "");
    }
}
