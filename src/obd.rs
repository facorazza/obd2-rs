//! OBD: the main connection class.
//!
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use crate::command::{OBDCommand, OBDResponse};
use crate::commands;
use crate::decoder::Value;
use crate::elm327::ELM327;
use crate::message::ecu_header;
use crate::util::{OBDStatus, scan_serial};

/// Class representing an OBD-II connection with its assorted
/// commands/sensors.
pub struct OBD {
    interface: Option<ELM327>,
    supported_commands: HashSet<&'static OBDCommand>,
    /// Global switch for disabling optimizations.
    fast: bool,
    timeout: Duration,
    /// Used for running the previous command with a CR.
    last_command: Vec<u8>,
    /// For comparing with the previously used header.
    last_header: &'static [u8],
    /// Keeps track of the number of return frames for each command.
    frame_counts: HashMap<&'static OBDCommand, usize>,
}

impl OBD {
    /// Connects to the car and loads the supported commands.
    ///
    /// Mirrors `OBD.__init__`: when `portstr` is `None` the serial
    /// ports are scanned, and after connecting the car's supported
    /// PIDs are queried.
    pub fn new(
        portstr: Option<&str>,
        baudrate: Option<u32>,
        protocol: Option<&str>,
        fast: bool,
        timeout: Duration,
        check_voltage: bool,
        start_low_power: bool,
    ) -> Self {
        tracing::info!("======================= obd2-rs =======================");
        let mut obd = OBD {
            interface: None,
            supported_commands: commands::base_commands().into_iter().collect(),
            fast,
            timeout,
            last_command: Vec::new(),
            last_header: ecu_header::ENGINE.as_bytes(),
            frame_counts: HashMap::new(),
        };
        obd.connect(portstr, baudrate, protocol, check_voltage, start_low_power);
        obd.load_commands();
        tracing::info!("======================================================");
        obd
    }

    /// Attempts to instantiate an ELM327 connection object.
    fn connect(
        &mut self,
        portstr: Option<&str>,
        baudrate: Option<u32>,
        protocol: Option<&str>,
        check_voltage: bool,
        start_low_power: bool,
    ) {
        match portstr {
            Some(port) => {
                tracing::info!("Explicit port defined");
                self.interface = Some(ELM327::new(
                    port,
                    baudrate,
                    protocol,
                    self.timeout,
                    check_voltage,
                    start_low_power,
                ));
            }
            None => {
                tracing::info!("Using scan_serial to select port");
                let port_names = scan_serial();
                tracing::info!("Available ports: {:?}", port_names);

                if port_names.is_empty() {
                    tracing::warn!("No OBD-II adapters found");
                    return;
                }

                for port in port_names {
                    tracing::info!("Attempting to use port: {}", port);
                    let elm = ELM327::new(
                        &port,
                        baudrate,
                        protocol,
                        self.timeout,
                        check_voltage,
                        start_low_power,
                    );

                    if elm.status() >= OBDStatus::ElmConnected {
                        self.interface = Some(elm);
                        break; // success! stop searching for serial
                    }
                }
            }
        }

        // if the connection failed, close it
        let failed = self
            .interface
            .as_ref()
            .is_none_or(|i| i.status() == OBDStatus::NotConnected);
        if failed {
            // the ELM327 class will report its own errors
            self.close();
        }
    }

    /// Queries for available PIDs, sets their support status, and
    /// compiles the list of supported commands.
    fn load_commands(&mut self) {
        if self.status() != OBDStatus::CarConnected {
            tracing::warn!("Cannot load commands: No connection to car");
            return;
        }

        tracing::info!("querying for supported commands");
        for get in commands::pid_getters() {
            // PID listing commands should sequentially become supported
            // Mode 1 PID 0 is assumed to always be supported
            if !self.test_cmd(get, false) {
                continue;
            }

            let response = self.query(get, false);

            if response.is_null() {
                tracing::info!("No valid data for PID listing command: {}", get.name);
                continue;
            }

            // loop through PIDs bit-array
            let Some(Value::BitArray(bits)) = response.value else {
                continue;
            };
            let mode = get.mode().expect("pid getter has a mode");
            let base_pid = get.pid().expect("pid getter has a pid");

            for i in 0..bits.len() {
                if !bits.get(i) {
                    continue;
                }

                let pid = base_pid + i as u16 + 1;

                if let Some(cmd) = commands::get(mode, pid) {
                    self.supported_commands.insert(cmd);
                }

                // set support for mode 2 commands
                if mode == 1 {
                    if let Some(cmd) = commands::get(2, pid) {
                        self.supported_commands.insert(cmd);
                    }
                }
            }
        }

        tracing::info!(
            "finished querying with {} commands supported",
            self.supported_commands.len()
        );
    }

