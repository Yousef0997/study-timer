//! The window: Today (dial + day timeline), Schedule, Progress and Settings.

use chrono::{DateTime, Datelike, Duration as ChronoDuration, Local, NaiveDate};
use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, CursorIcon, Frame, Layout, Margin, Pos2, Rect, RichText, Sense,
    Shape, Stroke, Ui, Vec2, ViewportCommand,
};
use std::f32::consts::{PI, TAU};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::alerts::{self, Alert, Sound};
use crate::autostart;
use crate::model::{self, days_label, duration_label, Activity, Data, Kind, Running, DAY_NAMES};
use crate::theme::{self, clock_font, display_font, kind_color, Palette};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Today,
    Schedule,
    Progress,
    Settings,
}

enum Confirm {
    Done(u64, String),
    Delete(u64, String),
}

struct Form {
    editing: Option<u64>,
    name: String,
    kind: Kind,
    hour: u32,
    minute: u32,
    duration: u32,
    days: [bool; 7],
    error: Option<String>,
}

impl Default for Form {
    fn default() -> Self {
        Form { editing: None, name: String::new(), kind: Kind::Course, hour: 8, minute: 0, duration: 60, days: [true; 7], error: None }
    }
}

pub struct StudyApp {
    data: Arc<Mutex<Data>>,
    tab: Tab,
    form: Form,
    autostart: bool,
    confirm: Option<Confirm>,
    toast: Option<(String, Instant)>,
    applied_dark: Option<bool>,
    start_hidden: bool,
    frames: u64,
    quitting: bool,
    focus: usize,
    dirty: bool,
    pending: Vec<Alert>,
}

impl StudyApp {
    pub fn new(data: Arc<Mutex<Data>>, start_hidden: bool) -> Self {
        let has_activities = !lock(&data).activities.is_empty();
        StudyApp {
            data,
            tab: if has_activities { Tab::Today } else { Tab::Schedule },
            form: Form::default(),
            autostart: autostart::is_enabled(),
            confirm: None,
            toast: None,
            applied_dark: None,
            start_hidden,
            frames: 0,
            quitting: false,
            focus: 0,
            dirty: false,
            pending: Vec::new(),
        }
    }

    fn say(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), Instant::now()));
    }
}

pub fn lock(data: &Mutex<Data>) -> std::sync::MutexGuard<'_, Data> {
    data.lock().unwrap_or_else(|e| e.into_inner())
}

impl eframe::App for StudyApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let now = Local::now();
        let shared = self.data.clone();
        let mut data = lock(&shared);

        let dark = data.settings.dark;
        if self.applied_dark != Some(dark) {
            theme::apply(&ctx, dark);
            self.applied_dark = Some(dark);
        }
        let p = theme::palette(dark);

        self.frames += 1;
        #[cfg(feature = "snapshot")]
        {
            if self.frames == 1 {
                self.tab = match crate::snapshot::requested_tab().as_deref() {
                    Some("schedule") => Tab::Schedule,
                    Some("progress") => Tab::Progress,
                    Some("settings") => Tab::Settings,
                    _ => self.tab,
                };
            }
            crate::snapshot::step(&ctx, self.frames);
        }
        if self.start_hidden && self.frames == 3 {
            ctx.send_viewport_cmd(ViewportCommand::Minimized(true));
        }
        if ctx.input(|i| i.viewport().close_requested()) && data.settings.minimize_on_close && !self.quitting {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(ViewportCommand::Minimized(true));
            self.say("Study Timer is still running. Alarms will ring on schedule.");
        }

        egui::Panel::top("top")
            .frame(Frame::new().fill(p.raised).inner_margin(Margin::symmetric(24, 12)).stroke(Stroke::new(1.0, p.line)))
            .show(ui, |ui| self.top_bar(ui, &p, now));

        let toast = self.toast.as_ref().filter(|(_, at)| at.elapsed() < Duration::from_secs(5)).map(|(m, _)| m.clone());
        if let Some(msg) = toast {
            egui::Panel::bottom("toast")
                .frame(Frame::new().fill(p.raised).inner_margin(Margin::symmetric(24, 8)).stroke(Stroke::new(1.0, p.line)))
                .show(ui, |ui| {
                    ui.label(RichText::new(msg).color(p.dim));
                });
        }

        egui::CentralPanel::default_margins()
            .frame(Frame::new().fill(p.bg).inner_margin(Margin::symmetric(28, 20)))
            .show(ui, |ui| {
                paint_notebook(ui, &p);
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match self.tab {
                    Tab::Today => self.today(ui, &mut data, &p, now),
                    Tab::Schedule => self.schedule(ui, &mut data, &p, now),
                    Tab::Progress => self.progress(ui, &data, &p, now),
                    Tab::Settings => self.settings(ui, &mut data, &p, &ctx),
                });
            });

        self.confirm_dialog(&ctx, &mut data, &p, now);

        if self.dirty {
            self.dirty = false;
            if let Err(e) = data.save() {
                self.say(format!("Could not save your schedule: {e}"));
            }
        }
        let alerts = std::mem::take(&mut self.pending);
        alerts::fire(alerts, data.settings.sound, data.settings.voice);

        let busy = data.running.iter().any(|r| !r.is_paused());
        drop(data);
        ctx.request_repaint_after(Duration::from_millis(if busy { 200 } else { 1000 }));
    }
}

// ---------- shared widgets ----------

