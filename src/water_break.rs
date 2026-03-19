use std::{io::Cursor, thread, time::Duration};

use anyhow::Context;
use rodio::{Decoder, DeviceSinkBuilder, Player};

const DEFAULT_WORK_MINUTES: u32 = 30;
const DEFAULT_BREAK_MINUTES: u32 = 5;

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub(crate) struct WaterBreakSettings {
    pub(crate) work_minutes: u32,
    pub(crate) break_minutes: u32,
}

impl Default for WaterBreakSettings {
    fn default() -> Self {
        Self {
            work_minutes: DEFAULT_WORK_MINUTES,
            break_minutes: DEFAULT_BREAK_MINUTES,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Phase {
    pub(crate) name: &'static str,
    pub(crate) duration: Duration,
}

impl Phase {
    pub(crate) fn new(on_break: bool, settings: &WaterBreakSettings) -> Self {
        if on_break {
            Self::on_break(settings)
        } else {
            Self::work(settings)
        }
    }

    pub(crate) fn work(settings: &WaterBreakSettings) -> Self {
        Self {
            name: "Work",
            duration: Duration::from_secs(u64::from(settings.work_minutes) * 60),
        }
    }

    pub(crate) fn on_break(settings: &WaterBreakSettings) -> Self {
        Self {
            name: "Break",
            duration: Duration::from_secs(u64::from(settings.break_minutes) * 60),
        }
    }
}

pub(crate) fn chime(on_break: bool) {
    thread::spawn(move || {
        let play = || -> anyhow::Result<()> {
            let mut handle = DeviceSinkBuilder::open_default_sink()
                .context("Failed to open default audio sink for chime")?;
            handle.log_on_drop(false);
            let player = Player::connect_new(handle.mixer());
            let source = if on_break {
                Decoder::new(Cursor::new(include_bytes!("../static/Work.mp3").as_slice()))
                    .context("Failed to decode Work.mp3")?
            } else {
                Decoder::new(Cursor::new(
                    include_bytes!("../static/Break.mp3").as_slice(),
                ))
                .context("Failed to decode Break.mp3")?
            };
            player.append(source);
            player.sleep_until_end();
            Ok(())
        };

        if let Err(e) = play() {
            tracing::error!("Failed to play chime: {:?}", e);
        }
    });
}

pub(crate) fn due_chime() {
    thread::spawn(move || {
        let play = || -> anyhow::Result<()> {
            let mut handle = DeviceSinkBuilder::open_default_sink()
                .context("Failed to open default audio sink for due chime")?;
            handle.log_on_drop(false);
            let player = Player::connect_new(handle.mixer());
            let source = Decoder::new(Cursor::new(include_bytes!("../static/due.mp3").as_slice()))
                .context("Failed to decode due.mp3")?;
            player.append(source);
            player.sleep_until_end();
            Ok(())
        };

        if let Err(e) = play() {
            tracing::error!("Failed to play due chime: {:?}", e);
        }
    });
}
