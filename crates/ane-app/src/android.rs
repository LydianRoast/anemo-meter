//! Android platform glue. Application logic remains Rust; btleplug supplies its JNI classes.
use android_activity::AndroidApp;
use anyhow::{anyhow, bail, Context, Result};
use jni::{
    objects::{JClass, JObject, JString, JValue},
    JNIEnv, JavaVM, NativeMethod,
};
use std::{
    ffi::c_void,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

static APP: Mutex<Option<AndroidApp>> = Mutex::new(None);
static BLE_INITIALIZED: OnceLock<std::result::Result<(), String>> = OnceLock::new();

fn app() -> Result<AndroidApp> {
    APP.lock()
        .map_err(|_| anyhow!("Android activity lock failed"))?
        .as_ref()
        .cloned()
        .ok_or_else(|| anyhow!("Android activity unavailable"))
}

fn with_activity<T>(f: impl FnOnce(&mut JNIEnv<'_>, &JObject<'_>) -> Result<T>) -> Result<T> {
    let app = app()?;
    // AndroidApp owns this global reference and keeps it valid throughout the closure.
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) }?;
    let mut env = vm.attach_current_thread()?;
    let activity = unsafe { JObject::from_raw(app.activity_as_ptr().cast()) };
    let result = env.with_local_frame(48, |env| f(env, &activity));
    if result.is_err() && env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
    }
    result
}

/// btleplug uses JNI on Tokio threads; call this from each worker's startup hook.
pub fn attach_worker_thread() -> Result<()> {
    let app = app()?;
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) }?;
    vm.attach_current_thread_permanently()?;
    Ok(())
}

// The existing dependency callback provides an application ClassLoader JNI frame.
// Initialization immediately installs btleplug's real callbacks over this temporary one.
// No scan event is fabricated or emitted to application code.
unsafe extern "system" fn initialize_in_java_frame(
    env: *mut jni::sys::JNIEnv,
    _object: jni::sys::jobject,
    _unused: jni::sys::jobject,
) {
    let result = std::panic::catch_unwind(|| {
        let legacy = jni_legacy::JNIEnv::from_raw(env.cast()).map_err(|e| e.to_string())?;
        btleplug::platform::init(&legacy).map_err(|e| e.to_string())
    })
    .unwrap_or_else(|_| Err("Bluetooth JNI initialization failed".into()));
    let _ = BLE_INITIALIZED.set(result);
}

fn initialize_bluetooth() -> Result<()> {
    if let Some(result) = BLE_INITIALIZED.get() {
        return result.clone().map_err(anyhow::Error::msg);
    }
    with_activity(|env, activity| {
        let loader = env
            .call_method(activity, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])?
            .l()?;
        let class_name =
            JObject::from(env.new_string("com.nonpolynomial.btleplug.android.impl.Adapter")?);
        let adapter_class = JClass::from(
            env.call_method(
                &loader,
                "loadClass",
                "(Ljava/lang/String;)Ljava/lang/Class;",
                &[JValue::Object(&class_name)],
            )?
            .l()?,
        );
        env.register_native_methods(
            &adapter_class,
            &[NativeMethod {
                name: "reportScanResult".into(),
                sig: "(Landroid/bluetooth/le/ScanResult;)V".into(),
                fn_ptr: initialize_in_java_frame as *mut c_void,
            }],
        )?;
        let adapter = env.new_object(&adapter_class, "()V", &[])?;
        env.call_method(
            &adapter,
            "reportScanResult",
            "(Landroid/bluetooth/le/ScanResult;)V",
            &[JValue::Object(&JObject::null())],
        )?;
        BLE_INITIALIZED
            .get()
            .ok_or_else(|| anyhow!("Bluetooth startup callback did not run"))?
            .clone()
            .map_err(anyhow::Error::msg)
    })
}

fn sdk(env: &mut JNIEnv<'_>) -> Result<i32> {
    Ok(env
        .get_static_field("android/os/Build$VERSION", "SDK_INT", "I")?
        .i()?)
}

fn has_permission(env: &mut JNIEnv<'_>, activity: &JObject<'_>, permission: &str) -> Result<bool> {
    let permission = JObject::from(env.new_string(permission)?);
    Ok(env
        .call_method(
            activity,
            "checkSelfPermission",
            "(Ljava/lang/String;)I",
            &[JValue::Object(&permission)],
        )?
        .i()?
        == 0)
}

