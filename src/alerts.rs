//! Desktop notification, alarm sound and spoken message, using tools that ship with RHEL/GNOME.

use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

/// How long a popup stays on screen before it closes itself.
const POPUP_SECS: u64 = 6;

/// Held while a sound or voice plays, so two moments never talk over each other.
static PLAYING: Mutex<()> = Mutex::new(());

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

    /// When several alerts fire together, the loudest kind is the one played.
    fn rank(self) -> u8 {
        match self {
            Sound::Start => 0,
            Sound::Warn => 1,
            Sound::Done => 2,
            Sound::End => 3,
        }
    }
}

/// Anything left empty or `None` is skipped: no title means no popup, no speech means silence.
#[derive(Clone, Debug, Default)]
pub struct Alert {
    pub title: String,
    pub body: String,
    pub speech: String,
    pub sound: Option<Sound>,
    /// Urgent popups show even in Do Not Disturb.
    pub urgent: bool,
    /// Close the popup and cut the sound off after this many seconds (default: popup `POPUP_SECS`, full sound).
    pub lasts: Option<u64>,
}

/// Alerts that fire together (a session ending as the next one starts) belong to the same moment:
/// all their popups appear at once, then one sound plays, then each message is spoken in turn.
pub fn fire(alerts: Vec<Alert>, sound: bool, voice: bool) {
    if alerts.is_empty() {
        return;
    }
    thread::spawn(move || {
        for alert in alerts.iter().filter(|a| !a.title.is_empty()) {
            notify(alert);
        }
        let _turn = PLAYING.lock().unwrap_or_else(|e| e.into_inner());
        if sound {
            if let Some((chosen, lasts)) = batch_sound(&alerts) {
                match lasts {
                    Some(secs) => play_for(chosen.file(), Duration::from_secs(secs)),
                    None => play(chosen.file()),
                }
            }
        }
        if voice {
            for alert in alerts.iter().filter(|a| !a.speech.is_empty()) {
                speak(&alert.speech);
            }
        }
    });
}

fn batch_sound(alerts: &[Alert]) -> Option<(Sound, Option<u64>)> {
    alerts.iter().filter_map(|a| a.sound.map(|s| (s, a.lasts))).max_by_key(|(s, _)| s.rank())
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
    let secs = alert.lasts.unwrap_or(POPUP_SECS);
    let expire = format!("--expire-time={}", secs * 1000);
    let output = Command::new("notify-send")
        .args(["--app-name=Study Timer", "--icon=study-timer", urgency, "--transient", "--print-id", &expire, &alert.title, &alert.body])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();
    // GNOME ignores --expire-time, so close the popup ourselves.
    let Some(id) = output.ok().and_then(|o| String::from_utf8(o.stdout).ok()).and_then(|s| s.trim().parse::<u32>().ok()) else { return };
    thread::spawn(move || {
        thread::sleep(Duration::from_secs(secs));
        quiet(Command::new("gdbus").args([
            "call",
            "--session",
            "--dest=org.freedesktop.Notifications",
            "--object-path=/org/freedesktop/Notifications",
            "--method=org.freedesktop.Notifications.CloseNotification",
            &id.to_string(),
        ]));
    });
}

pub fn play(file: &str) {
    if !quiet(&mut player_command("paplay", file)) {
        quiet(&mut player_command("pw-play", file));
    }
}

/// Always ask for full volume: the sound server remembers each player's last volume, and a
/// muted one would otherwise silence every alarm.
fn player_command(player: &str, file: &str) -> Command {
    let mut cmd = Command::new(player);
    cmd.arg(if player == "paplay" { "--volume=65536" } else { "--volume=1.0" }).arg(file);
    cmd
}

/// Play a sound, stopping it after `limit` if it runs longer.
fn play_for(file: &str, limit: Duration) {
    for player in ["paplay", "pw-play"] {
        if run_for(player_command(player, file), limit) {
            return;
        }
    }
}

/// Run a command, killing it once `limit` has passed. False if it could not start or failed.
fn run_for(mut cmd: Command, limit: Duration) -> bool {
    let Ok(mut child) = cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() else {
        return false;
    };
    let deadline = std::time::Instant::now() + limit;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Err(_) => return false,
            Ok(None) if std::time::Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return true;
            }
            Ok(None) => thread::sleep(Duration::from_millis(20)),
        }
    }
}

pub fn speak(text: &str) {
    if !quiet(Command::new("espeak-ng").args(["-v", "en-us", "-s", "150", text])) {
        quiet(Command::new("spd-say").args(["-w", "-l", "en", text]));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alert(sound: Sound) -> Alert {
        Alert { sound: Some(sound), ..Default::default() }
    }

    #[test]
    fn one_sound_per_moment_and_the_alarm_wins() {
        assert!(batch_sound(&[]).is_none());
        assert!(batch_sound(&[Alert::default()]).is_none());
        assert!(matches!(batch_sound(&[alert(Sound::Start)]), Some((Sound::Start, None))));
        assert!(matches!(batch_sound(&[alert(Sound::End), alert(Sound::Start)]), Some((Sound::End, None))));
        assert!(matches!(batch_sound(&[alert(Sound::Start), alert(Sound::End)]), Some((Sound::End, None))));
    }

    #[test]
    fn sound_is_cut_off_at_the_limit() {
        let start = std::time::Instant::now();
        let mut long = Command::new("sleep");
        long.arg("5");
        assert!(run_for(long, Duration::from_millis(300)));
        assert!(start.elapsed() < Duration::from_secs(2));
        assert!(!run_for(Command::new("false"), Duration::from_secs(1)));
    }
}
