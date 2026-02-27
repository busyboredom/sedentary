# Sedentary

<p align="center">
  <img src="static/sedentary.webp" width="128" alt="Sedentary icon">
</p>

A simple to-do app with Pomodoro-style work/break reminders, built for the [COSMIC](https://system76.com/cosmic) desktop.

## Features

- **Task management** — create, edit, delete, and nest tasks as subtasks
- **Drag-and-drop reordering** — drag tasks above, below, or inside other tasks
- **Work/break timer** — configurable Pomodoro-style timer with audio chimes
- **Persistent config** — tasks and settings are saved via `cosmic-config`

## Building

Requires a Rust toolchain and the COSMIC desktop libraries.

```sh
cargo build --release
```

## Running

```sh
cargo run
```
