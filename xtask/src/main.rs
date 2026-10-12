//! Compact Android APK builder. All project-owned build logic is Rust.
use anyhow::{anyhow, bail, Context, Result};
use sha2::{Digest, Sha256};
use std::{
    env,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use zip::{write::SimpleFileOptions, CompressionMethod, ZipArchive, ZipWriter};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const TARGET: &str = "aarch64-linux-android";

fn main() -> Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.first().map(String::as_str) != Some("apk") {
        println!("Usage: cargo run -p xtask -- apk [--skip-build] [--debug]");
        println!("Requires the local Android toolchain, NDK, Rust Android target and local development signing key.");
        return Ok(());
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned();
    let tools = env::var_os("ANE_ANDROID_TOOLS")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("local/android-toolchain"));
    let ndk = env::var_os("ANDROID_NDK_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| tools.join("android-ndk-r30"));
    let build = root.join("local/android-rust-build");
    fs::create_dir_all(&build)?;
    let java = tools.join("jdk/bin/java.exe");
    let javac = tools.join("jdk/bin/javac.exe");
    let bt = tools.join("build-tools");
    let android_jar = tools.join("platform/android.jar");
    for path in [
        &java,
        &javac,
        &android_jar,
        &bt.join("aapt2.exe"),
        &bt.join("zipalign.exe"),
    ] {
        if !path.is_file() {
            bail!("Required tool missing: {}", path.display());
        }
    }
    let release = !args.iter().any(|a| a == "--debug");
    let target_dir = env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("local/target"));
    let profile = if release { "release" } else { "debug" };
    let library = target_dir.join(TARGET).join(profile).join("libane_app.so");
    if !args.iter().any(|a| a == "--skip-build") {
        let compiler = ndk
            .join("toolchains/llvm/prebuilt/windows-x86_64/bin/aarch64-linux-android26-clang.cmd");
        if !compiler.is_file() {
            bail!("Android NDK compiler missing: {}", compiler.display());
        }
        let mut cargo = Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
        cargo
            .current_dir(&root)
            .args(["build", "-p", "ane-app", "--lib", "--target", TARGET]);
        if release {
            cargo.arg("--release");
        }
        cargo
            .env("CARGO_TARGET_DIR", &target_dir)
            .env("CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER", &compiler)
            .env("CC_aarch64_linux_android", &compiler)
            .env(
                "AR_aarch64_linux_android",
                ndk.join("toolchains/llvm/prebuilt/windows-x86_64/bin/llvm-ar.exe"),
            )
            .env(
                "CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS",
                "-C link-arg=-Wl,-z,max-page-size=16384 -C link-arg=-Wl,-z,common-page-size=16384",
            );
        run(&mut cargo, "Build Android native library")?;
    }
    if !library.is_file() {
        bail!("Native library missing: {}", library.display());
    }
    let readelf = ndk.join("toolchains/llvm/prebuilt/windows-x86_64/bin/llvm-readelf.exe");
    validate_native_alignment(&readelf, &library)?;
    let mut native_libraries = vec![library.clone()];
    let dependencies = Command::new(&readelf)
        .arg("--dynamic")
        .arg(&library)
        .output()?;
    if !dependencies.status.success() {
        bail!("Cannot inspect native library dependencies");
    }
    if String::from_utf8_lossy(&dependencies.stdout).contains("libc++_shared.so") {
        let runtime = ndk.join("toolchains/llvm/prebuilt/windows-x86_64/sysroot/usr/lib/aarch64-linux-android/libc++_shared.so");
        validate_native_alignment(&readelf, &runtime)?;
        native_libraries.push(runtime);
    }

    let manifest = root.join("android-native/AndroidManifest.xml");
    let manifest_text = fs::read_to_string(&manifest)?;
    if !manifest_text.contains(&format!("android:versionName=\"{VERSION}\"")) {
        bail!("Manifest version does not match Rust package version {VERSION}");
    }
    let assets = build.join("assets");
    fs::create_dir_all(assets.join("licenses"))?;
    for name in ["LICENSE", "THIRD_PARTY_NOTICES.md"] {
        fs::copy(root.join(name), assets.join("licenses").join(name))?;
    }
    fs::copy(
        root.join("assets/fonts/OFL.txt"),
        assets.join("licenses/OFL.txt"),
    )?;
    collect_dependency_licenses(&root, &assets.join("licenses/dependencies"))?;
    if root.join("design/tokens.json").is_file() {
        fs::copy(root.join("design/tokens.json"), assets.join("tokens.json"))?;
    }
    let resources = build.join("resources.apk");
    run(
        Command::new(bt.join("aapt2.exe")).args([
            OsStr::new("link"),
            OsStr::new("-o"),
            resources.as_os_str(),
            OsStr::new("-I"),
            android_jar.as_os_str(),
            OsStr::new("--manifest"),
            manifest.as_os_str(),
            OsStr::new("-A"),
            assets.as_os_str(),
        ]),
        "Compile Android manifest",
    )?;

    let registry = cargo_registry(&root)?;
    let btleplug = find_crate(&registry, "btleplug-0.12.0")?;
    let java_root = btleplug.join("src/droidplug/java/src/main/java");
    let mut java_sources = Vec::new();
    walk_files(&java_root, &mut java_sources)?;
    java_sources.retain(|p| p.extension() == Some(OsStr::new("java")));
    if java_sources.is_empty() {
        bail!("btleplug's third-party Java support sources are missing");
    }
    let classes = build.join("classes");
    let dex = build.join("dex");
    fs::create_dir_all(&classes)?;
    fs::create_dir_all(&dex)?;
    let mut compile = Command::new(javac);
    compile
        .args(["--release", "8", "-encoding", "UTF-8", "-classpath"])
        .arg(&android_jar)
        .arg("-d")
        .arg(&classes)
        .args(&java_sources);
    run(&mut compile, "Compile dependency Android Bluetooth bridge")?;
    let input_jar = build.join("btleplug-jni.jar");
    zip_tree(&classes, &input_jar)?;
    run(
        Command::new(&java)
            .args(["-Xmx256m", "-cp"])
            .arg(bt.join("lib/d8.jar"))
            .args(["com.android.tools.r8.D8", "--min-api", "26", "--lib"])
            .arg(&android_jar)
            .arg("--output")
            .arg(&dex)
            .arg(&input_jar),
        "Dex dependency bridge",
    )?;

    let unsigned = build.join("unsigned.apk");
    let mut output = ZipWriter::new(fs::File::create(&unsigned)?);
    let mut base = ZipArchive::new(fs::File::open(&resources)?)?;
    for index in 0..base.len() {
        let mut entry = base.by_index(index)?;
        if entry.is_dir() {
            continue;
        }
        let opts = SimpleFileOptions::default().compression_method(entry.compression());
        output.start_file(entry.name().replace('\\', "/"), opts)?;
        std::io::copy(&mut entry, &mut output)?;
    }
    let mut dex_files = Vec::new();
    walk_files(&dex, &mut dex_files)?;
    for path in dex_files
        .into_iter()
        .filter(|p| p.extension() == Some(OsStr::new("dex")))
    {
        output.start_file(
            path.file_name().unwrap().to_string_lossy(),
            SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
        )?;
        std::io::copy(&mut fs::File::open(path)?, &mut output)?;
    }
    for native in &native_libraries {
        output.start_file(
            format!(
                "lib/arm64-v8a/{}",
                native.file_name().unwrap().to_string_lossy()
            ),
            // extractNativeLibs=true allows compression for a smaller email attachment.
            // The extracted ELF retains its independently verified 16 KB alignment.
            SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
        )?;
        std::io::copy(&mut fs::File::open(native)?, &mut output)?;
    }
    output.finish()?;
    let aligned = build.join("aligned.apk");
    run(
        Command::new(bt.join("zipalign.exe"))
            .args(["-P", "16", "-f", "4"])
            .arg(&unsigned)
            .arg(&aligned),
        "Align APK (16 KB native pages)",
    )?;

    let signing = root.join("local/signing");
    let keystore = env::var_os("ANE_KEYSTORE")
        .map(PathBuf::from)
        .unwrap_or_else(|| signing.join("ane200w.p12"));
    let password_file = env::var_os("ANE_KEYSTORE_PASSWORD_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| signing.join("password.txt"));
    if !keystore.is_file() || !password_file.is_file() {
        bail!("Development signing key/password file missing; provide ANE_KEYSTORE and ANE_KEYSTORE_PASSWORD_FILE. Keep both private.");
    }
    let secret =
        fs::read_to_string(&password_file).context("Cannot read private signing password")?;
    if secret.trim().is_empty() {
        bail!("Signing password file is empty");
    }
    let apk = root.join(format!("ANE-200W-{VERSION}.apk"));
    // The secret is an environment value only: never command text or logged arguments.
    run(
        Command::new(&java)
            .arg("-jar")
            .arg(bt.join("lib/apksigner.jar"))
            .args(["sign", "--ks"])
            .arg(&keystore)
            .args([
                "--ks-key-alias",
                "ane200w",
                "--ks-pass",
                "env:ANE_STORE_PASS",
                "--key-pass",
                "env:ANE_STORE_PASS",
                "--out",
            ])
            .arg(&apk)
            .arg(&aligned)
            .env("ANE_STORE_PASS", secret.trim()),
        "Sign APK",
    )?;
    run(
        Command::new(&java)
            .arg("-jar")
            .arg(bt.join("lib/apksigner.jar"))
            .args(["verify", "--verbose", "--print-certs"])
            .arg(&apk),
        "Verify APK signature",
    )?;
    run(
        Command::new(bt.join("zipalign.exe"))
            .args(["-c", "-P", "16", "4"])
            .arg(&apk),
        "Verify APK alignment",
    )?;
    run(
        Command::new(bt.join("aapt2.exe"))
            .args(["dump", "badging"])
            .arg(&apk),
        "Verify package metadata",
    )?;
    let mut zip = ZipArchive::new(fs::File::open(&apk)?)?;
    for expected in [
        "AndroidManifest.xml",
        "classes.dex",
        "lib/arm64-v8a/libane_app.so",
        "assets/licenses/LICENSE",
    ] {
        zip.by_name(expected)
            .with_context(|| format!("APK entry missing: {expected}"))?;
    }
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        std::io::copy(&mut entry, &mut std::io::sink())?;
    }
    let bytes = fs::read(&apk)?;
    let hash = format!("{:x}", Sha256::digest(&bytes));
    let receipt = serde_json::json!({"apk":apk,"bytes":bytes.len(),"sha256":hash,"version":VERSION,"architecture":"arm64-v8a","minimum_android":26,"target_android":35,"rust_application":true,"dependency_java_sources":java_sources.len(),"physical_phone_tested":false});
    fs::write(
        build.join("build-receipt.json"),
        serde_json::to_vec_pretty(&receipt)?,
    )?;
    println!(
        "APK ready: {}\nBytes: {}\nSHA256: {}",
        apk.display(),
        bytes.len(),
        hash
    );
    Ok(())
}

