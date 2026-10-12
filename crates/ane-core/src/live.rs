use crate::{finite, invalid, parse_timestamp, payload_hex, Result, FRESH_SECONDS, MODEL, PROFILE};
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReadingContext {
    pub received_at_utc: String,
    pub session_id: String,
    pub sequence: u64,
    pub device_id: String,
    pub service_uuid: String,
    pub characteristic_uuid: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Measurement {
    #[serde(flatten)]
    pub context: ReadingContext,
    pub velocity_mps: f64,
    pub temperature_c: f64,
    pub humidity_percent: f64,
    pub meter_velocity_unit: String,
    pub meter_temperature_unit: String,
    pub meter_temperature_value: f64,
    pub meter_flow_unit: String,
    pub device_model: String,
    pub source: String,
    pub protocol_profile: String,
    pub decoder_status: String,
    pub quality: String,
    pub payload_hex: String,
}
impl Measurement {
    pub fn fpm(&self) -> f64 {
        self.velocity_mps * 60.0 / 0.3048
    }
    pub fn fahrenheit(&self) -> f64 {
        self.temperature_c * 9.0 / 5.0 + 32.0
    }
    pub fn validate(&self) -> Result<()> {
        parse_timestamp(&self.context.received_at_utc)?;
        finite(self.velocity_mps, "Velocity", Some(0.0))?;
        finite(self.temperature_c, "Temperature", None)?;
        finite(self.humidity_percent, "Humidity", Some(0.0))?;
        finite(self.meter_temperature_value, "Meter temperature", None)?;
        if self.velocity_mps > 30.0
            || !(-10.0..=45.0).contains(&self.temperature_c)
            || self.humidity_percent > 100.0
        {
            return invalid("Reading outside documented measurement ranges");
        }
        if self.quality != "valid" || self.source != "live-bluetooth" || self.device_model != MODEL
        {
            return invalid("Only valid ANE-200W Bluetooth samples may enter the live display");
        }
        Ok(())
    }
}

/// Companion-app-derived layout, checked on real zero and nonzero packets.
/// Bytes 9, 14, and 15 remain uninterpreted; no checksum or stored-record format is claimed.
pub fn decode_packet(data: &[u8], context: ReadingContext) -> Result<Measurement> {
    if data.len() != 17 || data[..3] != [0xff, 0x57, 0x4c] || data[3] != 1 || data[16] != 0x3b {
        return invalid("Not a 17-byte live reading response");
    }
    if data[6] > 4 || data[10] > 1 || data[13] > 1 {
        return invalid("Unsupported meter unit code");
    }
    let velocity_mps = u16::from_be_bytes([data[4], data[5]]) as f64 / 100.0;
    let meter_temperature_value = i16::from_be_bytes([data[7], data[8]]) as f64 / 10.0;
    let temperature_c = if data[10] == 0 {
        meter_temperature_value
    } else {
        (meter_temperature_value - 32.0) * 5.0 / 9.0
    };
    let measurement = Measurement {
        context,
        velocity_mps,
        temperature_c,
        humidity_percent: u16::from_be_bytes([data[11], data[12]]) as f64 / 10.0,
        meter_velocity_unit: ["m/s", "km/h", "ft/min", "knots", "mph"][data[6] as usize].into(),
        meter_temperature_unit: if data[10] == 0 { "C" } else { "F" }.into(),
        meter_temperature_value,
        meter_flow_unit: if data[13] == 0 { "CMM" } else { "CFM" }.into(),
        device_model: MODEL.into(),
        source: "live-bluetooth".into(),
        protocol_profile: PROFILE.into(),
        decoder_status: "vendor-app-derived".into(),
        quality: "valid".into(),
        payload_hex: payload_hex(data),
    };
    measurement.validate()?;
    Ok(measurement)
}

/// Holds an owned copy while new samples continue to replace `latest`.
/// Freshness is measured with a monotonic clock, independently of wall-clock timestamps.
#[derive(Debug)]
pub struct LiveReading {
    latest: Option<Measurement>,
    held: Option<Measurement>,
    updated: Option<Instant>,
    pub active: bool,
    pub problem: String,
    pub saved_count: usize,
}
impl Default for LiveReading {
    fn default() -> Self {
        Self::new()
    }
}
impl LiveReading {
    pub fn new() -> Self {
        Self {
            latest: None,
            held: None,
            updated: None,
            active: false,
            problem: "Waiting for meter".into(),
            saved_count: 0,
        }
    }
    pub fn begin(&mut self) {
        self.latest = None;
        self.held = None;
        self.updated = None;
        self.active = true;
        self.problem = "Waiting for meter".into();
    }
    pub fn receive(&mut self, measurement: Measurement) -> Result<()> {
        self.receive_at(measurement, Instant::now())
    }
    pub fn receive_at(&mut self, measurement: Measurement, now: Instant) -> Result<()> {
        if let Err(error) = measurement.validate() {
            self.invalidate(error.to_string());
            return Err(error);
        }
        self.latest = Some(measurement);
        self.updated = Some(now);
        self.active = true;
        self.problem.clear();
        Ok(())
    }
    pub fn invalidate(&mut self, reason: impl Into<String>) {
        self.latest = None;
        self.updated = None;
        self.problem = reason.into();
    }
    pub fn end(&mut self, reason: impl Into<String>) {
        self.active = false;
        self.problem = reason.into();
    }
    pub fn fresh(&self) -> bool {
        self.fresh_at(Instant::now())
    }
    pub fn fresh_at(&self, now: Instant) -> bool {
        self.active
            && self.latest.is_some()
            && self.updated.is_some_and(|t| {
                now.checked_duration_since(t)
                    .is_some_and(|d| d <= Duration::from_secs(FRESH_SECONDS))
            })
    }
    pub fn displayed(&self) -> Option<&Measurement> {
        self.displayed_at(Instant::now())
    }
    pub fn displayed_at(&self, now: Instant) -> Option<&Measurement> {
        self.held.as_ref().or_else(|| {
            if self.fresh_at(now) {
                self.latest.as_ref()
            } else {
                None
            }
        })
    }
    pub fn latest(&self) -> Option<&Measurement> {
        self.latest.as_ref()
    }
    pub fn is_held(&self) -> bool {
        self.held.is_some()
    }
    pub fn toggle_hold(&mut self) -> Result<()> {
        self.toggle_hold_at(Instant::now())
    }
    pub fn toggle_hold_at(&mut self, now: Instant) -> Result<()> {
        if self.held.is_some() {
            self.held = None;
            return Ok(());
        }
        if !self.fresh_at(now) {
            return invalid("No fresh reading to hold");
        }
        self.held = self.latest.clone();
        Ok(())
    }
    pub fn save(&mut self, directory: &std::path::Path, note: &str) -> Result<std::path::PathBuf> {
        let reading = self
            .displayed()
            .ok_or_else(|| crate::CoreError("No fresh or held reading to save".into()))?;
        let path = crate::save_single_reading(directory, reading, self.is_held(), note)?;
        self.saved_count += 1;
        Ok(path)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TracePoint {
    pub seconds: f64,
    pub fpm: Option<f64>,
}
#[derive(Debug, Default)]
pub struct TraceBuffer {
    points: VecDeque<TracePoint>,
}
impl TraceBuffer {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn add(&mut self, seconds: f64, fpm: Option<f64>) -> Result<()> {
        finite(seconds, "Receive elapsed time", Some(0.0))?;
        if let Some(value) = fpm {
            finite(value, "FPM", Some(0.0))?;
        }
        if self.points.back().is_some_and(|p| seconds < p.seconds) {
            return invalid("Trace timestamps must be monotonic");
        }
        self.points.push_back(TracePoint { seconds, fpm });
        self.prune(seconds);
        while self.points.len() > 10_000 {
            self.points.pop_front();
        }
        Ok(())
    }
    pub fn prune(&mut self, now: f64) {
        while self.points.front().is_some_and(|p| p.seconds < now - 600.0) {
            self.points.pop_front();
        }
    }
    pub fn window(&mut self, now: f64, seconds: f64) -> Result<Vec<TracePoint>> {
        finite(now, "Receive elapsed time", Some(0.0))?;
        if !seconds.is_finite() || !(5.0..=600.0).contains(&seconds) {
            return invalid("Trace window must be 5–600 seconds");
        }
        self.prune(now);
        Ok(self
            .points
            .iter()
            .filter(|p| p.seconds >= now - seconds && p.seconds <= now)
            .copied()
            .collect())
    }
}