fn paint_notebook(ui: &Ui, p: &Palette) {
    if p.dark {
        return;
    }
    let rect = ui.clip_rect();
    let painter = ui.painter();
    let mut y = rect.top() + 34.0;
    while y < rect.bottom() {
        painter.line_segment([Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)], Stroke::new(1.0, p.rule));
        y += 34.0;
    }
    let x = rect.left() + 14.0;
    painter.line_segment([Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())], Stroke::new(1.0, p.margin));
}

fn section_title(ui: &mut Ui, text: &str, p: &Palette) {
    ui.label(RichText::new(text).font(display_font(20.0)).color(p.text));
    ui.add_space(2.0);
}

fn primary_button(ui: &mut Ui, text: &str, p: &Palette) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(text).color(p.on_accent).strong())
            .fill(p.accent)
            .stroke(Stroke::NONE)
            .corner_radius(CornerRadius::same(6)),
    )
    .on_hover_cursor(CursorIcon::PointingHand)
}

fn quiet_button(ui: &mut Ui, text: &str, color: Color32) -> egui::Response {
    ui.add(egui::Button::new(RichText::new(text).color(color)).frame(false)).on_hover_cursor(CursorIcon::PointingHand)
}

fn kind_dot(ui: &mut Ui, kind: Kind, p: &Palette) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.5, kind_color(kind, p.dark));
}

fn toggle_switch(ui: &mut Ui, on: &mut bool, p: &Palette) -> egui::Response {
    let size = Vec2::new(40.0, 22.0);
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let t = ui.ctx().animate_bool_responsive(response.id, *on);
    let track = if *on { p.accent } else { p.line };
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(11), track);
    let x = egui::lerp(rect.left() + 11.0..=rect.right() - 11.0, t);
    let knob = if *on { p.on_accent } else { p.text };
    painter.circle_filled(Pos2::new(x, rect.center().y), 8.0, knob);
    response.on_hover_cursor(CursorIcon::PointingHand)
}

fn clock_label(secs: i64) -> String {
    let secs = secs.max(0);
    let (h, m, s) = (secs / 3600, (secs / 60) % 60, secs % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m:02}:{s:02}") }
}

fn until_label(now: DateTime<Local>, at: DateTime<Local>) -> String {
    let mins = ((at - now).num_seconds() + 59) / 60;
    if mins <= 0 { "now".into() } else { format!("in {}", duration_label(mins as u32)) }
}

// ---------- top bar ----------

impl StudyApp {
    fn top_bar(&mut self, ui: &mut Ui, p: &Palette, now: DateTime<Local>) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Study Timer").font(display_font(22.0)).color(p.text));
            ui.add_space(10.0);
            ui.label(RichText::new(now.format("%A, %-d %B").to_string()).color(p.dim));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                for (tab, label) in [(Tab::Settings, "Settings"), (Tab::Progress, "Progress"), (Tab::Schedule, "Schedule"), (Tab::Today, "Today")] {
                    let selected = self.tab == tab;
                    let r = ui
                        .add(egui::Label::new(RichText::new(label).size(15.5).color(if selected { p.text } else { p.dim })).sense(Sense::click()))
                        .on_hover_cursor(CursorIcon::PointingHand);
                    if selected {
                        let y = r.rect.bottom() + 5.0;
                        ui.painter().line_segment([Pos2::new(r.rect.left(), y), Pos2::new(r.rect.right(), y)], Stroke::new(2.0, p.accent));
                    }
                    if r.clicked() {
                        self.tab = tab;
                    }
                    ui.add_space(14.0);
                }
            });
        });
    }
}

// ---------- Today ----------

