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
use jiff::civil::Date;
use uuid::Uuid;

/// A date that represents no date selected, 'cause cosmic
/// doesn't provide a way to represent that.
const CALENDAR_EMPTY: Date = Date::constant(1, 1, 1);

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
    config_path: Option<cosmic_config::Config>,
    drag: DragState,
    active_date_picker: ActiveDatePicker,
    calendar_model: cosmic::widget::calendar::CalendarModel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActiveDatePicker {
    /// Hide calendar.
    None,
    /// Show calendar to pick a deadline for this task.
    Task(Uuid),
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
    /// Enter pressed in a task title — create a sibling todo below.
    InsertTaskAfter(Uuid),
    ToggleNotes(Uuid),
    UpdateNotes(Uuid, cosmic::widget::text_editor::Action),
    /// Request deletion of a task (shows dialog).
    RequestDelete(Uuid),
    ConfirmDelete,
    CancelDelete,
    AddSubtask(Uuid),
    ToggleCollapse(Uuid),
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
    /// Toggle the date picker for a task by uuid.
    ToggleDatePicker(Uuid),
    CloseDatePicker,
    SetDeadlineDate(Uuid, jiff::civil::Date),
    UpdateDeadlineTimeInput(Uuid, String),
    SetDeadlineTime(Uuid, String),
    SetRecurrenceType(Uuid, RecurrenceType),
    ClearDeadline(Uuid),
    // These are Strings just because cosmic doesn't have a good
    // numeric input widget.
    UpdateRecurrenceIntervalDays(Uuid, String),
    UpdateRecurrenceIntervalHours(Uuid, String),
    UpdateRecurrenceIntervalMinutes(Uuid, String),
    ToggleWeekday(Uuid, crate::config::WeekdayConfig),
    CalendarPrevMonth,
    CalendarNextMonth,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecurrenceType {
    None,
    Interval,
    Weekly,
    Yearly,
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
        let (config, config_path) = match cosmic_config::Config::new(Self::APP_ID, Config::VERSION)
        {
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
            config_path,
            drag: DragState::Idle,
            active_date_picker: ActiveDatePicker::None,
            calendar_model: cosmic::widget::calendar::CalendarModel {
                selected: CALENDAR_EMPTY,
                visible: jiff::Zoned::now().date(),
            },
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
                            tasks.push(cosmic::iced::window::set_icon(main_window_id, icon));
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
        let mut main_col = widget::column::with_capacity(4).spacing(10);

        // Timer
        main_col = main_col.push(self.view_timer());

        // Settings
        let settings_icon = if self.show_settings {
            "pan-up-symbolic"
        } else {
            "pan-down-symbolic"
        };
        main_col = main_col.push(
            widget::button::custom(
                widget::row::with_capacity(2)
                    .spacing(8)
                    .align_y(cosmic::iced::Alignment::Center)
                    .push(widget::text::body("Settings"))
                    .push(widget::icon::from_name(settings_icon)),
            )
            .on_press(Message::ToggleSettings),
        );
        if let Some(settings) = self.view_settings() {
            main_col = main_col.push(settings);
        }

        // Todo list.

        // +1 is for the new task button.
        let mut todo_col = widget::column::with_capacity(self.todos.len() + 1).spacing(8);
        for todo in self.todos.iter() {
            todo_col = todo_col.push(view_task(
                todo,
                self.drag,
                self.active_date_picker,
                &self.calendar_model,
            ));
        }

        // Add space for the drag end zone.
        todo_col = todo_col.push(
            widget::mouse_area(widget::container(
                widget::Space::new()
                    .width(cosmic::iced::Length::Fill)
                    .height(10.0),
            ))
            .on_enter(Message::DragToEnd)
            .on_release(Message::DragEnd),
        );

        todo_col = todo_col.push(widget::button::text("+ New Task").on_press(Message::AddTodo));

        let content =
            widget::mouse_area(widget::scrollable(todo_col).height(cosmic::iced::Length::Fill))
                .on_release(Message::DragEnd);

        // Date picker
        let content_element: Element<'_, Message> =
            if self.active_date_picker == ActiveDatePicker::None {
                content.into()
            } else {
                widget::mouse_area(content)
                    .on_press(Message::CloseDatePicker)
                    .into()
            };
        main_col = main_col.push(content_element);

        widget::container(main_col)
            .padding(16)
            .width(cosmic::iced::Length::Fill)
            .height(cosmic::iced::Length::Fill)
            .into()
    }

    #[expect(clippy::too_many_lines)]
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
                if self.todos.tick_recurrences(jiff::Timestamp::now()) {
                    self.save_config();
                }
            }
            Message::SkipPhase => self.switch_phase(),
            Message::TogglePause => self.paused = !self.paused,
            Message::ToggleSettings => self.show_settings = !self.show_settings,
            Message::SetWorkMinutes(val) => {
                #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let mins = val as u32;
                self.water_break_settings.work_minutes = mins;
                if !self.on_break {
                    let phase = Phase::work(&self.water_break_settings);
                    self.seconds_remaining = phase.duration.as_secs();
                }
                self.save_config();
            }
            Message::SetBreakMinutes(val) => {
                #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
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
                    parent.collapsed = false;
                    parent.subtasks.push(todo);
                }
                self.save_config();
                return Self::focus_todo(id);
            }
            Message::ToggleCollapse(id) => {
                if let Some(task) = self.todos.find_mut(id) {
                    task.collapsed = !task.collapsed;
                }
                self.save_config();
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
                    // If we just marked it complete and it has no deadline yet, initialize it.
                    if task.complete && task.recurrence.is_some() && task.deadline.is_none() {
                        task.apply_recurrence_to_deadline();
                    }
                    self.sync_calendar_to_task(id);
                }
                self.todos.pop_completed();
                self.save_config();
            }
            Message::UpdateTitle(id, title) => {
                if let Some(task) = self.todos.find_mut(id) {
                    task.title = title;
                }
                self.save_config();
            }
            Message::InsertTaskAfter(id) => {
                let todo = Todo::default();
                let new_id = todo.id;
                if !self.todos.insert_after(id, todo) {
                    // Fallback: if we can't find the target, just push to end.
                    tracing::warn!("Failed to insert task after {}. Falling back to pushing to end.", id);
                    self.todos.push(Todo::default());
                }
                self.save_config();
                return Self::focus_todo(new_id);
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
            Message::UpdateConfig(new_cfg) => {
                let current_mins = self.water_break_settings.work_minutes;
                let current_break = self.water_break_settings.break_minutes;
                if current_mins != new_cfg.work_minutes || current_break != new_cfg.break_minutes {
                    self.water_break_settings.work_minutes = new_cfg.work_minutes;
                    self.water_break_settings.break_minutes = new_cfg.break_minutes;
                    if self.on_break {
                        let phase = Phase::on_break(&self.water_break_settings);
                        self.seconds_remaining = phase.duration.as_secs();
                    } else {
                        let phase = Phase::work(&self.water_break_settings);
                        self.seconds_remaining = phase.duration.as_secs();
                    }
                }
            }
            Message::ToggleDatePicker(id) => {
                if self.active_date_picker == ActiveDatePicker::Task(id) {
                    self.active_date_picker = ActiveDatePicker::None;
                } else {
                    self.active_date_picker = ActiveDatePicker::Task(id);
                    self.sync_calendar_to_task(id);
                }
            }
            Message::CloseDatePicker => {
                self.active_date_picker = ActiveDatePicker::None;
            }
            Message::SetDeadlineDate(id, date) => {
                if let Some(task) = self.todos.find_mut(id) {
                    let time = if let Some(ts) = task.deadline {
                        ts.to_zoned(jiff::tz::TimeZone::system()).time()
                    } else {
                        jiff::civil::Time::constant(12, 0, 0, 0)
                    };
                    if let Ok(zdt) = date.to_zoned(jiff::tz::TimeZone::system())
                        && let Ok(new_zdt) = zdt.with().time(time).build()
                    {
                        task.deadline = Some(new_zdt.timestamp());
                        if let Some(crate::config::RecurrenceRule::Interval(span)) =
                            task.recurrence.as_mut()
                        {
                            *span = rebuild_interval(
                                task.recurrence_int_d.as_deref().unwrap_or("1"),
                                task.recurrence_int_h.as_deref().unwrap_or("0"),
                                task.recurrence_int_m.as_deref().unwrap_or("0"),
                            );
                        }
                    }
                }

                if self.active_date_picker == ActiveDatePicker::Task(id) {
                    self.calendar_model.selected = date;
                    self.calendar_model.visible = date;
                }
                self.save_config();
            }
            Message::UpdateDeadlineTimeInput(id, input) => {
                if let Some(task) = self.todos.find_mut(id) {
                    task.deadline_time_input = Some(input);
                }
            }
            Message::SetDeadlineTime(id, input) => {
                if let Some(task) = self.todos.find_mut(id)
                    && let Ok(time) = input.parse::<jiff::civil::Time>()
                {
                    if let Some(ts) = task.deadline {
                        if let Ok(zdt) = ts
                            .to_zoned(jiff::tz::TimeZone::system())
                            .with()
                            .time(time)
                            .build()
                        {
                            task.deadline = Some(zdt.timestamp());
                        }
                    } else if let Ok(zdt) = jiff::Timestamp::now()
                        .to_zoned(jiff::tz::TimeZone::system())
                        .with()
                        .time(time)
                        .build()
                    {
                        task.deadline = Some(zdt.timestamp());
                    }
                }
                self.save_config();
            }
            Message::SetRecurrenceType(id, rtype) => {
                if let Some(task) = self.todos.find_mut(id) {
                    task.recurrence = match rtype {
                        RecurrenceType::None => None,
                        RecurrenceType::Interval => {
                            Some(crate::config::RecurrenceRule::Interval(rebuild_interval(
                                task.recurrence_int_d.as_deref().unwrap_or("1"),
                                task.recurrence_int_h.as_deref().unwrap_or("0"),
                                task.recurrence_int_m.as_deref().unwrap_or("0"),
                            )))
                        }
                        RecurrenceType::Weekly => Some(crate::config::RecurrenceRule::Weekly(
                            indexmap::IndexSet::new(),
                        )),
                        RecurrenceType::Yearly => {
                            let (m, d) = if let Some(ts) = task.deadline {
                                let zdt = ts.to_zoned(jiff::tz::TimeZone::system());
                                (zdt.month().cast_unsigned(), zdt.day().cast_unsigned())
                            } else {
                                (1, 1)
                            };
                            Some(crate::config::RecurrenceRule::Yearly { month: m, day: d })
                        }
                    };
                }
                self.apply_recurrence_and_sync_calendar(id);
                self.save_config();
            }
            Message::ClearDeadline(id) => {
                if let Some(task) = self.todos.find_mut(id) {
                    task.deadline = None;
                    task.recurrence = None;
                }
                self.sync_calendar_to_task(id);
                self.save_config();
            }
            Message::UpdateRecurrenceIntervalDays(id, val) => {
                if let Some(task) = self.todos.find_mut(id) {
                    task.recurrence_int_d = Some(val);
                    if let Some(crate::config::RecurrenceRule::Interval(span)) =
                        &mut task.recurrence
                    {
                        *span = rebuild_interval(
                            task.recurrence_int_d.as_deref().unwrap_or("1"),
                            task.recurrence_int_h.as_deref().unwrap_or("0"),
                            task.recurrence_int_m.as_deref().unwrap_or("0"),
                        );
                    }
                }
                self.apply_recurrence_and_sync_calendar(id);
                self.save_config();
            }
            Message::UpdateRecurrenceIntervalHours(id, val) => {
                if let Some(task) = self.todos.find_mut(id) {
                    task.recurrence_int_h = Some(val);
                    if let Some(crate::config::RecurrenceRule::Interval(span)) =
                        &mut task.recurrence
                    {
                        *span = rebuild_interval(
                            task.recurrence_int_d.as_deref().unwrap_or("1"),
                            task.recurrence_int_h.as_deref().unwrap_or("0"),
                            task.recurrence_int_m.as_deref().unwrap_or("0"),
                        );
                    }
                }
                self.apply_recurrence_and_sync_calendar(id);
                self.save_config();
            }
            Message::UpdateRecurrenceIntervalMinutes(id, val) => {
                if let Some(task) = self.todos.find_mut(id) {
                    task.recurrence_int_m = Some(val);
                    if let Some(crate::config::RecurrenceRule::Interval(span)) =
                        &mut task.recurrence
                    {
                        *span = rebuild_interval(
                            task.recurrence_int_d.as_deref().unwrap_or("1"),
                            task.recurrence_int_h.as_deref().unwrap_or("0"),
                            task.recurrence_int_m.as_deref().unwrap_or("0"),
                        );
                    }
                }
                self.apply_recurrence_and_sync_calendar(id);
                self.save_config();
            }
            Message::ToggleWeekday(id, wd) => {
                if let Some(task) = self.todos.find_mut(id)
                    && let Some(crate::config::RecurrenceRule::Weekly(days)) = &mut task.recurrence
                    && !days.insert(wd.clone())
                {
                    days.swap_remove(&wd);
                }
                self.apply_recurrence_and_sync_calendar(id);
                self.save_config();
            }
            Message::CalendarPrevMonth => {
                self.calendar_model.show_prev_month();
            }
            Message::CalendarNextMonth => {
                self.calendar_model.show_next_month();
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
                cosmic::iced::keyboard::listen().filter_map(|event| match event {
                    cosmic::iced::keyboard::Event::KeyPressed {
                        key:
                            cosmic::iced::keyboard::Key::Named(
                                cosmic::iced::keyboard::key::Named::Escape,
                            ),
                        ..
                    } => Some(Message::CloseDatePicker),
                    _ => None,
                }),
            ];

        if !self.paused {
            subs.push(Subscription::run(|| {
                cosmic::iced_futures::stream::channel(1, |mut emitter: cosmic::iced::futures::channel::mpsc::Sender<Message>| async move {
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
        if let Some(config_path) = &self.config_path {
            let config = Config {
                work_minutes: self.water_break_settings.work_minutes,
                break_minutes: self.water_break_settings.break_minutes,
                todos: self.todos.iter().map(TodoData::from).collect(),
            };
            if let Err(e) = config.write_entry(config_path) {
                tracing::error!("Failed to save config: {:?}", e);
            }
        }
    }

    /// Returns a [`Task`] that focuses a todo's text input by its ID.
    fn focus_todo(id: Uuid) -> Task<Message> {
        let widget_id = cosmic::widget::Id::new(id.to_string());
        Task::batch(vec![
            cosmic::widget::text_input::focus(widget_id.clone()),
            cosmic::widget::text_input::select_all(widget_id),
        ])
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
        #[expect(clippy::cast_precision_loss)]
        let progress = if total_secs > 0 {
            elapsed as f32 / total_secs as f32
        } else {
            0.0
        };

        let minutes = self.seconds_remaining / 60;
        let seconds = self.seconds_remaining % 60;

        widget::row::with_capacity(4)
            .push(widget::progress_bar(0.0..=1.0, progress).length(cosmic::iced::Length::Fill))
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
fn view_task<'a>(
    task: &'a Todo,
    drag: DragState,
    active_date_picker: ActiveDatePicker,
    calendar_model: &'a cosmic::widget::calendar::CalendarModel,
) -> Element<'a, Message> {
    let id = task.id;
    let active_zone = match drag {
        DragState::Over {
            source,
            target,
            zone,
        } if target == id && source != id => Some(zone),
        _ => None,
    };

    let mut row = widget::row::with_capacity(5)
        .spacing(8)
        .align_y(cosmic::iced::Alignment::Center);

    row = row.push(view_left_handles(id, task));

    row = row.push(widget::checkbox(task.complete).on_toggle(move |_| Message::ToggleComplete(id)));

    row = row.push(
        widget::text_input("Task title", &task.title)
            .id(widget::Id::new(id.to_string()))
            .on_input(move |s| Message::UpdateTitle(id, s))
            .on_submit(move |_| Message::InsertTaskAfter(id)),
    );

    if let Some(timestamp) = task.deadline {
        let zdt = timestamp.to_zoned(jiff::tz::TimeZone::system());
        let date_str = zdt.strftime("%b %-d, %H:%M").to_string();
        row = row.push(widget::text::body(date_str));
    }

    row = row.push(view_right_buttons(id, task));

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

    let mut col = widget::column::with_capacity(3 + task.subtasks.len())
        .spacing(4)
        .push(styled_row);

    if task.show_notes {
        col = col.push(
            widget::text_editor(&task.notes)
                .on_action(move |action| Message::UpdateNotes(id, action)),
        );
    }

    if active_date_picker == ActiveDatePicker::Task(id) {
        col = col.push(
            widget::container(view_deadline_picker(task, calendar_model))
                .width(cosmic::iced::Length::Fill)
                .center_x(cosmic::iced::Length::Fill)
                .padding([8, 16]),
        );
    }

    // Subtasks (indented)
    if !task.subtasks.is_empty() && !task.collapsed {
        let mut sub_col = widget::column::with_capacity(task.subtasks.len()).spacing(4);
        for sub in &task.subtasks {
            sub_col = sub_col.push(view_task(sub, drag, active_date_picker, calendar_model));
        }
        col = col.push(widget::container(sub_col).padding([0, 0, 0, 24]));
    }

    col.into()
}

fn view_left_handles(id: Uuid, task: &Todo) -> widget::Row<'_, Message> {
    let mut row = widget::row::with_capacity(2)
        .spacing(2)
        .align_y(cosmic::iced::Alignment::Center);

    if task.subtasks.is_empty() {
        row = row.push(widget::Space::new().width(20));
    } else {
        let icon_name = if task.collapsed {
            "pan-end-symbolic"
        } else {
            "pan-down-symbolic"
        };
        row = row.push(
            widget::button::icon(widget::icon::from_name(icon_name))
                .on_press(Message::ToggleCollapse(id))
                .padding(2),
        );
    }

    row.push(
        widget::mouse_area(
            widget::container(widget::icon::from_name("grip-lines-symbolic")).padding(4),
        )
        .on_press(Message::DragStart(id))
        .on_release(Message::DragEnd),
    )
}

fn view_right_buttons(id: Uuid, task: &Todo) -> widget::Row<'_, Message> {
    let mut row = widget::row::with_capacity(4)
        .spacing(2)
        .align_y(cosmic::iced::Alignment::Center);

    row = row.push(
        widget::button::icon(widget::icon::from_name("list-add-symbolic"))
            .on_press(Message::AddSubtask(id)),
    );

    row = row.push(
        widget::button::icon(widget::icon::from_name("text-x-generic-symbolic"))
            .on_press(Message::ToggleNotes(id)),
    );

    let deadline_icon = if task.deadline.is_none() {
        "appointment-new-symbolic"
    } else {
        "alarm-symbolic"
    };

    let deadline_btn = widget::button::icon(widget::icon::from_name(deadline_icon));
    row = row.push(deadline_btn.on_press(Message::ToggleDatePicker(id)));

    row.push(
        widget::button::icon(widget::icon::from_name("edit-delete-symbolic"))
            .on_press(Message::RequestDelete(id)),
    )
}

/// Renders visual feedback for the active drop zone around a task row.
fn view_drop_feedback<'a>(
    task_area: impl Into<Element<'a, Message>>,
    active_zone: Option<DropZone>,
) -> Element<'a, Message> {
    let accent_line = || -> Element<'_, Message> {
        widget::container(
            widget::Space::new()
                .width(cosmic::iced::Length::Fill)
                .height(4.0),
        )
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

fn rebuild_interval(days: &str, hrs: &str, mins: &str) -> jiff::Span {
    jiff::Span::new()
        .days(days.parse().unwrap_or(0))
        .hours(hrs.parse().unwrap_or(0))
        .minutes(mins.parse().unwrap_or(0))
}

impl AppModel {
    fn apply_recurrence_and_sync_calendar(&mut self, id: Uuid) {
        if let Some(task) = self.todos.find_mut(id) {
            task.apply_recurrence_to_deadline();
            self.sync_calendar_to_task(id);
        }
    }

    fn sync_calendar_to_task(&mut self, id: Uuid) {
        if let Some(task) = self.todos.find(id)
            && self.active_date_picker == ActiveDatePicker::Task(id)
        {
            if let Some(ts) = task.deadline {
                let d = ts.to_zoned(jiff::tz::TimeZone::system()).date();
                self.calendar_model.selected = d;
                self.calendar_model.visible = d;
            } else {
                self.calendar_model.selected = CALENDAR_EMPTY;
                // Keep visible where it is, or reset to now?
                // Reset to now if it's currently showing year 1 (likely from sentinel)
                if self.calendar_model.visible.year() == 1 {
                    self.calendar_model.visible = jiff::Zoned::now().date();
                }
            }
        }
    }
}

fn view_cal_col<'a>(
    task: &'a Todo,
    calendar_model: &'a cosmic::widget::calendar::CalendarModel,
) -> Element<'a, Message> {
    let id = task.id;
    let cal = widget::calendar(
        calendar_model,
        move |date| Message::SetDeadlineDate(id, date),
        || Message::CalendarPrevMonth,
        || Message::CalendarNextMonth,
        jiff::civil::Weekday::Sunday,
    );

    let time_row = widget::row::with_capacity(3)
        .spacing(4)
        .align_y(cosmic::iced::Alignment::Center)
        .push(widget::text::body("Time:"))
        .push(
            widget::text_input("12:00", task.deadline_time_input.as_deref().unwrap_or(""))
                .width(80)
                .on_input(move |s| Message::UpdateDeadlineTimeInput(id, s))
                .on_submit(move |s| Message::SetDeadlineTime(id, s)),
        )
        .push(
            widget::button::text("Set").on_press(Message::SetDeadlineTime(
                id,
                task.deadline_time_input
                    .clone()
                    .unwrap_or_else(|| "12:00".to_string()),
            )),
        );

    widget::column::with_capacity(3)
        .spacing(8)
        .push(widget::text::title3("Deadline"))
        .push(widget::container(cal).class(cosmic::theme::Container::Card))
        .push(time_row)
        .into()
}

