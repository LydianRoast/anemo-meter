use crate::ble::{BleCommand, BleEvent, BleWorker, DeviceSummary};
use ane_core::{self as core, ImportedMeasurement, LiveReading};
use eframe::egui::{self, Color32, FontFamily, FontId, RichText, Stroke, Vec2};
use std::{
    collections::VecDeque,
    path::PathBuf,
    time::{Duration, Instant},
};

const BG: Color32 = Color32::from_rgb(32, 41, 54);
const SURFACE: Color32 = Color32::from_rgb(43, 55, 71);
const INSET: Color32 = Color32::from_rgb(24, 34, 47);
const TEXT: Color32 = Color32::from_rgb(229, 237, 246);
const MUTED: Color32 = Color32::from_rgb(169, 185, 204);
const RIM: Color32 = Color32::from_rgb(104, 126, 152);
const BLUE: Color32 = Color32::from_rgb(22, 61, 114);
const ACCENT: Color32 = Color32::from_rgb(134, 197, 255);
const READOUT: Color32 = Color32::from_rgb(175, 229, 255);
const RED: Color32 = Color32::from_rgb(201, 34, 50);
const AMBER: Color32 = Color32::from_rgb(242, 200, 121);

#[derive(PartialEq)]
enum Page {
    Live,
    Saved,
    Analysis,
}

pub struct Console {
    worker: BleWorker,
    data_dir: PathBuf,
    devices: Vec<DeviceSummary>,
    selected: String,
    connected: bool,
    streaming: bool,
    busy: bool,
    show_connection: bool,
    live: LiveReading,
    status: String,
    notice: String,
    last_error: String,
    note: String,
    saved_count: usize,
    last_saved: String,
    log: VecDeque<String>,
    trace: VecDeque<(Instant, Option<f64>)>,
    seconds: u32,
    page: Page,
    file_path: String,
    area: String,
    multiplier: String,
    rows: Vec<ImportedMeasurement>,
    calculation: String,
    references: String,
    reference_result: String,
    capture_path: Option<PathBuf>,
    active_session: Option<String>,
    wants_live: bool,
    #[cfg(target_os = "android")]
    was_focused: bool,
}

impl Console {
    pub fn new(cc: &eframe::CreationContext<'_>, data_dir: Option<PathBuf>) -> Self {
        configure(&cc.egui_ctx);
        let data_dir = data_dir.unwrap_or_else(default_data_directory);
        let saved_count =
            core::count_saved_readings(&data_dir.join("single-readings")).unwrap_or(0);
        Self {
            worker: BleWorker::spawn(data_dir.join("captures")),
            data_dir,
            devices: Vec::new(),
            selected: String::new(),
            connected: false,
            streaming: false,
            busy: false,
            show_connection: true,
            live: LiveReading::new(),
            status: "Disconnected -> Connect your meter to begin".into(),
            notice: String::new(),
            last_error: String::new(),
            note: String::new(),
            saved_count,
            last_saved: String::new(),
            log: VecDeque::new(),
            trace: VecDeque::new(),
            seconds: 30,
            page: Page::Live,
            file_path: String::new(),
            area: "1.0".into(),
            multiplier: "1.0".into(),
            rows: Vec::new(),
            calculation: "Import readings to calculate airflow.".into(),
            references: String::new(),
            reference_result: String::new(),
            capture_path: None,
            active_session: None,
            wants_live: false,
            #[cfg(target_os = "android")]
            was_focused: true,
        }
    }

    fn command(&mut self, command: BleCommand) {
        self.last_error.clear();
        if matches!(command, BleCommand::StartLive) {
            self.wants_live = true;
        }
        if matches!(
            command,
            BleCommand::StopLive | BleCommand::Disconnect | BleCommand::Shutdown
        ) {
            self.wants_live = false;
            self.active_session = None;
            self.live.end("Stopping readings");
            #[cfg(target_os = "android")]
            let _ = crate::android::set_keep_screen_on(false);
        }
        if self.worker.commands.send(command).is_err() {
            self.last_error = "Bluetooth worker stopped. Restart the application.".into();
        }
    }