impl StudyApp {
    fn today(&mut self, ui: &mut Ui, data: &mut Data, p: &Palette, now: DateTime<Local>) {
        let today = now.date_naive();
        let wide = ui.available_width() > 720.0;
        let left_w = if wide { 340.0 } else { ui.available_width() };
        if wide {
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(Vec2::new(left_w, 0.0), Layout::top_down(Align::Center), |ui| self.now_column(ui, data, p, now));
                ui.add_space(36.0);
                ui.vertical(|ui| self.plan_column(ui, data, p, now, today));
            });
        } else {
            ui.vertical_centered(|ui| self.now_column(ui, data, p, now));
            ui.add_space(24.0);
            self.plan_column(ui, data, p, now, today);
        }
    }

    fn now_column(&mut self, ui: &mut Ui, data: &mut Data, p: &Palette, now: DateTime<Local>) {
        if self.focus >= data.running.len() {
            self.focus = 0;
        }
        let next = next_up(data, now);
        dial(ui, p, now, data.running.get(self.focus), next.as_ref());
        ui.add_space(10.0);

        if let Some(r) = data.running.get(self.focus) {
            let paused = r.is_paused();
            let index = self.focus;
            ui.horizontal(|ui| {
                let row_w = 270.0;
                ui.add_space(((ui.available_width() - row_w) / 2.0).max(0.0));
                if primary_button(ui, if paused { "Resume" } else { "Pause" }, p).clicked() {
                    let r = &mut data.running[index];
                    if paused { r.resume(now) } else { r.pause(now) }
                    self.dirty = true;
                }
                if ui.button("Add 5 min").clicked() {
                    data.running[index].add_minutes(5);
                    self.dirty = true;
                }
                if ui.button("Stop").on_hover_text("Stop now and keep the time you spent").clicked() {
                    data.stop(index, now);
                    self.dirty = true;
                }
            });
        }

        if data.running.len() > 1 {
            ui.add_space(8.0);
            ui.label(RichText::new("Also running").color(p.dim).small());
            for (i, r) in data.running.iter().enumerate() {
                if i == self.focus {
                    continue;
                }
                ui.horizontal(|ui| {
                    kind_dot(ui, r.kind, p);
                    let text = format!("{}  {}", r.name, clock_label(r.secs_left(now)));
                    if quiet_button(ui, &text, p.text).on_hover_text("Show on the dial").clicked() {
                        self.focus = i;
                    }
                });
            }
        }

        ui.add_space(22.0);
        ui.label(RichText::new("Take a break now").color(p.dim));
        ui.horizontal(|ui| {
            ui.add_space(((ui.available_width() - 230.0) / 2.0).max(0.0));
            for minutes in [5, 10, 15] {
                if ui.button(format!("{minutes} min")).clicked() {
                    let alert = data.quick_break(minutes, now);
                    self.pending.push(alert);
                    self.focus = data.running.len() - 1;
                    self.dirty = true;
                }
            }
        });
    }

    fn plan_column(&mut self, ui: &mut Ui, data: &mut Data, p: &Palette, now: DateTime<Local>, today: NaiveDate) {
        section_title(ui, "Today's plan", p);
        let mut items: Vec<Activity> = data.activities.iter().filter(|a| a.runs_on(today)).cloned().collect();
        items.sort_by_key(|a| a.minutes_of_day());
        if items.is_empty() {
            ui.add_space(6.0);
            ui.label(RichText::new("Nothing is scheduled for today.").color(p.dim));
            ui.label(RichText::new("Add your courses, rest breaks, gym and meals, and they will start here on time.").color(p.dim));
            ui.add_space(8.0);
            if primary_button(ui, "Add to schedule", p).clicked() {
                self.tab = Tab::Schedule;
            }
            return;
        }
        ui.add_space(6.0);
        let count = items.len();
        for (i, a) in items.iter().enumerate() {
            let start = a.start_on(today).unwrap_or(now);
            let running = data.is_running(a.id);
            let started = a.last_started == Some(today);
            let status = if running {
                Status::Now
            } else if started {
                Status::Done
            } else if start <= now {
                Status::Missed
            } else {
                Status::Upcoming(until_label(now, start))
            };
            if self.timeline_row(ui, a, &status, p, i == 0, i + 1 == count) {
                if let Some(alert) = data.start_now(a.id, now) {
                    self.pending.push(alert);
                    self.focus = data.running.len() - 1;
                    self.dirty = true;
                }
            }
        }
    }

    /// One stop on the day's rail. Returns true when "Start now" was clicked.
    fn timeline_row(&mut self, ui: &mut Ui, a: &Activity, status: &Status, p: &Palette, first: bool, last: bool) -> bool {
        let mut start_clicked = false;
        let color = kind_color(a.kind, p.dark);
        let row_h = 58.0;
        ui.allocate_ui_with_layout(Vec2::new(ui.available_width(), row_h), Layout::left_to_right(Align::Center), |ui| {
            ui.set_min_height(row_h);
            ui.add_sized([52.0, row_h], egui::Label::new(RichText::new(a.time_label()).font(clock_font(15.0)).color(p.dim)));

            let (rail, _) = ui.allocate_exact_size(Vec2::new(22.0, row_h), Sense::hover());
            let painter = ui.painter();
            let c = rail.center();
            let top = if first { c.y } else { rail.top() - 4.0 };
            let bottom = if last { c.y } else { rail.bottom() + 4.0 };
            painter.line_segment([Pos2::new(c.x, top), Pos2::new(c.x, bottom)], Stroke::new(2.0, p.line));
            match status {
                Status::Upcoming(_) | Status::Missed => {
                    painter.circle_filled(c, 7.0, p.bg);
                    painter.circle_stroke(c, 6.0, Stroke::new(2.0, color));
                }
                Status::Now => {
                    painter.circle_filled(c, 11.0, color.gamma_multiply(0.25));
                    painter.circle_filled(c, 7.0, color);
                }
                Status::Done => {
                    painter.circle_filled(c, 6.5, color);
                }
            }

            ui.add_space(6.0);
            ui.vertical(|ui| {
                ui.add_space(9.0);
                let name_color = if matches!(status, Status::Done | Status::Missed) { p.dim } else { p.text };
                ui.label(RichText::new(&a.name).size(16.5).color(name_color));
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    ui.label(RichText::new(a.kind.label()).small().color(color));
                    ui.label(RichText::new(format!("for {}", duration_label(a.duration_min))).small().color(p.dim));
                });
            });

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| match status {
                Status::Now => {
                    ui.label(RichText::new("In progress").color(color));
                }
                Status::Done => {
                    ui.label(RichText::new("✔ Done").color(p.dim));
                }
                Status::Missed => {
                    start_clicked = ui.button("Start now").clicked();
                    ui.label(RichText::new("Missed").color(p.faint));
                }
                Status::Upcoming(when) => {
                    start_clicked = ui.button("Start now").clicked();
                    ui.label(RichText::new(when).color(p.dim));
                }
            });
        });
        start_clicked
    }
}

enum Status {
    Upcoming(String),
    Now,
    Done,
    Missed,
}

