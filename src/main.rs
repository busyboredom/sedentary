//! Sedentary — a to-do app with work/break (Pomodoro-style) reminders.

#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![warn(unreachable_pub)]
#![warn(clippy::pedantic)]
#![warn(clippy::cargo)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::multiple_crate_versions)]

mod config;
mod todo;
mod water_break;

use std::time::Duration;

use anyhow::Context;
use cosmic::{
    Element,
    app::Task,
    cosmic_config::{self, CosmicConfigEntry},
    iced::{Subscription, futures::SinkExt},
    widget,
};
use uuid::Uuid;

use crate::{
    config::{Config, TodoData},
    todo::{DropZone, Todo, TodoList},
    water_break::{Phase, WaterBreakSettings},
};

fn main() -> cosmic::iced::Result {
    tracing_subscriber::fmt::init();
    let settings = cosmic::app::Settings::default();
    cosmic::app::run::<AppModel>(settings, ())
}

struct AppModel {
    core: cosmic::Core,
    water_break_settings: WaterBreakSettings,
    on_break: bool,
    seconds_remaining: u64,
    paused: bool,
    todos: TodoList,
    /// ID of the task currently pending deletion confirmation.
    pending_delete: Option<Uuid>,
    /// Whether the settings panel is visible.
    show_settings: bool,
    config_context: Option<cosmic_config::Config>,
    drag: DragState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DragState {
    /// No drag in progress.
    Idle,
    /// A task is being dragged but hasn't hovered over a target yet.
    Dragging { source: Uuid },
    /// Dragging over a specific task with a drop zone.
    Over {
        source: Uuid,
        target: Uuid,
        zone: DropZone,
    },
    /// Dragging over the bottom drop zone (move to end of list).
    ToEnd { source: Uuid },
}

/// Messages emitted by the application and its widgets.
#[derive(Debug, Clone)]
enum Message {
    /// Timer tick (every second).
    TimerTick,
    SkipPhase,
    TogglePause,
    ToggleSettings,
    SetWorkMinutes(f64),
    SetBreakMinutes(f64),
    AddTodo,
    ToggleComplete(Uuid),
    UpdateTitle(Uuid, String),
    ToggleNotes(Uuid),
    UpdateNotes(Uuid, cosmic::widget::text_editor::Action),
    /// Request deletion of a task (shows dialog).
    RequestDelete(Uuid),
    ConfirmDelete,
    CancelDelete,
    AddSubtask(Uuid),
    /// Begin dragging a task.
    DragStart(Uuid),
    /// Drag moved within a task row — update drop zone.
    DragMove(Uuid, DropZone),
    /// Drag ended (mouse released) — commit reorder.
    DragEnd,
    /// Drag entered the bottom drop zone — move task to end of list.
    DragToEnd,
    /// External config changed.
    UpdateConfig(Config),
}

impl cosmic::Application for AppModel {
    type Executor = cosmic::executor::Default;
    type Flags = ();
    type Message = Message;

    const APP_ID: &'static str = "com.busyboredom.Sedentary";

    fn core(&self) -> &cosmic::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut cosmic::Core {
        &mut self.core
    }

    fn init(core: cosmic::Core, _flags: Self::Flags) -> (Self, Task<Self::Message>) {
        // Load persisted config.
        let (config, config_context) =
            match cosmic_config::Config::new(Self::APP_ID, Config::VERSION) {
                Ok(ctx) => match Config::get_entry(&ctx) {
                    Ok(cfg) => (cfg, Some(ctx)),
                    Err((errs, cfg)) => {
                        for err in errs {
                            tracing::error!("Failed to load config entry: {}", err);
                        }
                        (cfg, Some(ctx))
                    }
                },
                Err(e) => {
                    tracing::error!("Failed to create cosmic config: {:?}", e);
                    (Config::default(), None)
                }
            };

        let settings = WaterBreakSettings {
            work_minutes: config.work_minutes,
            break_minutes: config.break_minutes,
        };
        let phase = Phase::work(&settings);

        let app = Self {
            core,
            seconds_remaining: phase.duration.as_secs(),
            water_break_settings: settings,
            on_break: false,
            paused: false,
            todos: TodoList::new(config.todos.into_iter().map(Todo::from).collect()),
            pending_delete: None,
            show_settings: false,
            config_context,
            drag: DragState::Idle,
        };

        let mut tasks = vec![];
        if let Some(main_window_id) = app.core.main_window_id() {
            let icon_bytes = include_bytes!("../static/sedentary.webp");
            match image::load_from_memory(icon_bytes).context("Failed to load icon from memory") {
                Ok(img) => {
                    let rgba = img.to_rgba8();
                    let (w, h) = rgba.dimensions();
                    match cosmic::iced::window::icon::from_rgba(rgba.into_raw(), w, h)
                        .context("Failed to create iced window icon")
                    {
                        Ok(icon) => {
                            tasks.push(cosmic::iced::window::change_icon(main_window_id, icon));
                        }
                        Err(e) => tracing::warn!("{:?}", e),
                    }
                }
                Err(e) => tracing::warn!("{:?}", e),
            }
        }

        (app, Task::batch(tasks))
    }

