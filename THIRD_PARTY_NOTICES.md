# Third-party material

Original suite code and documentation are distributed under the [MIT License](LICENSE).

The bundled Routed Gothic font is copyright 2017 Darren Embry, with Reserved Font Name Routed Gothic, and is distributed under the **SIL Open Font License 1.1**. Its complete upstream notice and license are included at [OFL.txt](assets/fonts/OFL.txt). The upstream project is [dse/routed-gothic](https://github.com/dse/routed-gothic). The MIT license does not replace the font license.

The Rust applications use [egui/eframe](https://github.com/emilk/egui) and [btleplug](https://github.com/deviceplug/btleplug), available under their MIT/Apache-2.0 licenses, plus the dependencies locked in Cargo.lock. Android includes btleplug's upstream Java JNI support classes; those are third-party runtime dependencies, not application-owned source. Windows uses native operating-system APIs through Rust dependencies. Rust/LLVM, the Android NDK, JDK, Android SDK and Windows build tools retain their respective licenses and are not redistributed as application source.

Serde, serde_json, Tokio, futures, chrono, uuid, CSV, rusqlite, rfd, JNI, android-activity, ZIP and SHA-2 dependencies retain their upstream license terms. SQLite is public-domain software. The bundled dependencies' license texts are collected with local distributable builds. The suite license applies to original project code, not to these dependencies.

No Elitech companion application, proprietary artwork, decompiled source, design-book figure or CARI gallery asset is distributed in the application or repository.

The ANE-200W wire-profile description records observed protocol facts and companion-app behavior. The private research copy of the companion app, original manual photographs, device identifiers, field captures, compiler tools, build outputs and signing keys are excluded from this repository. Their respective owners retain their rights.