fn request_permissions(permissions: Vec<&'static str>) -> Result<()> {
    let app = app()?;
    app.run_on_java_main_thread(Box::new(move || {
        if let Err(error) = with_activity(|env, activity| {
            let array = env.new_object_array(
                permissions.len() as i32,
                "java/lang/String",
                JObject::null(),
            )?;
            for (index, permission) in permissions.iter().enumerate() {
                let value = env.new_string(permission)?;
                env.set_object_array_element(&array, index as i32, value)?;
            }
            env.call_method(
                activity,
                "requestPermissions",
                "([Ljava/lang/String;I)V",
                &[JValue::Object(array.as_ref()), JValue::Int(3401)],
            )?;
            Ok(())
        }) {
            log::error!("Permission request failed: {error}");
        }
    }));
    Ok(())
}

/// Returns false after requesting permission; retry Scan once Android grants it.
pub fn ensure_permissions() -> Result<bool> {
    initialize_bluetooth()?;
    let missing = with_activity(|env, activity| {
        let required = if sdk(env)? >= 31 {
            vec![
                "android.permission.BLUETOOTH_SCAN",
                "android.permission.BLUETOOTH_CONNECT",
            ]
        } else {
            vec!["android.permission.ACCESS_FINE_LOCATION"]
        };
        let mut missing = Vec::new();
        for permission in required {
            if !has_permission(env, activity, permission)? {
                missing.push(permission);
            }
        }
        Ok(missing)
    })?;
    if missing.is_empty() {
        return Ok(true);
    }
    request_permissions(missing)?;
    Ok(false)
}

/// Conservative foreground check for active capture. Permission/dialog windows may pause capture.
pub fn has_window_focus() -> bool {
    with_activity(|env, activity| {
        Ok(env
            .call_method(activity, "hasWindowFocus", "()Z", &[])?
            .z()?)
    })
    .unwrap_or(false)
}

/// Keep the display awake only while a user-started live capture is running.
pub fn set_keep_screen_on(active: bool) -> Result<()> {
    app()?.run_on_java_main_thread(Box::new(move || {
        if let Err(error) = with_activity(|env, activity| {
            let window = env
                .call_method(activity, "getWindow", "()Landroid/view/Window;", &[])?
                .l()?;
            let method = if active { "addFlags" } else { "clearFlags" };
            env.call_method(window, method, "(I)V", &[JValue::Int(128)])?;
            Ok(())
        }) {
            log::error!("Screen wake setting failed: {error}");
        }
    }));
    Ok(())
}

