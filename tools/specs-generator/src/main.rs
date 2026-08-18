//! specs-generator: regenerates the data tables in `src/` from the raw
//! references under `tools/specs-generator/sources/`.
//!
//! Usage:
//!   specs-generator <sources-dir> <output-dir>
//!
//! Emits `dtc_table.rs` (SAE J2012 DTCs) and `commands_table.rs`
//! (SAE J1979 PIDs, modes 01/02/03/04/06/07/09 + ELM327 commands).
//! See `docs/tables.md` for full provenance of the sources.
//!
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

use clap::Parser;

/// Command-line arguments for the table generator.
#[derive(Parser)]
#[command(about = "Regenerates obd2-rs command/DTC tables from the SAE J1979/J2012 references")]
struct Cli {
    /// Directory containing the raw source references
    sources: PathBuf,
    /// Directory to write the generated tables into
    out_dir: PathBuf,
}

// Command model

#[derive(Clone)]
struct Command {
    name: String,
    desc: String,
    command: String,
    bytes: u32,
    decoder: String,
    ecu: String,
    fast: bool,
}

type Table = Vec<Option<Command>>;

/// All command tables of the library.
struct Spec {
    mode1: Table,
    mode3: Table,
    mode4: Table,
    mode6: Table,
    mode7: Table,
    mode9: Table,
    misc: Table,
}

// Spec building from raw sources

