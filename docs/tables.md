# Data tables: provenance and regeneration

The command and DTC tables in `src/` are generated, not hand-written.
This document records where every piece of data comes from and how to
regenerate it.

## Pipeline

```
tools/specs-generator/sources/   (raw public references, committed)
        |
        |  specs-generator   (parses sources, applies SAE J1979 formulas)
        v
src/commands_table.rs   (modes 01/02/03/04/06/07/09/0A + ELM327 commands)
src/dtc_table.rs        (DTC code -> description)
```

Regenerate everything with:

```sh
cargo run -p specs-generator -- tools/specs-generator/sources src
cargo fmt
```

## Sources

### DTCs — SAE J2012

- `sources/{p,u,c,b}_codes.txt`: [Wal33D/dtc-database](https://github.com/Wal33D/dtc-database),
  MIT licensed, mirrored verbatim. 9415 codes (P: 7387, U: 1230, C: 498, B: 300).
- `specs-generator` parses `CODE - description` lines, deduplicates (first
  description wins), and sorts by code. The result is the DTC table.

### Mode 01 PIDs — SAE J1979

- `sources/pids_wikipedia.txt`: the Wikipedia
  [OBD-II PIDs](https://en.wikipedia.org/wiki/OBD-II_PIDs) article's
  mode-01 table (PID, data bytes, description, formula), mirrored verbatim.
- `specs-generator` maps each PID to a command name and decoder. The decoder
  follows the SAE J1979 formula column:

  | Formula pattern | Decoder |
  |---|---|
  | `A` (0-100 %) | `Percent` |
  | `A-128` (centered %) | `PercentCentered` |
  | `A-40` (°C) | `Temp` |
  | `A*3` (kPa) | `Pressure` |
  | `(A*256+B)/4` (rpm) | `Uas(0x07)` |
  | `A` (km/h) | `Uas(0x09)` |
  | `(A*256+B)/100` (g/s) | `Uas(0x27)` |
  | `(A*256+B)/21.6` (kPa) | `Uas(0x19)` |
  | `(A*256+B)/10` (kPa) | `Uas(0x1B)` |
  | `(A*256+B)/10` (V) | `Uas(0x0B)` |
  | `(A*256+B)/4` (mA) | `CurrentCentered` |
  | `(A*256+B)/10` (mV) | `SensorVoltageBig` |
  | `A/200` (V) | `SensorVoltage` |
  | `(A*256+B)/32768` (ratio) | `Uas(0x1E)` |
  | `(A*256+B)/10` (s) | `Uas(0x12)` |
  | `(A*256+B)` (s) | `Uas(0x34)` |
  | `(A*256+B)` (km) | `Uas(0x25)` |
  | `(A*256+B)` (count) | `Uas(0x01)` |
  | `(A*256+B)/10` (°C) | `Uas(0x16)` |
  | `(A*256+B)/200` (kPa) | `EvapPressure` |
  | `(A*256+B)/200` (kPa abs) | `AbsEvapPressure` |
  | `(A*256+B)/200` (kPa, alt) | `EvapPressureAlt` |
  | `(A*256+B)/4` (kPa) | `FuelPressure` |
  | `(A*256+B)/4` (kPa) | `AbsoluteLoad` |
  | `(A*256+B)/32` (deg) | `TimingAdvance` |
  | `(A*256+B)/20` (L/h) | `FuelRate` |
  | `(A*256+B)/128` (deg) | `InjectTiming` |
  | `(A*256+B)/10` (g/s) | `MaxMaf` |
  | bitfields | `Status`, `FuelStatus`, `AirStatus`, `O2Sensors`, `O2SensorsAlt`, `AuxInputStatus`, `ObdCompliance`, `FuelType`, `SingleDtc` |
  | supported-PID bitmask | `Pid` |
  | no value | `Drop` |

  The `Uas(n)` decoders resolve units through the SAE J1979 unit-and-scaling
  table in `src/units.rs` (PID 0x0C rpm, 0x0D km/h, 0x10 g/s, etc.).

  Data bytes are the Wikipedia counts plus 2 (mode + PID bytes
  are part of the command, not the payload).

#### Compression-ignition PIDs (0x60–0x83)

PIDs above `0x5F` report several sensors in one reply. The Wikipedia mirror
stops at `0x5F`, so `sources/pids_wikipedia.txt` was extended with the
`0x60`–`0x83` rows from SAE J1979 (byte counts and descriptions cross-checked
against the [DashLogic PID listing](https://www.dashlogic.com/docs/technical/obdii_pids)).
Their decoders are `Sensors(&'static [decoder::Sensor])` tables in
`src/decoder.rs`, one table per payload layout:

| PIDs | Table | Contents |
|---|---|---|
| 0x61, 0x62 | `ENGINE_TORQUE` | driver's demand / actual percent torque |
| 0x63 | `REFERENCE_TORQUE` | engine reference torque |
| 0x64 | `ENGINE_PERCENT_TORQUE` | torque at idle and five reference points |
| 0x66 | `MAF_SENSORS` | two mass-air-flow sensors |
| 0x67 | `COOLANT_SENSORS` | two coolant temperature sensors |
| 0x68 | `INTAKE_AIR_SENSORS` | intake air temperature, two banks |
| 0x69 | `EGR_VALVES` | commanded/actual/error, two EGR valves |
| 0x6A | `INTAKE_AIR_FLOW` | commanded and relative intake air flow |
| 0x6B | `EGR_TEMPERATURES` | EGR temperature, two banks |
| 0x6C | `THROTTLE_ACTUATOR` | commanded and relative throttle position |
| 0x6D | `FUEL_PRESSURE_CONTROL` | two fuel pressure control loops |
| 0x6E | `INJECTION_PRESSURE` | two injection pressure loops |
| 0x6F | `TURBO_INLET_PRESSURE` | compressor inlet pressure, two sensors |
| 0x70 | `BOOST_PRESSURE` | commanded/measured boost, two controls |
| 0x71, 0x72 | `VGT_POSITION`, `WASTEGATE_POSITION` | commanded/actual position |
| 0x73 | `EXHAUST_PRESSURE` | exhaust pressure, two banks |
| 0x74 | `TURBO_SPEED` | turbocharger rpm, two turbos |
| 0x75, 0x76 | `TURBO_TEMPERATURES` | compressor and turbine temperatures |
| 0x77 | `CHARGE_AIR_TEMPERATURES` | charge air cooler, two banks |
| 0x78, 0x79 | `EGT_BANK` | four exhaust gas temperatures |
| 0x7A, 0x7B | `DPF_PRESSURES` | delta (signed), inlet and outlet pressure |
| 0x7C | `DPF_TEMPERATURES` | DPF inlet/outlet, two banks |
| 0x7F | `ENGINE_RUN_TIMES` | total, idle and PTO run time |
| 0x83 | `NOX_SENSORS` | NOx concentration, two banks |

Every table starts at the first byte after the PID; byte `A` of the SAE
layout is the per-sensor support bitmap and is skipped. A reply that is too
short for a sensor drops that sensor, and a table with a single sensor
decodes to `Value::Quantity` instead of `Value::Readings`. PIDs `0x60` and
`0x80` are the support bitmaps for the two new blocks, so `SupportedPids`
keeps five bitmaps (`0x00`–`0xA0`).

### Mode 02

Derived from mode 01: same PIDs with
`02` commands, `DTC_` name prefix, and the `Pid` decoder replaced by `Drop`
(mode 02 responses carry no PID byte).

### Mode 06 — SAE J1979 test IDs

`specs-generator` assigns the standard MID layout:

| MID range | Monitor |
|---|---|
| 0x01-0x10 | O2 sensor, bank 1-4 x sensor 1-4 |
| 0x21-0x24 | Catalyst, per bank |
| 0x31-0x34 | EGR, per bank |
| 0x35-0x38 | VVT, per bank |
| 0x39-0x3C | EVAP leak (0.150"/0.090"/0.040"/0.020") |
| 0x3D | Purge flow |
| 0x41-0x50 | O2 sensor heater, bank x sensor |
| 0x61-0x64 | Heated catalyst, per bank |
| 0x71-0x74 | Secondary air |
| 0x81-0x84 | Fuel system, per bank |
| 0x85-0x86 | Boost pressure control, per bank |
| 0x90-0x91 | NOx absorber, per bank |
| 0x98-0x99 | NOx catalyst, per bank |
| 0xA1 | Misfire general data |
| 0xA2-0xAD | Misfire, per cylinder (1-12) |
| 0xB0-0xB1 | PM filter, per bank |
| 0x00/0x20/0x40/0x60/0x80/0xA0 | Supported-MID queries |

### Modes 03/04/07/0A and 09

- 03/04/07/0A: single fixed commands (`GET_DTC`, `CLEAR_DTC`,
  `GET_CURRENT_DTC`, `GET_PERMANENT_DTC`). Mode 0A permanent codes cannot be
  erased with Mode 04.
- 09: `specs-generator` defines the seven standard PIDs (supported-PIDs,
  VIN message count, VIN, calibration ID count, calibration ID, CVN count,
  CVN) with their SAE J1979 byte counts.

### ELM327 commands

`ATI` (version string) and `ATRV` (adapter voltage) are defined as
as `misc` entries.

## License

The generated tables (`src/commands_table.rs`, `src/dtc_table.rs`), the raw
references under `tools/specs-generator/sources/`, and the generator's emitted
output are MIT licensed. The rest of the crate is GPL-3.0-or-later.

## Notes

- Command names are the public API. Two typos from the original hand-built
  table were fixed during the ex-novo rebuild: `AMBIANT_AIR_TEMP` ->
  `AMBIENT_AIR_TEMP` and the mode-06 `MONITOR_BOOST_PRESSURE_B2` description
  ("Bank 1" -> "Bank 2").
- DTC descriptions follow the Wal33D wording, which may differ from the
  SAE J2012 text in minor ways (e.g. P0101 "Mass or Volume Air Flow Sensor A
  Circuit Range/Performance").