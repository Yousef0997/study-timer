# Study Timer

A daily study scheduler for Linux (built and tested on RHEL 10 / GNOME). Add your courses with a start time and
length; each one starts by itself every chosen day, with a desktop notification, a bell and a spoken message,
and rings an alarm when time is up. It keeps repeating until you mark the course done.

Plan the rest of the day the same way: rest breaks, gym, lunch and dinner.

## Features

- **Daily scheduler**: courses, rest, gym, meals and other activities, each on chosen weekdays.
- **Focus Mode** switch in the top bar: while on, each scheduled item starts and ends by itself, back to back.
  While off, nothing starts by itself (use "Start now"). Switching it on mid-slot starts that item for the time left.
- **Mark done**: a finished course leaves the schedule (you can restore it).
- **Alarms**: at the exact start time, a popup, a bell and an English voice. 5 seconds before the end, a popup and
  the alarm for 3 seconds, so the next item starts on the dot with no overlap. If nothing follows, a voice says time
  is up at the end time. Popups close by themselves. Optional reminder 5 minutes before the end.
- **Today view**: a countdown dial with Pause, Add 5 min and Stop, plus the day's timeline and quick 5/10/15 minute breaks.
- **Progress**: time studied today and this week, a study streak, a 7-day chart and totals per course.
- **Start at login** switch in Settings (adds or removes `~/.config/autostart/study-timer.desktop`).
- Closing the window minimizes it so alarms keep ringing (can be switched off); only one copy runs at a time.
- Chalkboard (dark) and notebook (light) themes.

## Install

```bash
./install.sh
```

This builds a release binary and installs it to `~/.local/bin/study-timer` with a menu entry and icon.
It needs Rust (`curl https://sh.rustup.rs -sSf | sh`) and uses tools RHEL already ships:
`notify-send`, `paplay`/`pw-play` and `espeak-ng`/`spd-say`.

Your data lives in `~/.local/share/study-timer/data.json`.

## Share with friends (app only, no source code)

```bash
packaging/build-installer.sh      # creates dist/study-timer-installer.run
```

Send them `dist/study-timer-installer.run`. They run:

```bash
chmod +x study-timer-installer.run
./study-timer-installer.run              # install or update (no root needed)
./study-timer-installer.run --uninstall  # remove
```

It needs a 64-bit Linux with glibc 2.39 or newer (RHEL 10, Fedora 40+, Ubuntu 24.04+, Debian 13+).

## Develop

```bash
cargo test        # scheduler, autostart and speech tests
cargo run
```

`cargo build --features snapshot` adds a developer-only screenshot mode
(`STUDY_TIMER_SNAPSHOT=out.png STUDY_TIMER_TAB=today|schedule|progress|settings`).