/// The next scheduled start, looking ahead up to a week.
fn next_up(data: &Data, now: DateTime<Local>) -> Option<(String, Kind, DateTime<Local>)> {
    (0..8)
        .flat_map(|d| {
            let date = now.date_naive() + ChronoDuration::days(d);
            data.activities
                .iter()
                .filter(move |a| a.runs_on(date) && !(d == 0 && a.last_started == Some(date)))
                .filter_map(move |a| a.start_on(date).map(|t| (a.name.clone(), a.kind, t)))
        })
        .filter(|(_, _, t)| *t > now)
        .min_by_key(|(_, _, t)| *t)
}

fn dial(ui: &mut Ui, p: &Palette, now: DateTime<Local>, running: Option<&Running>, next: Option<&(String, Kind, DateTime<Local>)>) {
    let size = ui.available_width().min(300.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let painter = ui.painter();
    let c = rect.center();
    let ring = size / 2.0 - 14.0;
    let width = 12.0;

    painter.circle_stroke(c, ring, Stroke::new(width, p.line));
    // Kitchen-timer ticks: one per five minutes of the hour.
    for i in 0..12 {
        let a = i as f32 / 12.0 * TAU - PI / 2.0;
        let dir = Vec2::new(a.cos(), a.sin());
        let (r1, r2) = (ring - width - 4.0, ring - width - if i % 3 == 0 { 14.0 } else { 9.0 });
        painter.line_segment([c + dir * r1, c + dir * r2], Stroke::new(if i % 3 == 0 { 2.0 } else { 1.0 }, p.faint));
    }

    match running {
        Some(r) => {
            let color = kind_color(r.kind, p.dark);
            let left = 1.0 - r.fraction_done(now);
            if left > 0.0 {
                let steps = ((180.0 * left).ceil() as usize).max(2);
                let points: Vec<Pos2> = (0..=steps)
                    .map(|i| {
                        let a = -PI / 2.0 + TAU * left * i as f32 / steps as f32;
                        c + Vec2::new(a.cos(), a.sin()) * ring
                    })
                    .collect();
                painter.circle_filled(points[0], width / 2.0, color);
                painter.circle_filled(*points.last().unwrap(), width / 2.0, color);
                painter.add(Shape::line(points, Stroke::new(width, color)));
            }
            let secs = r.secs_left(now);
            let big = if secs >= 3600 { 46.0 } else { 56.0 };
            let time_color = if r.is_paused() { p.dim } else { p.text };
            painter.text(c + Vec2::new(0.0, -2.0), Align2::CENTER_CENTER, clock_label(secs), clock_font(big), time_color);
            painter.text(c + Vec2::new(0.0, -50.0), Align2::CENTER_CENTER, truncate(&r.name, 22), display_font(18.0), p.text);
            let sub = if r.is_paused() {
                "Paused".to_string()
            } else {
                format!("Ends at {}", r.ends_at.format("%H:%M"))
            };
            painter.text(c + Vec2::new(0.0, 42.0), Align2::CENTER_CENTER, sub, egui::FontId::proportional(14.0), p.dim);
            painter.text(c + Vec2::new(0.0, 62.0), Align2::CENTER_CENTER, r.kind.label(), egui::FontId::proportional(13.0), color);
        }
        None => {
            painter.text(c + Vec2::new(0.0, -6.0), Align2::CENTER_CENTER, now.format("%H:%M").to_string(), clock_font(56.0), p.dim);
            let (line1, line2, color) = match next {
                Some((name, kind, at)) => {
                    let when = if at.date_naive() == now.date_naive() {
                        at.format("%H:%M").to_string()
                    } else {
                        at.format("%a %H:%M").to_string()
                    };
                    (format!("Next: {}", truncate(name, 20)), format!("{when}, {}", until_label(now, *at)), kind_color(*kind, p.dark))
                }
                None => ("Nothing scheduled".to_string(), "Add something in Schedule".to_string(), p.faint),
            };
            painter.text(c + Vec2::new(0.0, 40.0), Align2::CENTER_CENTER, line1, egui::FontId::proportional(15.0), p.text);
            painter.text(c + Vec2::new(0.0, 60.0), Align2::CENTER_CENTER, line2, egui::FontId::proportional(13.0), color);
        }
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max - 1).collect::<String>())
    }
}

// ---------- Schedule ----------