fn run(command: &mut Command, label: &str) -> Result<()> {
    println!("{label}");
    let status = command
        .status()
        .with_context(|| format!("Cannot start: {label}"))?;
    if !status.success() {
        bail!("{label} failed with {status}");
    }
    Ok(())
}

fn validate_native_alignment(readelf: &Path, library: &Path) -> Result<()> {
    let output = Command::new(readelf)
        .args(["--program-headers", "--wide"])
        .arg(library)
        .output()
        .context("Cannot inspect ELF native library alignment")?;
    if !output.status.success() {
        bail!("ELF inspection failed for {}", library.display());
    }
    let report = String::from_utf8(output.stdout)?;
    let mut segments = 0;
    for line in report
        .lines()
        .filter(|line| line.trim_start().starts_with("LOAD "))
    {
        let alignment = line
            .split_whitespace()
            .last()
            .context("Missing ELF LOAD alignment")?;
        let alignment = u64::from_str_radix(alignment.trim_start_matches("0x"), 16)?;
        if alignment < 16_384 {
            bail!(
                "{} has a LOAD segment below 16 KB alignment",
                library.display()
            );
        }
        segments += 1;
    }
    if segments == 0 {
        bail!("ELF library has no loadable segments");
    }
    println!(
        "Verified 16 KB ELF alignment: {}",
        library.file_name().unwrap().to_string_lossy()
    );
    Ok(())
}