/// PID hex -> (command name, decoder). The decoder follows the SAE J1979
/// formula column; see docs/tables.md for the mapping rules.
const MODE1_MAP: &[(u8, &str, &str)] = &[
    (0x00, "PIDS_A", "Pid"),
    (0x01, "STATUS", "Status"),
    (0x02, "FREEZE_DTC", "SingleDtc"),
    (0x03, "FUEL_STATUS", "FuelStatus"),
    (0x04, "ENGINE_LOAD", "Percent"),
    (0x05, "COOLANT_TEMP", "Temp"),
    (0x06, "SHORT_FUEL_TRIM_1", "PercentCentered"),
    (0x07, "LONG_FUEL_TRIM_1", "PercentCentered"),
    (0x08, "SHORT_FUEL_TRIM_2", "PercentCentered"),
    (0x09, "LONG_FUEL_TRIM_2", "PercentCentered"),
    (0x0A, "FUEL_PRESSURE", "FuelPressure"),
    (0x0B, "INTAKE_PRESSURE", "Pressure"),
    (0x0C, "RPM", "Uas(0x07)"),
    (0x0D, "SPEED", "Uas(0x09)"),
    (0x0E, "TIMING_ADVANCE", "TimingAdvance"),
    (0x0F, "INTAKE_TEMP", "Temp"),
    (0x10, "MAF", "Uas(0x27)"),
    (0x11, "THROTTLE_POS", "Percent"),
    (0x12, "AIR_STATUS", "AirStatus"),
    (0x13, "O2_SENSORS", "O2Sensors"),
    (0x14, "O2_B1S1", "SensorVoltage"),
    (0x15, "O2_B1S2", "SensorVoltage"),
    (0x16, "O2_B1S3", "SensorVoltage"),
    (0x17, "O2_B1S4", "SensorVoltage"),
    (0x18, "O2_B2S1", "SensorVoltage"),
    (0x19, "O2_B2S2", "SensorVoltage"),
    (0x1A, "O2_B2S3", "SensorVoltage"),
    (0x1B, "O2_B2S4", "SensorVoltage"),
    (0x1C, "OBD_COMPLIANCE", "ObdCompliance"),
    (0x1D, "O2_SENSORS_ALT", "O2SensorsAlt"),
    (0x1E, "AUX_INPUT_STATUS", "AuxInputStatus"),
    (0x1F, "RUN_TIME", "Uas(0x12)"),
    (0x20, "PIDS_B", "Pid"),
    (0x21, "DISTANCE_W_MIL", "Uas(0x25)"),
    (0x22, "FUEL_RAIL_PRESSURE_VAC", "Uas(0x19)"),
    (0x23, "FUEL_RAIL_PRESSURE_DIRECT", "Uas(0x1B)"),
    (0x24, "O2_S1_WR_VOLTAGE", "SensorVoltageBig"),
    (0x25, "O2_S2_WR_VOLTAGE", "SensorVoltageBig"),
    (0x26, "O2_S3_WR_VOLTAGE", "SensorVoltageBig"),
    (0x27, "O2_S4_WR_VOLTAGE", "SensorVoltageBig"),
    (0x28, "O2_S5_WR_VOLTAGE", "SensorVoltageBig"),
    (0x29, "O2_S6_WR_VOLTAGE", "SensorVoltageBig"),
    (0x2A, "O2_S7_WR_VOLTAGE", "SensorVoltageBig"),
    (0x2B, "O2_S8_WR_VOLTAGE", "SensorVoltageBig"),
    (0x2C, "COMMANDED_EGR", "Percent"),
    (0x2D, "EGR_ERROR", "PercentCentered"),
    (0x2E, "EVAPORATIVE_PURGE", "Percent"),
    (0x2F, "FUEL_LEVEL", "Percent"),
    (0x30, "WARMUPS_SINCE_DTC_CLEAR", "Uas(0x01)"),
    (0x31, "DISTANCE_SINCE_DTC_CLEAR", "Uas(0x25)"),
    (0x32, "EVAP_VAPOR_PRESSURE", "EvapPressure"),
    (0x33, "BAROMETRIC_PRESSURE", "Pressure"),
    (0x34, "O2_S1_WR_CURRENT", "CurrentCentered"),
    (0x35, "O2_S2_WR_CURRENT", "CurrentCentered"),
    (0x36, "O2_S3_WR_CURRENT", "CurrentCentered"),
    (0x37, "O2_S4_WR_CURRENT", "CurrentCentered"),
    (0x38, "O2_S5_WR_CURRENT", "CurrentCentered"),
    (0x39, "O2_S6_WR_CURRENT", "CurrentCentered"),
    (0x3A, "O2_S7_WR_CURRENT", "CurrentCentered"),
    (0x3B, "O2_S8_WR_CURRENT", "CurrentCentered"),
    (0x3C, "CATALYST_TEMP_B1S1", "Uas(0x16)"),
    (0x3D, "CATALYST_TEMP_B2S1", "Uas(0x16)"),
    (0x3E, "CATALYST_TEMP_B1S2", "Uas(0x16)"),
    (0x3F, "CATALYST_TEMP_B2S2", "Uas(0x16)"),
    (0x40, "PIDS_C", "Pid"),
    (0x41, "STATUS_DRIVE_CYCLE", "Status"),
    (0x42, "CONTROL_MODULE_VOLTAGE", "Uas(0x0B)"),
    (0x43, "ABSOLUTE_LOAD", "AbsoluteLoad"),
    (0x44, "COMMANDED_EQUIV_RATIO", "Uas(0x1E)"),
    (0x45, "RELATIVE_THROTTLE_POS", "Percent"),
    (0x46, "AMBIENT_AIR_TEMP", "Temp"),
    (0x47, "THROTTLE_POS_B", "Percent"),
    (0x48, "THROTTLE_POS_C", "Percent"),
    (0x49, "ACCELERATOR_POS_D", "Percent"),
    (0x4A, "ACCELERATOR_POS_E", "Percent"),
    (0x4B, "ACCELERATOR_POS_F", "Percent"),
    (0x4C, "THROTTLE_ACTUATOR", "Percent"),
    (0x4D, "RUN_TIME_MIL", "Uas(0x34)"),
    (0x4E, "TIME_SINCE_DTC_CLEARED", "Uas(0x34)"),
    (0x4F, "MAX_VALUES", "Drop"),
    (0x50, "MAX_MAF", "MaxMaf"),
    (0x51, "FUEL_TYPE", "FuelType"),
    (0x52, "ETHANOL_PERCENT", "Percent"),
    (0x53, "EVAP_VAPOR_PRESSURE_ABS", "AbsEvapPressure"),
    (0x54, "EVAP_VAPOR_PRESSURE_ALT", "EvapPressureAlt"),
    (0x55, "SHORT_O2_TRIM_B1", "PercentCentered"),
    (0x56, "LONG_O2_TRIM_B1", "PercentCentered"),
    (0x57, "SHORT_O2_TRIM_B2", "PercentCentered"),
    (0x58, "LONG_O2_TRIM_B2", "PercentCentered"),
    (0x59, "FUEL_RAIL_PRESSURE_ABS", "Uas(0x1B)"),
    (0x5A, "RELATIVE_ACCEL_POS", "Percent"),
    (0x5B, "HYBRID_BATTERY_REMAINING", "Percent"),
    (0x5C, "OIL_TEMP", "Temp"),
    (0x5D, "FUEL_INJECT_TIMING", "InjectTiming"),
    (0x5E, "FUEL_RATE", "FuelRate"),
    (0x5F, "EMISSION_REQ", "Drop"),
];

