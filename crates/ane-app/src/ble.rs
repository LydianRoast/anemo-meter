//! BLE transport for the one verified ANE-200W reading profile.
//!
//! Only the current-reading request is transmitted. Device settings, memory,
//! unknown writable characteristics and firmware services are never written.

use ane_core::{decode_packet, FrameRecorder, Measurement, NOTIFY_UUID, READ_REQUEST, WRITE_UUID};
use btleplug::api::{
    Central, CharPropFlags, Characteristic, Manager as _, Peripheral as _, ScanFilter, WriteType,
};
use btleplug::platform::{Manager, Peripheral};
use futures::StreamExt;
use std::{
    collections::HashMap,
    future::Future,
    path::PathBuf,
    sync::mpsc,
    time::{Duration, Instant},
};
use tokio::{
    sync::{mpsc as async_mpsc, watch},
    task::JoinHandle,
    time,
};
use uuid::Uuid;

const OPERATION_TIMEOUT: Duration = Duration::from_secs(5);
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(12);
const SCAN_TIME: Duration = Duration::from_secs(10);

#[derive(Debug, Clone)]
pub struct DeviceSummary {
    /// Opaque platform identifier; not an instrument serial number.
    pub id: String,
    pub name: String,
    pub rssi: Option<i16>,
}

#[derive(Debug)]
pub enum BleCommand {
    Scan,
    Connect { device_id: String },
    StartLive,
    StopLive,
    Disconnect,
    Shutdown,
}

#[derive(Debug)]
pub enum BleEvent {
    Status(String),
    ScanStarted,
    ConnectStarted,
    Devices(Vec<DeviceSummary>),
    Connected {
        device_id: String,
        name: String,
    },
    LiveStarted {
        capture_path: PathBuf,
        session_id: String,
    },
    Reading {
        measurement: Measurement,
        received: Instant,
    },
    InvalidPacket(String),
    LiveStopped,
    Disconnected,
    Error(String),
}

pub struct BleWorker {
    pub commands: async_mpsc::UnboundedSender<BleCommand>,
    pub events: mpsc::Receiver<BleEvent>,
    _thread: Option<std::thread::JoinHandle<()>>,
}

impl BleWorker {
    pub fn spawn(capture_directory: PathBuf) -> Self {
        let (commands, receiver) = async_mpsc::unbounded_channel();
        let (events, event_receiver) = mpsc::channel();
        let failures = events.clone();
        let thread = std::thread::Builder::new()
            .name("ane-bluetooth".into())
            .spawn(move || {
                #[cfg(target_os = "android")]
                if let Err(error) = crate::android::attach_worker_thread() {
                    let _ = events.send(BleEvent::Error(format!(
                        "Cannot attach Bluetooth worker to Android: {error}"
                    )));
                    return;
                }
                let mut builder = tokio::runtime::Builder::new_current_thread();
                builder.enable_all();
                #[cfg(target_os = "android")]
                {
                    let failures = events.clone();
                    builder.on_thread_start(move || {
                        if let Err(error) = crate::android::attach_worker_thread() {
                            let _ = failures.send(BleEvent::Error(format!(
                                "Cannot attach Bluetooth task to Android: {error}"
                            )));
                        }
                    });
                }
                match builder.build() {
                    Ok(runtime) => {
                        runtime.block_on(run_worker(capture_directory, receiver, events))
                    }
                    Err(error) => {
                        let _ = events.send(BleEvent::Error(format!("Bluetooth worker: {error}")));
                    }
                }
            });
        let thread = match thread {
            Ok(thread) => Some(thread),
            Err(error) => {
                let _ = failures.send(BleEvent::Error(format!(
                    "Cannot start Bluetooth worker: {error}"
                )));
                None
            }
        };
        Self {
            commands,
            events: event_receiver,
            _thread: thread,
        }
    }
}

impl Drop for BleWorker {
    fn drop(&mut self) {
        // Cleanup runs on the worker; never freeze the render thread waiting on a radio.
        let _ = self.commands.send(BleCommand::Shutdown);
    }
}

struct Task<T> {
    cancel: watch::Sender<bool>,
    join: JoinHandle<T>,
}

type Discovered = Vec<(DeviceSummary, Peripheral)>;
type OperationResult<T> = Result<T, String>;

