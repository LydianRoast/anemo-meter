use crate::{finite, invalid, parse_timestamp, CoreError, Measurement, Result};
use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, fs::File, path::Path};

pub fn velocity_fpm(mps: f64) -> Result<f64> {
    finite(
        finite(mps, "Velocity", Some(0.0))? * 60.0 / 0.3048,
        "Converted velocity",
        Some(0.0),
    )
}
pub fn airflow_cfm(mps: f64, area_ft2: f64, multiplier: f64) -> Result<f64> {
    let result = velocity_fpm(mps)?
        * finite(area_ft2, "Area", Some(0.0))?
        * finite(multiplier, "Multiplier", Some(0.0))?;
    finite(result, "Calculated airflow", Some(0.0))
}

/// Exchange-file rows are analysis inputs, never live-device samples.
/// `source` retains provenance supplied by the exporting application.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ImportedMeasurement {
    pub received_at_utc: String,
    pub original_received_at: String,
    pub velocity_mps: Option<f64>,
    pub temperature_c: Option<f64>,
    pub humidity_percent: Option<f64>,
    pub quality: String,
    pub source: String,
}
impl ImportedMeasurement {
    pub fn validate(&self) -> Result<()> {
        parse_timestamp(&self.received_at_utc)?;
        parse_timestamp(&self.original_received_at)?;
        if !["valid", "stale", "error"].contains(&self.quality.as_str()) {
            return invalid("Unknown quality value");
        }
        if self.quality == "valid" && self.velocity_mps.is_none() {
            return invalid("Valid readings must contain velocity");
        }
        if let Some(v) = self.velocity_mps {
            finite(v, "Velocity", Some(0.0))?;
        }
        if let Some(t) = self.temperature_c {
            finite(t, "Temperature", None)?;
        }
        if let Some(h) = self.humidity_percent {
            finite(h, "Humidity", Some(0.0))?;
            if h > 100.0 {
                return invalid("Humidity must be between 0 and 100");
            }
        }
        Ok(())
    }
}
impl From<&Measurement> for ImportedMeasurement {
    fn from(m: &Measurement) -> Self {
        Self {
            received_at_utc: m.context.received_at_utc.clone(),
            original_received_at: m.context.received_at_utc.clone(),
            velocity_mps: Some(m.velocity_mps),
            temperature_c: Some(m.temperature_c),
            humidity_percent: Some(m.humidity_percent),
            quality: m.quality.clone(),
            source: m.source.clone(),
        }
    }
}