impl StudyApp {
    fn schedule(&mut self, ui: &mut Ui, data: &mut Data, p: &Palette, now: DateTime<Local>) {
        let wide = ui.available_width() > 760.0;
        if wide {
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(Vec2::new(340.0, 0.0), Layout::top_down(Align::Min), |ui| self.form(ui, data, p));
                ui.add_space(36.0);
                ui.vertical(|ui| self.lists(ui, data, p, now));
            });
        } else {
            self.form(ui, data, p);
            ui.add_space(24.0);
            self.lists(ui, data, p, now);
        }
    }

    fn form(&mut self, ui: &mut Ui, data: &mut Data, p: &Palette) {
        let editing = self.form.editing;
        let (mut submit, mut cancel) = (false, false);
        section_title(ui, if editing.is_some() { "Edit activity" } else { "Add to your schedule" }, p);
        Frame::new()
            .fill(p.raised)
            .stroke(Stroke::new(1.0, p.line))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(Margin::same(16))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let f = &mut self.form;

                ui.label(RichText::new("Type").color(p.dim));
                ui.horizontal_wrapped(|ui| {
                    for kind in Kind::ALL {
                        let selected = f.kind == kind;
                        let text = RichText::new(format!("   {}", kind.label())).color(if selected { p.text } else { p.dim });
                        let r = ui.add(egui::Button::selectable(selected, text)).on_hover_cursor(CursorIcon::PointingHand);
                        let dot = Pos2::new(r.rect.left() + 15.0, r.rect.center().y);
                        ui.painter().circle_filled(dot, 4.0, kind_color(kind, p.dark));
                        if r.clicked() && !selected {
                            let old = f.kind;
                            f.kind = kind;
                            if f.name.trim().is_empty() || f.name == old.suggested_name() {
                                f.name = kind.suggested_name().into();
                            }
                            if f.duration == old.default_minutes() {
                                f.duration = kind.default_minutes();
                            }
                        }
                    }
                });

                ui.add_space(6.0);
                ui.label(RichText::new(if f.kind == Kind::Course { "Course name" } else { "Name" }).color(p.dim));
                let hint = match f.kind {
                    Kind::Course => "e.g. System Design",
                    Kind::Meal => "e.g. Lunch or Dinner",
                    Kind::Rest => "e.g. Afternoon rest",
                    Kind::Gym => "e.g. Gym",
                    Kind::Other => "e.g. Reading",
                };
                ui.add(egui::TextEdit::singleline(&mut f.name).hint_text(hint).desired_width(f32::INFINITY));

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Starts at").color(p.dim));
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;
                            ui.add(egui::DragValue::new(&mut f.hour).range(0..=23).custom_formatter(|v, _| format!("{:02}", v as u32)));
                            ui.label(":");
                            ui.add(egui::DragValue::new(&mut f.minute).range(0..=59).custom_formatter(|v, _| format!("{:02}", v as u32)));
                        });
                    });
                    ui.add_space(18.0);
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Length").color(p.dim));
                        ui.add(egui::DragValue::new(&mut f.duration).range(1..=600).suffix(" min"));
                    });
                });
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    ui.spacing_mut().button_padding = egui::vec2(8.0, 5.0);
                    for m in [15, 30, 45, 60, 90] {
                        if ui.add(egui::Button::selectable(f.duration == m, duration_label(m))).clicked() {
                            f.duration = m;
                        }
                    }
                });

                ui.add_space(6.0);
                ui.label(RichText::new("Repeat on").color(p.dim));
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    ui.spacing_mut().button_padding = egui::vec2(6.0, 5.0);
                    for (i, day) in DAY_NAMES.iter().enumerate() {
                        let on = f.days[i];
                        if ui.add(egui::Button::selectable(on, *day)).clicked() {
                            f.days[i] = !on;
                        }
                    }
                });
                ui.horizontal(|ui| {
                    if quiet_button(ui, "Every day", p.dim).clicked() {
                        f.days = [true; 7];
                    }
                    if quiet_button(ui, "Weekdays", p.dim).clicked() {
                        f.days = [true, true, true, true, true, false, false];
                    }
                    if quiet_button(ui, "Weekends", p.dim).clicked() {
                        f.days = [false, false, false, false, false, true, true];
                    }
                });

                ui.add_space(4.0);
                let note = if f.kind == Kind::Course {
                    "A course starts on its own at this time on each chosen day, until you mark it done."
                } else {
                    "This repeats on each chosen day. Switch it off in the list to pause it."
                };
                ui.label(RichText::new(note).small().color(p.dim));
                if let Some(err) = &f.error {
                    ui.label(RichText::new(err).color(p.danger));
                }

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let label = if editing.is_some() { "Save changes" } else { "Add to schedule" };
                    submit = primary_button(ui, label, p).clicked();
                    cancel = editing.is_some() && ui.button("Cancel").clicked();
                });
            });
        if submit {
            self.submit_form(data);
        }
        if cancel {
            self.form = Form::default();
        }
    }

    fn submit_form(&mut self, data: &mut Data) {
        let f = &mut self.form;
        let name = f.name.trim().to_string();
        if name.is_empty() {
            f.error = Some("Give it a name, for example \"System Design\".".into());
            return;
        }
        if !f.days.iter().any(|d| *d) {
            f.error = Some("Pick at least one day to repeat on.".into());
            return;
        }
        let msg = match f.editing {
            Some(id) => {
                if let Some(a) = data.activity_mut(id) {
                    let time_changed = a.hour != f.hour || a.minute != f.minute;
                    a.name = name.clone();
                    a.kind = f.kind;
                    a.hour = f.hour;
                    a.minute = f.minute;
                    a.duration_min = f.duration;
                    a.days = f.days;
                    // A new start time later today should still fire today.
                    if time_changed && !data.is_running(id) {
                        if let Some(a) = data.activity_mut(id) {
                            a.last_started = None;
                        }
                    }
                }
                format!("Saved {name}.")
            }
            None => {
                data.add_activity(Activity {
                    id: 0,
                    name: name.clone(),
                    kind: f.kind,
                    hour: f.hour,
                    minute: f.minute,
                    duration_min: f.duration,
                    days: f.days,
                    enabled: true,
                    done_on: None,
                    last_started: None,
                });
                format!("Added {name} at {:02}:{:02}, {}.", f.hour, f.minute, days_label(&f.days).to_lowercase())
            }
        };
        let kind = f.kind;
        self.form = Form { kind, name: kind.suggested_name().into(), duration: kind.default_minutes(), ..Form::default() };
        self.dirty = true;
        self.say(msg);
    }

    fn lists(&mut self, ui: &mut Ui, data: &mut Data, p: &Palette, now: DateTime<Local>) {
        // Leave room for the scrollbar so the row buttons are never clipped.
        ui.set_max_width(ui.available_width() - 18.0);
        let mut activities = data.activities.clone();
        activities.sort_by_key(|a| a.minutes_of_day());
        let courses: Vec<&Activity> = activities.iter().filter(|a| a.kind == Kind::Course && !a.is_done()).collect();
        let routine: Vec<&Activity> = activities.iter().filter(|a| a.kind != Kind::Course).collect();
        let finished: Vec<&Activity> = activities.iter().filter(|a| a.is_done()).collect();

        section_title(ui, "Courses", p);
        if courses.is_empty() {
            ui.label(RichText::new("No courses yet. Add one to start a daily study habit.").color(p.dim));
        }
        for a in &courses {
            self.activity_row(ui, data, a, p, now);
        }

        ui.add_space(18.0);
        section_title(ui, "Daily routine", p);
        if routine.is_empty() {
            ui.label(RichText::new("Add rest breaks, gym, lunch and dinner to plan the whole day.").color(p.dim));
        }
        for a in &routine {
            self.activity_row(ui, data, a, p, now);
        }

        if !finished.is_empty() {
            ui.add_space(18.0);
            section_title(ui, "Completed courses", p);
            for a in &finished {
                self.activity_row(ui, data, a, p, now);
            }
        }
    }

    fn activity_row(&mut self, ui: &mut Ui, data: &mut Data, a: &Activity, p: &Palette, now: DateTime<Local>) {
        let color = kind_color(a.kind, p.dark);
        let faded = !a.enabled || a.is_done();
        let response = ui.horizontal(|ui| {
            let (bar, _) = ui.allocate_exact_size(Vec2::new(4.0, 44.0), Sense::hover());
            ui.painter().rect_filled(bar, CornerRadius::same(2), if faded { color.gamma_multiply(0.4) } else { color });
            ui.add_space(4.0);
            ui.vertical(|ui| {
                ui.add_space(2.0);
                ui.label(RichText::new(&a.name).size(16.0).color(if faded { p.dim } else { p.text }));
                let meta = match a.done_on {
                    Some(d) => format!("Completed on {}", d.format("%-d %b %Y")),
                    None if !a.enabled => format!("Paused. Normally {} for {}, {}", a.time_label(), duration_label(a.duration_min), a.days_label().to_lowercase()),
                    None => format!("{} for {}, {}", a.time_label(), duration_label(a.duration_min), a.days_label().to_lowercase()),
                };
                ui.label(RichText::new(meta).small().color(p.dim));
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if quiet_button(ui, "Delete", p.danger).clicked() {
                    self.confirm = Some(Confirm::Delete(a.id, a.name.clone()));
                }
                if a.is_done() {
                    if ui.button("Restore").on_hover_text("Put this course back on the daily schedule").clicked() {
                        data.restore(a.id);
                        self.dirty = true;
                    }
                    return;
                }
                if quiet_button(ui, "Edit", p.text).clicked() {
                    self.form = Form {
                        editing: Some(a.id),
                        name: a.name.clone(),
                        kind: a.kind,
                        hour: a.hour,
                        minute: a.minute,
                        duration: a.duration_min,
                        days: a.days,
                        error: None,
                    };
                }
                if a.kind == Kind::Course && ui.button("✔ Mark done").on_hover_text("Finished this course? It will stop repeating.").clicked() {
                    self.confirm = Some(Confirm::Done(a.id, a.name.clone()));
                }
                let mut on = a.enabled;
                if toggle_switch(ui, &mut on, p).on_hover_text(if on { "On: starts on schedule" } else { "Paused: will not start" }).changed() {
                    if let Some(x) = data.activity_mut(a.id) {
                        x.enabled = on;
                    }
                    self.dirty = true;
                }
                if !data.is_running(a.id) && a.enabled && quiet_button(ui, "Start now", p.text).clicked() {
                    if let Some(alert) = data.start_now(a.id, now) {
                        self.pending.push(alert);
                        self.focus = data.running.len() - 1;
                        self.tab = Tab::Today;
                        self.dirty = true;
                    }
                }
            });
        });
        let r = response.response.rect;
        ui.painter().line_segment(
            [Pos2::new(r.left(), r.bottom() + 4.0), Pos2::new(r.right(), r.bottom() + 4.0)],
            Stroke::new(1.0, p.line.gamma_multiply(0.7)),
        );
        ui.add_space(8.0);
    }

    fn confirm_dialog(&mut self, ctx: &egui::Context, data: &mut Data, p: &Palette, now: DateTime<Local>) {
        let Some(confirm) = self.confirm.take() else { return };
        let (title, body, action) = match &confirm {
            Confirm::Done(_, name) => (
                format!("Finished {name}?"),
                "It will stop starting every day and move to Completed courses. Your study time stays in Progress.".to_string(),
                "Mark done",
            ),
            Confirm::Delete(_, name) => (
                format!("Delete {name}?"),
                "It will be removed from your schedule. Time already logged stays in Progress.".to_string(),
                "Delete",
            ),
        };
        let mut decision = None;
        let modal = egui::Modal::new(egui::Id::new("confirm")).show(ctx, |ui| {
            ui.set_width(360.0);
            ui.label(RichText::new(title).font(display_font(19.0)).color(p.text));
            ui.add_space(4.0);
            ui.label(RichText::new(body).color(p.dim));
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if primary_button(ui, action, p).clicked() {
                    decision = Some(true);
                }
                if ui.button("Cancel").clicked() {
                    decision = Some(false);
                }
            });
        });
        if modal.should_close() && decision.is_none() {
            decision = Some(false);
        }
        match decision {
            None => self.confirm = Some(confirm),
            Some(false) => {}
            Some(true) => {
                match confirm {
                    Confirm::Done(id, name) => {
                        if let Some(alert) = data.mark_done(id, now) {
                            self.pending.push(alert);
                        }
                        self.say(format!("{name} is complete. Well done!"));
                    }
                    Confirm::Delete(id, name) => {
                        data.delete(id);
                        if self.form.editing == Some(id) {
                            self.form = Form::default();
                        }
                        self.say(format!("Deleted {name}."));
                    }
                }
                self.dirty = true;
            }
        }
    }
}