    fn collect_events(&mut self) {
        while let Ok(event) = self.worker.events.try_recv() {
            match event {
                BleEvent::ScanStarted => {
                    self.busy = true;
                    self.devices.clear();
                    self.selected.clear();
                }
                BleEvent::ConnectStarted => {
                    self.busy = true;
                }
                BleEvent::Status(s) => {
                    self.status = s.clone();
                    self.log.push_back(s);
                }
                BleEvent::Devices(devices) => {
                    self.devices = devices;
                    self.busy = false;
                    if self.devices.len() == 1 {
                        self.selected = self.devices[0].id.clone();
                    }
                }
                BleEvent::Connected { name, .. } => {
                    self.connected = true;
                    self.busy = false;
                    self.show_connection = false;
                    self.status = format!("Connected -> {name} -> Ready to start");
                }
                BleEvent::LiveStarted {
                    capture_path,
                    session_id,
                } => {
                    if !self.wants_live {
                        continue;
                    }
                    self.capture_path = Some(capture_path);
                    self.streaming = true;
                    self.busy = false;
                    self.active_session = Some(session_id);
                    self.live.begin();
                    self.trace.clear();
                    self.status = "Receiving live meter readings".into();
                    #[cfg(target_os = "android")]
                    if let Err(e) = crate::android::set_keep_screen_on(true) {
                        self.last_error = e.to_string();
                    }
                }
                BleEvent::Reading {
                    measurement,
                    received,
                } => {
                    if self.active_session.as_deref()
                        != Some(measurement.context.session_id.as_str())
                    {
                        continue;
                    }
                    self.trace.push_back((received, Some(measurement.fpm())));
                    if let Err(e) = self.live.receive_at(measurement, received) {
                        self.last_error = e.to_string();
                    }
                    self.status = "Receiving live meter readings".into();
                }
                BleEvent::InvalidPacket(reason) => {
                    self.live.invalidate(&reason);
                    self.trace.push_back((Instant::now(), None));
                    self.status = format!("Invalid response -> {reason}");
                }
                BleEvent::LiveStopped => {
                    self.wants_live = false;
                    #[cfg(target_os = "android")]
                    let _ = crate::android::set_keep_screen_on(false);
                    self.active_session = None;
                    self.streaming = false;
                    self.busy = false;
                    self.live.end("Readings stopped");
                    self.trace.push_back((Instant::now(), None));
                    self.status = "Readings stopped -> Saved data retained".into();
                }
                BleEvent::Disconnected => {
                    self.wants_live = false;
                    #[cfg(target_os = "android")]
                    let _ = crate::android::set_keep_screen_on(false);
                    self.active_session = None;
                    self.connected = false;
                    self.streaming = false;
                    self.busy = false;
                    self.live.end("Disconnected");
                    self.trace.push_back((Instant::now(), None));
                    self.status = "Disconnected -> Saved data retained".into();
                }
                BleEvent::Error(error) => {
                    self.busy = false;
                    self.last_error = error.clone();
                    self.log.push_back(error);
                }
            }
        }
        while self.log.len() > 100 {
            self.log.pop_front();
        }
        let now = Instant::now();
        while self
            .trace
            .front()
            .is_some_and(|(t, _)| now.duration_since(*t) > Duration::from_secs(600))
        {
            self.trace.pop_front();
        }
    }

