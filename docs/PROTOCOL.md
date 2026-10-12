# ANE-200W live-reading profile

Scope: the Elitech ANE-200W, using the companion app identified in the supplied manual as **AnemoMeter26**. Profile name: `anemometer26-v3.2-live-v1`.

The profile comes from independently inspecting the identified companion application's behavior, followed by real meter notification capture. The APK research copy was obtained from a mirror, not directly authenticated against a developer-published certificate. Its package identity matches [the developer's Google Play listing](https://play.google.com/store/apps/details?id=sport.com.example.android.anemometer). The behavior was then checked against the actual meter. Private research binaries/decompilation, addresses and field captures stay under ignored `local/protocol/`; they are not distributed as project source.

## GATT operations

Select the actual meter advertising `Anemometer`. Discover services rather than assuming a service UUID. Within the same discovered service, use characteristic `0000fff1-0000-1000-8000-00805f9b34fb` for notifications and `0000fff2-0000-1000-8000-00805f9b34fb` for live-reading requests. Verify their advertised notify/write properties first. Configure the standard notification CCCD through the platform API before requesting readings.

The identified current-reading request is four bytes: `57 48 01 3b`. Send it about once a second using the discovered write mode, preferring write without response when supported. It requests a reading; it does not start/stop meter-memory recording or change units/area. No other vendor command is implemented. Stopping/Hold/Save are local app actions.

## Response

Each supported live response is exactly 17 bytes. The observed envelope is `ff 57 4c` at bytes 0–2 and `3b` at byte 16. Byte 3 is command `01`. A different shape, command, envelope, invalid unit code or out-of-range reading is rejected, while raw evidence is retained.

| Byte positions (zero based) | Meaning |
| --- | --- |
| 4–5 | Unsigned big-endian velocity × 100; canonical m/s, regardless of selected velocity unit |
| 6 | Meter velocity display unit: 0 m/s, 1 km/h, 2 ft/min, 3 knots, 4 mph |
| 7–8 | Signed big-endian temperature × 10, in the unit indicated by byte 10 |
| 9 | Not interpreted; no measurement meaning is asserted |
| 10 | Temperature unit: 0 °C, 1 °F |
| 11–12 | Unsigned big-endian relative humidity × 10 |
| 13 | Flow-display unit: 0 CMM, 1 CFM; no transmitted flow value is asserted |
| 14–15 | Not interpreted |

The apps convert canonical m/s to FPM by `m/s × 60 / 0.3048`, and Fahrenheit temperature to Celsius for storage by `(°F − 32) × 5 / 9`. Display temperature uses Fahrenheit. Preserve full numeric precision in saved values; presentation rounding is separate. No CFM is inferred without an entered area/reference factor in the Windows analysis tools.

## Integrity and limitations

The response envelope, lengths and values are checked. No checksum semantics are claimed for the uninterpreted bytes. Stored-data responses, fragmented frames and unknown firmware formats are not treated as this live profile. The signed temperature conversion deliberately handles valid negative temperatures and low-byte sign boundaries without carrying a previous reading forward.

Physical captures received valid live packets at zero and nonzero vane speed. A three-minute Windows capture on 2026-10-12 retained 173 valid nonzero packets and no rejected frames, spanning 0.96 to 3.16 m/s (approximately 189 to 622 FPM). A meter photograph at 12:50:00.574 p.m. CDT showed 3.08 m/s and 60.7% RH. The closest packet, received at 12:50:00.770 p.m. CDT, decoded to those same values; 3.08 m/s converts to 606.3 FPM. Photo and receive clocks were not independently synchronized, so this is a nearby display comparison rather than an asserted measurement-time alignment.

This establishes nonzero reception and supports decoding at the observed airflow. It does not establish airflow calibration, accuracy across the instrument range, operation with every selected display unit, or physical Android operation. Records retain `decoder_status: vendor-app-derived`; the single display comparison does not change provenance or certify every decoded field. Raw capture, decoded records, photograph and comparison details remain private.

Live freshness uses a monotonic receive-time clock with a five-second limit. Host UTC receive time is not a meter-generated measurement time. Hold preserves one immutable reading while receiving continues. A held sample can be saved later, including after disconnect, with its original receive time and `display_state: held`.
