# ANE-200W capabilities and practical extensions

Reviewed October 12, 2026. This is an evidence review and proposed feature list, not a claim that the unimplemented controls work in this suite. No connection, setting change, memory operation, firmware operation or experimental command was sent during this review.

## What the evidence establishes

The ANE-200W manual identifies AnemoMeter26 as the companion app. Its pages 7–10 describe live measurement, charts, exports, units, flow area, recording interval, and starting, stopping, retrieving and deleting records in the instrument. Pages 1–3 describe additional operations on the physical buttons. These are distinct from controls this suite has implemented.

The [manufacturer product page](https://www.elitecheu.com/products/elitech-ane-200w-bluetooth-anemometer) confirms Bluetooth, USB export, multiple units and MAX/MIN/AVG functions. The [companion developer's Google Play listing](https://play.google.com/store/apps/details?id=sport.com.example.android.anemometer) describes live Bluetooth measurements, charting and spreadsheet export. Neither source publishes a complete wire protocol.

Offline inspection of the AnemoMeter26 research APK identifies the commands below. That copy came from a mirror; its package identity matches the developer listing, but its certificate has not been independently matched to a developer-published certificate. Research binaries and decompiled material are excluded from the repository. This document independently summarizes protocol facts rather than distributing the vendor's implementation.

Only the current-reading exchange has been exercised with a physical ANE-200W. Previous captures produced valid live packets at zero vane speed. That establishes communication; comparison at nonzero velocities and across all units remains necessary before claiming full measurement validation. See [the live profile](PROTOCOL.md).

## Instrument controls

| Capability | Evidence | Present suite / next step |
| --- | --- | --- |
| Read current velocity, temperature and humidity | Manual pp. 7–10; companion command; actual notifications | Implemented live profile; nonzero/display comparison remains outstanding |
| Change velocity, temperature and flow units | Manual p. 10; companion command `getSet_unit` | Candidate device control, not hardware-validated or enabled; local display conversions can work independently |
| Set and query flow area | Manual pp. 3, 10; `getSet_area`, `getQuery_area` | Candidate; read back the area and unit after a future authorized change |
| Set and query instrument recording interval | Manual pp. 3, 10; `getSet_record_time`, `getQuery_record_time` | Candidate; separate instrument interval from app polling and averaging windows |
| Start and stop internal recording | Manual pp. 3, 10; `getStart_to_record`, `getOver_to_record` | Candidate; different from starting/stopping host capture |
| Query stored-data amount and retrieve records | Manual pp. 9–10; `getQuery_datanumber`, `getRead_record_data` | Strong next research step; packet assembly, count meaning, units and timestamp provenance are unresolved |
| Erase meter memory | Manual pp. 3, 10; `getClear_data` | Destructive candidate; keep separate from deleting app sessions and require an explicit operator action |
| Meter HOLD, MAX/MIN, 30-sample AVG and two-thirds-MAX modes | Manual pp. 2–3, physical button instructions | No remote command established; app Hold and computed statistics are independent features |
| Buzzer enable/disable | Manual p. 2, physical setting | No Bluetooth command identified |
| LCD backlight and LED illumination | Manual pp. 1–2, physical buttons | No Bluetooth command identified |
| Automatic shutdown, power and Bluetooth enable/disable | Manual pp. 2–3, physical settings | No remote command established; a powered-off radio cannot receive this app's commands |
| Model, firmware, hardware, software, manufacturer and serial information | Saved GATT snapshot exposes the standard Device Information characteristics | Read-only identification is a useful future feature; actual returned strings have not been verified |
| Battery percentage | Manual shows a low-battery icon; saved snapshot has no standard Battery Service | Do not invent a percentage or interpret an unknown response byte as battery level |

The Device Information identifiers in the saved snapshot are consistent with the [Bluetooth SIG Device Information Service](https://www.bluetooth.com/wp-content/uploads/Files/Specification/HTML/DIS_v1.2/out/en/index-en.html). Exposing a characteristic does not guarantee its value is populated, unique or correct.

### Offline command inventory

These identifiers are research notes, not newly enabled app actions. The command byte is the third byte in the request. The companion request envelope begins `57 48` and ends `3b`. Simple commands use four bytes; setting commands have payloads.

| Command byte | Companion method | Meaning / caution |
| --- | --- | --- |
| `01` | `getReal_time` | Current reading; hardware exchange already observed |
| `03` | `getRead_record_data` | Stored-data transfer; decoding not validated |
| `04` | `getStart_to_record` | Start meter-memory recording |
| `05` | `getOver_to_record` | Stop meter-memory recording |
| `06` | `getClear_data` | Erase meter-memory records |
| `07` | `getQuery_datanumber` | Query data quantity; packet count versus record count must be established |
| `08` | `getQuery_record_time` | Query recording interval |
| `09` | `getQuery_area` | Query configured flow area |
| `82` | `getSet_unit` | Set three unit selections; seven-byte request |
| `88` | `getSet_record_time` | Set interval; eight-byte request with four decimal digit bytes |
| `89` | `getSet_area` | Set area; nine-byte request with decimal digits and decimal position |

The area format is not a generic IEEE floating-point payload. Its exact rounding, limits and readback need validation before a writer is implemented. The companion interval dialog accepts positive values below 3600, whereas the physical manual describes a setting maximum of 9999; do not silently equate the app's validation range to the instrument's full capability.

The saved meter snapshot advertises `FFF1` notifications and `FFF2` writes in the same `FFF0` service. Its `FFF2` advertises writes with response. The companion uses a different write preference, so the suite must follow discovered properties instead of unconditionally selecting write-without-response. Other writable characteristics are not substitutes for `FFF2`.

## Useful additions, ranked

These are proposed product features, not hidden capabilities claimed for the instrument.

I. **Timed sampling with stability feedback.** Select a fixed duration, show elapsed/remaining time, arithmetic or explicitly time-weighted mean, minimum, maximum, standard deviation and valid-sample coverage. Save the whole interval and its method; never choose only the most favorable samples. Distinguish this from the meter's 30-sample AVG.

II. **Device/outlet workflow.** Project, system, equipment, outlet and test-stage labels; a large Save-and-next action; repeat readings; editable notes; a reviewable session table. Manual labels keep the suite standalone.

III. **Traceable flow calculation.** Enter geometry or effective area, display `CFM = FPM × effective area`, and preserve the uncorrected velocity, area, correction, reference measurement and calculation version. Do not treat a nominal grille face area as a calibrated effective area.

IV. **Calibration comparison.** Pair reference traverses/hood totals with stable sampled velocities, show the factor and residuals from multiple pairs, and compare successive fits. Keep calibration correction in the app until a real meter calibration command is established; changing flow area does not calibrate the vane sensor.

V. **Session recovery and export.** Persist incrementally, recover after interruption, export CSV plus original evidence, and offer an Android share action. Deleting or revising derived results should retain an audit trail of raw readings.

VI. **Connection health and instrument identity.** Read model/firmware information, show sample age and disconnect gaps, and offer controlled reconnect to the selected instrument. Signal strength is a connection clue, not a distance measurement or battery estimate.

VII. **Instrument-memory download.** Useful when the phone cannot remain nearby. First establish packet boundaries, record completeness and timing. Do not label host download time as the original measurement time.

VIII. **Validated settings panel.** Add units, area and recording interval only after command/readback tests. Clearly separate settings on the meter from display/calculation preferences in the app.

IX. **Hands-free cues.** Phone/PC spoken readings, threshold tones or phone vibration help when the technician is watching the instrument or grille. Keep muted by default and expose the control plainly.

The highest-value next combination is timed samples, outlet labels and traceable flow/calibration records. It turns a live gauge into an efficient field worksheet without needing additional device writes.

## Songs, pixel art and Doom

**A song from the meter:** it has a buzzer and an on/off setting, but the reviewed Bluetooth API exposes no play-tone, pitch or duration command. We cannot currently make its buzzer play a tune. A phone or PC can synthesize a tone from live velocity, quantize it to musical notes, or play a short sound when a capture completes.

**Pixel art on the meter:** the manual's LCD diagram shows fixed digit segments and dedicated icons, not a pixel-addressable screen. There is no established command for drawing even arbitrary segments. General pixel art is therefore not a feature supported by the current hardware/interface evidence. The companion app can display pixel art, animated turbines or an airflow-controlled visualizer.

**Doom on the meter:** there is no demonstrated executable-loading toolchain, suitable display interface or known processor/memory specification for a practical port. The segmented display would remain a fundamental obstacle to displaying the game. A feasible joke project is Doom running on the PC with airflow mapped to one input, such as movement speed, while ordinary controls handle the rest; it would be a meter-controlled game, not a game running inside the meter.

**Small ideas that suit this instrument:** a wind-powered Pong paddle, a sailboat whose speed follows airflow, a tuner-style tone for reaching a target flow, or a pixel turbine responding to velocity. The verified read stream is sufficient for these in principle; useful responsiveness at nonzero speeds still needs testing. All game/sound output belongs to a separate play view and must not contaminate saved measurement records.

## One genuine firmware research lead

The recorded GATT enumeration also includes service `F000FFC0-0451-4000-B000-000000000000` and characteristics ending `FFC1` and `FFC2`. These match Texas Instruments' documented **Over-the-Air Download (OAD)** service, Image Identify and Image Block identifiers. [TI OAD documentation](https://software-dl.ti.com/lprf/simplelink_cc2640r2_sdk/1.35.00.33/exports/docs/ble5stack/ble_user_guide/html/oad/oad_concepts.html#oad-service-0xffc0)

That is a meaningful clue that a firmware-update interface may exist. It does not identify the exact chip, prove that arbitrary firmware is accepted, establish whether the radio and measurement processor are separate, or prove any vulnerability. No writes or subscriptions to these characteristics were attempted in this review. Useful prerequisites for a separate firmware investigation are exact hardware identification, vendor firmware/recovery material, and a recoverable spare unit; an OAD UUID alone is not enough to enable a firmware-upload feature.

## Glitches and caveats found in the research

\- **Companion temperature parsing:** the reviewed APK appears to sign-extend the low temperature byte incorrectly, then reuse the previous temperature whenever the result is zero or negative. This can make a plausible but stale value survive a decoding error. The suite already reconstructs the signed 16-bit value correctly and does not replace valid negative values with a prior reading.

\- **Companion short-packet handling:** one receive path tests for a length of at least three and then accesses byte index three. A three-byte notification could therefore fail in that path. This is an offline code finding, not a reproduced device fault or an instruction to send malformed traffic.

\- **Companion humidity gate:** bytecode inspection shows a live-display branch gated on humidity being greater than 20.0. This appears capable of suppressing updates at lower humidity. It has not been reproduced on the physical meter and should not be copied into the new decoder.

\- **Repeated setting writes:** the companion repeats some start/stop/clear commands. Repeating destructive commands is not a reliability model to copy. A future implementation should serialize operations and verify state/readback.

\- **Range/resolution inconsistencies:** the supplied manual's velocity ranges do not exactly agree after unit conversion, and its humidity resolution differs from the web listing. Preserve the manufacturer's source values and the actual raw reading; precision on screen is not extra accuracy.

\- **No inferred mode flags:** unknown live bytes remain unknown. The relationship between physical HOLD/AVG modes and transmitted live readings still needs an explicit comparison. Do not assume the app's local Hold changes the meter.

\- **USB remains unresolved:** the manual describes desktop software and USB export. USB transport and record formats require validation before integration into this suite.

## References

\- Elitech ANE-200W instruction manual, pages 1–3: controls, LCD, settings, recording and flow area.

\- Manual pages 4–6: specifications and USB software.

\- Manual pages 7–10: Bluetooth acquisition, exports, records and settings.

\- Companion-app protocol analysis and recorded GATT enumeration, summarized in [the implemented live profile](PROTOCOL.md).

\- [Manual reference](MANUAL_FINDINGS.md).

Original vendor materials, device identifiers, measurements and research captures are excluded from the public repository.