fn walk_files(root: &Path, paths: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            walk_files(&entry.path(), paths)?;
        } else if entry.file_type()?.is_file() {
            paths.push(entry.path());
        }
    }
    paths.sort();
    Ok(())
}

fn zip_tree(root: &Path, target: &Path) -> Result<()> {
    let mut output = ZipWriter::new(fs::File::create(target)?);
    let mut files = Vec::new();
    walk_files(root, &mut files)?;
    for file in files {
        output.start_file(
            file.strip_prefix(root)?
                .to_string_lossy()
                .replace('\\', "/"),
            SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
        )?;
        std::io::copy(&mut fs::File::open(&file)?, &mut output)?;
    }
    output.finish()?;
    Ok(())
}

fn cargo_registry(root: &Path) -> Result<PathBuf> {
    let cargo_home = env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("local/cargo"));
    let registry = cargo_home.join("registry/src");
    if !registry.is_dir() {
        bail!("Cargo registry not found. Run cargo fetch first.");
    }
    Ok(registry)
}

fn find_crate(registry: &Path, name: &str) -> Result<PathBuf> {
    for entry in fs::read_dir(registry)? {
        let path = entry?.path().join(name);
        if path.is_dir() {
            return Ok(path);
        }
    }
    Err(anyhow!(
        "Cargo registry does not contain {name}; run cargo fetch"
    ))
}

