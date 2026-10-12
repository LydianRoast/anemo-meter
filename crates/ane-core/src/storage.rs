use crate::{
    finite, invalid, parse_timestamp, payload_hex, utc_now, valid_hex, CoreError, Measurement,
    ReadingContext, Result, MODEL,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
    time::Instant,
};
use uuid::Uuid;

fn parent_directory(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

/// One new JSONL file per explicit Save action. Holds preserve receive time.
/// A private staging file is synchronized before an atomic rename publishes it;
/// incomplete writes never enter the saved-reading list or its CSV exports.
pub fn save_single_reading(
    directory: &Path,
    measurement: &Measurement,
    held: bool,
    note: &str,
) -> Result<PathBuf> {
    measurement.validate()?;
    fs::create_dir_all(directory)?;
    let reading_id = Uuid::new_v4().to_string();
    let mut record = serde_json::to_value(measurement)?;
    let object = record
        .as_object_mut()
        .expect("measurement is a JSON object");
    object.insert("schema_version".into(), json!(1));
    object.insert("type".into(), json!("single_reading"));
    object.insert("reading_id".into(), json!(reading_id));
    object.insert("saved_at_utc".into(), json!(utc_now()));
    object.insert(
        "display_state".into(),
        json!(if held { "held" } else { "live" }),
    );
    object.insert("velocity_fpm".into(), json!(measurement.fpm()));
    object.insert("note".into(), json!(note.trim()));
    let path = directory.join(format!("{reading_id}.jsonl"));
    let pending = directory.join(format!("{reading_id}.pending"));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&pending)?;
    let written = (|| -> Result<()> {
        serde_json::to_writer(&mut file, &record)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        Ok(())
    })();
    // Close the handle before rename/removal, including on Windows.
    drop(file);
    let published = written.and_then(|()| fs::rename(&pending, &path).map_err(CoreError::from));
    if let Err(error) = published {
        // Only remove this call's exclusively created staging file. A process
        // interruption can still leave .pending evidence, which readers ignore.
        let _ = fs::remove_file(&pending);
        return Err(error);
    }
    Ok(path)
}

fn saved_paths(directory: &Path) -> Result<Vec<PathBuf>> {
    if !directory.exists() {
        return Ok(Vec::new());
    }
    let mut paths = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if entry.file_type()?.is_file() && entry.path().extension().is_some_and(|x| x == "jsonl") {
            paths.push(entry.path());
        }
    }
    paths.sort();
    Ok(paths)
}
pub fn count_saved_readings(directory: &Path) -> Result<usize> {
    Ok(saved_paths(directory)?.len())
}

/// Reads prior Android/Windows single-reading JSONL as well as this Rust format.
/// Optional fields absent from old Windows files stay absent; they are not invented.
pub fn load_saved_readings(directory: &Path) -> Result<Vec<Value>> {
    let paths = saved_paths(directory)?;
    if paths.len() > 100_000 {
        return invalid("Saved export is limited to 100,000 readings");
    }
    let mut rows = Vec::new();
    for path in paths {
        let file = File::open(&path)?;
        if file.metadata()?.len() > 1024 * 1024 {
            return invalid("Single-reading file exceeds 1 MB");
        }
        let row: Value = serde_json::from_reader(file)?;
        if row.get("schema_version").and_then(Value::as_u64) != Some(1)
            || row.get("type").and_then(Value::as_str) != Some("single_reading")
        {
            return invalid(format!(
                "Unsupported single-reading file: {}",
                path.display()
            ));
        }
        if row
            .get("device_model")
            .and_then(Value::as_str)
            .is_some_and(|m| m != MODEL)
        {
            return invalid("Wrong instrument model in saved reading");
        }
        for field in ["received_at_utc", "saved_at_utc"] {
            parse_timestamp(
                row.get(field)
                    .and_then(Value::as_str)
                    .ok_or_else(|| CoreError(format!("Missing {field}")))?,
            )?;
        }
        let velocity = row
            .get("velocity_mps")
            .and_then(Value::as_f64)
            .ok_or_else(|| CoreError("Saved reading has no velocity".into()))?;
        finite(velocity, "Velocity", Some(0.0))?;
        for field in ["temperature_c", "humidity_percent", "velocity_fpm"] {
            if let Some(value) = row.get(field).filter(|v| !v.is_null()) {
                let number = value
                    .as_f64()
                    .ok_or_else(|| CoreError(format!("Invalid {field}")))?;
                finite(number, field, None)?;
                if field == "humidity_percent" && !(0.0..=100.0).contains(&number) {
                    return invalid("Humidity must be between 0 and 100");
                }
            }
        }
        rows.push(row);
    }
    rows.sort_by(|a, b| a["saved_at_utc"].as_str().cmp(&b["saved_at_utc"].as_str()));
    Ok(rows)
}

