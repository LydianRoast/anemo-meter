//! Shared instrument evidence and analysis. Only the documented live profile is decoded.
//! Missing values are never substituted with measured zero.

mod analysis;
mod live;
mod storage;

pub use analysis::*;
pub use live::*;
pub use storage::*;

use chrono::{DateTime, FixedOffset, SecondsFormat, Utc};
use std::{error::Error, fmt};

pub const MODEL: &str = "Elitech ANE-200W";
pub const PROFILE: &str = "anemometer26-v3.2-live-v1";
pub const NOTIFY_UUID: &str = "0000fff1-0000-1000-8000-00805f9b34fb";
pub const WRITE_UUID: &str = "0000fff2-0000-1000-8000-00805f9b34fb";
pub const READ_REQUEST: [u8; 4] = [0x57, 0x48, 0x01, 0x3b];
pub const FRESH_SECONDS: u64 = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreError(pub String);
impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
impl Error for CoreError {}
macro_rules! from_error {
    ($($ty:ty),*) => {$(impl From<$ty> for CoreError {
        fn from(e: $ty) -> Self { Self(e.to_string()) }
    })*};
}
from_error!(
    std::io::Error,
    serde_json::Error,
    csv::Error,
    rusqlite::Error,
    chrono::ParseError
);
pub type Result<T> = std::result::Result<T, CoreError>;
pub(crate) fn invalid<T>(message: impl Into<String>) -> Result<T> {
    Err(CoreError(message.into()))
}

pub fn utc_now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Nanos, true)
}
/// Validate a timezone-bearing RFC 3339 timestamp; callers retain the original text.
pub fn parse_timestamp(stamp: &str) -> Result<DateTime<FixedOffset>> {
    Ok(DateTime::parse_from_rfc3339(stamp)?)
}
pub(crate) fn finite(value: f64, label: &str, minimum: Option<f64>) -> Result<f64> {
    if !value.is_finite() || minimum.is_some_and(|min| value < min) {
        return invalid(format!(
            "{label} must be finite{}",
            minimum
                .map(|n| format!(" and at least {n}"))
                .unwrap_or_default()
        ));
    }
    Ok(value)
}
pub fn payload_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
pub(crate) fn valid_hex(text: &str) -> bool {
    text.len() % 2 == 0 && text.bytes().all(|b| b.is_ascii_hexdigit())
}
