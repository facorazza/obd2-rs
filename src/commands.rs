//! The command registry.
//!
//! Wraps the generated [`commands_table`] with lookup helpers:
//! [`base_commands`], [`pid_getters`], [`has_pid`] and [`get`].
//!
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::command::{Decoder, OBDCommand};
use crate::commands_table;

/// Looks up a command by mode and PID (e.g. `get(1, 12)` for RPM).
///
/// Returns `None` for reserved PIDs and for modes without a table
/// (0, 5, 8), mirroring `Commands.__getitem__` + `has_pid`.
pub fn get(mode: u8, pid: u16) -> Option<&'static OBDCommand> {
    let table = match mode {
        1 => commands_table::MODE1,
        2 => commands_table::MODE2,
        3 => commands_table::MODE3,
        4 => commands_table::MODE4,
        6 => commands_table::MODE6,
        7 => commands_table::MODE7,
        9 => commands_table::MODE9,
        _ => return None,
    };
    table.get(pid as usize).and_then(|c| c.as_ref())
}

/// Looks up a command by name (e.g. `"RPM"`), mirroring
/// `Commands.__getitem__` for strings.
pub fn by_name(name: &str) -> Option<&'static OBDCommand> {
    all().find(|c| c.name == name)
}

/// Iterates over every defined command (all modes plus MISC).
pub fn all() -> impl Iterator<Item = &'static OBDCommand> {
    commands_table::MODE1
        .iter()
        .chain(commands_table::MODE2)
        .chain(commands_table::MODE3)
        .chain(commands_table::MODE4)
        .chain(commands_table::MODE6)
        .chain(commands_table::MODE7)
        .chain(commands_table::MODE9)
        .chain(commands_table::MISC)
        .filter_map(|c| c.as_ref())
}

/// The commands that should always be supported by the ELM327,
/// mirroring `Commands.base_commands`.
pub fn base_commands() -> Vec<&'static OBDCommand> {
    [
        "PIDS_A",
        "PIDS_9A",
        "MIDS_A",
        "GET_DTC",
        "CLEAR_DTC",
        "GET_CURRENT_DTC",
        "ELM_VERSION",
        "ELM_VOLTAGE",
    ]
    .iter()
    .map(|name| by_name(name).expect("base command missing from table"))
    .collect()
}

/// The PID-listing commands (decoder == `pid`), used to discover
/// which PIDs the car supports, mirroring `Commands.pid_getters`.
pub fn pid_getters() -> Vec<&'static OBDCommand> {
    all().filter(|c| c.decoder == Decoder::Pid).collect()
}

/// True when the given mode/PID has a defined (non-reserved) command,
/// mirroring `Commands.has_pid`.
pub fn has_pid(mode: u8, pid: u16) -> bool {
    get(mode, pid).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_by_mode_and_pid() {
        let rpm = get(1, 12).expect("RPM is PID 0x0C");
        assert_eq!(rpm.name, "RPM");
        assert_eq!(get(1, 0).map(|c| c.name), Some("PIDS_A"));
        // reserved PID (mode 06 index 17 is None)
        assert!(get(6, 17).is_none());
        // modes without tables
        assert!(get(0, 0).is_none());
        assert!(get(5, 0).is_none());
        assert!(get(8, 0).is_none());
        // out of range
        assert!(get(1, 0xFFFF).is_none());
    }

    #[test]
    fn by_name_finds_commands() {
        assert_eq!(by_name("RPM").map(|c| c.name), Some("RPM"));
        assert_eq!(by_name("ELM_VERSION").map(|c| c.name), Some("ELM_VERSION"));
        assert!(by_name("NOT_A_COMMAND").is_none());
    }

    #[test]
    fn base_commands_are_all_present() {
        let base = base_commands();
        assert_eq!(base.len(), 8);
        let names: Vec<&str> = base.iter().map(|c| c.name).collect();
        for expected in [
            "PIDS_A",
            "PIDS_9A",
            "MIDS_A",
            "GET_DTC",
            "CLEAR_DTC",
            "GET_CURRENT_DTC",
            "ELM_VERSION",
            "ELM_VOLTAGE",
        ] {
            assert!(names.contains(&expected), "missing {expected}");
        }
    }

    #[test]
    fn pid_getters_are_pid_decoders() {
        let getters = pid_getters();
        assert!(!getters.is_empty());
        for g in &getters {
            assert_eq!(g.decoder, Decoder::Pid);
        }
        // PIDS_A (0100) and PIDS_9A (0900) must be among them
        let names: Vec<&str> = getters.iter().map(|c| c.name).collect();
        assert!(names.contains(&"PIDS_A"));
        assert!(names.contains(&"PIDS_9A"));
    }

    #[test]
    fn has_pid_matches_get() {
        assert!(has_pid(1, 12));
        assert!(!has_pid(6, 17));
        assert!(!has_pid(0, 0));
        assert!(!has_pid(1, 0xFFFF));
    }
}