    fn view(&self) -> Element<'_, Self::Message> {
        // Todo list
        let mut todo_col = widget::column::with_capacity(self.todos.len() + 1).spacing(8);
        for todo in self.todos.iter() {
            todo_col = todo_col.push(view_task(todo, self.drag));
        }
        if self.drag != DragState::Idle {
            todo_col = todo_col.push(
                widget::mouse_area(widget::container(widget::Space::new(
                    cosmic::iced::Length::Fill,
                    40.0,
                )))
                .on_enter(Message::DragToEnd)
                .on_release(Message::DragEnd),
            );
        }
        todo_col = todo_col.push(widget::button::text("+ New Task").on_press(Message::AddTodo));

        // Main layout
        let mut main_col = widget::column::with_capacity(4).spacing(12);
        main_col = main_col.push(self.view_timer());
        main_col = main_col.push(
            widget::button::text(if self.show_settings {
                "Hide Settings"
            } else {
                "Settings"
            })
            .on_press(Message::ToggleSettings),
        );
        if let Some(settings) = self.view_settings() {
            main_col = main_col.push(settings);
        }
        main_col = main_col.push(
            widget::mouse_area(widget::scrollable(todo_col).height(cosmic::iced::Length::Fill))
                .on_release(Message::DragEnd),
        );

        widget::container(main_col)
            .padding(16)
            .width(cosmic::iced::Length::Fill)
            .height(cosmic::iced::Length::Fill)
            .into()
    }

    #[allow(clippy::too_many_lines)]
    fn update(&mut self, message: Self::Message) -> Task<Self::Message> {
        match message {
            Message::TimerTick => {
                if !self.paused {
                    if self.seconds_remaining > 0 {
                        self.seconds_remaining -= 1;
                    } else {
                        self.switch_phase();
                    }
                }
            }
            Message::SkipPhase => self.switch_phase(),
            Message::TogglePause => self.paused = !self.paused,
            Message::ToggleSettings => self.show_settings = !self.show_settings,
            Message::SetWorkMinutes(val) => {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let mins = val as u32;
                self.water_break_settings.work_minutes = mins;
                if !self.on_break {
                    let phase = Phase::work(&self.water_break_settings);
                    self.seconds_remaining = phase.duration.as_secs();
                }
                self.save_config();
            }
            Message::SetBreakMinutes(val) => {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let mins = val as u32;
                self.water_break_settings.break_minutes = mins;
                if self.on_break {
                    let phase = Phase::on_break(&self.water_break_settings);
                    self.seconds_remaining = phase.duration.as_secs();
                }
                self.save_config();
            }
            Message::AddTodo => {
                let todo = Todo::default();
                let id = todo.id;
                self.todos.push(todo);
                self.save_config();
                return Self::focus_todo(id);
            }
            Message::AddSubtask(parent_id) => {
                let todo = Todo::default();
                let id = todo.id;
                if let Some(parent) = self.todos.find_mut(parent_id) {
                    parent.subtasks.push(todo);
                }
                self.save_config();
                return Self::focus_todo(id);
            }
            Message::DragStart(id) => self.drag = DragState::Dragging { source: id },
            Message::DragMove(id, zone) => {
                if let DragState::Dragging { source }
                | DragState::Over { source, .. }
                | DragState::ToEnd { source } = self.drag
                    && source != id
                {
                    self.drag = DragState::Over {
                        source,
                        target: id,
                        zone,
                    };
                }
            }
            Message::DragEnd => self.commit_drag(),
            Message::DragToEnd => {
                if let DragState::Dragging { source } | DragState::Over { source, .. } = self.drag {
                    self.drag = DragState::ToEnd { source };
                }
            }
            Message::ToggleComplete(id) => {
                if let Some(task) = self.todos.find_mut(id) {
                    task.complete = !task.complete;
                }
                self.save_config();
            }
            Message::UpdateTitle(id, title) => {
                if let Some(task) = self.todos.find_mut(id) {
                    task.title = title;
                }
                self.save_config();
            }
            Message::ToggleNotes(id) => {
                if let Some(task) = self.todos.find_mut(id) {
                    task.show_notes = !task.show_notes;
                }
            }
            Message::UpdateNotes(id, action) => {
                if let Some(task) = self.todos.find_mut(id) {
                    task.notes.perform(action);
                }
                self.save_config();
            }
            Message::RequestDelete(id) => self.pending_delete = Some(id),
            Message::ConfirmDelete => {
                if let Some(id) = self.pending_delete.take() {
                    self.todos.remove(id);
                }
                self.save_config();
            }
            Message::CancelDelete => self.pending_delete = None,
            Message::UpdateConfig(config) => {
                self.water_break_settings.work_minutes = config.work_minutes;
                self.water_break_settings.break_minutes = config.break_minutes;
                // Note: We don't overwrite `self.todos` from the watcher anymore.
                // Doing so destroys ephemeral UI state (e.g., cursor position, `show_notes`).
            }
        }
        Task::none()
    }