    /// Sends an `AT SH` command when the header changes.
    fn set_header(&mut self, header: &'static [u8]) {
        if header == self.last_header {
            return;
        }

        let header_str = String::from_utf8_lossy(header);
        let cmd = format!("AT SH {} ", header_str);
        let r = self
            .interface
            .as_mut()
            .unwrap()
            .send_and_parse(cmd.as_bytes());

        let Some(msgs) = r else {
            tracing::info!("Set Header ('AT SH {}') did not return data", header_str);
            return;
        };
        let joined = msgs.iter().map(|m| m.raw()).collect::<Vec<_>>().join("\n");
        if joined != "OK" {
            tracing::info!("Set Header ('AT SH {}') did not return 'OK'", header_str);
            return;
        }

        self.last_header = header;
    }

    /// Closes the connection and clears `supported_commands`.
    pub fn close(&mut self) {
        self.supported_commands.clear();

        if self.interface.is_some() {
            tracing::info!("Closing connection");
            self.set_header(ecu_header::ENGINE.as_bytes());
            self.interface.as_mut().unwrap().close();
            self.interface = None;
        }
    }

    /// Returns the OBD connection status.
    pub fn status(&self) -> OBDStatus {
        self.interface
            .as_ref()
            .map_or(OBDStatus::NotConnected, |i| i.status())
    }

    /// Enters low power mode.
    pub fn low_power(&mut self) -> OBDStatus {
        match self.interface.as_mut() {
            Some(i) => {
                i.low_power();
                i.status()
            }
            None => OBDStatus::NotConnected,
        }
    }

    /// Exits low power mode.
    pub fn normal_power(&mut self) -> OBDStatus {
        match self.interface.as_mut() {
            Some(i) => {
                i.normal_power();
                i.status()
            }
            None => OBDStatus::NotConnected,
        }
    }

