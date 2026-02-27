use std::{
    io::Cursor,
    time::{Duration, Instant},
};

use eframe::{
    egui::{self, Button, CollapsingHeader, Ui, WidgetText},
    epaint::Color32,
};
use rodio::{Decoder, DeviceSinkBuilder, Player};

use crate::MyApp;

const DEFAULT_WORK_MINUTES: u32 = 30;
const DEFAULT_BREAK_MINUTES: u32 = 5;
const BREAK_COLOR: &str = "#ff7700";
const WORK_COLOR: &str = "#0a9dff";

impl MyApp {
    pub(crate) fn progress_bar(
        &mut self,
        ui: &mut Ui,
        phase: &Phase,
        elapsed: Duration,
        time_remaining: Duration,
    ) {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
            if ui
                .add(
                    Button::new(
                        WidgetText::from(format!("Skip {}", phase.name)).color(Color32::BLACK),
                    )
                    .fill(phase.next_color),
                )
                .clicked()
            {
                self.switch_phase();
            }

            if ui
                .add(Button::new(
                    WidgetText::from(
                        (if self.paused_at.is_some() {
                            "▶"
                        } else {
                            " || "
                        })
                        .to_string(),
                    )
                    .color(Color32::BLACK),
                ))
                .clicked()
            {
                if self.paused_at.is_none() {
                    self.paused_at = Some(Instant::now());
                } else {
                    self.phase_start += self
                        .paused_at
                        .map_or(Duration::ZERO, |paused| paused.elapsed());
                    self.paused_at = None;
                }
            }

            ui.add(
                egui::ProgressBar::new(elapsed.as_secs_f32() / phase.duration.as_secs_f32())
                    .fill(phase.color)
                    .text(
                        WidgetText::from(format!(
                            "{} time remaining: {}m {}s",
                            phase.name,
                            time_remaining.as_secs() / 60,
                            time_remaining.as_secs() % 60,
                        ))
                        .color(Color32::WHITE),
                    ),
            );
        });
    }

    pub(crate) fn settings(&mut self, ui: &mut Ui) {
        CollapsingHeader::new("Water break settings").show(ui, |ui| {
            ui.add(
                egui::Slider::new(&mut self.water_break_settings.work_minutes, 0..=120)
                    .text("Work duration (minutes)"),
            );
            ui.add(
                egui::Slider::new(&mut self.water_break_settings.break_minutes, 0..=60)
                    .text("Break duration (minutes)"),
            );
        });
    }

    pub(crate) fn switch_phase(&mut self) {
        self.chime();
        self.phase_start = Instant::now();
        self.on_break = !self.on_break;
    }

    pub(crate) fn chime(&self) {
        let on_break = self.on_break;
        std::thread::spawn(move || {
            // Open the default audio device.
            let mut handle = DeviceSinkBuilder::open_default_sink().unwrap();
            handle.log_on_drop(false);
            let player = Player::connect_new(handle.mixer());
            // Load and decode the appropriate sound from the embedded bytes.
            let source = if on_break {
                Decoder::new(Cursor::new(include_bytes!("../static/Work.mp3").as_slice())).unwrap()
            } else {
                Decoder::new(Cursor::new(
                    include_bytes!("../static/Break.mp3").as_slice(),
                ))
                .unwrap()
            };
            player.append(source);
            // Block this thread until playback finishes so the handle stays alive.
            player.sleep_until_end();
        });
    }
}

#[derive(serde::Deserialize, serde::Serialize)]
pub(crate) struct WaterBreakSettings {
    pub(crate) break_minutes: u32,
    pub(crate) work_minutes: u32,
}

impl Default for WaterBreakSettings {
    fn default() -> Self {
        WaterBreakSettings {
            break_minutes: DEFAULT_BREAK_MINUTES,
            work_minutes: DEFAULT_WORK_MINUTES,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Phase {
    pub(crate) name: &'static str,
    pub(crate) color: Color32,
    pub(crate) next_color: Color32,
    pub(crate) duration: Duration,
}

impl Phase {
    pub(crate) fn new(app: &MyApp) -> Phase {
        if app.on_break {
            Phase {
                name: "Break",
                color: Color32::from_hex(BREAK_COLOR).unwrap(),
                next_color: Color32::from_hex(WORK_COLOR).unwrap(),
                duration: Duration::from_secs(
                    u64::from(app.water_break_settings.break_minutes) * 60,
                ),
            }
        } else {
            Phase {
                name: "Work",
                color: Color32::from_hex(WORK_COLOR).unwrap(),
                next_color: Color32::from_hex(BREAK_COLOR).unwrap(),
                duration: Duration::from_secs(
                    u64::from(app.water_break_settings.work_minutes) * 60,
                ),
            }
        }
    }
}
