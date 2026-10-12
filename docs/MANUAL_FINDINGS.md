# ANE-200W manual findings

Source: the Elitech ANE-200W instruction manual. The following notes summarize documented capabilities and distinguish them from implemented software features.

| Section | Pages | Contents |
|---|---|---|
| Operation | 1–3 | Features, buttons/display indicators, normal/MAX/MIN/HOLD/AVG modes, units, auto-off, Bluetooth enable, record and area operations |
| Specifications and USB | 4–6 | Specification table, USB desktop-software reference and desktop workflow |
| Bluetooth acquisition | 7–8 and partial 9 | Companion-app reference, Bluetooth connection and live interface |
| Records and settings | 9–10 and cover | Saved/live data workflow, export/settings, Elitech ANE-200W model identification |

## Read specification values

| Measurement | Manual range | Resolution | Threshold | Stated accuracy |
|---|---|---|---|---|
| Velocity m/s | 0–30 | 0.01 | 0.3 | ±5% ±0.1 |
| Velocity ft/min | 0–5860 | 1.968 | 60 | ±5% ±20 |
| Velocity knots | 0–55 | 0.019 | 0.6 | ±5% ±0.2 |
| Velocity km/h | 0–90 | 0.036 | 1 | ±5% ±0.4 |
| Velocity mph | 0–65 | 0.022 | 0.7 | ±5% ±0.2 |
| Airflow CFM/CMM | 0–999900 | 0.001–100 | Not stated | Not stated separately |
| Temperature °C | −10–45 | 0.1 | — | ±2 |
| Temperature °F | 14–113 | 0.2 | — | ±3.6 |
| Relative humidity %RH | 10–90 | 0.1 | — | ±5%, non-condensing test condition |

The area table shows 0.001–9999; corresponding ft²/m² depends on the selected flow/area unit. This is a configured area, not independently measured flow. Preserve area/unit settings with each decoded session before trusting the meter's displayed CFM.

The velocity range entries are not mathematically identical after conversion: 30 m/s is approximately 5905.51 ft/min and 108 km/h. Retain what the manual actually states; do not silently rewrite manufacturer limits or clamp a converted value to make the rows agree. The suite uses exact unit conversions for analysis and requires later validation of the device's display behavior.

The manufacturer product page lists humidity resolution 0.001, which differs from the manual table's 0.1. The manual governs this model profile; packet/display testing must resolve actual behavior. Source: [Elitech product specifications](https://www.elitecheu.com/products/elitech-ane-200w-bluetooth-anemometer). No unit resolution is a guarantee of measurement accuracy.

## Model-specific operations

\- Normal mode measures velocity and temperature. `VEL/AVG` averages 30 current readings; this is not documented as a 30-second or time-weighted average.

\- MAX/MIN, HOLD, average, air-flow and two-thirds-maximum display modes must remain identifiable when decoding; a held or averaged display is not automatically an instantaneous reading.

\- Auto-shutdown is described as 10 minutes of inactivity; battery is 9V/6F22. Bluetooth changes stated consumption from approximately 6 mA to 16 mA. A long capture needs explicit power/lifecycle testing.

\- To enable Bluetooth, the supplied instructions describe SET/HOLD setting mode, `bLE`, `OPE`, then saving/returning to normal operation. Device name is `Anemometer`; connection changes its Bluetooth icon from blinking to steady.

\- Companion app named in this manual is `Anemometer26`. [Its Google Play listing](https://play.google.com/store/apps/details?id=sport.com.example.android.anemometer) confirms Bluetooth measurement/export functionality. A generic newer "Elitech app" description is not proof of compatibility with the documented firmware.

\- The app/desktop software supports live acquisition and stored-meter records. Record interval and area settings are described; commands and storage capacity are not sufficiently specified for implementation. No storage capacity was invented from third-party summaries.

\- USB software is named `setup_AnemometerMK25.zip` at `www.downloadsupport.tech`; it supports charts, configurations and `.ane`/`.xls` export. That site could not be retrieved during research. No vendor installer was executed.

## Unresolved hardware/protocol information

The pages do not specify BLE service/characteristic UUIDs, whether an explicit poll command is required, command framing, checksums, measurement byte offsets, scaling, sign encoding, unit codes, mode flags, device measurement timestamps or USB interface/baud format. Those are requirements for actual decoding, not fields to fill with typical values.

Device capture must be paired with the LCD and selected units, including zero, several velocities, temperature, unit changes, HOLD/AVG behavior and disconnect/reconnect. Configuration or memory-deleting commands are separate operations and must not be inferred from incoming bytes.