    fn connection(&mut self, ui: &mut egui::Ui) {
        section(ui, "Meter connection", |ui| {
            ui.label(
                RichText::new("On the meter: bLE → OPE. Disconnect other meter apps first.")
                    .color(MUTED),
            );
            ui.horizontal_wrapped(|ui| {
                if action(ui, "Scan for meter", !self.busy && !self.connected, BLUE).clicked() {
                    #[cfg(target_os = "android")]
                    match crate::android::ensure_permissions() {
                        Ok(true) => {
                            self.busy = true;
                            self.command(BleCommand::Scan);
                        }
                        Ok(false) => {
                            self.notice =
                                "Allow Nearby Devices, then press Scan for meter again.".into()
                        }
                        Err(e) => self.last_error = e.to_string(),
                    }
                    #[cfg(not(target_os = "android"))]
                    {
                        self.busy = true;
                        self.command(BleCommand::Scan);
                    }
                }
                let selected_name = self
                    .devices
                    .iter()
                    .find(|d| d.id == self.selected)
                    .map(|d| d.name.as_str())
                    .unwrap_or("Select meter");
                egui::ComboBox::from_id_salt("meter")
                    .selected_text(selected_name)
                    .width(160.0)
                    .show_ui(ui, |ui| {
                        for device in &self.devices {
                            ui.selectable_value(
                                &mut self.selected,
                                device.id.clone(),
                                format!("{} -> {}", device.name, device.id),
                            );
                        }
                    });
                if action(
                    ui,
                    "Connect",
                    !self.busy && !self.connected && !self.selected.is_empty(),
                    BLUE,
                )
                .clicked()
                {
                    self.busy = true;
                    self.command(BleCommand::Connect {
                        device_id: self.selected.clone(),
                    });
                }
                if action(ui, "Disconnect", self.connected && !self.busy, SURFACE).clicked() {
                    self.command(BleCommand::Disconnect);
                }
                if self.busy {
                    ui.spinner();
                    ui.label("Working…");
                }
            });
        });
    }

