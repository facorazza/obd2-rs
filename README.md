# obd2-rs

Talk to your car's ECU from Rust. obd2-rs is a pure-Rust OBD-II interface
that speaks to ELM327 adapters over a plain serial port: query RPM, speed,
coolant temperature, fuel status, trouble codes, VIN and 200+ more PIDs —
no C dependencies, no embedded hardware required beyond the $10 dongle.

## Usage

```rust,no_run
use std::time::Duration;
use obd2_rs::commands;
use obd2_rs::obd::OBD;

let mut obd = OBD::new(
    Some("/dev/tty.OBDII"), // serial port; None scans available ports
    None,                   // baud rate; None auto-detects
    None,                   // ELM protocol id ("1".."A"); None auto-detects
    false,                  // fast mode
    Duration::from_secs(30),// command timeout
    true,                   // check the 12V rail voltage
    false,                  // start in low-power mode
);

if obd.is_connected() {
    if let Some(rpm_cmd) = commands::by_name("RPM") {
        let rpm = obd.query(rpm_cmd, false);
        if let Some(value) = &rpm.value {
            println!("RPM: {:?}", value);
        }
    }
}
```

`OBD::new` connects, reads the ECU's supported PIDs, and populates
`supported_commands`.

## Features

- ELM327 init sequence: baud probing, voltage check, protocol auto-detect
- Protocol parsers for ELM protocol IDs `1`–`A` (SAE J1850 PWM/VPW, ISO
  9141-2, ISO 14230-4, ISO 15765-4 CAN 11/29-bit, SAE J1939) plus an
  unknown-protocol fallback
- 200+ OBD commands (modes 1–9) with generated command and DTC tables
- All standard decoders: scalars, UAS-scaled values, status/monitor trees,
  DTCs, VIN, fuel status
- `query` / `supported_commands` / `supports` high-level API

## Supported platforms

- Linux, macOS and Windows via the `serialport` crate. On Linux, the user
  needs read/write access to the serial device (usually the `dialout`
  group or a udev rule).

## MSRV

Rust 1.85 (edition 2024).

## Build & test

```sh
cargo build --release
cargo test              # 116 unit tests + doctests
cargo clippy -- -D warnings
cargo fmt --check
```

## Data tables

`src/commands_table.rs` and `src/dtc_table.rs` are generated and checked
in; do not edit by hand. They are rebuilt from the raw public references
under `tools/specs-generator/sources/` (SAE J1979 PIDs, SAE J2012 DTCs) by
the `specs-generator` tool. See [docs/tables.md](docs/tables.md) for full
provenance. Regenerate with:

```sh
cargo run -p specs-generator -- tools/specs-generator/sources src
cargo fmt
```

## License

GPL-3.0-or-later (library code). The data tables in `src/commands_table.rs`
and `src/dtc_table.rs`, the raw references under
`tools/specs-generator/sources/`, and the generator's emitted output are MIT.