async fn bounded<T>(future: impl Future<Output = btleplug::Result<T>>) -> OperationResult<T> {
    time::timeout(OPERATION_TIMEOUT, future)
        .await
        .map_err(|_| "Bluetooth operation timed out".to_owned())?
        .map_err(|error| error.to_string())
}

async fn await_task<T>(task: &mut Option<Task<T>>) -> Result<T, tokio::task::JoinError> {
    match task {
        Some(task) => (&mut task.join).await,
        None => std::future::pending().await,
    }
}

async fn cancel_task<T>(task: &mut Option<Task<T>>) {
    if let Some(task) = task.take() {
        let _ = task.cancel.send(true);
        // Every awaited BLE operation is bounded, and the task performs its own
        // unsubscribe/scan cleanup before returning. Do not abort it mid-cleanup.
        let _ = task.join.await;
    }
}

/// Capture is foreground-only on Android. This runs on the attached Bluetooth
/// worker, independently of rendering (which can stop when the app is paused).
async fn wait_for_background() {
    #[cfg(target_os = "android")]
    loop {
        if !crate::android::has_window_focus() {
            return;
        }
        time::sleep(Duration::from_millis(250)).await;
    }
    #[cfg(not(target_os = "android"))]
    std::future::pending::<()>().await;
}

