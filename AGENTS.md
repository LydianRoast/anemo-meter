# Working instructions

Use clear, neutral language in public documentation and application messages. Do not include personal names, local drive letters, development-machine details or project-specific examples.

Use literal hyphens (`-`), arrows (`->`), or traditional Roman numeral outlines for lists. Do not use thick bullet dots. In Markdown, escape a leading hyphen as `\-` so it remains a visible hyphen rather than rendering as a dot. Separate entries with blank lines for readable rendering.

This standalone suite supports only the Elitech ANE-200W. Changes to supported instrument scope require explicit direction. The default branch is `Master`.

## Implementation

All application-owned code and build orchestration must be Rust. Android platform APIs and the upstream Bluetooth JNI bridge are third-party dependencies.

Keep the root README about software capabilities, operation, development and limitations. Beginner field instructions belong in `README-TECHNICIAN.md`. Do not add UI/UX/design discussion to either README. Original research reports may be separate documents.

## Interface

Preserve the existing dark chrome theme, Routed Gothic lettering, readability, keyboard focus and touch targets. Use compact, square controls and frames. Convey status with words as well as color. Reference artwork is excluded from the repository and application.

## Instrument and measurement integrity

Bluetooth framing, characteristics, units and meanings must be established from the actual meter or identified protocol evidence. The implemented live profile is documented in `docs/PROTOCOL.md`. It may subscribe to discovered FFF1 and send only the identified current-reading request `57 48 01 3b` to FFF2 in the same discovered service, using the advertised write mode. Do not send unknown commands, alter firmware or clear instrument memory. The capabilities review describes possible future controls; it is not permission to execute them.

Never label synthetic/imported/demo readings as live. Preserve raw bytes, receive timestamps and provenance. Receive time is not instrument measurement time. Unknown bytes stay uninterpreted. Missing, error, stale and disconnected states are distinct from measured zero.

Use monotonic receipt time for the five-second freshness limit, including across UI queues. Hold preserves an immutable displayed sample while incoming data and the graph continue. Save publishes exactly one complete timestamped record per explicit click; write/sync a staging file before atomic publication. A held save keeps its original receive time and label. Break graph lines across invalid/long gaps. Do not replay old-session packets as fresh after Stop or reconnection.

Android capture is foreground-only and must stop independently of repaint when the activity loses focus. Keep the screen awake while capturing and release the flag afterward. Persist/export errors must be visible, not silently discarded.

## Files, builds and validation

Private captures, saved measurements, device identifiers, signing keys, vendor binaries/decompilation, downloaded tools, build outputs, previews and references belong under ignored `local/`. Preserve original manual photographs. Do not expose signing secrets in commands or logs.

Use ignored local directories for Cargo caches, targets, temporary files and Android tools. The Rust workspace contains `ane-core`, `ane-app` and `xtask`. Run meaningful Rust tests and build the actual Windows executable and Android APK when changing shipped behavior. Keep the signing certificate consistent for Android updates.

Original code/documentation are MIT licensed. Preserve all dependency/font notices. Compilation, mocked tests, layout previews and signature verification do not establish physical Android operation or meter calibration. Report phone and nonzero/full-range validation separately.