    /// Returns the name of the protocol being used by the ELM327.
    pub fn protocol_name(&self) -> &'static str {
        self.interface.as_ref().map_or("", |i| i.protocol_name())
    }

    /// Returns the ID of the protocol being used by the ELM327.
    pub fn protocol_id(&self) -> &'static str {
        self.interface.as_ref().map_or("", |i| i.protocol_id())
    }

    /// Returns the name of the currently connected port.
    pub fn port_name(&self) -> String {
        self.interface
            .as_ref()
            .map_or_else(String::new, |i| i.port_name())
    }

    /// True when a connection with the car was made.
    ///
    /// Note: returns `false` when the status is only
    /// `OBDStatus::ElmConnected`.
    pub fn is_connected(&self) -> bool {
        self.status() == OBDStatus::CarConnected
    }

    /// True when the given command is supported by the car.
    pub fn supports(&self, cmd: &'static OBDCommand) -> bool {
        self.supported_commands.contains(cmd)
    }

    /// True when a command will be sent without using `force = true`.
    pub fn test_cmd(&self, cmd: &'static OBDCommand, warn: bool) -> bool {
        // test if the command is supported
        if !self.supports(cmd) {
            if warn {
                tracing::warn!("'{}' is not supported", cmd.name);
            }
            return false;
        }

        // mode 06 is only implemented for the CAN protocols
        if cmd.mode() == Some(6) && !matches!(self.protocol_id(), "6" | "7" | "8" | "9") {
            if warn {
                tracing::warn!("Mode 06 commands are only supported over CAN protocols");
            }
            return false;
        }

        true
    }

    /// Primary API function: sends commands to the car, and protects
    /// against sending unsupported commands.
    pub fn query(&mut self, cmd: &'static OBDCommand, force: bool) -> OBDResponse<'static> {
        if self.status() == OBDStatus::NotConnected {
            tracing::warn!("Query failed, no connection available");
            return OBDResponse::null(cmd);
        }

        // if the user forces, skip all checks
        if !force && !self.test_cmd(cmd, true) {
            return OBDResponse::null(cmd);
        }

        self.set_header(cmd.header);

        tracing::info!("Sending command: {}", cmd.name);
        let cmd_string = self.build_command_string(cmd);
        let messages = self.interface.as_mut().unwrap().send_and_parse(&cmd_string);

        // if we're sending a new command, note it
        // first check that the current command WASN'T sent as an empty CR
        // (CR is added by the ELM327 class)
        if !cmd_string.is_empty() {
            self.last_command = cmd_string;
        }

        // if we don't already know how many frames this command returns,
        // log it, so we can specify it next time
        if !self.frame_counts.contains_key(cmd) {
            let count = messages
                .as_ref()
                .map_or(0, |ms| ms.iter().map(|m| m.frames.len()).sum());
            self.frame_counts.insert(cmd, count);
        }

        let Some(messages) = messages else {
            tracing::info!("No valid OBD Messages returned");
            return OBDResponse::null(cmd);
        };

        cmd.call(&messages) // compute a response object
    }

    /// Assembles the appropriate command string.
    fn build_command_string(&self, cmd: &'static OBDCommand) -> Vec<u8> {
        let mut cmd_string = cmd.command.to_vec();

        // if we know the number of frames that this command returns,
        // only wait for exactly that number. This avoids some harsh
        // timeouts from the ELM, thus speeding up queries.
        if self.fast && cmd.fast {
            if let Some(count) = self.frame_counts.get(cmd) {
                cmd_string.extend_from_slice(count.to_string().as_bytes());
            }
        }

        // if we sent this last time, just send a CR
        // (CR is added by the ELM327 class)
        if self.fast && cmd_string == self.last_command {
            cmd_string.clear();
        }

        cmd_string
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands_table;
    use crate::elm327::tests::FakePort;
    use crate::message::Protocol;
    use crate::protocol::CanProtocol;

    /// An ELM327 wired to a scripted fake port, already "connected".
    fn fake_elm(script: Vec<Vec<u8>>, protocol: CanProtocol) -> ELM327 {
        ELM327::from_parts(
            OBDStatus::CarConnected,
            Box::new(FakePort::new(script)),
            Protocol::new(Box::new(protocol), &[]),
            Duration::from_secs(1),
        )
    }

    fn obd_with(elm: ELM327) -> OBD {
        OBD {
            interface: Some(elm),
            supported_commands: commands::base_commands().into_iter().collect(),
            fast: true,
            timeout: Duration::from_millis(100),
            last_command: Vec::new(),
            last_header: ecu_header::ENGINE.as_bytes(),
            frame_counts: HashMap::new(),
        }
    }

    #[test]
    fn status_reflects_interface() {
        let obd = obd_with(fake_elm(vec![], CanProtocol::Iso15765411bit500k));
        assert_eq!(obd.status(), OBDStatus::CarConnected);
        assert!(obd.is_connected());
        assert_eq!(obd.protocol_id(), "6");
        assert_eq!(obd.port_name(), "/dev/fake");

        let mut obd = obd_with(fake_elm(vec![], CanProtocol::Iso15765411bit500k));
        obd.interface = None;
        assert_eq!(obd.status(), OBDStatus::NotConnected);
        assert!(!obd.is_connected());
        assert_eq!(obd.protocol_id(), "");
        assert_eq!(obd.port_name(), "");
    }

    #[test]
    fn test_cmd_checks_support_and_can_only_mode6() {
        let obd = obd_with(fake_elm(vec![], CanProtocol::Iso15765411bit500k));
        let rpm = commands_table::MODE1[12].as_ref().unwrap();
        // RPM is not in the base set, so unsupported
        assert!(!obd.test_cmd(rpm, false));

        let elm_version = commands::by_name("ELM_VERSION").unwrap();
        assert!(obd.test_cmd(elm_version, false));

        // mode 06 over CAN protocol "6" is allowed
        let monitor = commands_table::MODE6[1].as_ref().unwrap();
        let mut obd = obd_with(fake_elm(vec![], CanProtocol::Iso15765411bit500k));
        obd.supported_commands.insert(monitor);
        assert!(obd.test_cmd(monitor, false));
    }

    #[test]
    fn test_cmd_rejects_mode6_over_legacy() {
        let mut obd = obd_with(fake_elm(vec![], CanProtocol::SaeJ1939));
        // SAE J1939 is protocol "A", not CAN 6/7/8/9
        assert_eq!(obd.protocol_id(), "A");
        let monitor = commands_table::MODE6[1].as_ref().unwrap();
        obd.supported_commands.insert(monitor);
        assert!(!obd.test_cmd(monitor, false));
    }

    #[test]
    fn build_command_string_appends_frame_count() {
        let mut obd = obd_with(fake_elm(vec![], CanProtocol::Iso15765411bit500k));
        let rpm = commands_table::MODE1[12].as_ref().unwrap();
        obd.frame_counts.insert(rpm, 1);

        // fast + fast command + known count -> "010C1"
        let s = obd.build_command_string(rpm);
        assert_eq!(s, b"010C1");

        // sending the same command again -> empty (CR only)
        obd.last_command = s.clone();
        let s2 = obd.build_command_string(rpm);
        assert!(s2.is_empty());

        // non-fast command never gets the count appended
        let elm_version = commands::by_name("ELM_VERSION").unwrap();
        assert_eq!(obd.build_command_string(elm_version), b"ATI");
    }

    #[test]
    fn set_header_sends_at_sh() {
        // header change -> "AT SH 7E1 " is sent and remembered
        let script = vec![b"OK\r\r>".to_vec()];
        let mut obd = obd_with(fake_elm(script, CanProtocol::Iso15765411bit500k));
        obd.set_header(b"7E1");
        assert_eq!(obd.last_header, b"7E1");

        // unchanged header -> nothing sent
        let mut obd = obd_with(fake_elm(vec![], CanProtocol::Iso15765411bit500k));
        obd.set_header(b"7E0");
        assert_eq!(obd.last_header, b"7E0");

        // non-OK reply -> header not remembered
        let script = vec![b"?\r\r>".to_vec()];
        let mut obd = obd_with(fake_elm(script, CanProtocol::Iso15765411bit500k));
        obd.set_header(b"7E2");
        assert_eq!(obd.last_header, b"7E0");
    }

    #[test]
    fn query_sets_header_and_counts_frames() {
        // header is already 7E0, so only the PID replies are scripted
        let script = vec![
            b"7E8 06 41 0C 1A F8 00 00\r\r>".to_vec(),
            b"7E8 06 41 0C 1A F8 00 00\r\r>".to_vec(),
        ];
        let mut obd = obd_with(fake_elm(script, CanProtocol::Iso15765411bit500k));
        let rpm = commands_table::MODE1[12].as_ref().unwrap();
        obd.supported_commands.insert(rpm);

        // first query: frame count unknown, so no digit appended
        let r = obd.query(rpm, false);
        assert!(!r.is_null());
        assert_eq!(obd.frame_counts.get(rpm), Some(&1));
        assert_eq!(obd.last_command, b"010C");

        // second query: count is known, so "1" is appended
        let r = obd.query(rpm, false);
        assert!(!r.is_null());
        assert_eq!(obd.last_command, b"010C1");
    }

    #[test]
    fn query_returns_null_when_not_connected() {
        let mut obd = obd_with(fake_elm(vec![], CanProtocol::Iso15765411bit500k));
        obd.interface = None;
        let rpm = commands_table::MODE1[12].as_ref().unwrap();
        let r = obd.query(rpm, false);
        assert!(r.is_null());
    }

    #[test]
    fn query_returns_null_for_unsupported_command() {
        let mut obd = obd_with(fake_elm(vec![], CanProtocol::Iso15765411bit500k));
        let rpm = commands_table::MODE1[12].as_ref().unwrap();
        // RPM not in supported_commands
        let r = obd.query(rpm, false);
        assert!(r.is_null());

        // force skips the check
        let script = vec![b"7E8 06 41 0C 1A F8 00 00\r\r>".to_vec()];
        let mut obd = obd_with(fake_elm(script, CanProtocol::Iso15765411bit500k));
        let r = obd.query(rpm, true);
        assert!(!r.is_null());
    }

    #[test]
    fn load_commands_discovers_pids() {
        // PIDS_A (0100) reply: 6 data bytes. The pid decoder reads
        // data[2..] = 80 00 00 00; bit 7 of byte 0 -> PID 01.
        let script = vec![b"7E8 06 41 00 80 00 00 00\r\r>".to_vec()];
        let mut obd = obd_with(fake_elm(script, CanProtocol::Iso15765411bit500k));
        obd.load_commands();

        // PID 01 (STATUS) should now be supported
        let status = commands_table::MODE1[1].as_ref().unwrap();
        assert!(obd.supports(status));
        // PID 02 (FREEZE_DTC) should not
        let freeze = commands_table::MODE1[2].as_ref().unwrap();
        assert!(!obd.supports(freeze));
    }
}