async fn run_worker(
    directory: PathBuf,
    mut commands: async_mpsc::UnboundedReceiver<BleCommand>,
    events: mpsc::Sender<BleEvent>,
) {
    let mut discovered: HashMap<String, (DeviceSummary, Peripheral)> = HashMap::new();
    let mut selected: Option<(DeviceSummary, Peripheral)> = None;
    let mut scan: Option<Task<OperationResult<Discovered>>> = None;
    let mut live: Option<Task<()>> = None;
    loop {
        tokio::select! {
            biased;
            _ = wait_for_background(), if selected.is_some() || scan.is_some() || live.is_some() => {
                cancel_task(&mut scan).await;
                cancel_task(&mut live).await;
                disconnect(&mut selected, &events).await;
                discovered.clear();
                let _ = events.send(BleEvent::Devices(Vec::new()));
                let _ = events.send(BleEvent::Status("Paused -> app left foreground; scan and reconnect to resume".into()));
            }
            command = commands.recv() => {
                match command {
                    None | Some(BleCommand::Shutdown) => break,
                    Some(BleCommand::Scan) => {
                        #[cfg(target_os = "android")]
                        match crate::android::ensure_permissions() {
                            Ok(true) => (),
                            Ok(false) => {
                                let _ = events.send(BleEvent::Error("Allow Nearby devices permission, then Scan again".into()));
                                continue;
                            }
                            Err(error) => {
                                let _ = events.send(BleEvent::Error(format!("Bluetooth permission check failed: {error}")));
                                continue;
                            }
                        }
                        if live.is_some() {
                            let _ = events.send(BleEvent::Error("Stop live capture before scanning".into()));
                            continue;
                        }
                        cancel_task(&mut scan).await;
                        discovered.clear();
                        let _ = events.send(BleEvent::ScanStarted);
                        let _ = events.send(BleEvent::Status("Scanning for ANE-200W -> 10 seconds".into()));
                        let (cancel, receiver) = watch::channel(false);
                        scan = Some(Task { cancel, join: tokio::spawn(scan_devices(receiver)) });
                    }
                    Some(BleCommand::Connect { device_id }) => {
                        let Some(candidate) = discovered.get(&device_id).cloned() else {
                            let _ = events.send(BleEvent::Error("Select a meter from the completed scan".into()));
                            continue;
                        };
                        cancel_task(&mut scan).await;
                        cancel_task(&mut live).await;
                        disconnect(&mut selected, &events).await;
                        let _ = events.send(BleEvent::ConnectStarted);
                        let _ = events.send(BleEvent::Status(format!("Connecting to {}", candidate.0.name)));
                        let result: OperationResult<()> = tokio::select! {
                            biased;
                            _ = wait_for_background() => Err("Connection canceled because the app left foreground".into()),
                            result = time::timeout(CONNECTION_TIMEOUT, async {
                                candidate.1.connect().await?;
                                candidate.1.discover_services().await?;
                                Ok::<(), btleplug::Error>(())
                            }) => result
                                .map_err(|_| "Connection/discovery timed out".to_owned())
                                .and_then(|result| result.map_err(|error| error.to_string())),
                        };
                        match result {
                            Ok(()) => {
                                let _ = events.send(BleEvent::Connected { device_id: candidate.0.id.clone(), name: candidate.0.name.clone() });
                                let _ = events.send(BleEvent::Status("Connected -> ready for live readings".into()));
                                selected = Some(candidate);
                            }
                            Err(description) => {
                                let _ = bounded(candidate.1.disconnect()).await;
                                let _ = events.send(BleEvent::Error(description));
                                let _ = events.send(BleEvent::Disconnected);
                            }
                        }
                    }
                    Some(BleCommand::StartLive) => {
                        if live.is_some() { continue; }
                        let Some((summary, peripheral)) = selected.clone() else {
                            let _ = events.send(BleEvent::Error("Connect to the selected meter first".into()));
                            continue;
                        };
                        cancel_task(&mut scan).await;
                        match bounded(peripheral.is_connected()).await {
                            Ok(true) => (),
                            _ => {
                                selected = None;
                                let _ = events.send(BleEvent::Error("Meter disconnected; reconnect before capture".into()));
                                let _ = events.send(BleEvent::Disconnected);
                                continue;
                            }
                        }
                        let characteristics = peripheral.characteristics().into_iter().collect::<Vec<_>>();
                        let (notify, write, write_type) = match select_profile(&characteristics) {
                            Ok(profile) => profile,
                            Err(error) => { let _ = events.send(BleEvent::Error(error)); continue; }
                        };
                        let recorder = match FrameRecorder::new(
                            &directory, &summary.id, &notify.uuid.to_string(), &notify.service_uuid.to_string(),
                        ) {
                            Ok(recorder) => recorder,
                            Err(error) => { let _ = events.send(BleEvent::Error(format!("Cannot save raw evidence: {error}"))); continue; }
                        };
                        let (cancel, receiver) = watch::channel(false);
                        let sender = events.clone();
                        live = Some(Task { cancel, join: tokio::spawn(capture_live(peripheral, notify, write, write_type, recorder, receiver, sender)) });
                    }
                    Some(BleCommand::StopLive) => {
                        cancel_task(&mut live).await;
                        let _ = events.send(BleEvent::Status("Live capture stopped".into()));
                    }
                    Some(BleCommand::Disconnect) => {
                        cancel_task(&mut scan).await;
                        cancel_task(&mut live).await;
                        disconnect(&mut selected, &events).await;
                        let _ = events.send(BleEvent::Status("Disconnected".into()));
                    }
                }
            }
            result = await_task(&mut scan) => {
                scan = None;
                match result {
                    Ok(Ok(devices)) => {
                        let count = devices.len();
                        let summaries = devices.iter().map(|(summary, _)| summary.clone()).collect();
                        discovered = devices.into_iter().map(|(summary, peripheral)| (summary.id.clone(), (summary, peripheral))).collect();
                        let _ = events.send(BleEvent::Devices(summaries));
                        let _ = events.send(BleEvent::Status(if count == 0 { "No Anemometer found -> check meter Bluetooth and other connections".into() } else { format!("{count} meter(s) found -> select your instrument") }));
                    }
                    Ok(Err(error)) => { let _ = events.send(BleEvent::Error(error)); }
                    Err(error) => { let _ = events.send(BleEvent::Error(format!("Bluetooth scan failed: {error}"))); }
                }
            }
            result = await_task(&mut live) => {
                live = None;
                if let Err(error) = result {
                    let _ = events.send(BleEvent::Error(format!("Capture worker failed: {error}")));
                    let _ = events.send(BleEvent::LiveStopped);
                    disconnect(&mut selected, &events).await;
                }
            }
        }
    }
    cancel_task(&mut scan).await;
    cancel_task(&mut live).await;
    disconnect(&mut selected, &events).await;
}

async fn disconnect(
    selected: &mut Option<(DeviceSummary, Peripheral)>,
    events: &mpsc::Sender<BleEvent>,
) {
    #[cfg(target_os = "android")]
    let _ = crate::android::set_keep_screen_on(false);
    if let Some((_, peripheral)) = selected.take() {
        if let Err(error) = bounded(peripheral.disconnect()).await {
            let _ = events.send(BleEvent::Error(format!(
                "Disconnect did not confirm: {error}"
            )));
        }
    }
    let _ = events.send(BleEvent::Disconnected);
}

