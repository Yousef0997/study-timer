//! Data model and persistence: activities, running timers, session log and settings.

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, TimeZone};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::PathBuf;

use crate::alerts::{Alert, Sound};

/// The finish popup and alarm start this long before a session ends...
pub const FINISH_LEAD_SECS: i64 = 5;
/// ...and stop after this long, leaving a quiet gap before the next session starts on the dot.
pub const FINISH_ALARM_SECS: u64 = 3;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Course,
    Rest,
    Gym,
    Meal,
    Other,
}

impl Kind {
    pub const ALL: [Kind; 5] = [Kind::Course, Kind::Rest, Kind::Gym, Kind::Meal, Kind::Other];

    pub fn label(self) -> &'static str {
        match self {
            Kind::Course => "Course",
            Kind::Rest => "Rest",
            Kind::Gym => "Gym",
            Kind::Meal => "Meal",
            Kind::Other => "Other",
        }
    }

    /// Name suggested in the add form when this kind is picked.
    pub fn suggested_name(self) -> &'static str {
        match self {
            Kind::Course => "",
            Kind::Rest => "Rest break",
            Kind::Gym => "Gym",
            Kind::Meal => "Lunch",
            Kind::Other => "",
        }
    }

    pub fn default_minutes(self) -> u32 {
        match self {
            Kind::Course => 60,
            Kind::Rest => 15,
            Kind::Gym => 60,
            Kind::Meal => 30,
            Kind::Other => 30,
        }
    }
}

pub const DAY_NAMES: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

fn yes() -> bool {
    true
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Activity {
    pub id: u64,
    pub name: String,
    pub kind: Kind,
    pub hour: u32,
    pub minute: u32,
    pub duration_min: u32,
    /// Monday first.
    pub days: [bool; 7],
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Set when a course is marked done; it then stops repeating.
    #[serde(default)]
    pub done_on: Option<NaiveDate>,
    /// The last day the scheduler (or the user) started it, so it fires once per day.
    #[serde(default)]
    pub last_started: Option<NaiveDate>,
}

impl Activity {
    pub fn is_done(&self) -> bool {
        self.done_on.is_some()
    }

    pub fn runs_on(&self, date: NaiveDate) -> bool {
        self.enabled && !self.is_done() && self.days[date.weekday().num_days_from_monday() as usize]
    }

    pub fn start_on(&self, date: NaiveDate) -> Option<DateTime<Local>> {
        let naive = date.and_hms_opt(self.hour, self.minute, 0)?;
        Local.from_local_datetime(&naive).earliest()
    }

    pub fn minutes_of_day(&self) -> u32 {
        self.hour * 60 + self.minute
    }

    pub fn time_label(&self) -> String {
        format!("{:02}:{:02}", self.hour, self.minute)
    }

    pub fn days_label(&self) -> String {
        days_label(&self.days)
    }
}

pub fn days_label(days: &[bool; 7]) -> String {
    match days {
        [true, true, true, true, true, true, true] => "Every day".into(),
        [true, true, true, true, true, false, false] => "Weekdays".into(),
        [false, false, false, false, false, true, true] => "Weekends".into(),
        [false, false, false, false, false, false, false] => "No days".into(),
        _ => DAY_NAMES
            .iter()
            .zip(days)
            .filter(|(_, on)| **on)
            .map(|(n, _)| *n)
            .collect::<Vec<_>>()
            .join(", "),
    }
}

pub fn duration_label(minutes: u32) -> String {
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m} min"),
    }
}