// ---------- Progress ----------

impl StudyApp {
    fn progress(&mut self, ui: &mut Ui, data: &Data, p: &Palette, now: DateTime<Local>) {
        let today = now.date_naive();
        let week_start = today - ChronoDuration::days(today.weekday().num_days_from_monday() as i64);
        let week: u32 = (0..7).map(|d| data.course_minutes_on(week_start + ChronoDuration::days(d))).sum();
        let streak = data.streak(today);

        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 48.0;
            stat(ui, p, &duration_label(data.course_minutes_on(today)), "studied today");
            stat(ui, p, &duration_label(week), "studied this week");
            stat(ui, p, &format!("{streak} {}", if streak == 1 { "day" } else { "days" }), "study streak");
        });

        ui.add_space(24.0);
        section_title(ui, "Study time, last 7 days", p);
        week_chart(ui, data, p, today);

        ui.add_space(24.0);
        section_title(ui, "Courses", p);
        course_table(ui, data, p);

        ui.add_space(24.0);
        section_title(ui, "Routine this week", p);
        let since = today - ChronoDuration::days(6);
        let count = |k: Kind| data.log.iter().filter(|e| e.kind == k && e.date >= since).count();
        let minutes = |k: Kind| data.log.iter().filter(|e| e.kind == k && e.date >= since).map(|e| e.minutes).sum::<u32>();
        for (kind, label) in [(Kind::Gym, "gym sessions"), (Kind::Rest, "rest breaks"), (Kind::Meal, "meals on time")] {
            ui.horizontal(|ui| {
                kind_dot(ui, kind, p);
                ui.label(RichText::new(format!("{} {label}", count(kind))).color(p.text));
                if minutes(kind) > 0 {
                    ui.label(RichText::new(format!("({})", duration_label(minutes(kind)))).color(p.dim));
                }
            });
        }
    }
}

