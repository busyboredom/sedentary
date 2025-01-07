#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release
#![forbid(unsafe_code)]
//#![warn(missing_docs)]
#![warn(unreachable_pub)]
#![warn(clippy::pedantic)]
#![warn(clippy::cargo)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::multiple_crate_versions)]

mod water_break;

use std::time::Instant;

use eframe::{
    egui::{
        self, include_image, Button, Checkbox, CollapsingHeader, Context, CursorIcon, Id, Label,
        ScrollArea, Sense, TextEdit, Ui, Window,
    },
    emath::{Align, Vec2b},
    epaint::Color32,
    CreationContext,
};
use egui_dnd::{dnd, DragDropItem, Handle};
use uuid::Uuid;

use water_break::{Phase, WaterBreakSettings};

fn main() -> Result<(), eframe::Error> {
    env_logger::init();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([480.0, 320.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Sedentary",
        options,
        Box::new(|cc| {
            // This gives us image support:
            egui_extras::install_image_loaders(&cc.egui_ctx);

            Box::new(MyApp::new(cc))
        }),
    )
}

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(default)]
struct MyApp {
    water_break_settings: WaterBreakSettings,
    on_break: bool,
    #[serde(skip)]
    phase_start: Instant,
    #[serde(skip)]
    paused_at: Option<Instant>,
    todos: Vec<Todo>,
}

impl Default for MyApp {
    fn default() -> Self {
        Self {
            water_break_settings: WaterBreakSettings::default(),
            on_break: false,
            phase_start: Instant::now(),
            paused_at: None,
            todos: vec![Todo::default()],
        }
    }
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top(Id::new("Waterbreak")).show(ctx, |ui| {
            let phase = Phase::new(self);
            let elapsed = self.paused_at.map_or(self.phase_start.elapsed(), |paused| {
                paused.saturating_duration_since(self.phase_start)
            });
            let time_remaining = phase.duration.saturating_sub(elapsed);

            self.progress_bar(ui, phase, elapsed, time_remaining);
            self.settings(ui);

            if time_remaining.is_zero() {
                self.switch_phase();
            }

            if self.paused_at.is_none() {
                ui.ctx().request_repaint();
            }
        });
        egui::TopBottomPanel::bottom(Id::new("Logo"))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.add(
                    egui::Image::new(egui::include_image!("../static/ingwaz.webp")).max_width(20.0),
                );
                if ui.add(Label::new("Donate XMR: 8BxpZ...UbLrb (click to copy)").sense(Sense::click())).on_hover_and_drag_cursor(CursorIcon::Default).is_pointer_button_down_on() {
                    ui.output_mut(|o| o.copied_text = "8BxpZSKtD9XZwgMLWrLV8S2hapXqMuUdvSrFncShVzXaXmVttjPB5ktE7MV5DVHufwRuZPXwdPFctHkckfkJ7eTpApUbLrb".to_string());
                    ui.add(Label::new("Copied!"));
                }
            })
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            ScrollArea::vertical()
                .auto_shrink(Vec2b::new(false, false))
                .show(ui, |ui| {
                    dnd(ui, "dnd_example")
                        .show(self.todos.iter_mut(), |ui, task, handle, _pressed| {
                            Self::show_task(ctx, ui, task, handle);
                        })
                        .update_vec(&mut self.todos);
                    if ui.add(Button::new("New Task")).clicked() {
                        self.todos.push(Todo::default());
                    }

                    // Remove tasks marked for deletion.
                    self.todos.retain(|task| !task.delete);
                });
        });
    }

    /// Called by the frame work to save state before shutdown.
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, self);
    }
}

impl MyApp {
    /// Called once before the first frame.
    fn new(cc: &CreationContext<'_>) -> Self {
        // This is also where you can customize the look and feel of egui using
        // `cc.egui_ctx.set_visuals` and `cc.egui_ctx.set_fonts`.

        // Load previous app state (if any).
        // Note that you must enable the `persistence` feature for this to work.
        if let Some(storage) = cc.storage {
            return eframe::get_value(storage, eframe::APP_KEY).unwrap_or_default();
        }

        Default::default()
    }

    fn show_task(ctx: &Context, ui: &mut Ui, task: &mut Todo, handle: Handle) {
        ui.horizontal(|ui| {
            ui.add(Checkbox::without_text(&mut task.complete)).clicked();

            // Drag and drop handle.
            handle.ui(ui, |ui| {
                ui.add_enabled(!task.complete, TextEdit::singleline(&mut task.title));
            });

            // Button to show task notes.
            if ui
                .add(Button::image(include_image!("../static/notes.png")).fill(Color32::LIGHT_GRAY))
                .clicked()
            {
                task.show_notes = !task.show_notes;
            }

            // Button to delete task.
            if ui
                .add(Button::image(include_image!("../static/trash.png")).fill(Color32::RED))
                .clicked()
            {
                task.confirm_deletion = true;
            }

            if task.confirm_deletion {
                Window::new("Delete task")
                    .resizable(false)
                    .collapsible(false)
                    .show(ctx, |ui| {
                        ui.add(Label::new("Are you sure you want to delete this task?"));
                        ui.with_layout(egui::Layout::right_to_left(Align::TOP), |ui| {
                            if ui.add(Button::new("No")).clicked() {
                                task.delete = false;
                                task.confirm_deletion = false;
                            }
                            if ui.add(Button::new("Yes").fill(Color32::RED)).clicked() {
                                task.delete = true;
                                task.confirm_deletion = false;
                            }
                        });
                    });
            }
        });
        if task.show_notes {
            ui.add(TextEdit::multiline(&mut task.notes));
        };

        CollapsingHeader::new(format!(
            "Subtasks ({}/{})",
            task.subtasks.iter().filter(|t| t.complete).count(),
            task.subtasks.iter().count()
        ))
        .id_source(task.id())
        .show(ui, |ui| {
            dnd(ui, &task.title)
                .show(task.subtasks.iter_mut(), |ui, task, handle, _pressed| {
                    Self::show_task(ctx, ui, task, handle);
                })
                .update_vec(&mut task.subtasks);
            if ui.add(Button::new("New Subtask")).clicked() {
                task.subtasks.push(Todo::default());
            }

            // Remove subtasks marked for deletion.
            task.subtasks.retain(|task| !task.delete);
        });
    }
}

#[derive(serde::Deserialize, serde::Serialize)]
struct Todo {
    id: Uuid,
    title: String,
    show_notes: bool,
    notes: String,
    subtasks: Vec<Todo>,
    delete: bool,
    confirm_deletion: bool,
    complete: bool,
}

impl DragDropItem for Todo {
    fn id(&self) -> Id {
        Id::new(format!("Task ID: {}", self.id))
    }
}

impl DragDropItem for &mut Todo {
    fn id(&self) -> Id {
        Id::new(format!("Task ID: {}", self.id))
    }
}

impl Default for Todo {
    fn default() -> Self {
        Todo {
            id: Uuid::new_v4(),
            title: "New task title".to_string(),
            notes: "New task notes".to_string(),
            show_notes: false,
            subtasks: Vec::new(),
            delete: false,
            confirm_deletion: false,
            complete: false,
        }
    }
}