/// The same as `duration_label`, but written out for the speech engine.
pub fn spoken_duration(minutes: u32) -> String {
    let unit = |n: u32, word: &str| format!("{n} {word}{}", if n == 1 { "" } else { "s" });
    match (minutes / 60, minutes % 60) {
        (0, m) => unit(m, "minute"),
        (h, 0) => unit(h, "hour"),
        (h, m) => format!("{} and {}", unit(h, "hour"), unit(m, "minute")),
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Running {
    pub activity_id: Option<u64>,
    pub name: String,
    pub kind: Kind,
    pub total_secs: i64,
    pub ends_at: DateTime<Local>,
    /// Seconds left while paused; `None` while counting down.
    pub paused_left: Option<i64>,
    /// Active seconds credited before the current segment (pauses excluded).
    pub credited_secs: i64,
    pub segment_start: Option<DateTime<Local>>,
    pub warned: bool,
    /// The finish popup and alarm have played (they start `FINISH_LEAD_SECS` before the end).
    #[serde(default)]
    pub finish_alerted: bool,
    pub started_on: NaiveDate,
}

impl Running {
    fn new(activity_id: Option<u64>, name: String, kind: Kind, total_secs: i64, ends_at: DateTime<Local>, now: DateTime<Local>) -> Self {
        Running {
            activity_id,
            name,
            kind,
            total_secs,
            ends_at,
            paused_left: None,
            credited_secs: 0,
            segment_start: Some(now),
            warned: false,
            finish_alerted: false,
            started_on: now.date_naive(),
        }
    }

    pub fn millis_left(&self, now: DateTime<Local>) -> i64 {
        match self.paused_left {
            Some(s) => s * 1000,
            None => (self.ends_at - now).num_milliseconds().max(0),
        }
    }

    pub fn secs_left(&self, now: DateTime<Local>) -> i64 {
        (self.millis_left(now) + 999) / 1000
    }

    pub fn fraction_done(&self, now: DateTime<Local>) -> f32 {
        if self.total_secs <= 0 {
            return 1.0;
        }
        (1.0 - self.millis_left(now) as f32 / (self.total_secs as f32 * 1000.0)).clamp(0.0, 1.0)
    }

    pub fn is_paused(&self) -> bool {
        self.paused_left.is_some()
    }

    fn active_secs(&self, until: DateTime<Local>) -> i64 {
        let segment = self.segment_start.map(|s| (until - s).num_seconds().max(0)).unwrap_or(0);
        self.credited_secs + segment
    }

    pub fn pause(&mut self, now: DateTime<Local>) {
        if self.paused_left.is_none() {
            self.credited_secs = self.active_secs(now);
            self.segment_start = None;
            self.paused_left = Some(self.secs_left(now));
        }
    }

    pub fn resume(&mut self, now: DateTime<Local>) {
        if let Some(left) = self.paused_left.take() {
            self.ends_at = now + Duration::seconds(left);
            self.segment_start = Some(now);
        }
    }

    pub fn add_minutes(&mut self, minutes: i64) {
        self.total_secs += minutes * 60;
        match &mut self.paused_left {
            Some(left) => *left += minutes * 60,
            None => self.ends_at += Duration::minutes(minutes),
        }
        self.warned = false;
        self.finish_alerted = false;
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LogEntry {
    pub activity_id: Option<u64>,
    pub name: String,
    pub kind: Kind,
    pub date: NaiveDate,
    pub minutes: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Settings {
    pub voice: bool,
    pub sound: bool,
    pub warn_before_end: bool,
    pub minimize_on_close: bool,
    pub dark: bool,
    /// While on, scheduled activities start and end by themselves; while off, only "Start now" starts them.
    pub focus_mode: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { voice: true, sound: true, warn_before_end: true, minimize_on_close: true, dark: true, focus_mode: false }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Data {
    pub next_id: u64,
    pub activities: Vec<Activity>,
    pub running: Vec<Running>,
    pub log: Vec<LogEntry>,
    pub settings: Settings,
}

pub fn data_dir() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home_dir().join(".local/share"));
    base.join("study-timer")
}

pub fn home_dir() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp"))
}

pub fn data_file() -> PathBuf {
    data_dir().join("data.json")
}

/// A session shorter than this is not worth logging.
const MIN_LOGGED_SECS: i64 = 60;

impl Data {
    pub fn load() -> Data {
        let path = data_file();
        match fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|err| {
                eprintln!("study-timer: {} is unreadable ({err}); keeping a copy and starting fresh", path.display());
                let _ = fs::rename(&path, path.with_extension("json.broken"));
                Data::default()
            }),
            Err(_) => Data::default(),
        }
    }

    pub fn save(&self) -> io::Result<()> {
        let dir = data_dir();
        fs::create_dir_all(&dir)?;
        let tmp = dir.join("data.json.tmp");
        fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        fs::rename(tmp, data_file())
    }

    pub fn add_activity(&mut self, mut activity: Activity) {
        self.next_id += 1;
        activity.id = self.next_id;
        self.activities.push(activity);
    }

    pub fn activity_mut(&mut self, id: u64) -> Option<&mut Activity> {
        self.activities.iter_mut().find(|a| a.id == id)
    }

    pub fn is_running(&self, id: u64) -> bool {
        self.running.iter().any(|r| r.activity_id == Some(id))
    }

    /// Start an activity right now (from a "Start now" button).
    pub fn start_now(&mut self, id: u64, now: DateTime<Local>) -> Option<Alert> {
        if self.is_running(id) {
            return None;
        }
        let a = self.activity_mut(id)?;
        a.last_started = Some(now.date_naive());
        let secs = a.duration_min as i64 * 60;
        let r = Running::new(Some(id), a.name.clone(), a.kind, secs, now + Duration::seconds(secs), now);
        let alert = start_alert(&r);
        self.running.push(r);
        Some(alert)
    }

    /// A one-off break that is not on the schedule.
    pub fn quick_break(&mut self, minutes: u32, now: DateTime<Local>) -> Alert {
        let secs = minutes as i64 * 60;
        let r = Running::new(None, format!("{minutes}-minute break"), Kind::Rest, secs, now + Duration::seconds(secs), now);
        let alert = start_alert(&r);
        self.running.push(r);
        alert
    }

    /// Stop a running timer early, logging the time spent so far.
    pub fn stop(&mut self, index: usize, now: DateTime<Local>) {
        if index < self.running.len() {
            let r = self.running.remove(index);
            self.log_session(&r, r.active_secs(now));
        }
    }

    /// Mark a course as finished for good: it leaves the daily schedule.
    pub fn mark_done(&mut self, id: u64, now: DateTime<Local>) -> Option<Alert> {
        if let Some(i) = self.running.iter().position(|r| r.activity_id == Some(id)) {
            self.stop(i, now);
        }
        let a = self.activity_mut(id)?;
        a.done_on = Some(now.date_naive());
        let name = a.name.clone();
        Some(Alert {
            title: format!("{name} completed"),
            body: "This course is finished and has left your daily schedule.".into(),
            speech: format!("Congratulations! You finished the {name} course."),
            sound: Some(Sound::Done),
            ..Default::default()
        })
    }

    pub fn restore(&mut self, id: u64) {
        if let Some(a) = self.activity_mut(id) {
            a.done_on = None;
        }
    }

    pub fn delete(&mut self, id: u64) {
        self.running.retain(|r| r.activity_id != Some(id));
        self.activities.retain(|a| a.id != id);
    }

    fn log_session(&mut self, r: &Running, active_secs: i64) {
        if active_secs >= MIN_LOGGED_SECS {
            self.log.push(LogEntry {
                activity_id: r.activity_id,
                name: r.name.clone(),
                kind: r.kind,
                date: r.started_on,
                minutes: ((active_secs + 30) / 60) as u32,
            });
        }
    }

    /// Called once a second by the scheduler thread. Returns alerts to fire and whether anything changed.
    pub fn tick(&mut self, now: DateTime<Local>) -> (Vec<Alert>, bool) {
        let today = now.date_naive();
        let mut alerts = Vec::new();
        let mut finish_words = Vec::new();
        let mut changed = false;

        let mut i = 0;
        while i < self.running.len() {
            let r = &mut self.running[i];
            if !r.is_paused() && r.millis_left(now) == 0 {
                let r = self.running.remove(i);
                self.log_session(&r, r.active_secs(r.ends_at));
                // Don't ring for a session that ended long ago while the app was closed.
                if (now - r.ends_at).num_seconds() < 120 {
                    if !r.finish_alerted {
                        alerts.push(finish_alert(&r));
                    }
                    finish_words.push(finish_speech(&r));
                }
                changed = true;
                continue;
            }
            if !r.finish_alerted && !r.is_paused() && r.total_secs > FINISH_LEAD_SECS && r.millis_left(now) <= FINISH_LEAD_SECS * 1000 {
                r.finish_alerted = true;
                alerts.push(finish_alert(r));
                changed = true;
            }
            if self.settings.warn_before_end && !r.warned && !r.is_paused() && r.total_secs >= 15 * 60 && r.secs_left(now) <= 5 * 60 {
                r.warned = true;
                alerts.push(Alert {
                    title: format!("5 minutes left: {}", r.name),
                    body: "Start wrapping up.".into(),
                    speech: format!("5 minutes left for {}.", r.name),
                    sound: Some(Sound::Warn),
                    ..Default::default()
                });
                changed = true;
            }
            i += 1;
        }

        let mut started_any = false;
        for a in self.activities.iter_mut() {
            if !self.settings.focus_mode {
                break;
            }
            if !a.runs_on(today) || a.last_started == Some(today) {
                continue;
            }
            let Some(start) = a.start_on(today) else { continue };
            let end = start + Duration::minutes(a.duration_min as i64);
            if now >= start && now < end - Duration::seconds(30) {
                a.last_started = Some(today);
                changed = true;
                if self.running.iter().any(|r| r.activity_id == Some(a.id)) {
                    continue;
                }
                let r = Running::new(Some(a.id), a.name.clone(), a.kind, a.duration_min as i64 * 60, end, now);
                alerts.push(start_alert(&r));
                self.running.push(r);
                started_any = true;
            }
        }
        // The spoken "time is up" would talk over the next session, so it is only said when nothing starts now.
        if !started_any {
            alerts.extend(finish_words);
        }

        (alerts, changed)
    }

    /// Minutes spent on courses on a given day.
    pub fn course_minutes_on(&self, date: NaiveDate) -> u32 {
        self.log.iter().filter(|e| e.kind == Kind::Course && e.date == date).map(|e| e.minutes).sum()
    }

    /// Consecutive days with course study, counting back from today (or yesterday if nothing yet today).
    pub fn streak(&self, today: NaiveDate) -> u32 {
        let mut day = if self.course_minutes_on(today) > 0 { today } else { today - Duration::days(1) };
        let mut n = 0;
        while self.course_minutes_on(day) > 0 {
            n += 1;
            day -= Duration::days(1);
        }
        n
    }
}

fn start_alert(r: &Running) -> Alert {
    let total = ((r.total_secs + 59) / 60) as u32;
    let (minutes, spoken) = (duration_label(total), spoken_duration(total));
    let name = &r.name;
    let (title, speech) = match r.kind {
        Kind::Course => (format!("Time to study {name}"), format!("Time to study {name}. Focus for {spoken}.")),
        Kind::Rest => (format!("{name} started"), format!("Time to rest. Relax for {spoken}.")),
        Kind::Gym => (format!("{name} time"), format!("{name} time. Let's go!")),
        Kind::Meal => (format!("{name} time"), format!("Time for {name}. Enjoy your meal.")),
        Kind::Other => (format!("{name} started"), format!("Time for {name}.")),
    };
    Alert { title, body: format!("Timer set for {minutes}."), speech, sound: Some(Sound::Start), urgent: true, lasts: None }
}

/// Popup and alarm only, played in the last seconds of a session.
fn finish_alert(r: &Running) -> Alert {
    let name = &r.name;
    let title = match r.kind {
        Kind::Course => format!("{name} session complete"),
        Kind::Rest => "Break is over".to_string(),
        Kind::Gym | Kind::Other => format!("{name} finished"),
        Kind::Meal => format!("{name} is over"),
    };
    Alert { title, body: "Timer finished.".into(), sound: Some(Sound::End), urgent: true, lasts: Some(FINISH_ALARM_SECS), ..Default::default() }
}

/// Voice only, said at the end time.
fn finish_speech(r: &Running) -> Alert {
    let name = &r.name;
    let speech = match r.kind {
        Kind::Course => format!("Time is up. Great work on {name}. Take a short break."),
        Kind::Rest => "Break is over. Let's get back to work.".to_string(),
        Kind::Gym => format!("{name} session finished. Well done."),
        Kind::Meal => format!("{name} time is over."),
        Kind::Other => format!("Time is up for {name}."),
    };
    Alert { speech, ..Default::default() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(h: u32, m: u32, s: u32) -> DateTime<Local> {
        // A Wednesday, so weekday filters can be checked.
        let d = NaiveDate::from_ymd_opt(2026, 10, 14).unwrap();
        Local.from_local_datetime(&d.and_hms_opt(h, m, s).unwrap()).earliest().unwrap()
    }

    fn focused() -> Data {
        let mut data = Data::default();
        data.settings.focus_mode = true;
        data
    }

    fn course(data: &mut Data, hour: u32, minutes: u32) -> u64 {
        data.add_activity(Activity {
            id: 0,
            name: "System Design".into(),
            kind: Kind::Course,
            hour,
            minute: 0,
            duration_min: minutes,
            days: [true; 7],
            enabled: true,
            done_on: None,
            last_started: None,
        });
        data.next_id
    }

    #[test]
    fn starts_at_scheduled_time_once_per_day() {
        let mut data = focused();
        course(&mut data, 8, 60);
        assert!(data.tick(at(7, 59, 59)).0.is_empty());
        let (alerts, _) = data.tick(at(8, 0, 0));
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].title, "Time to study System Design");
        assert_eq!(data.running.len(), 1);
        assert!(data.tick(at(8, 0, 1)).0.is_empty());
        assert_eq!(data.running.len(), 1);
    }

    #[test]
    fn nothing_starts_by_itself_outside_focus_mode() {
        let mut data = Data::default();
        let id = course(&mut data, 8, 120);
        assert!(data.tick(at(8, 0, 0)).0.is_empty());
        assert!(data.running.is_empty());
        // "Start now" still works with Focus Mode off.
        assert!(data.start_now(id, at(8, 5, 0)).is_some());
        assert_eq!(data.running.len(), 1);
    }

    #[test]
    fn focus_mode_runs_the_day_back_to_back() {
        let mut data = focused();
        data.settings.warn_before_end = false;
        course(&mut data, 8, 120);
        data.add_activity(Activity {
            id: 0,
            name: "Rest break".into(),
            kind: Kind::Rest,
            hour: 10,
            minute: 0,
            duration_min: 15,
            days: [true; 7],
            enabled: true,
            done_on: None,
            last_started: None,
        });
        assert_eq!(data.tick(at(8, 0, 0)).0.len(), 1);
        // 09:59:55: the course's finish popup and alarm, with no voice, and nothing starts yet.
        assert!(data.tick(at(9, 59, 54)).0.is_empty());
        let (alerts, _) = data.tick(at(9, 59, 55));
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].title, "System Design session complete");
        assert!(matches!(alerts[0].sound, Some(Sound::End)));
        assert_eq!(alerts[0].lasts, Some(FINISH_ALARM_SECS));
        assert!(alerts[0].speech.is_empty());
        for s in 56..60 {
            assert!(data.tick(at(9, 59, s)).0.is_empty());
        }
        // 10:00:00 exactly: only the break's start; the course's spoken goodbye would talk over it.
        let (alerts, _) = data.tick(at(10, 0, 0));
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].title, "Rest break started");
        assert_eq!(alerts[0].speech, "Time to rest. Relax for 15 minutes.");
        assert_eq!(data.running.len(), 1);
        assert_eq!(data.running[0].name, "Rest break");
        // Nothing follows the break, so its end time also gets the voice.
        assert!(data.tick(at(10, 14, 55)).0[0].title.starts_with("Break is over"));
        let (alerts, _) = data.tick(at(10, 15, 0));
        assert_eq!(alerts.len(), 1);
        assert!(alerts[0].title.is_empty() && alerts[0].sound.is_none());
        assert_eq!(alerts[0].speech, "Break is over. Let's get back to work.");
        assert!(data.running.is_empty());
        // Ending on time does not mark the course done; that stays manual.
        assert!(!data.activities[0].is_done());
    }

    #[test]
    fn turning_focus_on_mid_slot_starts_the_remaining_time() {
        let mut data = Data::default();
        course(&mut data, 8, 120);
        assert!(data.tick(at(9, 0, 0)).0.is_empty());
        data.settings.focus_mode = true;
        assert_eq!(data.tick(at(9, 0, 1)).0.len(), 1);
        assert_eq!(data.running[0].secs_left(at(9, 0, 1)), 60 * 60 - 1);
    }

    #[test]
    fn rings_at_end_and_logs_session() {
        let mut data = focused();
        data.settings.warn_before_end = false;
        course(&mut data, 8, 60);
        data.tick(at(8, 0, 0));
        assert_eq!(data.tick(at(8, 59, 55)).0.len(), 1);
        let (alerts, _) = data.tick(at(9, 0, 0));
        assert_eq!(alerts.len(), 1);
        assert!(alerts[0].speech.starts_with("Time is up"));
        assert!(data.running.is_empty());
        assert_eq!(data.log.len(), 1);
        assert_eq!(data.log[0].minutes, 60);
    }

    #[test]
    fn adding_time_moves_the_finish_alarm() {
        let mut data = focused();
        data.settings.warn_before_end = false;
        course(&mut data, 8, 60);
        data.tick(at(8, 0, 0));
        assert_eq!(data.tick(at(8, 59, 55)).0.len(), 1);
        data.running[0].add_minutes(5);
        assert!(data.tick(at(9, 0, 0)).0.is_empty());
        assert_eq!(data.tick(at(9, 4, 55)).0.len(), 1);
    }

    #[test]
    fn warns_five_minutes_before_end() {
        let mut data = focused();
        course(&mut data, 8, 60);
        data.tick(at(8, 0, 0));
        let (alerts, _) = data.tick(at(8, 55, 0));
        assert_eq!(alerts.len(), 1);
        assert!(alerts[0].title.starts_with("5 minutes left"));
        assert!(data.tick(at(8, 56, 0)).0.is_empty());
    }

    #[test]
    fn starting_late_runs_only_the_remaining_time() {
        let mut data = focused();
        course(&mut data, 8, 60);
        data.tick(at(8, 40, 0));
        assert_eq!(data.running[0].secs_left(at(8, 40, 0)), 20 * 60);
    }

    #[test]
    fn done_course_stops_repeating() {
        let mut data = focused();
        let id = course(&mut data, 8, 60);
        data.tick(at(8, 0, 0));
        data.mark_done(id, at(8, 30, 0));
        assert!(data.running.is_empty());
        assert_eq!(data.log[0].minutes, 30);
        let tomorrow = at(8, 0, 0) + Duration::days(1);
        assert!(data.tick(tomorrow).0.is_empty());
        data.restore(id);
        assert_eq!(data.tick(tomorrow).0.len(), 1);
    }

    #[test]
    fn skips_days_not_chosen_and_paused_items() {
        let mut data = focused();
        let id = course(&mut data, 8, 60);
        data.activity_mut(id).unwrap().days = [false, false, false, false, false, true, true];
        assert!(data.tick(at(8, 0, 0)).0.is_empty());
        let mut data = focused();
        let id = course(&mut data, 8, 60);
        data.activity_mut(id).unwrap().enabled = false;
        assert!(data.tick(at(8, 0, 0)).0.is_empty());
    }

    #[test]
    fn pause_freezes_the_countdown() {
        let mut data = focused();
        data.settings.warn_before_end = false;
        course(&mut data, 8, 30);
        data.tick(at(8, 0, 0));
        data.running[0].pause(at(8, 10, 0));
        assert!(data.tick(at(9, 0, 0)).0.is_empty());
        assert_eq!(data.running[0].secs_left(at(9, 0, 0)), 20 * 60);
        data.running[0].resume(at(9, 0, 0));
        assert!(data.tick(at(9, 19, 54)).0.is_empty());
        assert_eq!(data.tick(at(9, 19, 55)).0.len(), 1);
        assert_eq!(data.tick(at(9, 20, 0)).0.len(), 1);
        assert_eq!(data.log[0].minutes, 30);
    }

    #[test]
    fn no_alarm_for_sessions_that_ended_while_closed() {
        let mut data = focused();
        course(&mut data, 8, 60);
        data.tick(at(8, 0, 0));
        assert!(data.tick(at(12, 0, 0)).0.is_empty());
        assert!(data.running.is_empty());
    }

    #[test]
    fn durations_are_spoken_in_words() {
        assert_eq!(spoken_duration(1), "1 minute");
        assert_eq!(spoken_duration(45), "45 minutes");
        assert_eq!(spoken_duration(60), "1 hour");
        assert_eq!(spoken_duration(90), "1 hour and 30 minutes");
    }

    #[test]
    fn saved_data_round_trips() {
        let mut data = focused();
        course(&mut data, 8, 60);
        data.tick(at(8, 0, 0));
        let json = serde_json::to_string(&data).unwrap();
        let back: Data = serde_json::from_str(&json).unwrap();
        assert_eq!(back.activities.len(), 1);
        assert_eq!(back.running.len(), 1);
    }
}