async fn scan_devices(mut cancel: watch::Receiver<bool>) -> OperationResult<Discovered> {
    let setup = async {
        let manager = bounded(Manager::new()).await?;
        bounded(manager.adapters()).await
    };
    let adapters = tokio::select! {
        biased;
        _ = wait_for_background() => return Err("Scan canceled because the app left foreground".into()),
        _ = cancel.changed() => return Err("Scan canceled".into()),
        result = setup => result?,
    };
    if adapters.is_empty() {
        return Err("No Bluetooth adapter available".into());
    }
    let work = async {
        for adapter in &adapters {
            bounded(adapter.start_scan(ScanFilter::default())).await?;
        }
        time::sleep(SCAN_TIME).await;
        let mut devices = Vec::new();
        for (adapter_index, adapter) in adapters.iter().enumerate() {
            for peripheral in bounded(adapter.peripherals()).await? {
                if let Some(properties) = bounded(peripheral.properties()).await? {
                    let name = properties.local_name.unwrap_or_default();
                    if name.to_ascii_lowercase().contains("anem") {
                        devices.push((
                            DeviceSummary {
                                id: format!("{adapter_index}:{}", peripheral.id()),
                                name,
                                rssi: properties.rssi,
                            },
                            peripheral,
                        ));
                    }
                }
            }
        }
        devices.sort_by(|(a, _), (b, _)| b.rssi.cmp(&a.rssi).then(a.name.cmp(&b.name)));
        Ok(devices)
    };
    let result = tokio::select! {
        biased;
        _ = wait_for_background() => Err("Scan canceled because the app left foreground".into()),
        _ = cancel.changed() => Err("Scan canceled".into()),
        result = work => result,
    };
    // Stop every adapter even if a start request was canceled or failed halfway.
    let mut cleanup_error = None;
    for adapter in &adapters {
        if let Err(error) = bounded(adapter.stop_scan()).await {
            cleanup_error = Some(error);
        }
    }
    match (result, cleanup_error) {
        (Ok(_), Some(error)) => Err(format!("Scan stop did not confirm: {error}")),
        (result, _) => result,
    }
}

fn select_profile(
    characteristics: &[Characteristic],
) -> OperationResult<(Characteristic, Characteristic, WriteType)> {
    let notify_uuid = Uuid::parse_str(NOTIFY_UUID).map_err(|error| error.to_string())?;
    let write_uuid = Uuid::parse_str(WRITE_UUID).map_err(|error| error.to_string())?;
    let mut pairs = Vec::new();
    for notify in characteristics
        .iter()
        .filter(|c| c.uuid == notify_uuid && c.properties.contains(CharPropFlags::NOTIFY))
    {
        for write in characteristics
            .iter()
            .filter(|c| c.uuid == write_uuid && c.service_uuid == notify.service_uuid)
        {
            let write_type = if write
                .properties
                .contains(CharPropFlags::WRITE_WITHOUT_RESPONSE)
            {
                WriteType::WithoutResponse
            } else if write.properties.contains(CharPropFlags::WRITE) {
                WriteType::WithResponse
            } else {
                continue;
            };
            pairs.push((notify.clone(), write.clone(), write_type));
        }
    }
    if pairs.len() != 1 {
        return Err(
            "Expected one verified FFF1 notify / FFF2 write pair in the same service".into(),
        );
    }
    Ok(pairs.remove(0))
}