fn stat(ui: &mut Ui, p: &Palette, value: &str, label: &str) {
    ui.vertical(|ui| {
        ui.label(RichText::new(value).font(display_font(30.0)).color(p.text));
        ui.label(RichText::new(label).color(p.dim));
    });
}

fn week_chart(ui: &mut Ui, data: &Data, p: &Palette, today: NaiveDate) {
    let days: Vec<(NaiveDate, u32)> = (0..7).rev().map(|d| today - ChronoDuration::days(d)).map(|d| (d, data.course_minutes_on(d))).collect();
    let max = days.iter().map(|(_, m)| *m).max().unwrap_or(0);
    // Gridlines on whole hours (or half hours for light weeks).
    let step = if max <= 120 { 30 } else if max <= 360 { 60 } else { 120 };
    let top = (((max.max(60) + step - 1) / step) * step) as f32;

    let width = ui.available_width().min(620.0);
    let height = 190.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let plot = Rect::from_min_max(rect.min + Vec2::new(78.0, 8.0), rect.max - Vec2::new(0.0, 26.0));
    let painter = ui.painter_at(rect);
    let y_of = |m: f32| plot.bottom() - m / top * plot.height();

    let mut g = 0;
    while g as f32 <= top {
        let y = y_of(g as f32);
        painter.line_segment([Pos2::new(plot.left(), y), Pos2::new(plot.right(), y)], Stroke::new(1.0, p.line.gamma_multiply(if g == 0 { 1.0 } else { 0.6 })));
        painter.text(Pos2::new(plot.left() - 8.0, y), Align2::RIGHT_CENTER, duration_label(g), egui::FontId::proportional(11.5), p.dim);
        g += step;
    }

    let color = kind_color(Kind::Course, p.dark);
    let slot = plot.width() / 7.0;
    let bar_w = (slot * 0.5).min(36.0);
    let pointer = ui.ctx().pointer_hover_pos();
    for (i, (date, minutes)) in days.iter().enumerate() {
        let cx = plot.left() + slot * (i as f32 + 0.5);
        let label = if *date == today { "Today".to_string() } else { date.format("%a").to_string() };
        painter.text(Pos2::new(cx, plot.bottom() + 14.0), Align2::CENTER_CENTER, label, egui::FontId::proportional(12.0), if *date == today { p.text } else { p.dim });
        let hit = Rect::from_min_max(Pos2::new(cx - slot / 2.0, plot.top()), Pos2::new(cx + slot / 2.0, plot.bottom()));
        let hovered = pointer.is_some_and(|pos| hit.contains(pos));
        if *minutes > 0 {
            let bar = Rect::from_min_max(Pos2::new(cx - bar_w / 2.0, y_of(*minutes as f32)), Pos2::new(cx + bar_w / 2.0, plot.bottom()));
            let fill = if hovered { color } else { color.gamma_multiply(0.85) };
            painter.rect_filled(bar, CornerRadius { nw: 4, ne: 4, sw: 0, se: 0 }, fill);
        }
        if *date == today && *minutes > 0 {
            painter.text(Pos2::new(cx, y_of(*minutes as f32) - 10.0), Align2::CENTER_CENTER, duration_label(*minutes), egui::FontId::proportional(12.0), p.text);
        }
        if hovered {
            egui::Tooltip::always_open(ui.ctx().clone(), ui.layer_id(), egui::Id::new(("bar", i)), egui::PopupAnchor::Pointer).show(|ui| {
                ui.label(RichText::new(date.format("%A, %-d %B").to_string()).strong());
                ui.label(if *minutes > 0 { format!("{} of study", duration_label(*minutes)) } else { "No study logged".to_string() });
            });
        }
    }
}