fn collect_dependency_licenses(root: &Path, destination: &Path) -> Result<()> {
    println!("Collect locked dependency license notices");
    let output = Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .current_dir(root)
        .args([
            "metadata",
            "--locked",
            "--format-version",
            "1",
            "--filter-platform",
            TARGET,
        ])
        .output()
        .context("Cannot read Cargo license metadata")?;
    if !output.status.success() {
        bail!(
            "Cargo license metadata failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let nodes = metadata["resolve"]["nodes"]
        .as_array()
        .context("Missing Cargo dependency graph")?;
    let selected: std::collections::HashSet<&str> =
        nodes.iter().filter_map(|n| n["id"].as_str()).collect();
    fs::create_dir_all(destination)?;
    let mut catalog = Vec::new();
    let mut missing_license_files = Vec::new();
    for package in metadata["packages"]
        .as_array()
        .context("Missing Cargo package metadata")?
    {
        if !selected.contains(package["id"].as_str().unwrap_or("")) || package["source"].is_null() {
            continue;
        }
        let name = package["name"]
            .as_str()
            .context("Missing dependency name")?;
        let version = package["version"]
            .as_str()
            .context("Missing dependency version")?;
        let manifest = Path::new(
            package["manifest_path"]
                .as_str()
                .context("Missing dependency manifest")?,
        );
        let directory = manifest.parent().context("Invalid dependency directory")?;
        let target = destination.join(format!("{name}-{version}"));
        fs::create_dir_all(&target)?;
        let mut notices = Vec::new();
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let filename = entry.file_name();
            let lower = filename.to_string_lossy().to_ascii_lowercase();
            if lower.starts_with("license")
                || lower.starts_with("licence")
                || lower.starts_with("copying")
                || lower.starts_with("notice")
                || lower.starts_with("copyright")
            {
                if entry.file_type()?.is_file() {
                    fs::copy(entry.path(), target.join(&filename))?;
                    notices.push(filename.to_string_lossy().into_owned());
                } else if entry.file_type()?.is_dir() {
                    let mut files = Vec::new();
                    walk_files(&entry.path(), &mut files)?;
                    for file in files {
                        let relative = file.strip_prefix(directory)?;
                        let output = target.join(relative);
                        fs::create_dir_all(output.parent().unwrap())?;
                        fs::copy(&file, output)?;
                        notices.push(relative.to_string_lossy().into_owned());
                    }
                }
            }
        }
        if let Some(filename) = package["license_file"].as_str() {
            let source = directory.join(filename);
            let output_name = source
                .file_name()
                .context("Invalid dependency license path")?;
            if source.is_file() {
                fs::copy(&source, target.join(output_name))?;
                notices.push(output_name.to_string_lossy().into_owned());
            }
        }
        // Workspace crates sometimes omit their upstream repository's license files
        // from the crates.io package. Retain the separately reviewed upstream notice.
        let upstream = match name {
            "ecolor"
            | "eframe"
            | "egui"
            | "egui-wgpu"
            | "egui-winit"
            | "egui_glow"
            | "emath"
            | "epaint"
            | "epaint_default_fonts" => Some("emilk-egui"),
            "gl_generator" | "khronos_api" => Some("brendanzab-gl-rs"),
            "hexf-parse" => Some("lifthrasiir-hexf"),
            "jni" | "jni-macros" => Some("jni-rs-jni-rs"),
            "jni-sys-macros" => Some("jni-rs-jni-sys"),
            "naga" | "wgpu-core-deps-windows-linux-android" => Some("gfx-rs-wgpu"),
            "ndk" | "ndk-sys" => Some("rust-mobile-ndk"),
            "ndk-context" => Some("rust-windowing-android-ndk-rs"),
            "profiling" => Some("aclysma-profiling"),
            _ => None,
        };
        if let Some(group) = upstream {
            let source = root.join("licenses/upstream").join(group);
            let mut files = Vec::new();
            walk_files(&source, &mut files)?;
            for file in files {
                let filename = file
                    .file_name()
                    .context("Invalid upstream notice filename")?;
                fs::copy(&file, target.join(filename))?;
                notices.push(filename.to_string_lossy().into_owned());
            }
        }
        if name == "epaint_default_fonts" {
            for filename in [
                "OFL.txt",
                "UFL.txt",
                "Hack-Regular.txt",
                "emoji-icon-font-mit-license.txt",
            ] {
                fs::copy(
                    directory.join("fonts").join(filename),
                    target.join(filename),
                )?;
                notices.push(filename.into());
            }
        }
        // Some crates put their license notice only in README. Retain it, and report this limitation.
        let has_dedicated_license_file = !notices.is_empty();
        if !has_dedicated_license_file {
            missing_license_files.push(format!("{name}-{version}"));
            for filename in ["README.md", "README", "readme.md"] {
                let source = directory.join(filename);
                if source.is_file() {
                    fs::copy(source, target.join(filename))?;
                    notices.push(filename.into());
                    break;
                }
            }
        }
        catalog.push(serde_json::json!({"crate":name,"version":version,"license":package["license"],"repository":package["repository"],"has_dedicated_license_file":has_dedicated_license_file,"included_notice_files":notices}));
    }
    fs::write(
        destination.join("index.json"),
        serde_json::to_vec_pretty(&catalog)?,
    )?;
    fs::write(
        destination.join("notice-audit.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"resolved_crates":catalog.len(),"crates_without_dedicated_license_file":missing_license_files,"note":"README retained when available for packages without a dedicated license file; SPDX metadata alone is not a full license text."}),
        )?,
    )?;
    if !missing_license_files.is_empty() {
        println!("License audit: {} crates ship no dedicated license file; see assets/licenses/dependencies/notice-audit.json", missing_license_files.len());
    }
    Ok(())
}