    fn live_page(&mut self, ui: &mut egui::Ui) {
        if self.show_connection || !self.connected {
            self.connection(ui);
            ui.add_space(10.0);
        }
        section(ui, "Live velocity", |ui| {
            let reading = self.live.displayed().cloned();
            let held = self.live.is_held();
            let state = if held {
                "HELD SNAPSHOT"
            } else if self.live.fresh() {
                "LIVE"
            } else if self.streaming {
                "WAITING / STALE"
            } else {
                "STOPPED"
            };
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(state).strong().color(
                    if held || (self.streaming && reading.is_none()) {
                        AMBER
                    } else {
                        ACCENT
                    },
                ));
                if self.connected
                    && !self.streaming
                    && action(ui, "Start live readings", !self.busy, BLUE).clicked()
                {
                    self.busy = true;
                    self.command(BleCommand::StartLive);
                }
                if self.streaming && action(ui, "Stop", true, RED).clicked() {
                    self.command(BleCommand::StopLive);
                }
                if self.connected && ui.small_button("Connection").clicked() {
                    self.show_connection = !self.show_connection;
                }
            });
            let display_background = ui.painter().add(egui::Shape::Noop);
            let display_frame = egui::Frame::new()
                .fill(Color32::TRANSPARENT)
                .inner_margin(10.0)
                .corner_radius(0)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let font_size = if ui.available_width() < 420.0 {
                        36.0
                    } else {
                        50.0
                    };
                    ui.label(
                        RichText::new(
                            reading
                                .as_ref()
                                .map(|r| format!("{:.1} FPM", r.fpm()))
                                .unwrap_or_else(|| "— FPM".into()),
                        )
                        .font(FontId::monospace(font_size))
                        .color(READOUT),
                    );
                    if let Some(r) = reading.as_ref() {
                        ui.horizontal_wrapped(|ui| {
                            for text in [
                                format!("{:.2} m/s", r.velocity_mps),
                                format!("{:.1} °F", r.fahrenheit()),
                                format!("{:.1}% RH", r.humidity_percent),
                            ] {
                                ui.label(RichText::new(text).size(17.0).color(TEXT));
                                ui.add_space(12.0);
                            }
                        });
                        ui.label(
                            RichText::new(format!("Received {}", r.context.received_at_utc))
                                .small()
                                .color(MUTED),
                        );
                    } else {
                        ui.label(
                            RichText::new("No fresh reading. Missing data is not zero.")
                                .color(MUTED),
                        );
                    }
                });
            ui.painter().set(
                display_background,
                crate::chrome::inset_chassis(display_frame.response.rect),
            );
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                if action(
                    ui,
                    if held { "Resume live" } else { "Hold" },
                    held || self.live.fresh(),
                    SURFACE,
                )
                .clicked()
                {
                    if let Err(e) = self.live.toggle_hold() {
                        self.last_error = e.to_string();
                    }
                }
                if action(ui, "Save one reading", reading.is_some(), BLUE).clicked() {
                    self.save_one();
                }
                ui.label(RichText::new(format!("{} saved", self.saved_count)).color(MUTED));
            });
            ui.add(
                egui::TextEdit::singleline(&mut self.note)
                    .hint_text("Outlet / equipment / reading note")
                    .desired_width(f32::INFINITY),
            );
            if held {
                ui.label(
                    RichText::new(
                        "Held value keeps its original time. The trace continues receiving.",
                    )
                    .color(AMBER),
                );
            }
            ui.add_space(10.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Live trace").strong());
                egui::ComboBox::from_id_salt("window")
                    .selected_text(format!("{} seconds", self.seconds))
                    .show_ui(ui, |ui| {
                        for seconds in [5, 10, 30, 60, 90, 120, 180, 300, 600] {
                            ui.selectable_value(
                                &mut self.seconds,
                                seconds,
                                format!("{seconds} seconds"),
                            );
                        }
                    });
                ui.label(RichText::new("Receive time -> FPM").small().color(MUTED));
            });
            self.draw_trace(ui);
        });
        ui.add_space(10.0);
        egui::CollapsingHeader::new("Diagnostics & raw evidence").show(ui, |ui| {
            ui.label("Raw packets are retained with their receive times. No commands change meter settings.");
            if let Some(path) = &self.capture_path { ui.label(format!("Capture: {}", path.file_name().unwrap_or_default().to_string_lossy())); }
            if action(ui, "Export latest raw capture", self.capture_path.is_some() && !self.streaming, SURFACE).clicked() { self.export_raw(); }
            egui::ScrollArea::vertical().max_height(160.0).id_salt("log").show(ui, |ui| { for line in self.log.iter().rev() { ui.label(RichText::new(line).small().color(MUTED)); } });
        });
    }

    fn save_one(&mut self) {
        match self
            .live
            .save(&self.data_dir.join("single-readings"), &self.note)
        {
            Ok(path) => {
                self.saved_count += 1;
                self.last_saved = format!(
                    "Saved one reading -> {}",
                    path.file_name().unwrap_or_default().to_string_lossy()
                );
                self.notice = self.last_saved.clone();
            }
            Err(e) => self.last_error = format!("Save failed: {e}"),
        }
    }

    fn draw_trace(&self, ui: &mut egui::Ui) {
        let size = Vec2::new(ui.available_width().max(220.0), 165.0);
        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
        let painter = ui.painter_at(rect);
        painter.add(crate::chrome::inset_chassis(rect));
        let plot = egui::Rect::from_min_max(
            rect.min + egui::vec2(54.0, 12.0),
            rect.max - egui::vec2(15.0, 30.0),
        );
        let now = Instant::now();
        let visible: Vec<_> = self
            .trace
            .iter()
            .filter(|(t, _)| now.duration_since(*t).as_secs_f64() <= self.seconds as f64)
            .collect();
        let high = visible
            .iter()
            .filter_map(|(_, v)| *v)
            .fold(1.0f64, |a, v| a.max(v * 1.12));
        let grid = Color32::from_rgb(51, 68, 88);
        for i in 0..=4 {
            let x = egui::lerp(plot.x_range(), i as f32 / 4.0);
            painter.line_segment(
                [egui::pos2(x, plot.top()), egui::pos2(x, plot.bottom())],
                Stroke::new(1.0, grid),
            );
            let sec = (i as f64 / 4.0 - 1.0) * self.seconds as f64;
            painter.text(
                egui::pos2(x, plot.bottom() + 10.0),
                egui::Align2::CENTER_TOP,
                format!("{sec}s"),
                FontId::monospace(11.0),
                MUTED,
            );
        }
        for i in 0..=2 {
            let y = egui::lerp(plot.y_range(), i as f32 / 2.0);
            painter.line_segment(
                [egui::pos2(plot.left(), y), egui::pos2(plot.right(), y)],
                Stroke::new(1.0, grid),
            );
            let value = high * (1.0 - i as f64 / 2.0);
            let label = if high < 10.0 {
                format!("{value:.1}")
            } else {
                format!("{value:.0}")
            };
            painter.text(
                egui::pos2(plot.left() - 7.0, y),
                egui::Align2::RIGHT_CENTER,
                label,
                FontId::monospace(11.0),
                MUTED,
            );
        }
        let mut previous: Option<(Instant, egui::Pos2)> = None;
        for (t, value) in visible {
            let Some(value) = value else {
                previous = None;
                continue;
            };
            let age = now.duration_since(*t).as_secs_f32();
            let point = egui::pos2(
                plot.right() - age / self.seconds as f32 * plot.width(),
                plot.bottom() - (*value / high) as f32 * plot.height(),
            );
            if let Some((old, xy)) = previous {
                if t.duration_since(old) <= Duration::from_secs(5) {
                    painter.line_segment([xy, point], Stroke::new(2.0, READOUT));
                } else {
                    painter.circle_filled(point, 2.0, READOUT);
                }
            } else {
                painter.circle_filled(point, 2.0, READOUT);
            }
            previous = Some((*t, point));
        }
        if self.trace.is_empty() {
            painter.text(
                plot.center(),
                egui::Align2::CENTER_CENTER,
                "Waiting for meter readings",
                FontId::monospace(13.0),
                MUTED,
            );
        }
    }

    fn export_saved(&mut self) {
        let dir = self.data_dir.join("exports");
        let result = (|| -> anyhow::Result<(usize, PathBuf)> {
            std::fs::create_dir_all(&dir)?;
            let path = dir.join(format!(
                "ANE-200W-readings-{}.csv",
                chrono::Utc::now().format("%Y%m%d-%H%M%S-%3f")
            ));
            let count = core::export_saved_csv(&self.data_dir.join("single-readings"), &path)?;
            Ok((count, path))
        })();
        match result {
            Ok((count, path)) => self.publish_export(path, &format!("Exported {count} readings")),
            Err(e) => self.last_error = e.to_string(),
        }
    }

    fn export_raw(&mut self) {
        if let Some(source) = self.capture_path.clone() {
            let folder = self.data_dir.join("exports");
            let path = folder.join(source.file_name().unwrap_or_default());
            match std::fs::create_dir_all(&folder).and_then(|_| std::fs::copy(&source, &path)) {
                Ok(_) => self.publish_export(path, "Exported raw capture"),
                Err(e) => self.last_error = e.to_string(),
            }
        }
    }

    fn publish_export(&mut self, path: PathBuf, message: &str) {
        #[cfg(target_os = "android")]
        match crate::android::export_to_downloads(&path) {
            Ok(location) => self.notice = format!("{message} -> {location}"),
            Err(e) => self.last_error = format!("Local export saved, Downloads copy failed: {e}"),
        }
        #[cfg(not(target_os = "android"))]
        {
            self.notice = format!(
                "{message} -> {} -> Open data folder to locate exports",
                path.file_name().unwrap_or_default().to_string_lossy()
            );
        }
    }

    fn saved_page(&mut self, ui: &mut egui::Ui) {
        section(ui, "Saved single readings", |ui| {
            ui.heading(format!("{} readings", self.saved_count));
            ui.label("Each Save click stores one reading, its note, original receive time and save time.");
            if action(ui, "Export saved CSV", self.saved_count > 0, BLUE).clicked() {
                self.export_saved();
            }
            #[cfg(target_os = "windows")]
            if action(ui, "Open data folder", true, SURFACE).clicked() {
                if let Err(e) = std::process::Command::new("explorer.exe")
                    .arg(&self.data_dir)
                    .spawn()
                {
                    self.last_error = e.to_string();
                }
            }
            if !self.last_saved.is_empty() {
                ui.label(&self.last_saved);
            }
            ui.add_space(8.0);
            ui.label(RichText::new("Hold does not pause capture. Saved held readings keep the time at which they were received.").color(MUTED));
            ui.label(
                RichText::new(
                    "Saved readings and captures are stored in the application data folder.",
                )
                .small()
                .color(MUTED),
            );
        });
    }

    fn analysis_page(&mut self, ui: &mut egui::Ui) {
        section(ui, "Data & modeling", |ui| {
            ui.label("Import the suite's measurement CSV. Imported data never appears as a live reading.");
            ui.add(
                egui::TextEdit::singleline(&mut self.file_path)
                    .hint_text("Full CSV or raw JSONL file path")
                    .desired_width(f32::INFINITY),
            );
            ui.horizontal_wrapped(|ui| {
                #[cfg(target_os = "windows")]
                if action(ui, "Choose file…", true, SURFACE).clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Measurements or raw evidence", &["csv", "jsonl"])
                        .pick_file()
                    {
                        self.file_path = path.display().to_string();
                    }
                }
                if action(ui, "Import CSV", !self.file_path.trim().is_empty(), BLUE).clicked() {
                    match core::import_measurements(&PathBuf::from(
                        self.file_path.trim().trim_matches('"'),
                    )) {
                        Ok(rows) => {
                            self.rows = rows;
                            self.recalculate();
                        }
                        Err(e) => self.last_error = e.to_string(),
                    }
                }
                if action(
                    ui,
                    "Archive raw JSONL",
                    !self.file_path.trim().is_empty(),
                    SURFACE,
                )
                .clicked()
                {
                    match core::archive_raw(
                        &PathBuf::from(self.file_path.trim().trim_matches('"')),
                        &self.data_dir.join("ane200w.sqlite3"),
                    ) {
                        Ok(report) => self.notice = format!("Raw evidence archived -> {report:?}"),
                        Err(e) => self.last_error = e.to_string(),
                    }
                }
            });
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                ui.label("Area ft²");
                ui.add(egui::TextEdit::singleline(&mut self.area).desired_width(80.0));
                ui.label("Multiplier");
                ui.add(egui::TextEdit::singleline(&mut self.multiplier).desired_width(80.0));
                if action(ui, "Calculate", !self.rows.is_empty(), BLUE).clicked() {
                    self.recalculate();
                }
                if action(ui, "Export calculated CFM", !self.rows.is_empty(), SURFACE).clicked() {
                    self.export_calculated();
                }
            });
            ui.label(RichText::new(&self.calculation).color(ACCENT));
            ui.label(RichText::new("CFM = FPM × area × multiplier. Arithmetic sample mean; not a time-weighted mean.").small().color(MUTED));
            egui::ScrollArea::both()
                .max_height(220.0)
                .id_salt("imported")
                .show(ui, |ui| {
                    egui::Grid::new("measurements")
                        .striped(true)
                        .spacing([14.0, 8.0])
                        .show(ui, |ui| {
                            for label in ["Received UTC", "m/s", "Quality", "Source"] {
                                ui.strong(label);
                            }
                            ui.end_row();
                            for r in self.rows.iter().rev().take(100) {
                                ui.label(&r.received_at_utc);
                                ui.label(
                                    r.velocity_mps
                                        .map(|v| format!("{v:.2}"))
                                        .unwrap_or_else(|| "—".into()),
                                );
                                ui.label(&r.quality);
                                ui.label(&r.source);
                                ui.end_row();
                            }
                        });
                });
        });
        ui.add_space(10.0);
        section(ui, "Reference coefficient", |ui| {
            ui.label(
                "Enter independent FPM, CFM reference pairs, one pair per line (at least two).",
            );
            ui.add(
                egui::TextEdit::multiline(&mut self.references)
                    .desired_rows(4)
                    .desired_width(f32::INFINITY)
                    .hint_text("Measured FPM, reference CFM"),
            );
            if action(ui, "Fit effective area", true, BLUE).clicked() {
                let result = (|| -> anyhow::Result<String> {
                    let pairs = self
                        .references
                        .lines()
                        .filter(|s| !s.trim().is_empty())
                        .map(|line| {
                            let (a, b) = line
                                .split_once(',')
                                .ok_or_else(|| anyhow::anyhow!("Each line needs FPM, CFM"))?;
                            Ok((a.trim().parse::<f64>()?, b.trim().parse::<f64>()?))
                        })
                        .collect::<anyhow::Result<Vec<_>>>()?;
                    let fit = core::fit_effective_area(&pairs)?;
                    Ok(format!(
                        "Effective area {:.4} ft² -> RMSE {:.1} CFM -> residuals {:?}",
                        fit.effective_area_ft2, fit.rmse_cfm, fit.residuals_cfm
                    ))
                })();
                match result {
                    Ok(s) => self.reference_result = s,
                    Err(e) => self.last_error = e.to_string(),
                }
            }
            ui.label(&self.reference_result);
            ui.label(RichText::new("An entered-reference fit does not establish instrument calibration or transfer to a different air device.").small().color(MUTED));
        });
    }

    fn recalculate(&mut self) {
        let result = (|| -> anyhow::Result<String> {
            let summary =
                core::summarize(&self.rows, self.area.parse()?, self.multiplier.parse()?)?;
            Ok(format!(
                "{} valid / {} excluded -> mean {:.1} FPM -> calculated {:.1} CFM",
                summary.count, summary.excluded, summary.mean_fpm, summary.calculated_cfm
            ))
        })();
        match result {
            Ok(s) => self.calculation = s,
            Err(e) => self.last_error = e.to_string(),
        }
    }

    fn export_calculated(&mut self) {
        let result = (|| -> anyhow::Result<PathBuf> {
            let folder = self.data_dir.join("exports");
            std::fs::create_dir_all(&folder)?;
            let path = folder.join(format!(
                "ANE-200W-calculated-{}.csv",
                chrono::Utc::now().format("%Y%m%d-%H%M%S-%3f")
            ));
            core::export_measurements(
                &path,
                &self.rows,
                self.area.parse()?,
                self.multiplier.parse()?,
            )?;
            Ok(path)
        })();
        match result {
            Ok(path) => self.publish_export(path, "Calculated CFM exported"),
            Err(e) => self.last_error = e.to_string(),
        }
    }
}