    fn dialog(&self) -> Option<Element<'_, Self::Message>> {
        self.pending_delete.as_ref()?;
        Some(
            widget::dialog()
                .title("Delete task")
                .body("Are you sure you want to delete this task and all of its subtasks?")
                .primary_action(widget::button::suggested("Yes").on_press(Message::ConfirmDelete))
                .secondary_action(widget::button::standard("No").on_press(Message::CancelDelete))
                .into(),
        )
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        let mut subs = vec![
            self.core()
                .watch_config::<Config>(Self::APP_ID)
                .map(|update| Message::UpdateConfig(update.config)),
        ];

        if !self.paused {
            subs.push(Subscription::run(|| {
                cosmic::iced_futures::stream::channel(1, |mut emitter| async move {
                    let mut interval = tokio::time::interval(Duration::from_secs(1));
                    loop {
                        interval.tick().await;
                        if let Err(e) = emitter.send(Message::TimerTick).await {
                            tracing::error!("Failed to send TimerTick: {:?}", e);
                            break;
                        }
                    }
                })
            }));
        }

        Subscription::batch(subs)
    }
}

impl AppModel {
    fn save_config(&self) {
        if let Some(ctx) = &self.config_context {
            let config = Config {
                work_minutes: self.water_break_settings.work_minutes,
                break_minutes: self.water_break_settings.break_minutes,
                todos: self.todos.iter().map(TodoData::from).collect(),
            };
            if let Err(e) = config.write_entry(ctx) {
                tracing::error!("Failed to save config: {:?}", e);
            }
        }
    }

    /// Returns a [`Task`] that focuses a todo's text input by its ID.
    fn focus_todo(id: Uuid) -> Task<Message> {
        cosmic::widget::text_input::focus(cosmic::widget::Id::new(id.to_string()))
    }

    /// Switches between work and break phases.
    fn switch_phase(&mut self) {
        water_break::chime(self.on_break);
        self.on_break = !self.on_break;
        let phase = Phase::new(self.on_break, &self.water_break_settings);
        self.seconds_remaining = phase.duration.as_secs();
    }

    /// Renders the timer row (progress bar, time display, pause/skip buttons).
    fn view_timer(&self) -> Element<'_, Message> {
        let phase = Phase::new(self.on_break, &self.water_break_settings);
        let total_secs = phase.duration.as_secs();
        let elapsed = total_secs.saturating_sub(self.seconds_remaining);
        #[allow(clippy::cast_precision_loss)]
        let progress = if total_secs > 0 {
            elapsed as f32 / total_secs as f32
        } else {
            0.0
        };

        let minutes = self.seconds_remaining / 60;
        let seconds = self.seconds_remaining % 60;

        widget::row::with_capacity(4)
            .push(widget::progress_bar(0.0..=1.0, progress).width(cosmic::iced::Length::Fill))
            .push(widget::text::body(format!(
                "{} — {}m {:02}s",
                phase.name, minutes, seconds
            )))
            .push(
                widget::button::text(if self.paused { "▶" } else { "⏸" })
                    .on_press(Message::TogglePause),
            )
            .push(widget::button::text(format!("Skip {}", phase.name)).on_press(Message::SkipPhase))
            .spacing(8)
            .align_y(cosmic::iced::Alignment::Center)
            .into()
    }

    /// Renders the settings panel, if visible.
    fn view_settings(&self) -> Option<Element<'_, Message>> {
        if !self.show_settings {
            return None;
        }
        Some(
            widget::settings::section()
                .title("Timer Settings")
                .add(
                    widget::settings::item::builder(format!(
                        "Work: {} min",
                        self.water_break_settings.work_minutes
                    ))
                    .control(widget::slider(
                        1.0..=120.0,
                        f64::from(self.water_break_settings.work_minutes),
                        Message::SetWorkMinutes,
                    )),
                )
                .add(
                    widget::settings::item::builder(format!(
                        "Break: {} min",
                        self.water_break_settings.break_minutes
                    ))
                    .control(widget::slider(
                        1.0..=60.0,
                        f64::from(self.water_break_settings.break_minutes),
                        Message::SetBreakMinutes,
                    )),
                )
                .into(),
        )
    }

    /// Commits the current drag operation and resets drag state.
    fn commit_drag(&mut self) {
        match self.drag {
            DragState::ToEnd { source } => {
                if let Some(todo) = self.todos.remove(source) {
                    self.todos.push(todo);
                }
                self.save_config();
            }
            DragState::Over {
                source,
                target,
                zone,
            } if source != target => {
                if let Some(todo) = self.todos.remove(source) {
                    let placed = match zone {
                        DropZone::Above => self.todos.insert_before(target, todo.clone()),
                        DropZone::Inside => self.todos.nest_inside(target, todo.clone()),
                        DropZone::Below => self.todos.insert_after(target, todo.clone()),
                    };
                    if !placed {
                        self.todos.push(todo);
                    }
                }
                self.save_config();
            }
            _ => {}
        }
        self.drag = DragState::Idle;
    }
}

