# ANE-200W Console

A standalone Rust application for capturing and working with measurements from the **Elitech ANE-200W** vane anemometer on Windows and Android.

Start with [the beginner technician instructions](README-TECHNICIAN.md). For instrument controls, possible additions and hardware limitations, see [the capability review](docs/METER_CAPABILITIES.md).

## Software capabilities

\- Discovers the selected meter over Bluetooth LE and requests individual readings about once per second.

\- Displays velocity in FPM and m/s, temperature in °F, and relative humidity.

\- Shows a scrolling receive-time trace with 5, 10, 30, 60, 90, 120, 180, 300 and 600-second windows.

\- Holds one displayed reading while reception continues, and saves one reading per explicit Save action.

\- Records original receive time, save time, equipment/outlet note, units, source, quality and supporting packet bytes.

\- Exports saved readings as CSV and retains original raw captures separately.

\- Imports measurement CSV, calculates airflow from entered area and multiplier, and fits effective area to independent velocity/flow references.

\- Archives raw JSONL in SQLite with duplicate detection and transaction rollback on conflicting records.

Missing, invalid, stale or disconnected data is never converted into measured zero. A live reading expires after five seconds; a held snapshot retains its original receive time. Calculated CFM is derived from entered area and multiplier. The live display does not average samples.

The application sends only the verified current-reading request. It does not change the meter's units, area, logging interval, internal recording state, stored records or firmware.

## Run

### Application screenshots

Both views show the application before a meter is connected.

| Windows view | Phone-width preview |
| --- | --- |
| ![Windows application before connection](docs/screenshots/desktop.png) | ![Phone-width layout preview before connection](docs/screenshots/phone-layout.png) |

### Installation

On Windows, open `ane-console.exe` or use `Start Windows.cmd` from the application folder. On Android, install the APK. Packages signed with the same certificate can update compatible previous installations.

Android capture is foreground-only. Keep the application open while measuring; interruptions disconnect the meter and preserve saved data. Android exports are copied to Downloads when platform permissions permit. Windows stores captures and readings in adjacent `local/`; `ANE_DATA_DIR` can select another location.

## Development

Application code, measurement logic and build orchestration are Rust.

| Crate | Responsibility |
| --- | --- |
| `ane-core` | Packet validation, measurement state, durable records, CSV, SQLite and calculations |
| `ane-app` | Windows/Android application and Bluetooth transport |
| `xtask` | Native Android package assembly and signing |

Rust, a Windows C/C++ linker, and Android NDK/JDK/build tools are build dependencies. Android platform APIs and the upstream Bluetooth JNI bridge remain runtime dependencies; they are not application-owned Java source. Dependency versions are recorded in `Cargo.lock`. Downloaded tools, dependency sources and build products belong under ignored `local/`.

```powershell
$env:CARGO_HOME = "$PWD\local\cargo"
$env:TEMP = "$PWD\local\temp"
$env:TMP = $env:TEMP
cargo test --workspace
cargo run -p ane-app --bin ane-console
cargo xtask apk
```

See [Android build/install details](docs/ANDROID_INSTALL.md). Preserve the private signing key in `local/signing/` for future update-compatible packages. Never publish that directory.

## Evidence and limitations

The packet profile was derived from the manual-identified AnemoMeter26 companion and checked against real zero-speed ANE-200W notifications. Nonzero comparison, full-range accuracy and physical phone operation require separate field validation. Compilation and automated tests are not instrument calibration.

[Protocol evidence](docs/PROTOCOL.md), [manual findings](docs/MANUAL_FINDINGS.md) and [the capture contract](docs/CAPTURE_CONTRACT.md) describe the known format and data boundaries. USB capture, instrument-memory retrieval and additional meter-setting commands are not implemented.

## License and private material

Original software is [MIT licensed](LICENSE). Dependencies retain their own licenses; see [notices](THIRD_PARTY_NOTICES.md). The default branch is `Master`.

Private measurements, signing keys, vendor research, downloaded manuals, reference books, archived websites and build products are excluded from Git.