impl eframe::App for Console {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.collect_events();
        #[cfg(target_os = "android")]
        {
            let focused = ctx.input(|i| i.viewport().focused.unwrap_or(true));
            if self.was_focused && !focused && self.connected {
                self.command(BleCommand::Disconnect);
            }
            self.was_focused = focused;
        }
        egui::TopBottomPanel::top("header")
            .frame(egui::Frame::new().fill(BG).inner_margin(8.0))
            .show(ctx, |ui| {
                crate::chrome::header(ui);
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    if action(
                        ui,
                        "Live",
                        true,
                        if self.page == Page::Live {
                            BLUE
                        } else {
                            SURFACE
                        },
                    )
                    .clicked()
                    {
                        self.page = Page::Live;
                    }
                    if action(
                        ui,
                        "Saved",
                        true,
                        if self.page == Page::Saved {
                            BLUE
                        } else {
                            SURFACE
                        },
                    )
                    .clicked()
                    {
                        self.page = Page::Saved;
                    }
                    if action(
                        ui,
                        "Modeling",
                        true,
                        if self.page == Page::Analysis {
                            BLUE
                        } else {
                            SURFACE
                        },
                    )
                    .clicked()
                    {
                        self.page = Page::Analysis;
                    }
                });
            });
        egui::TopBottomPanel::bottom("status")
            .frame(egui::Frame::new().fill(BG).inner_margin(8.0))
            .show(ctx, |ui| {
                crate::chrome::status_surface(ui.painter(), ui.max_rect().expand(8.0));
                ui.label(RichText::new(&self.status).small().color(MUTED));
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG).inner_margin(10.0))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if !self.last_error.is_empty() {
                            ui.horizontal_wrapped(|ui| {
                                ui.colored_label(AMBER, format!("Problem: {}", self.last_error));
                                if ui.small_button("Dismiss").clicked() {
                                    self.last_error.clear();
                                }
                            });
                            ui.add_space(8.0);
                        }
                        if !self.notice.is_empty() {
                            ui.horizontal_wrapped(|ui| {
                                ui.colored_label(ACCENT, &self.notice);
                                if ui.small_button("Dismiss").clicked() {
                                    self.notice.clear();
                                }
                            });
                            ui.add_space(8.0);
                        }
                        match self.page {
                            Page::Live => self.live_page(ui),
                            Page::Saved => self.saved_page(ui),
                            Page::Analysis => self.analysis_page(ui),
                        }
                    });
            });
        ctx.request_repaint_after(Duration::from_millis(100));
    }
}