/// Saves an explicit export in the Android Downloads collection without exposing internal files.
pub fn export_text(name: &str, contents: &str) -> Result<String> {
    if name.is_empty()
        || Path::new(name).file_name().and_then(|n| n.to_str()) != Some(name)
        || name.contains('\\')
    {
        bail!("Invalid export filename");
    }
    let mime = match Path::new(name).extension().and_then(|s| s.to_str()) {
        Some("csv") => "text/csv",
        Some("jsonl") => "application/x-ndjson",
        _ => bail!("Only CSV and JSONL exports are supported"),
    };
    let modern = with_activity(|env, _| Ok(sdk(env)? >= 29))?;
    if !modern {
        let granted = with_activity(|env, activity| {
            has_permission(env, activity, "android.permission.WRITE_EXTERNAL_STORAGE")
        })?;
        if !granted {
            request_permissions(vec!["android.permission.WRITE_EXTERNAL_STORAGE"])?;
            bail!("Allow storage access, then tap Export again");
        }
        let directory = with_activity(|env, _| {
            let downloads = JObject::from(env.new_string("Download")?);
            let file = env
                .call_static_method(
                    "android/os/Environment",
                    "getExternalStoragePublicDirectory",
                    "(Ljava/lang/String;)Ljava/io/File;",
                    &[JValue::Object(&downloads)],
                )?
                .l()?;
            let path = JString::from(
                env.call_method(file, "getAbsolutePath", "()Ljava/lang/String;", &[])?
                    .l()?,
            );
            let path_text = String::from(env.get_string(&path)?);
            Ok(PathBuf::from(path_text))
        })?
        .join("ANE-200W");
        std::fs::create_dir_all(&directory)?;
        let path = directory.join(name);
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)?;
        file.write_all(contents.as_bytes())?;
        file.sync_all()?;
        return Ok(format!("Downloads/ANE-200W/{name}"));
    }
    with_activity(|env, activity| {
        let resolver = env
            .call_method(
                activity,
                "getContentResolver",
                "()Landroid/content/ContentResolver;",
                &[],
            )?
            .l()?;
        let values = env.new_object("android/content/ContentValues", "()V", &[])?;
        for (key, value) in [
            ("_display_name", name),
            ("mime_type", mime),
            ("relative_path", "Download/ANE-200W"),
            ("is_pending", "1"),
        ] {
            let key = JObject::from(env.new_string(key)?);
            let value = JObject::from(env.new_string(value)?);
            env.call_method(
                &values,
                "put",
                "(Ljava/lang/String;Ljava/lang/String;)V",
                &[JValue::Object(&key), JValue::Object(&value)],
            )?;
        }
        let collection = env
            .get_static_field(
                "android/provider/MediaStore$Downloads",
                "EXTERNAL_CONTENT_URI",
                "Landroid/net/Uri;",
            )?
            .l()?;
        let uri = env
            .call_method(
                &resolver,
                "insert",
                "(Landroid/net/Uri;Landroid/content/ContentValues;)Landroid/net/Uri;",
                &[JValue::Object(&collection), JValue::Object(&values)],
            )?
            .l()?;
        if uri.is_null() {
            bail!("Android could not create a Downloads item");
        }
        let result: Result<()> = (|| {
            let output = env
                .call_method(
                    &resolver,
                    "openOutputStream",
                    "(Landroid/net/Uri;)Ljava/io/OutputStream;",
                    &[JValue::Object(&uri)],
                )?
                .l()?;
            if output.is_null() {
                bail!("Android could not open the export");
            }
            let bytes = env.byte_array_from_slice(contents.as_bytes())?;
            let written =
                env.call_method(&output, "write", "([B)V", &[JValue::Object(bytes.as_ref())]);
            if written.is_err() {
                let _ = env.exception_clear();
            }
            let closed = env.call_method(&output, "close", "()V", &[]);
            written?;
            closed?;
            env.call_method(&values, "clear", "()V", &[])?;
            let key = JObject::from(env.new_string("is_pending")?);
            let value = JObject::from(env.new_string("0")?);
            env.call_method(
                &values,
                "put",
                "(Ljava/lang/String;Ljava/lang/String;)V",
                &[JValue::Object(&key), JValue::Object(&value)],
            )?;
            let updated = env.call_method(&resolver, "update", "(Landroid/net/Uri;Landroid/content/ContentValues;Ljava/lang/String;[Ljava/lang/String;)I", &[JValue::Object(&uri), JValue::Object(&values), JValue::Object(&JObject::null()), JValue::Object(&JObject::null())])?.i()?;
            if updated != 1 {
                bail!("Android did not finalize the export");
            }
            Ok(())
        })();
        if let Err(error) = result {
            let _ = env.exception_clear();
            let _ = env.call_method(
                &resolver,
                "delete",
                "(Landroid/net/Uri;Ljava/lang/String;[Ljava/lang/String;)I",
                &[
                    JValue::Object(&uri),
                    JValue::Object(&JObject::null()),
                    JValue::Object(&JObject::null()),
                ],
            );
            return Err(error.context("Export failed; incomplete Downloads item was removed"));
        }
        Ok(format!("Downloads/ANE-200W/{name}"))
    })
}

pub fn export_to_downloads(path: &Path) -> Result<String> {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow!("Invalid export filename"))?;
    let contents = std::fs::read_to_string(path).context("Cannot read prepared export")?;
    export_text(name, &contents)
}

#[no_mangle]
pub fn android_main(android: AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default()
            .with_tag("ANE-200W")
            .with_max_level(log::LevelFilter::Info),
    );
    if let Ok(mut app) = APP.lock() {
        *app = Some(android.clone());
    }
    let data = android.internal_data_path();
    if let Err(error) = initialize_bluetooth() {
        log::error!("Bluetooth initialization: {error:#}");
    }
    let options = eframe::NativeOptions {
        android_app: Some(android),
        ..Default::default()
    };
    if let Err(error) = eframe::run_native(
        "ANE-200W Console",
        options,
        Box::new(move |cc| Ok(Box::new(crate::app::Console::new(cc, data)))),
    ) {
        log::error!("Application stopped: {error}");
    }
    if let Ok(mut app) = APP.lock() {
        *app = None;
    }
}