/// Parses a DTC line: `P0101 - Mass or Volume Air Flow Sensor A ...`.
fn parse_dtc_line(line: &str) -> Option<(String, String)> {
    let line = line.trim();
    let bytes = line.as_bytes();
    if bytes.len() < 7 || !matches!(bytes[0], b'P' | b'B' | b'C' | b'U') {
        return None;
    }
    let (code, rest) = line.split_at(5);
    if !code[1..].bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let desc = rest.trim().strip_prefix('-')?.trim();
    if desc.is_empty() {
        return None;
    }
    Some((code.to_string(), desc.to_string()))
}

/// Wal33D/dtc-database (MIT), see docs/tables.md. First description wins.
fn load_dtcs(sources: &Path) -> Vec<(String, String)> {
    let mut dtcs = BTreeMap::new();
    for cat in ["p", "u", "c", "b"] {
        let text = fs::read_to_string(sources.join(format!("{cat}_codes.txt")))
            .unwrap_or_else(|e| panic!("failed to read {cat}_codes.txt: {e}"));
        for line in text.lines() {
            if let Some((code, desc)) = parse_dtc_line(line) {
                dtcs.entry(code).or_insert(desc);
            }
        }
    }
    dtcs.into_iter().collect()
}

/// Parses a Wikipedia PID line: `0C | bytes=2 | Engine speed | formula: ...`.
fn parse_wiki_pid_line(line: &str) -> Option<(u8, u32, String)> {
    let (pid, rest) = line.split_once(" | ")?;
    if pid.len() != 2 || !pid.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let (bytes, rest) = rest.strip_prefix("bytes=")?.split_once(" | ")?;
    let bytes: u32 = bytes.parse().ok()?;
    let (desc, _formula) = rest.split_once(" | formula: ")?;
    Some((u8::from_str_radix(pid, 16).ok()?, bytes, desc.to_string()))
}

/// Mode 01 from the Wikipedia mirror; the Wikipedia byte count excludes
/// the mode and PID bytes, which are part of our command.
fn build_mode1(sources: &Path) -> Table {
    let mut table: Table = vec![None; 96];
    let text = fs::read_to_string(sources.join("pids_wikipedia.txt"))
        .unwrap_or_else(|e| panic!("failed to read pids_wikipedia.txt: {e}"));
    for line in text.lines() {
        let Some((pid, bytes, desc)) = parse_wiki_pid_line(line) else {
            continue;
        };
        let Some((name, decoder)) = MODE1_MAP
            .iter()
            .find(|(p, _, _)| *p == pid)
            .map(|(_, name, decoder)| (name, decoder))
        else {
            continue;
        };
        table[pid as usize] = Some(Command {
            name: name.to_string(),
            desc,
            command: format!("01{pid:02X}"),
            bytes: bytes + 2,
            decoder: decoder.to_string(),
            ecu: "ENGINE".to_string(),
            fast: true,
        });
    }
    table
}

fn mode6_entry(mid: usize, name: &str, desc: &str) -> Command {
    Command {
        name: name.to_string(),
        desc: desc.to_string(),
        command: format!("06{mid:02X}"),
        bytes: 0,
        decoder: "Monitor".to_string(),
        ecu: "ALL".to_string(),
        fast: false,
    }
}

