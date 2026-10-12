//! Constructed packets and temporary files only: these are not physical meter validation.
use ane_core::*;
use rusqlite::Connection;
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use uuid::Uuid;

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let base = std::env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let path = base.join("ane-core-tests").join(Uuid::new_v4().to_string());
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn context() -> ReadingContext {
    ReadingContext {
        received_at_utc: "2026-10-12T13:00:00.123456789Z".into(),
        session_id: Uuid::new_v4().to_string(),
        sequence: 2,
        device_id: "SYNTHETIC_DEVICE".into(),
        service_uuid: "SYNTHETIC_SERVICE".into(),
        characteristic_uuid: NOTIFY_UUID.into(),
    }
}
fn packet(velocity: u16, temperature: i16, humidity: u16) -> [u8; 17] {
    let mut bytes = [
        0xff, 0x57, 0x4c, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x3b,
    ];
    bytes[4..6].copy_from_slice(&velocity.to_be_bytes());
    bytes[7..9].copy_from_slice(&temperature.to_be_bytes());
    bytes[11..13].copy_from_slice(&humidity.to_be_bytes());
    bytes
}
fn measurement(velocity: u16) -> Measurement {
    decode_packet(&packet(velocity, 230, 490), context()).unwrap()
}
fn row(velocity: Option<f64>, quality: &str) -> ImportedMeasurement {
    ImportedMeasurement {
        received_at_utc: "2026-10-12T13:00:00Z".into(),
        original_received_at: "2026-10-12T13:00:00Z".into(),
        velocity_mps: velocity,
        temperature_c: None,
        humidity_percent: None,
        quality: quality.into(),
        source: "synthetic-test".into(),
    }
}
fn close(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-9, "{a} != {b}");
}

