//! Desktop notification, alarm sound and spoken message, using tools that ship with RHEL/GNOME.

use std::process::{Command, Stdio};
use std::thread;

#[derive(Clone, Copy, Debug)]
pub enum Sound {
    Start,
    Warn,
    End,
    Done,
}

impl Sound {
    fn file(self) -> &'static str {
        match self {
            Sound::Start => "/usr/share/sounds/freedesktop/stereo/bell.oga",
            Sound::Warn => "/usr/share/sounds/freedesktop/stereo/message-new-instant.oga",
            Sound::End => "/usr/share/sounds/freedesktop/stereo/alarm-clock-elapsed.oga",
            Sound::Done => "/usr/share/sounds/freedesktop/stereo/complete.oga",
        }
    }

    fn repeats(self) -> usize {
        match self {
            Sound::End => 2,
            _ => 1,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Alert {
    pub title: String,
    pub body: String,
    pub speech: String,
    pub sound: Sound,
    /// Urgent notifications stay on screen until dismissed.
    pub urgent: bool,
}

/// Fire alerts one after another on a background thread so sounds never overlap.
pub fn fire(alerts: Vec<Alert>, sound: bool, voice: bool) {
    if alerts.is_empty() {
        return;
    }
    thread::spawn(move || {
        for alert in alerts {
            notify(&alert);
            if sound {
                for _ in 0..alert.sound.repeats() {
                    play(alert.sound.file());
                }
            }
            if voice {
                speak(&alert.speech);
            }
        }
    });
}

fn quiet(cmd: &mut Command) -> bool {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn notify(alert: &Alert) {
    let urgency = if alert.urgent { "--urgency=critical" } else { "--urgency=normal" };
    quiet(Command::new("notify-send").args([
        "--app-name=Study Timer",
        "--icon=study-timer",
        urgency,
        &alert.title,
        &alert.body,
    ]));
}

pub fn play(file: &str) {
    if !quiet(Command::new("paplay").arg(file)) {
        quiet(Command::new("pw-play").arg(file));
    }
}

pub fn speak(text: &str) {
    if !quiet(Command::new("espeak-ng").args(["-v", "en-us", "-s", "150", text])) {
        quiet(Command::new("spd-say").args(["-w", "-l", "en", text]));
    }
}