impl Drop for Console {
    fn drop(&mut self) {
        let _ = self.worker.commands.send(BleCommand::Shutdown);
    }
}

fn action(ui: &mut egui::Ui, text: &str, enabled: bool, color: Color32) -> egui::Response {
    crate::chrome::button(ui, text, enabled, color)
}

fn section(ui: &mut egui::Ui, title: &str, body: impl FnOnce(&mut egui::Ui)) {
    crate::chrome::section(ui, title, body)
}

fn configure(ctx: &egui::Context) {
    ctx.set_theme(egui::ThemePreference::Dark);
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "Routed Gothic".into(),
        egui::FontData::from_static(include_bytes!("../../../assets/fonts/RoutedGothic.ttf"))
            .into(),
    );
    for family in [FontFamily::Monospace, FontFamily::Proportional] {
        fonts
            .families
            .entry(family)
            .or_default()
            .insert(0, "Routed Gothic".into());
    }
    ctx.set_fonts(fonts);
    let mut style = (*ctx.style()).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.window_corner_radius = egui::CornerRadius::ZERO;
    style.visuals.menu_corner_radius = egui::CornerRadius::ZERO;
    for widget in [
        &mut style.visuals.widgets.noninteractive,
        &mut style.visuals.widgets.inactive,
        &mut style.visuals.widgets.hovered,
        &mut style.visuals.widgets.active,
        &mut style.visuals.widgets.open,
    ] {
        widget.corner_radius = egui::CornerRadius::ZERO;
    }
    style.visuals.override_text_color = Some(TEXT);
    style.visuals.text_edit_bg_color = Some(INSET);
    style.visuals.panel_fill = BG;
    style.visuals.window_fill = SURFACE;
    style.visuals.extreme_bg_color = INSET;
    style.visuals.faint_bg_color = BG;
    style.visuals.selection.bg_fill = BLUE;
    style.visuals.selection.stroke = Stroke::new(1.0, ACCENT);
    style.visuals.widgets.noninteractive.bg_fill = SURFACE;
    style.visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, MUTED);
    style.visuals.widgets.inactive.bg_fill = SURFACE;
    style.visuals.widgets.inactive.weak_bg_fill = SURFACE;
    style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, RIM);
    style.visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(62, 81, 105);
    style.visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(62, 81, 105);
    style.visuals.widgets.active.bg_fill = BLUE;
    style.visuals.widgets.active.weak_bg_fill = BLUE;
    style.spacing.item_spacing = Vec2::new(8.0, 6.0);
    style.spacing.button_padding = Vec2::new(10.0, 6.0);
    style.spacing.interact_size = Vec2::new(36.0, 26.0);
    style
        .text_styles
        .insert(egui::TextStyle::Body, FontId::monospace(14.0));
    style
        .text_styles
        .insert(egui::TextStyle::Button, FontId::monospace(14.0));
    style
        .text_styles
        .insert(egui::TextStyle::Small, FontId::monospace(12.0));
    ctx.set_style(style);
}

fn default_data_directory() -> PathBuf {
    if let Some(path) = std::env::var_os("ANE_DATA_DIR") {
        return PathBuf::from(path);
    }
    let executable = std::env::current_exe().ok();
    if let Some(path) = executable.as_ref().and_then(|p| p.parent()) {
        for folder in path.ancestors() {
            if folder.join("Cargo.toml").exists() && folder.join("crates/ane-core").exists() {
                return folder.join("local");
            }
        }
        return path.join("local");
    }
    PathBuf::from("local")
}