/// Mode 06 test IDs: the standard SAE J1979 MID assignments.
fn build_mode6() -> Table {
    let mut table: Table = vec![None; 178]; // MIDs 0x00..=0xB1
    // O2 sensor monitors, 4 banks x 4 sensors.
    for b in 1..=4 {
        for s in 1..=4 {
            let mid = (b - 1) * 4 + s;
            table[mid] = Some(mode6_entry(
                mid,
                &format!("MONITOR_O2_B{b}S{s}"),
                &format!("O2 Sensor Monitor Bank {b} - Sensor {s}"),
            ));
        }
    }
    // Catalyst, EGR, VVT monitors per bank.
    for b in 1..=4 {
        table[0x20 + b] = Some(mode6_entry(
            0x20 + b,
            &format!("MONITOR_CATALYST_B{b}"),
            &format!("Catalyst Monitor Bank {b}"),
        ));
        table[0x30 + b] = Some(mode6_entry(
            0x30 + b,
            &format!("MONITOR_EGR_B{b}"),
            &format!("EGR Monitor Bank {b}"),
        ));
        table[0x34 + b] = Some(mode6_entry(
            0x34 + b,
            &format!("MONITOR_VVT_B{b}"),
            &format!("VVT Monitor Bank {b}"),
        ));
    }
    // EVAP leak monitors (SAE J1979 orifice sizes) and purge flow.
    let evap = [
        (0x39, "MONITOR_EVAP_150", "EVAP Monitor (Cap Off / 0.150\")"),
        (0x3A, "MONITOR_EVAP_090", "EVAP Monitor (0.090\")"),
        (0x3B, "MONITOR_EVAP_040", "EVAP Monitor (0.040\")"),
        (0x3C, "MONITOR_EVAP_020", "EVAP Monitor (0.020\")"),
    ];
    for (mid, name, desc) in evap {
        table[mid] = Some(mode6_entry(mid, name, desc));
    }
    table[0x3D] = Some(mode6_entry(
        0x3D,
        "MONITOR_PURGE_FLOW",
        "Purge Flow Monitor",
    ));
    // O2 sensor heater monitors, 4 banks x 4 sensors.
    for b in 1..=4 {
        for s in 1..=4 {
            let mid = 0x40 + (b - 1) * 4 + s;
            table[mid] = Some(mode6_entry(
                mid,
                &format!("MONITOR_O2_HEATER_B{b}S{s}"),
                &format!("O2 Sensor Heater Monitor Bank {b} - Sensor {s}"),
            ));
        }
    }
    // Heated catalyst, secondary air, fuel system per bank.
    for b in 1..=4 {
        table[0x60 + b] = Some(mode6_entry(
            0x60 + b,
            &format!("MONITOR_HEATED_CATALYST_B{b}"),
            &format!("Heated Catalyst Monitor Bank {b}"),
        ));
        table[0x70 + b] = Some(mode6_entry(
            0x70 + b,
            &format!("MONITOR_SECONDARY_AIR_{b}"),
            &format!("Secondary Air Monitor {b}"),
        ));
        table[0x80 + b] = Some(mode6_entry(
            0x80 + b,
            &format!("MONITOR_FUEL_SYSTEM_B{b}"),
            &format!("Fuel System Monitor Bank {b}"),
        ));
    }
    // Boost, NOx absorber/catalyst, PM filter (two banks).
    for b in 1..=2 {
        table[0x84 + b] = Some(mode6_entry(
            0x84 + b,
            &format!("MONITOR_BOOST_PRESSURE_B{b}"),
            &format!("Boost Pressure Control Monitor Bank {b}"),
        ));
        table[0x8F + b] = Some(mode6_entry(
            0x8F + b,
            &format!("MONITOR_NOX_ABSORBER_B{b}"),
            &format!("NOx Absorber Monitor Bank {b}"),
        ));
        table[0x97 + b] = Some(mode6_entry(
            0x97 + b,
            &format!("MONITOR_NOX_CATALYST_B{b}"),
            &format!("NOx Catalyst Monitor Bank {b}"),
        ));
        table[0xAF + b] = Some(mode6_entry(
            0xAF + b,
            &format!("MONITOR_PM_FILTER_B{b}"),
            &format!("PM Filter Monitor Bank {b}"),
        ));
    }
    // Misfire: general data plus one MID per cylinder (up to 12).
    table[0xA1] = Some(mode6_entry(
        0xA1,
        "MONITOR_MISFIRE_GENERAL",
        "Misfire Monitor General Data",
    ));
    for cyl in 1..=12 {
        table[0xA1 + cyl] = Some(mode6_entry(
            0xA1 + cyl,
            &format!("MONITOR_MISFIRE_CYLINDER_{cyl}"),
            &format!("Misfire Cylinder {cyl} Data"),
        ));
    }
    // MID support queries.
    let mids_support = [
        (0x00, "MIDS_A", "Supported MIDs [01-20]"),
        (0x20, "MIDS_B", "Supported MIDs [21-40]"),
        (0x40, "MIDS_C", "Supported MIDs [41-60]"),
        (0x60, "MIDS_D", "Supported MIDs [61-80]"),
        (0x80, "MIDS_E", "Supported MIDs [81-A0]"),
        (0xA0, "MIDS_F", "Supported MIDs [A1-C0]"),
    ];
    for (mid, name, desc) in mids_support {
        table[mid] = Some(Command {
            name: name.to_string(),
            desc: desc.to_string(),
            command: format!("06{mid:02X}"),
            bytes: 0,
            decoder: "Pid".to_string(),
            ecu: "ALL".to_string(),
            fast: false,
        });
    }
    table
}