#[test]
fn canonical_velocity_and_all_display_units() {
    for unit in 0..5 {
        let mut bytes = packet(125, 230, 491);
        bytes[6] = unit;
        let r = decode_packet(&bytes, context()).unwrap();
        close(r.velocity_mps, 1.25);
        close(r.fpm(), 1.25 * 60.0 / 0.3048);
        close(r.temperature_c, 23.0);
        close(r.humidity_percent, 49.1);
        assert_eq!(r.payload_hex, payload_hex(&bytes));
        assert_eq!(r.source, "live-bluetooth");
        assert_eq!(r.context.received_at_utc, "2026-10-12T13:00:00.123456789Z");
    }
}
#[test]
fn signed_temperature_and_unsigned_low_byte() {
    close(
        decode_packet(&packet(0, -100, 0), context())
            .unwrap()
            .temperature_c,
        -10.0,
    );
    close(
        decode_packet(&packet(0, 255, 1000), context())
            .unwrap()
            .temperature_c,
        25.5,
    );
    close(
        decode_packet(&packet(3000, 450, 0), context())
            .unwrap()
            .temperature_c,
        45.0,
    );
    let mut bytes = packet(0, 788, 490);
    bytes[10] = 1;
    let r = decode_packet(&bytes, context()).unwrap();
    close(r.temperature_c, 26.0);
    close(r.fahrenheit(), 78.8);
    assert_eq!(r.meter_temperature_unit, "F");
}
#[test]
fn invalid_frames_never_become_zero() {
    assert!(decode_packet(&[0; 16], context()).is_err());
    for index in [0, 1, 2, 3, 16] {
        let mut bytes = packet(0, 230, 490);
        bytes[index] = 0;
        assert!(decode_packet(&bytes, context()).is_err());
    }
    for (index, value) in [(6, 5), (10, 2), (13, 2)] {
        let mut bytes = packet(0, 230, 490);
        bytes[index] = value;
        assert!(decode_packet(&bytes, context()).is_err());
    }
    for bytes in [
        packet(3001, 230, 490),
        packet(0, -101, 490),
        packet(0, 451, 490),
        packet(0, 230, 1001),
    ] {
        assert!(decode_packet(&bytes, context()).is_err());
    }
    let mut bad_stamp = context();
    bad_stamp.received_at_utc = "2026-10-12T13:00:00".into();
    assert!(decode_packet(&packet(0, 230, 490), bad_stamp).is_err());
}
#[test]
fn unknown_bytes_are_preserved_without_assigned_meaning() {
    let mut bytes = packet(0, 230, 490);
    bytes[9] = 0xaa;
    bytes[14] = 0xbb;
    bytes[15] = 0xcc;
    assert_eq!(
        decode_packet(&bytes, context()).unwrap().payload_hex,
        payload_hex(&bytes)
    );
}
#[test]
fn stale_and_disconnected_are_not_fresh_zero() {
    let now = Instant::now();
    let mut live = LiveReading::new();
    assert!(live.displayed_at(now).is_none());
    assert!(live.toggle_hold_at(now).is_err());
    live.receive_at(measurement(0), now).unwrap();
    assert!(live.fresh_at(now + Duration::from_secs(5)));
    assert!(!live.fresh_at(now + Duration::from_millis(5001)));
    assert!(live.displayed_at(now + Duration::from_secs(6)).is_none());
    assert!(!live.fresh_at(now - Duration::from_millis(1)));
    live.end("Disconnected");
    assert!(live.displayed_at(now).is_none());
}
#[test]
fn hold_freezes_sample_while_receiving_and_survives_disconnect() {
    let now = Instant::now();
    let mut live = LiveReading::new();
    let first = measurement(100);
    live.receive_at(first.clone(), now).unwrap();
    live.toggle_hold_at(now).unwrap();
    live.receive_at(measurement(200), now + Duration::from_secs(1))
        .unwrap();
    close(
        live.displayed_at(now + Duration::from_secs(1))
            .unwrap()
            .velocity_mps,
        1.0,
    );
    close(live.latest().unwrap().velocity_mps, 2.0);
    live.invalidate("Malformed packet");
    assert_eq!(live.displayed_at(now), Some(&first));
    live.end("Disconnected");
    assert_eq!(
        live.displayed_at(now + Duration::from_secs(30)),
        Some(&first)
    );
    live.toggle_hold_at(now).unwrap();
    assert!(live.displayed_at(now).is_none());
}
#[test]
fn invalid_public_measurement_cannot_enter_live_state() {
    let mut live = LiveReading::new();
    let mut bad = measurement(100);
    bad.velocity_mps = f64::NAN;
    assert!(live.receive(bad).is_err());
    assert!(live.displayed().is_none());
    let mut imported = measurement(100);
    imported.source = "imported".into();
    assert!(live.receive(imported).is_err());
}
#[test]
fn each_save_creates_exactly_one_snapshot_and_keeps_original_time() {
    let dir = Temp::new();
    let mut live = LiveReading::new();
    live.receive(measurement(100)).unwrap();
    live.toggle_hold().unwrap();
    live.end("Disconnected");
    let first = live.save(&dir.0, "  Outlet 01  ").unwrap();
    let second = live.save(&dir.0, "").unwrap();
    assert_ne!(first, second);
    assert_eq!(count_saved_readings(&dir.0).unwrap(), 2);
    assert_eq!(live.saved_count, 2);
    let value: Value = serde_json::from_slice(&fs::read(first).unwrap()).unwrap();
    assert_eq!(value["display_state"], "held");
    assert_eq!(value["received_at_utc"], "2026-10-12T13:00:00.123456789Z");
    assert_eq!(value["note"], "Outlet 01");
    assert_ne!(value["received_at_utc"], value["saved_at_utc"]);
    let out = dir.path("saved.csv");
    assert_eq!(export_saved_csv(&dir.0, &out).unwrap(), 2);
    assert_eq!(import_measurements(&out).unwrap().len(), 2);
    live.toggle_hold().unwrap();
    assert!(live.save(&dir.0, "").is_err());
    assert_eq!(count_saved_readings(&dir.0).unwrap(), 2);
}
#[test]
fn unfinished_staging_file_is_not_a_saved_reading() {
    let dir = Temp::new();
    let interrupted = dir.path("interrupted-save.pending");
    fs::write(&interrupted, b"{\"type\":\"single_reading\"").unwrap();
    assert_eq!(count_saved_readings(&dir.0).unwrap(), 0);
    assert!(load_saved_readings(&dir.0).unwrap().is_empty());
    assert!(export_saved_csv(&dir.0, &dir.path("empty.csv")).is_err());

    let saved = save_single_reading(&dir.0, &measurement(125), false, "Complete save").unwrap();
    assert_eq!(saved.extension().unwrap(), "jsonl");
    assert_eq!(count_saved_readings(&dir.0).unwrap(), 1);
    let rows = load_saved_readings(&dir.0).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["note"], "Complete save");
    assert_eq!(rows[0]["velocity_mps"], 1.25);
    assert!(!saved.with_extension("pending").exists());
    assert!(interrupted.exists());
    assert_eq!(
        export_saved_csv(&dir.0, &dir.path("complete.csv")).unwrap(),
        1
    );
}