fn view_interval_controls(task: &Todo) -> widget::Row<'_, Message> {
    let id = task.id;
    widget::row::with_capacity(7)
        .spacing(8)
        .align_y(cosmic::iced::Alignment::Center)
        .push(widget::text::body("Every:"))
        .push(
            widget::row::with_capacity(2)
                .spacing(4)
                .align_y(cosmic::iced::Alignment::Center)
                .push(
                    widget::text_input("1", task.recurrence_int_d.as_deref().unwrap_or(""))
                        .width(40)
                        .on_input(move |s| Message::UpdateRecurrenceIntervalDays(id, s)),
                )
                .push(widget::text::body("d")),
        )
        .push(
            widget::row::with_capacity(2)
                .spacing(4)
                .align_y(cosmic::iced::Alignment::Center)
                .push(
                    widget::text_input("0", task.recurrence_int_h.as_deref().unwrap_or(""))
                        .width(40)
                        .on_input(move |s| Message::UpdateRecurrenceIntervalHours(id, s)),
                )
                .push(widget::text::body("h")),
        )
        .push(
            widget::row::with_capacity(2)
                .spacing(4)
                .align_y(cosmic::iced::Alignment::Center)
                .push(
                    widget::text_input("0", task.recurrence_int_m.as_deref().unwrap_or(""))
                        .width(40)
                        .on_input(move |s| Message::UpdateRecurrenceIntervalMinutes(id, s)),
                )
                .push(widget::text::body("m")),
        )
}