fn single(name: &str, desc: &str, command: &str, decoder: &str, ecu: &str) -> Table {
    vec![Some(Command {
        name: name.to_string(),
        desc: desc.to_string(),
        command: command.to_string(),
        bytes: 0,
        decoder: decoder.to_string(),
        ecu: ecu.to_string(),
        fast: false,
    })]
}

fn build_mode9() -> Table {
    let entries: &[(&str, &str, &str, u32, &str, &str, bool)] = &[
        (
            "PIDS_9A",
            "Supported PIDs [01-20]",
            "0900",
            7,
            "Pid",
            "ALL",
            true,
        ),
        (
            "VIN_MESSAGE_COUNT",
            "VIN Message Count",
            "0901",
            3,
            "Count",
            "ENGINE",
            true,
        ),
        (
            "VIN",
            "Vehicle Identification Number",
            "0902",
            22,
            "EncodedString(17)",
            "ENGINE",
            true,
        ),
        (
            "CALIBRATION_ID_MESSAGE_COUNT",
            "Calibration ID message count for PID 04",
            "0903",
            3,
            "Count",
            "ALL",
            true,
        ),
        (
            "CALIBRATION_ID",
            "Calibration ID",
            "0904",
            18,
            "EncodedString(16)",
            "ALL",
            true,
        ),
        (
            "CVN_MESSAGE_COUNT",
            "CVN Message Count for PID 06",
            "0905",
            3,
            "Count",
            "ALL",
            true,
        ),
        (
            "CVN",
            "Calibration Verification Numbers",
            "0906",
            10,
            "Cvn",
            "ALL",
            true,
        ),
    ];
    entries
        .iter()
        .map(|(name, desc, command, bytes, decoder, ecu, fast)| {
            Some(Command {
                name: name.to_string(),
                desc: desc.to_string(),
                command: command.to_string(),
                bytes: *bytes,
                decoder: decoder.to_string(),
                ecu: ecu.to_string(),
                fast: *fast,
            })
        })
        .collect()
}

fn build_spec(sources: &Path) -> Spec {
    Spec {
        mode1: build_mode1(sources),
        mode3: single("GET_DTC", "Get DTCs", "03", "Dtc", "ALL"),
        mode4: single(
            "CLEAR_DTC",
            "Clear DTCs and Freeze data",
            "04",
            "Drop",
            "ALL",
        ),
        mode6: build_mode6(),
        mode7: single(
            "GET_CURRENT_DTC",
            "Get DTCs from the current/last driving cycle",
            "07",
            "Dtc",
            "ALL",
        ),
        mode9: build_mode9(),
        misc: vec![
            Some(Command {
                name: "ELM_VERSION".to_string(),
                desc: "ELM327 version string".to_string(),
                command: "ATI".to_string(),
                bytes: 0,
                decoder: "RawString".to_string(),
                ecu: "UNKNOWN".to_string(),
                fast: false,
            }),
            Some(Command {
                name: "ELM_VOLTAGE".to_string(),
                desc: "Voltage detected by OBD-II adapter".to_string(),
                command: "ATRV".to_string(),
                bytes: 0,
                decoder: "ElmVoltage".to_string(),
                ecu: "UNKNOWN".to_string(),
                fast: false,
            }),
        ],
    }
}

// Table emission

const DTC_HEADER: &str = "//! Generated by tools/specs-generator from the SAE J2012 DTC
//! reference (`tools/specs-generator/sources/`). DO NOT EDIT BY HAND.
//!
// SPDX-License-Identifier: MIT