#[test]
fn save_rejects_stale_unheld_sample() {
    let dir = Temp::new();
    let mut live = LiveReading::new();
    live.receive_at(measurement(0), Instant::now() - Duration::from_secs(6))
        .unwrap();
    assert!(live.save(&dir.0, "").is_err());
    assert_eq!(count_saved_readings(&dir.0).unwrap(), 0);
}
#[test]
fn legacy_windows_saved_file_without_model_exports() {
    let dir = Temp::new();
    let path = save_single_reading(&dir.0, &measurement(100), false, "legacy").unwrap();
    let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value.as_object_mut().unwrap().remove("device_model");
    fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert_eq!(
        export_saved_csv(&dir.0, &dir.path("legacy.csv")).unwrap(),
        1
    );
}
#[test]
fn trace_windows_keep_gaps_and_reject_bad_values() {
    let mut trace = TraceBuffer::new();
    trace.add(0.0, Some(10.0)).unwrap();
    trace.add(598.0, Some(20.0)).unwrap();
    trace.add(599.0, None).unwrap();
    trace.add(600.0, Some(30.0)).unwrap();
    let points = trace.window(600.0, 5.0).unwrap();
    assert_eq!(points.len(), 3);
    assert!(points[1].fpm.is_none());
    assert_eq!(trace.window(601.0, 600.0).unwrap().len(), 3);
    assert!(trace.window(601.0, 4.0).is_err());
    assert!(trace.window(601.0, 601.0).is_err());
    assert!(trace.add(601.0, Some(f64::NAN)).is_err());
    assert!(trace.add(599.0, Some(2.0)).is_err());
}
#[test]
fn conversions_have_full_precision_and_validate_inputs() {
    close(velocity_fpm(0.3048).unwrap(), 60.0);
    close(airflow_cfm(0.3048, 2.0, 0.5).unwrap(), 60.0);
    assert_eq!(airflow_cfm(0.0, 2.0, 1.0).unwrap(), 0.0);
    for v in [-1.0, f64::NAN, f64::INFINITY, f64::MAX] {
        assert!(velocity_fpm(v).is_err());
    }
    assert!(airflow_cfm(1.0, -1.0, 1.0).is_err());
    assert!(airflow_cfm(1.0, 1.0, f64::INFINITY).is_err());
}
#[test]
fn summary_excludes_bad_rows_without_creating_zero() {
    let rows = [
        row(Some(1.0), "valid"),
        row(Some(3.0), "valid"),
        row(Some(100.0), "stale"),
        row(None, "error"),
    ];
    let stats = summarize(&rows, 1.0, 1.0).unwrap();
    assert_eq!(stats.count, 2);
    assert_eq!(stats.excluded, 2);
    close(stats.mean_mps, 2.0);
    close(stats.stddev_mps.unwrap(), 2.0f64.sqrt());
    assert!(summarize(&[row(None, "error")], 1.0, 1.0).is_err());
    assert!(summarize(&[row(None, "valid")], 1.0, 1.0).is_err());
    let zero = summarize(&[row(Some(0.0), "valid")], 1.0, 1.0).unwrap();
    assert_eq!(zero.calculated_cfm, 0.0);
    assert!(zero.stddev_mps.is_none());
}
#[test]
fn reference_fit_requires_independent_positive_references() {
    let fit = fit_effective_area(&[(500.0, 250.0), (1000.0, 500.0), (1500.0, 750.0)]).unwrap();
    close(fit.effective_area_ft2, 0.5);
    close(fit.rmse_cfm, 0.0);
    assert!(fit_effective_area(&[(0.0, 1.0), (0.0, 2.0)]).is_err());
    assert!(fit_effective_area(&[(1.0, 2.0)]).is_err());
    assert!(fit_effective_area(&[(1.0, f64::NAN), (2.0, 3.0)]).is_err());
}
#[test]
fn measurement_csv_keeps_nanosecond_origin_and_missing_error() {
    let dir = Temp::new();
    let path = dir.path("input.csv");
    fs::write(&path,"\u{feff}received_at_utc,velocity_mps,temperature_c,humidity_percent,quality,source\n2026-10-12T08:00:00.123456789-05:00,1.23456789,23.1,,valid,synthetic-test\n2026-10-12T13:00:01Z,,,,error,imported\n").unwrap();
    let rows = import_measurements(&path).unwrap();
    assert_eq!(rows[0].received_at_utc, "2026-10-12T13:00:00.123456789Z");
    assert_eq!(
        rows[0].original_received_at,
        "2026-10-12T08:00:00.123456789-05:00"
    );
    assert!(rows[1].velocity_mps.is_none());
    let out = dir.path("output.csv");
    export_measurements(&out, &rows, 2.0, 0.8).unwrap();
    assert_eq!(rows, import_measurements(&out).unwrap());
}
#[test]
fn csv_rejects_bad_numbers_timezone_and_duplicate_columns() {
    let dir = Temp::new();
    let path = dir.path("bad.csv");
    for content in [
        "received_at_utc,velocity_mps\n2026-10-12T13:00:00Z,NaN\n",
        "received_at_utc,velocity_mps\n2026-10-12T13:00:00,1\n",
        "received_at_utc,velocity_mps,velocity_mps\n2026-10-12T13:00:00Z,1,2\n",
    ] {
        fs::write(&path, content).unwrap();
        assert!(import_measurements(&path).is_err());
    }
}
fn capture(directory: &Path) -> PathBuf {
    let mut recorder = FrameRecorder::new(
        directory,
        "SYNTHETIC_DEVICE",
        NOTIFY_UUID,
        "SYNTHETIC_SERVICE",
    )
    .unwrap();
    let context = recorder.append(&[0x00, 0xff, 0x07]).unwrap();
    assert_eq!(context.sequence, 2);
    recorder.close("test").unwrap();
    assert!(recorder.append(&[0]).is_err());
    recorder.path().to_owned()
}
#[test]
fn raw_capture_and_archive_are_idempotent() {
    let dir = Temp::new();
    let path = capture(&dir.0);
    let db = dir.path("store.sqlite3");
    let report = archive_raw(&path, &db).unwrap();
    assert_eq!(report.records, 3);
    assert_eq!(report.raw_frames, 1);
    assert_eq!(report.inserted, 3);
    assert_eq!(archive_raw(&path, &db).unwrap().inserted, 0);
    let conn = Connection::open(&db).unwrap();
    let stored: String = conn
        .query_row("SELECT payload FROM raw_events WHERE sequence=2", [], |r| {
            r.get(0)
        })
        .unwrap();
    let value: Value = serde_json::from_str(&stored).unwrap();
    assert_eq!(value["payload_hex"], "00ff07");
    assert_eq!(value["decoder_status"], "unverified");
}
#[test]
fn conflicting_import_rolls_back_new_events_and_preserves_nanoseconds() {
    let dir = Temp::new();
    let path = capture(&dir.0);
    let db = dir.path("store.sqlite3");
    let mut rows: Vec<Value> = fs::read_to_string(&path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    rows[1]["received_at_utc"] = Value::String("2026-10-12T13:00:00.123456789Z".into());
    // Use compact JSONL; old archives may have different object key ordering/spacing.
    fs::write(
        &path,
        rows.iter()
            .map(|r| serde_json::to_string(r).unwrap())
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .unwrap();
    archive_raw(&path, &db).unwrap();
    let mut new = rows[1].clone();
    new["sequence"] = Value::from(4);
    rows[1]["payload_hex"] = Value::String("ff".into());
    let conflict = dir.path("conflict.jsonl");
    fs::write(&conflict, format!("{}\n{}\n", new, rows[1])).unwrap();
    assert!(archive_raw(&conflict, &db).is_err());
    let conn = Connection::open(&db).unwrap();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM raw_events", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 3);
    let stored: String = conn
        .query_row("SELECT payload FROM raw_events WHERE sequence=2", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert!(stored.contains("2026-10-12T13:00:00.123456789Z"));
    assert!(stored.contains("00ff07"));
    // Structural equality permits historical whitespace and ordering without replacement.
    let original: Value = serde_json::from_str(&stored).unwrap();
    conn.execute(
        "UPDATE raw_events SET payload=?1 WHERE sequence=2",
        [serde_json::to_string_pretty(&original).unwrap()],
    )
    .unwrap();
    assert_eq!(archive_raw(&path, &db).unwrap().inserted, 0);
}
#[test]
fn malformed_raw_event_does_not_create_archive() {
    let dir = Temp::new();
    let path = dir.path("bad.jsonl");
    let db = dir.path("bad.sqlite3");
    fs::write(
        &path,
        r#"{"schema_version":1,"device_model":"Other meter"}"#,
    )
    .unwrap();
    assert!(archive_raw(&path, &db).is_err());
    assert!(!db.exists());
}