pub fn export_saved_csv(directory: &Path, destination: &Path) -> Result<usize> {
    let rows = load_saved_readings(directory)?;
    if rows.is_empty() {
        return invalid("No saved single readings yet");
    }
    parent_directory(destination)?;
    let fields = [
        "reading_id",
        "received_at_utc",
        "saved_at_utc",
        "display_state",
        "velocity_mps",
        "velocity_fpm",
        "temperature_c",
        "humidity_percent",
        "quality",
        "source",
        "decoder_status",
        "note",
        "session_id",
        "sequence",
        "payload_hex",
    ];
    let mut writer = csv::Writer::from_path(destination)?;
    writer.write_record(fields)?;
    for row in &rows {
        writer.write_record(fields.iter().map(|name| match row.get(*name) {
            None | Some(Value::Null) => String::new(),
            Some(Value::String(text)) => text.clone(),
            Some(value) => value.to_string(),
        }))?;
    }
    writer.flush()?;
    writer.get_ref().sync_all()?;
    Ok(rows.len())
}

/// Append-only raw capture, with the unmodified payload recorded before decoding.
pub struct FrameRecorder {
    path: PathBuf,
    session_id: String,
    device_id: String,
    characteristic_uuid: String,
    service_uuid: String,
    stream: BufWriter<File>,
    clock: Instant,
    sequence: u64,
    closed: bool,
}
impl FrameRecorder {
    pub fn new(
        directory: &Path,
        device_id: &str,
        characteristic_uuid: &str,
        service_uuid: &str,
    ) -> Result<Self> {
        fs::create_dir_all(directory)?;
        let session_id = Uuid::new_v4().to_string();
        let path = directory.join(format!("{session_id}.jsonl"));
        let file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)?;
        let mut recorder = Self {
            path,
            session_id,
            device_id: device_id.into(),
            characteristic_uuid: characteristic_uuid.into(),
            service_uuid: service_uuid.into(),
            stream: BufWriter::new(file),
            clock: Instant::now(),
            sequence: 0,
            closed: false,
        };
        recorder.event(
            "session_start",
            json!({ "decoder_status": "unverified", "device_measurement_time": null }),
        )?;
        Ok(recorder)
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn session_id(&self) -> &str {
        &self.session_id
    }
    pub fn is_closed(&self) -> bool {
        self.closed
    }
    fn event(&mut self, kind: &str, payload: Value) -> Result<ReadingContext> {
        if self.closed {
            return invalid("Capture is closed");
        }
        let context = ReadingContext {
            received_at_utc: utc_now(),
            session_id: self.session_id.clone(),
            sequence: self.sequence + 1,
            device_id: self.device_id.clone(),
            service_uuid: self.service_uuid.clone(),
            characteristic_uuid: self.characteristic_uuid.clone(),
        };
        let mut record = serde_json::to_value(&context)?;
        let object = record.as_object_mut().expect("context is object");
        object.insert("schema_version".into(), json!(1));
        object.insert("device_model".into(), json!(MODEL));
        object.insert("transport".into(), json!("bluetooth-le"));
        object.insert("type".into(), json!(kind));
        object.insert(
            "elapsed_ns".into(),
            json!(self.clock.elapsed().as_nanos().min(u64::MAX as u128) as u64),
        );
        object.extend(
            payload
                .as_object()
                .expect("internal event payload is object")
                .clone(),
        );
        let written = (|| -> Result<()> {
            serde_json::to_writer(&mut self.stream, &record)?;
            self.stream.write_all(b"\n")?;
            self.stream.flush()?;
            self.stream.get_ref().sync_data()?;
            Ok(())
        })();
        if let Err(error) = written {
            self.closed = true;
            return Err(error);
        }
        self.sequence = context.sequence;
        Ok(context)
    }
    pub fn append(&mut self, bytes: &[u8]) -> Result<ReadingContext> {
        self.event(
            "raw_frame",
            json!({ "payload_hex": payload_hex(bytes), "decoder_status": "unverified" }),
        )
    }
    pub fn close(&mut self, reason: &str) -> Result<()> {
        if !self.closed {
            let result = self.event("session_end", json!({ "reason": reason }));
            self.closed = true;
            result?;
            self.stream.get_ref().sync_all()?;
        }
        Ok(())
    }
}
impl Drop for FrameRecorder {
    fn drop(&mut self) {
        let _ = self.close("recorder_dropped");
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveResult {
    pub records: usize,
    pub raw_frames: usize,
    pub inserted: usize,
    pub database: PathBuf,
}

/// Transactional import. Repeated evidence is idempotent; conflicting duplicates
/// abort the entire import. Original timestamp strings survive byte-for-byte.
pub fn archive_raw(path: &Path, database: &Path) -> Result<ArchiveResult> {
    let file = File::open(path)?;
    if file.metadata()?.len() > 32 * 1024 * 1024 {
        return invalid("Raw import exceeds 32 MB");
    }
    let mut records = Vec::new();
    for line in BufReader::new(file).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let record: Value = serde_json::from_str(&line)?;
        if record.get("schema_version").and_then(Value::as_u64) != Some(1)
            || record.get("device_model").and_then(Value::as_str) != Some(MODEL)
        {
            return invalid("Wrong format or instrument model");
        }
        let kind = record.get("type").and_then(Value::as_str).unwrap_or("");
        if !["session_start", "raw_frame", "session_end"].contains(&kind) {
            return invalid("Unsupported record type");
        }
        if kind == "raw_frame"
            && !record
                .get("payload_hex")
                .and_then(Value::as_str)
                .is_some_and(valid_hex)
        {
            return invalid("Invalid raw payload hex");
        }
        parse_timestamp(
            record
                .get("received_at_utc")
                .and_then(Value::as_str)
                .ok_or_else(|| CoreError("Missing timestamp".into()))?,
        )?;
        let sequence = record
            .get("sequence")
            .and_then(Value::as_u64)
            .ok_or_else(|| CoreError("Invalid sequence".into()))?;
        if sequence == 0 || sequence > i64::MAX as u64 {
            return invalid("Invalid sequence");
        }
        Uuid::parse_str(
            record
                .get("session_id")
                .and_then(Value::as_str)
                .unwrap_or(""),
        )
        .map_err(|_| CoreError("Invalid session ID".into()))?;
        records.push(record);
    }
    if records.is_empty() {
        return invalid("No raw records to archive");
    }
    parent_directory(database)?;
    let mut connection = Connection::open(database)?;
    let transaction = connection.transaction()?;
    transaction.execute("CREATE TABLE IF NOT EXISTS raw_events(session_id TEXT,sequence INTEGER,payload TEXT NOT NULL,PRIMARY KEY(session_id,sequence))", [])?;
    let mut inserted = 0;
    for record in &records {
        let session = record["session_id"].as_str().unwrap();
        let sequence = record["sequence"].as_u64().unwrap() as i64;
        let old: Option<String> = transaction
            .query_row(
                "SELECT payload FROM raw_events WHERE session_id=?1 AND sequence=?2",
                params![session, sequence],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(old) = old {
            let old_record: Value = serde_json::from_str(&old)?;
            if old_record != *record {
                return invalid("Conflicting duplicate event; import aborted");
            }
        } else {
            transaction.execute(
                "INSERT INTO raw_events(session_id,sequence,payload) VALUES(?1,?2,?3)",
                params![session, sequence, serde_json::to_string(record)?],
            )?;
            inserted += 1;
        }
    }
    transaction.commit()?;
    Ok(ArchiveResult {
        records: records.len(),
        raw_frames: records.iter().filter(|r| r["type"] == "raw_frame").count(),
        inserted,
        database: database.to_owned(),
    })
}