fn view_weekly_controls(task: &Todo) -> widget::Row<'_, Message> {
    let id = task.id;
    let mut rec_controls = widget::row::with_capacity(7)
        .spacing(8)
        .align_y(cosmic::iced::Alignment::Center);

    let mk_wd = |lbl,
                 wd: crate::config::WeekdayConfig,
                 days: &indexmap::IndexSet<crate::config::WeekdayConfig>| {
        if days.contains(&wd) {
            widget::button::suggested(lbl)
                .on_press(Message::ToggleWeekday(id, wd))
                .padding([4, 8])
        } else {
            widget::button::standard(lbl)
                .on_press(Message::ToggleWeekday(id, wd))
                .padding([4, 8])
        }
    };

    if let Some(crate::config::RecurrenceRule::Weekly(days)) = &task.recurrence {
        rec_controls = rec_controls
            .push(mk_wd("M", crate::config::WeekdayConfig::Monday, days))
            .push(mk_wd("T", crate::config::WeekdayConfig::Tuesday, days))
            .push(mk_wd("W", crate::config::WeekdayConfig::Wednesday, days))
            .push(mk_wd("Th", crate::config::WeekdayConfig::Thursday, days))
            .push(mk_wd("F", crate::config::WeekdayConfig::Friday, days))
            .push(mk_wd("Sa", crate::config::WeekdayConfig::Saturday, days))
            .push(mk_wd("Su", crate::config::WeekdayConfig::Sunday, days));
    }
    rec_controls
}