fn course_table(ui: &mut Ui, data: &Data, p: &Palette) {
    struct Row {
        name: String,
        minutes: u32,
        sessions: usize,
        last: Option<NaiveDate>,
        status: &'static str,
    }
    let mut rows: Vec<Row> = data
        .activities
        .iter()
        .filter(|a| a.kind == Kind::Course)
        .map(|a| {
            let entries: Vec<&model::LogEntry> = data.log.iter().filter(|e| e.activity_id == Some(a.id)).collect();
            Row {
                name: a.name.clone(),
                minutes: entries.iter().map(|e| e.minutes).sum(),
                sessions: entries.len(),
                last: entries.iter().map(|e| e.date).max(),
                status: if a.is_done() { "Completed" } else if a.enabled { "Active" } else { "Paused" },
            }
        })
        .collect();
    rows.sort_by(|a, b| b.minutes.cmp(&a.minutes));
    if rows.is_empty() {
        ui.label(RichText::new("Your courses and their total study time will show here.").color(p.dim));
        return;
    }
    egui::Grid::new("courses").num_columns(5).spacing([28.0, 10.0]).striped(false).show(ui, |ui| {
        for h in ["Course", "Total time", "Sessions", "Last studied", "Status"] {
            ui.label(RichText::new(h).color(p.dim).small());
        }
        ui.end_row();
        for r in rows {
            ui.label(RichText::new(r.name).color(p.text));
            ui.label(duration_label(r.minutes));
            ui.label(r.sessions.to_string());
            ui.label(r.last.map(|d| d.format("%-d %b").to_string()).unwrap_or_else(|| "Not yet".into()));
            ui.label(RichText::new(r.status).color(if r.status == "Completed" { kind_color(Kind::Course, p.dark) } else { p.dim }));
            ui.end_row();
        }
    });
}

// ---------- Settings ----------

impl StudyApp {
    fn settings(&mut self, ui: &mut Ui, data: &mut Data, p: &Palette, ctx: &egui::Context) {
        ui.set_max_width(620.0);
        section_title(ui, "Settings", p);
        ui.add_space(6.0);

        let mut autostart = self.autostart;
        if setting_row(ui, p, &mut autostart, "Start Study Timer when I log in", "Opens minimized at login so every scheduled course, break and meal starts on time.") {
            match autostart::set_enabled(autostart) {
                Ok(()) => {
                    self.autostart = autostart;
                    self.say(if autostart { "Study Timer will start when you log in." } else { "Study Timer will no longer start at login." });
                }
                Err(e) => self.say(format!("Could not change the login setting: {e}")),
            }
        }

        let s = &mut data.settings;
        let mut changed = false;
        changed |= setting_row(ui, p, &mut s.sound, "Play alarm sounds", "A bell when something starts and an alarm when time is up.");
        changed |= setting_row(ui, p, &mut s.voice, "Speak alerts out loud", "A voice says what is starting and when time is up.");
        changed |= setting_row(ui, p, &mut s.warn_before_end, "Warn me 5 minutes before the end", "For sessions of 15 minutes or longer.");
        changed |= setting_row(ui, p, &mut s.minimize_on_close, "Keep running when I close the window", "Closing the window minimizes it, so alarms still ring.");
        changed |= setting_row(ui, p, &mut s.dark, "Chalkboard theme", "Off uses the light notebook theme.");
        if changed {
            self.dirty = true;
        }

        ui.add_space(14.0);
        ui.horizontal(|ui| {
            if ui.button("Test alarm and voice").clicked() {
                self.pending.push(Alert {
                    title: "Study Timer test".into(),
                    body: "This is how your alarms will sound.".into(),
                    speech: "Time is up. This is how Study Timer will sound.".into(),
                    sound: Sound::End,
                    urgent: false,
                });
            }
            if ui.button("Quit Study Timer").on_hover_text("Scheduled alarms will not ring until you open it again").clicked() {
                self.quitting = true;
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
        });

        ui.add_space(18.0);
        ui.label(RichText::new(format!("Your schedule and history are saved in {}", model::data_file().display())).small().color(p.faint));
    }
}

/// A toggle with a title and a one-line explanation. Returns true when it was flipped.
fn setting_row(ui: &mut Ui, p: &Palette, value: &mut bool, title: &str, help: &str) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        changed = toggle_switch(ui, value, p).changed();
        ui.add_space(8.0);
        ui.vertical(|ui| {
            ui.label(RichText::new(title).color(p.text));
            ui.label(RichText::new(help).small().color(p.dim));
        });
    });
    ui.add_space(10.0);
    changed
}