/// Renders a single task row.
fn view_task(task: &Todo, drag: DragState) -> Element<'_, Message> {
    let id = task.id;
    let active_zone = match drag {
        DragState::Over {
            source,
            target,
            zone,
        } if target == id && source != id => Some(zone),
        _ => None,
    };

    let mut row = widget::row::with_capacity(8)
        .spacing(8)
        .align_y(cosmic::iced::Alignment::Center);

    // Drag handle: press to start dragging, release to drop
    row = row.push(
        widget::mouse_area(
            widget::container(widget::icon::from_name("grip-lines-symbolic")).padding(4),
        )
        .on_press(Message::DragStart(id))
        .on_release(Message::DragEnd),
    );

    row = row
        .push(widget::checkbox("", task.complete).on_toggle(move |_| Message::ToggleComplete(id)));

    row = row.push(
        widget::text_input("Task title", &task.title)
            .id(widget::Id::new(id.to_string()))
            .on_input(move |s| Message::UpdateTitle(id, s)),
    );

    row = row.push(
        widget::button::icon(widget::icon::from_name("list-add-symbolic"))
            .on_press(Message::AddSubtask(id)),
    );

    row = row.push(
        widget::button::icon(widget::icon::from_name(if task.show_notes {
            "pan-up-symbolic"
        } else {
            "pan-down-symbolic"
        }))
        .on_press(Message::ToggleNotes(id)),
    );

    row = row.push(
        widget::button::icon(widget::icon::from_name("edit-delete-symbolic"))
            .on_press(Message::RequestDelete(id)),
    );

    let task_area = widget::mouse_area(row)
        .on_move(move |point| {
            let zone = if point.y < 10.0 {
                DropZone::Above
            } else if point.y > 30.0 {
                DropZone::Below
            } else {
                DropZone::Inside
            };
            Message::DragMove(id, zone)
        })
        .on_release(Message::DragEnd);

    let styled_row = view_drop_feedback(task_area, active_zone);

    let mut col = widget::column::with_capacity(2 + task.subtasks.len())
        .spacing(4)
        .push(styled_row);

    if task.show_notes {
        col = col.push(
            widget::text_editor(&task.notes)
                .on_action(move |action| Message::UpdateNotes(id, action)),
        );
    }

    // Subtasks (indented)
    if !task.subtasks.is_empty() {
        let mut sub_col = widget::column::with_capacity(task.subtasks.len()).spacing(4);
        for sub in &task.subtasks {
            sub_col = sub_col.push(view_task(sub, drag));
        }
        col = col.push(widget::container(sub_col).padding([0, 0, 0, 24]));
    }

    col.into()
}

/// Renders visual feedback for the active drop zone around a task row.
fn view_drop_feedback<'a>(
    task_area: impl Into<Element<'a, Message>>,
    active_zone: Option<DropZone>,
) -> Element<'a, Message> {
    let accent_line = || -> Element<'_, Message> {
        widget::container(widget::Space::new(cosmic::iced::Length::Fill, 4.0))
            .class(cosmic::theme::Container::custom(|theme| {
                let accent = theme.cosmic().accent_color();
                cosmic::widget::container::Style {
                    background: Some(cosmic::iced::Background::Color(accent.into())),
                    ..Default::default()
                }
            }))
            .into()
    };
    match active_zone {
        Some(DropZone::Above) => widget::column::with_capacity(2)
            .push(accent_line())
            .push(task_area)
            .into(),
        Some(DropZone::Inside) => widget::container(task_area)
            .padding([0, 0, 0, 8])
            .class(cosmic::theme::Container::Primary)
            .into(),
        Some(DropZone::Below) => widget::column::with_capacity(2)
            .push(task_area)
            .push(accent_line())
            .into(),
        None => task_area.into(),
    }
}