";

fn emit_dtc_table(entries: &[(String, String)]) -> String {
    let mut out = String::from(DTC_HEADER);
    out.push_str("/// Looks up a DTC code (e.g. `\"P0101\"`), returning its description.\n");
    out.push_str("pub fn lookup(code: &str) -> Option<&'static str> {\n");
    out.push_str("    match code {\n");
    for (code, desc) in entries {
        out.push_str(&format!("        {code:?} => Some({desc:?}),\n"));
    }
    out.push_str("        _ => None,\n");
    out.push_str("    }\n");
    out.push_str("}\n");
    out
}

const COMMANDS_HEADER: &str = "//! Generated by tools/specs-generator from the SAE J1979 PID
//! reference (`tools/specs-generator/sources/`). DO NOT EDIT BY HAND.
//!
// SPDX-License-Identifier: MIT

";

/// Emits one `Some(OBDCommand { ... })` line.
fn emit_command(c: &Command) -> String {
    format!(
        "    Some(OBDCommand {{ name: {:?}, desc: {:?}, command: b\"{}\", bytes: {}, decoder: Decoder::{}, ecu: ecu::{}, fast: {}, header: b\"7E0\" }}),\n",
        c.name, c.desc, c.command, c.bytes, c.decoder, c.ecu, c.fast,
    )
}

/// Emits a `&[Option<OBDCommand>]` const from a table.
fn emit_table(name: &str, items: &[Option<Command>]) -> String {
    let mut out = String::new();
    out.push_str(&format!("pub const {name}: &[Option<OBDCommand>] = &[\n"));
    for item in items {
        match item {
            Some(c) => out.push_str(&emit_command(c)),
            None => out.push_str("    None,\n"),
        }
    }
    out.push_str("];\n");
    out
}

/// Mode 02 returns the same PIDs as mode 01, but for the freeze-frame
/// data (SAE J1979). The `Pid` decoder is replaced by `Drop` because
/// mode 02 responses carry no PID byte.
fn derive_mode2(mode1: &[Option<Command>]) -> Vec<Option<Command>> {
    mode1
        .iter()
        .map(|c| {
            c.as_ref().map(|c| Command {
                name: format!("DTC_{}", c.name),
                desc: format!("DTC {}", c.desc),
                command: format!("02{}", &c.command[2..]),
                bytes: c.bytes,
                decoder: if c.decoder == "Pid" {
                    "Drop".to_string()
                } else {
                    c.decoder.clone()
                },
                ecu: c.ecu.clone(),
                fast: c.fast,
            })
        })
        .collect()
}

fn emit_commands_table(spec: &Spec) -> String {
    let mode2 = derive_mode2(&spec.mode1);

    let mut out = String::from(COMMANDS_HEADER);
    out.push_str("use crate::command::{Decoder, OBDCommand};\n");
    out.push_str("use crate::message::ecu;\n\n");
    out.push_str(&emit_table("MODE1", &spec.mode1));
    out.push('\n');
    out.push_str(&emit_table("MODE2", &mode2));
    out.push('\n');
    out.push_str(&emit_table("MODE3", &spec.mode3));
    out.push('\n');
    out.push_str(&emit_table("MODE4", &spec.mode4));
    out.push('\n');
    out.push_str(&emit_table("MODE6", &spec.mode6));
    out.push('\n');
    out.push_str(&emit_table("MODE7", &spec.mode7));
    out.push('\n');
    out.push_str(&emit_table("MODE9", &spec.mode9));
    out.push('\n');
    out.push_str(&emit_table("MISC", &spec.misc));
    out
}

fn main() {
    let cli = Cli::parse();

    let dtcs = load_dtcs(&cli.sources);
    let out_path = cli.out_dir.join("dtc_table.rs");
    fs::write(&out_path, emit_dtc_table(&dtcs))
        .unwrap_or_else(|e| panic!("failed to write {}: {e}", out_path.display()));
    println!("wrote {} ({} entries)", out_path.display(), dtcs.len());

    let spec = build_spec(&cli.sources);
    let out_path = cli.out_dir.join("commands_table.rs");
    fs::write(&out_path, emit_commands_table(&spec))
        .unwrap_or_else(|e| panic!("failed to write {}: {e}", out_path.display()));
    println!("wrote {}", out_path.display());
}