/// Our documented CSV exchange format, including exports from the prior Java/Python apps.
/// This does not attempt to parse the vendor's proprietary .ane or .xls formats.
pub fn import_measurements(path: &Path) -> Result<Vec<ImportedMeasurement>> {
    let file = File::open(path)?;
    if file.metadata()?.len() > 32 * 1024 * 1024 {
        return invalid("Measurement CSV import exceeds 32 MB");
    }
    let mut reader = csv::Reader::from_reader(file);
    let headers = reader.headers()?.clone();
    let names: Vec<&str> = headers
        .iter()
        .map(|h| h.trim_start_matches('\u{feff}'))
        .collect();
    let unique: HashSet<_> = names.iter().copied().collect();
    if names.len() != unique.len() {
        return invalid("Duplicate CSV column names");
    }
    if !names.contains(&"received_at_utc") || !names.contains(&"velocity_mps") {
        return invalid("CSV needs received_at_utc and velocity_mps columns");
    }
    let mut result = Vec::new();
    for row in reader.records() {
        if result.len() >= 100_000 {
            return invalid("Import is limited to 100,000 rows");
        }
        let row = row?;
        let get = |name| {
            names
                .iter()
                .position(|h| *h == name)
                .and_then(|i| row.get(i))
                .unwrap_or("")
        };
        let number = |name| -> Result<Option<f64>> {
            let text = get(name).trim();
            if text.is_empty() {
                Ok(None)
            } else {
                Ok(Some(
                    text.parse::<f64>()
                        .map_err(|_| CoreError(format!("Invalid {name}")))?,
                ))
            }
        };
        let stamp = get("received_at_utc");
        let normalized = parse_timestamp(stamp)?
            .with_timezone(&Utc)
            .to_rfc3339_opts(SecondsFormat::AutoSi, true);
        let original = if get("original_received_at").is_empty() {
            stamp
        } else {
            get("original_received_at")
        };
        let measurement = ImportedMeasurement {
            received_at_utc: normalized,
            original_received_at: original.into(),
            velocity_mps: number("velocity_mps")?,
            temperature_c: number("temperature_c")?,
            humidity_percent: number("humidity_percent")?,
            quality: if get("quality").is_empty() {
                "valid"
            } else {
                get("quality")
            }
            .into(),
            source: if get("source").is_empty() {
                "imported"
            } else {
                get("source")
            }
            .into(),
        };
        measurement.validate()?;
        result.push(measurement);
    }
    if result.is_empty() {
        return invalid("No measurement rows");
    }
    Ok(result)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Summary {
    pub count: usize,
    pub excluded: usize,
    pub mean_mps: f64,
    pub min_mps: f64,
    pub max_mps: f64,
    pub stddev_mps: Option<f64>,
    pub mean_fpm: f64,
    pub calculated_cfm: f64,
    pub average_basis: String,
}
pub fn summarize(rows: &[ImportedMeasurement], area_ft2: f64, multiplier: f64) -> Result<Summary> {
    let mut values = Vec::new();
    for row in rows {
        row.validate()?;
        if row.quality == "valid" {
            values.push(row.velocity_mps.expect("validated valid row"));
        }
    }
    if values.is_empty() {
        return invalid("No valid measurements; missing readings are not zero");
    }
    let count = values.len();
    let mean_mps = finite(
        values.iter().map(|v| v / count as f64).sum(),
        "Mean velocity",
        Some(0.0),
    )?;
    let stddev_mps = if count > 1 {
        Some(finite(
            (values
                .iter()
                .map(|v| (v - mean_mps).powi(2) / (count - 1) as f64)
                .sum::<f64>())
            .sqrt(),
            "Sample standard deviation",
            Some(0.0),
        )?)
    } else {
        None
    };
    Ok(Summary {
        count,
        excluded: rows.len() - count,
        mean_mps,
        min_mps: values.iter().copied().fold(f64::INFINITY, f64::min),
        max_mps: values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        stddev_mps,
        mean_fpm: velocity_fpm(mean_mps)?,
        calculated_cfm: airflow_cfm(mean_mps, area_ft2, multiplier)?,
        average_basis: "Arithmetic sample mean; not a time-weighted mean".into(),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectiveAreaFit {
    pub effective_area_ft2: f64,
    pub rmse_cfm: f64,
    pub residuals_cfm: Vec<f64>,
    pub equation: String,
    pub note: String,
}
pub fn fit_effective_area(pairs: &[(f64, f64)]) -> Result<EffectiveAreaFit> {
    if pairs.len() < 2 {
        return invalid("At least two independent reference pairs are required");
    }
    let mut numerator = 0.0;
    let mut denominator = 0.0;
    for &(x, y) in pairs {
        finite(x, "FPM", Some(0.0))?;
        finite(y, "Reference CFM", Some(0.0))?;
        numerator += x * y;
        denominator += x * x;
    }
    if denominator <= 0.0 {
        return invalid("References require positive velocity");
    }
    finite(numerator, "Fit numerator", Some(0.0))?;
    finite(denominator, "Fit denominator", Some(0.0))?;
    let effective_area_ft2 = finite(numerator / denominator, "Effective area", Some(0.0))?;
    let residuals_cfm: Vec<f64> = pairs
        .iter()
        .map(|&(x, y)| y - effective_area_ft2 * x)
        .collect();
    let rmse_cfm = finite(
        (residuals_cfm
            .iter()
            .map(|e| e * e / pairs.len() as f64)
            .sum::<f64>())
        .sqrt(),
        "Fit RMSE",
        Some(0.0),
    )?;
    Ok(EffectiveAreaFit { effective_area_ft2, rmse_cfm, residuals_cfm, equation: "CFM = FPM * effective_area_ft2".into(),
        note: "Entered-reference fit; does not validate meter calibration or transfer to other devices".into() })
}

pub fn export_measurements(
    path: &Path,
    rows: &[ImportedMeasurement],
    area_ft2: f64,
    multiplier: f64,
) -> Result<()> {
    // Validate the entire data set before opening or replacing the destination.
    airflow_cfm(0.0, area_ft2, multiplier)?;
    for row in rows {
        row.validate()?;
        if row.quality == "valid" {
            airflow_cfm(row.velocity_mps.unwrap(), area_ft2, multiplier)?;
        }
    }
    let mut writer = csv::Writer::from_path(path)?;
    writer.write_record([
        "received_at_utc",
        "original_received_at",
        "velocity_mps",
        "temperature_c",
        "humidity_percent",
        "quality",
        "source",
        "area_ft2",
        "multiplier",
        "calculated_cfm",
    ])?;
    for row in rows {
        writer.write_record([
            row.received_at_utc.clone(),
            row.original_received_at.clone(),
            optional_number(row.velocity_mps),
            optional_number(row.temperature_c),
            optional_number(row.humidity_percent),
            row.quality.clone(),
            row.source.clone(),
            area_ft2.to_string(),
            multiplier.to_string(),
            if row.quality == "valid" {
                airflow_cfm(row.velocity_mps.unwrap(), area_ft2, multiplier)?.to_string()
            } else {
                String::new()
            },
        ])?;
    }
    writer.flush()?;
    writer.get_ref().sync_all()?;
    Ok(())
}
fn optional_number(value: Option<f64>) -> String {
    value.map(|v| v.to_string()).unwrap_or_default()
}