async fn capture_live(
    peripheral: Peripheral,
    notify: Characteristic,
    write: Characteristic,
    write_type: WriteType,
    mut recorder: FrameRecorder,
    mut cancel: watch::Receiver<bool>,
    events: mpsc::Sender<BleEvent>,
) {
    let capture_path = recorder.path().to_path_buf();
    let session_id = recorder.session_id().to_owned();
    let work = async {
        // Obtain the stream before subscription and polling, so the first reply
        // cannot be lost between installing the CCCD and opening the receiver.
        let mut notifications = bounded(peripheral.notifications()).await?;
        bounded(peripheral.subscribe(&notify)).await?;
        let _ = events.send(BleEvent::LiveStarted {
            capture_path,
            session_id,
        });
        let _ = events.send(BleEvent::Status("Live -> receiving meter readings".into()));
        let mut poll = time::interval(Duration::from_secs(1));
        poll.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                notification = notifications.next() => {
                    let received = Instant::now();
                    let notification = notification.ok_or_else(|| "Meter notification stream ended".to_owned())?;
                    if notification.uuid != notify.uuid || notification.service_uuid != notify.service_uuid { continue; }
                    // Persist first. A disk failure ends capture instead of silently
                    // displaying measurements for which raw evidence was lost.
                    let context = recorder.append(&notification.value)
                        .map_err(|error| format!("Raw evidence save failed; capture stopped: {error}"))?;
                    match decode_packet(&notification.value, context) {
                        Ok(measurement) => { if events.send(BleEvent::Reading { measurement, received }).is_err() { return Ok(()); } }
                        Err(error) => { let _ = events.send(BleEvent::InvalidPacket(error.to_string())); }
                    }
                }
                _ = poll.tick() => {
                    if !bounded(peripheral.is_connected()).await? { return Err("Meter disconnected".into()); }
                    // This is the only vendor request permitted in the transport.
                    bounded(peripheral.write(&write, &READ_REQUEST, write_type)).await?;
                }
            }
        }
    };
    let result: OperationResult<()> = tokio::select! {
        biased;
        _ = wait_for_background() => Ok(()),
        _ = cancel.changed() => Ok(()),
        result = work => result,
    };
    // Invalidate the live display immediately. Radio cleanup can take several
    // seconds; those seconds must not make a stopped capture appear active.
    let _ = events.send(BleEvent::LiveStopped);
    #[cfg(target_os = "android")]
    let _ = crate::android::set_keep_screen_on(false);
    let reason = match &result {
        Ok(()) => "stopped",
        Err(_) => "transport_or_storage_error",
    };
    if let Err(error) = recorder.close(reason) {
        let _ = events.send(BleEvent::Error(format!(
            "Raw session close failed: {error}"
        )));
    }
    if let Err(error) = bounded(peripheral.unsubscribe(&notify)).await {
        // A failed unsubscribe must not leave an untracked subscription active.
        let _ = events.send(BleEvent::Error(format!(
            "Unsubscribe did not confirm: {error}"
        )));
        let _ = bounded(peripheral.disconnect()).await;
        let _ = events.send(BleEvent::Disconnected);
    }
    if let Err(error) = result {
        let _ = events.send(BleEvent::Error(error));
        let _ = bounded(peripheral.disconnect()).await;
        let _ = events.send(BleEvent::Disconnected);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn characteristic(uuid: &str, service: u128, properties: CharPropFlags) -> Characteristic {
        Characteristic {
            uuid: Uuid::parse_str(uuid).unwrap(),
            service_uuid: Uuid::from_u128(service),
            properties,
            descriptors: BTreeSet::new(),
        }
    }

    #[test]
    fn never_pairs_characteristics_across_services() {
        let pair = [
            characteristic(NOTIFY_UUID, 1, CharPropFlags::NOTIFY),
            characteristic(WRITE_UUID, 2, CharPropFlags::WRITE),
        ];
        assert!(select_profile(&pair).is_err());
    }

    #[test]
    fn uses_the_discovered_write_mode() {
        let pair = [
            characteristic(NOTIFY_UUID, 1, CharPropFlags::NOTIFY),
            characteristic(WRITE_UUID, 1, CharPropFlags::WRITE),
        ];
        assert!(matches!(
            select_profile(&pair).unwrap().2,
            WriteType::WithResponse
        ));
        let pair = [
            characteristic(NOTIFY_UUID, 1, CharPropFlags::NOTIFY),
            characteristic(WRITE_UUID, 1, CharPropFlags::WRITE_WITHOUT_RESPONSE),
        ];
        assert!(matches!(
            select_profile(&pair).unwrap().2,
            WriteType::WithoutResponse
        ));
    }

    #[test]
    fn rejects_unsupported_or_ambiguous_profiles() {
        let bad = [
            characteristic(NOTIFY_UUID, 1, CharPropFlags::READ),
            characteristic(WRITE_UUID, 1, CharPropFlags::WRITE),
        ];
        assert!(select_profile(&bad).is_err());
        let duplicates = [
            characteristic(NOTIFY_UUID, 1, CharPropFlags::NOTIFY),
            characteristic(WRITE_UUID, 1, CharPropFlags::WRITE),
            characteristic(NOTIFY_UUID, 2, CharPropFlags::NOTIFY),
            characteristic(WRITE_UUID, 2, CharPropFlags::WRITE),
        ];
        assert!(select_profile(&duplicates).is_err());
    }
}
