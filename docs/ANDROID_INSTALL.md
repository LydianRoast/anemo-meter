# Android package and installation

`ANE-200W-0.4.1.apk` is the Rust application package. It targets **64-bit ARM phones**, Android 8.0/API 26 or newer, Bluetooth LE and OpenGL ES 3.0. This build does not include 32-bit ARM or x86 emulator libraries.

Transfer or email the APK to the phone, download it and open it. Allow installation from that email app or file manager if Android requests it. The package ID remains `com.kevin.ane200w`; signing with the existing private certificate and version code 6 permits an update over 0.1.0/0.2.0 while retaining app-private files. Do not uninstall first if you need those files.

Follow [the technician guide](../README-TECHNICIAN.md) to scan, connect, read, hold, save and export. The app requests Nearby Devices permissions on Android 12+, and location permission for scanning on older Android versions. Android 8–9 may request storage permission for exports. Exports are copied into `Downloads/ANE-200W`; captured evidence and saved readings stay in the app's private files directory.

The app uses foreground capture. Keep it open while measuring; losing focus can stop and disconnect the session. Interrupted periods are not recorded as zero. Compilation, signature checks and source review do not establish physical phone operation or instrument accuracy.

## Build

The workspace owns Rust source only. The Android entry point is the platform NativeActivity; btleplug supplies its third-party Java Bluetooth bridge. The Rust `xtask` compiles that dependency bridge, creates DEX, packages the Rust native library, aligns and signs the APK.

The builder expects tools under ignored `local/android-toolchain/`: JDK 17, Android platform 35, build-tools 35 and NDK r30. The Rust `aarch64-linux-android` target is required. Toolchain paths can be changed with `ANE_ANDROID_TOOLS` and `ANDROID_NDK_HOME`.

```powershell
$env:CARGO_HOME = "$PWD\local\cargo"
$env:TEMP = "$PWD\local\temp"
$env:TMP = $env:TEMP
rustup target add aarch64-linux-android
cargo xtask apk
```

`cargo xtask apk --debug` builds a development native library. `--skip-build` repackages an already-built library for the selected profile. Native ELF and APK alignment use 16 KB pages. The builder verifies ZIP entries, signature, alignment and package metadata, and writes a private receipt under `local/android-rust-build/`.

Signing credentials are read from `local/signing/ane200w.p12` and `local/signing/password.txt`, or from paths specified by `ANE_KEYSTORE` and `ANE_KEYSTORE_PASSWORD_FILE`. The password is passed privately through a child-process environment value. Preserve these files for future updates, and do not distribute them with the APK.
