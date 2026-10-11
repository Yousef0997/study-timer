# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

Study Timer is a single-binary Rust (edition 2024) desktop app for Linux/GNOME built on `eframe`/egui. It runs a
daily study schedule (courses, rest, gym, meals) and fires a popup, a sound and a spoken message at each start and end.

All app code lives on the `worktree-study-timer-app` branch, in `.claude/worktrees/study-timer-app`. `master` holds
only the initial scaffold.

## Commands

```bash
cargo test                                   # all tests (scheduler, alerts, autostart)
cargo test focus_mode_runs_the_day_back_to_back   # one test, by name substring
cargo run                                    # run the app (a second launch only raises the first)
./install.sh                                 # release build -> ~/.local/bin/study-timer + menu entry + icon
packaging/build-installer.sh                 # dist/study-timer-installer.run (binary only, no source)
cargo build --features snapshot              # dev build that can screenshot itself, see below
```

- `rustfmt` and `clippy` are not installed. The code uses lines of about 120–150 columns, so don't reformat
  existing code.
- Quit the running app before installing (`pkill -x study-timer`). Afterwards, check with
  `cmp ~/.local/bin/study-timer target/release/study-timer`.
- `dist/` is gitignored. Rebuild the installer after every change that should reach users.

## Architecture

**Threads and shared state.** `main.rs` loads `model::Data` into an `Arc<Mutex<Data>>` that two threads share:
- `spawn_scheduler` wakes on every whole-second boundary, calls `Data::tick(now)`, saves when something changed,
  and passes the returned alerts to `alerts::fire`.
- The egui UI (`ui.rs`, `StudyApp`) locks the same data each frame. It sets `self.dirty` to save, and queues alerts
  for user actions such as "Start now" in `self.pending`.

**`model.rs` contains all the scheduling logic, as plain code with no I/O.** `Data::tick(now) -> (Vec<Alert>, changed)`
decides everything time-based:
- It auto-starts activities only while `settings.focus_mode` is on. Each starts once per day (`last_started`). If it
  starts mid-slot, it runs only for the time left.
- It fires the finish popup and alarm `FINISH_LEAD_SECS` (5) before the end, then cuts them off after
  `FINISH_ALARM_SECS` (3). This leaves a quiet gap so the next item starts exactly on the second.
- The spoken "time is up" at the end time is dropped when another item starts in the same tick, so the two voices
  never overlap.
- Ending on time never marks a course done. Only `mark_done` does that, and it is user-triggered.

Tests drive `tick` with fixed times through `at(h, m, s)` (a Wednesday). Tests that expect auto-start use the
`focused()` helper, because `focus_mode` is off by default.

**`alerts.rs` handles the side effects**, calling external programs: `notify-send`, `gdbus`, `paplay`/`pw-play`,
`espeak-ng`/`spd-say`. Alerts returned from one `tick` count as one moment:
1. All their popups appear at once.
2. One sound plays, the highest `Sound::rank`.
3. Each non-empty `speech` is spoken in turn.

A global `PLAYING` mutex stops two moments from talking over each other. An empty `title`, `None` sound or empty
`speech` means that part is skipped.

Hard-won details to keep:
- GNOME ignores `--expire-time`. Popups are sent with `--print-id --transient` and closed explicitly through
  `gdbus … CloseNotification` after `lasts` (or `POPUP_SECS`) seconds.
- Players are always started at full volume (`player_command`). WirePlumber remembers each app's last stream volume
  in `~/.local/state/wireplumber/stream-properties`, so a single `paplay --volume=0` once muted every alarm.
  **Never test audio at volume 0.** Stub or log the player commands instead.

**Persistence.** `~/.local/share/study-timer/data.json` (respects `XDG_DATA_HOME`):
- It is written atomically, through a temp file and a rename.
- An unreadable file is renamed to `data.json.broken` and the app starts fresh.
- `Data` and `Settings` use `#[serde(default)]`, and every newly added field needs a serde default so existing
  users' files still load. See `Running::finish_alerted`.

**Process model.**
- Only one instance runs at a time, enforced through a Unix socket `$XDG_RUNTIME_DIR/study-timer.sock`. A second
  launch writes `show` to it and exits.
- `--background` starts minimized. The login entry (`autostart.rs`, `~/.config/autostart/study-timer.desktop`)
  uses it.
- With `minimize_on_close` on, closing the window only minimizes it. If it is off, closing quits, and no alarms
  ring until the app is opened again.

**UI** (`ui.rs`, `theme.rs`):
- Four tabs: Today (dial + day timeline), Schedule (add/edit form + lists), Progress, Settings. The Focus Mode
  switch sits in the top bar.
- `theme.rs` loads Red Hat fonts from `/usr/share/fonts/redhat-vf/`, falling back to egui's defaults, and
  defines a dark "chalkboard" palette and a light "notebook" palette.

## Testing the real app without touching the user's data or running instance

Point the data and runtime dirs at a scratch folder. Pass the Wayland socket as an absolute path, because moving
`XDG_RUNTIME_DIR` hides it. Also set `PULSE_SERVER`, or sound fails silently:

```bash
RT=/run/user/$(id -u)
WAYLAND_DISPLAY=$RT/wayland-0 PULSE_SERVER=unix:$RT/pulse/native \
XDG_DATA_HOME=$SCRATCH/data XDG_RUNTIME_DIR=$SCRATCH/run \
STUDY_TIMER_SNAPSHOT=$SCRATCH/out.png STUDY_TIMER_TAB=today target/debug/study-timer   # needs --features snapshot
```

To check alert timing end to end, put logging wrapper scripts for `notify-send`, `gdbus`, `paplay` and `espeak-ng`
first on `PATH`. Then run a test `data.json` with `focus_mode: true` and two back-to-back 1-minute activities.