fn view_rec_col(task: &Todo) -> Element<'_, Message> {
    let id = task.id;
    let recurrence_type = if let Some(rule) = &task.recurrence {
        match rule {
            crate::config::RecurrenceRule::Interval(_) => RecurrenceType::Interval,
            crate::config::RecurrenceRule::Weekly(_) => RecurrenceType::Weekly,
            crate::config::RecurrenceRule::Yearly { .. } => RecurrenceType::Yearly,
        }
    } else {
        RecurrenceType::None
    };

    let mut rec_controls = widget::row::with_capacity(7)
        .spacing(8)
        .align_y(cosmic::iced::Alignment::Center);

    match recurrence_type {
        RecurrenceType::Interval => {
            rec_controls = view_interval_controls(task);
        }
        RecurrenceType::Weekly => {
            rec_controls = view_weekly_controls(task);
        }
        RecurrenceType::Yearly => {
            rec_controls = rec_controls.push(widget::text::body(
                "Recurs yearly on the set deadline date.",
            ));
        }
        RecurrenceType::None => {}
    }

    widget::column::with_capacity(4)
        .spacing(8)
        .push(widget::text::title3("Recurrence"))
        .push(cosmic::widget::dropdown(
            vec!["None", "Interval", "Weekly", "Yearly"],
            Some(match recurrence_type {
                RecurrenceType::None => 0,
                RecurrenceType::Interval => 1,
                RecurrenceType::Weekly => 2,
                RecurrenceType::Yearly => 3,
            }),
            move |idx| {
                Message::SetRecurrenceType(
                    id,
                    match idx {
                        0 => RecurrenceType::None,
                        1 => RecurrenceType::Interval,
                        2 => RecurrenceType::Weekly,
                        _ => RecurrenceType::Yearly,
                    },
                )
            },
        ))
        .push(rec_controls.wrap())
        .width(280)
        .into()
}

fn view_deadline_picker<'a>(
    task: &'a Todo,
    calendar_model: &'a cosmic::widget::calendar::CalendarModel,
) -> Element<'a, Message> {
    let id = task.id;
    let layout = widget::row::with_capacity(2)
        .spacing(16)
        .push(view_cal_col(task, calendar_model))
        .push(view_rec_col(task))
        .wrap();

    let final_layout = widget::column::with_capacity(2)
        .spacing(8)
        .push(layout)
        .push(
            widget::row::with_capacity(3)
                .push(widget::Space::new().width(cosmic::iced::Length::Fill))
                .push(widget::button::standard("Clear").on_press(Message::ClearDeadline(id)))
                .push(
                    widget::button::standard("Close")
                        .on_press(Message::CloseDatePicker)
                        .class(cosmic::theme::Button::Suggested),
                )
                .spacing(8),
        );

    widget::container(final_layout)
        .class(cosmic::theme::Container::Card)
        .padding(12)
        .into()
}
