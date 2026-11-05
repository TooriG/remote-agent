//! Slow-interval reports in the desktop diagnostic log.
//!
//! GPUI journals each draw, the views whose own work in a draw phase reached
//! its view timing floor, platform inputs and foreground task polls. A report
//! sums one interval so that slow frames can be attributed afterwards to the
//! views, inputs and tasks that occupied the main thread. It names types and
//! source locations only, never content.
use gpui_kit::{
    App,
    journal::{ForegroundEvent, ForegroundJournalEntry},
};
use std::{
    collections::HashMap,
    fmt::Write as _,
    panic::Location,
    time::{Duration, Instant},
};

const INTERVAL: Duration = Duration::from_secs(5);
/// A task poll this long holds the main thread across a 60 Hz frame.
const SLOW_TASK: Duration = Duration::from_millis(16);
/// Views and tasks listed per report, by main-thread time.
const LISTED: usize = 6;

pub(crate) fn init(cx: &mut App) {
    let mut collector = cx.foreground_journal().collector();
    cx.spawn(async move |cx| {
        let mut since = Instant::now();
        loop {
            cx.background_executor().timer(INTERVAL).await;
            let drained = collector.collect_unseen();
            let now = Instant::now();
            if let Some(report) = Interval::of(&drained.entries, drained.lost).report(now - since) {
                tracing::info!(target: "agent", operation = "desktop.frames", message = %report);
            }
            since = now;
        }
    })
    .detach();
}

#[derive(Default, Clone, Copy)]
struct Tally {
    count: u32,
    time: Duration,
}
impl Tally {
    fn add(&mut self, time: Duration) {
        self.count += 1;
        self.time += time;
    }
}

#[derive(Default)]
struct Interval {
    frames: u32,
    over_budget: u32,
    draw: Duration,
    longest_draw: Duration,
    /// The longest wait from a frame's first invalidation to the end of its draw.
    longest_stall: Duration,
    request_layout: Duration,
    prepaint: Duration,
    paint: Duration,
    views: HashMap<(&'static str, &'static str), Tally>,
    inputs: u32,
    longest_input: Duration,
    tasks: HashMap<&'static Location<'static>, Tally>,
    longest_task: Duration,
    lost: u64,
}

impl Interval {
    fn of(entries: &[ForegroundJournalEntry], lost: u64) -> Self {
        let mut interval = Self {
            lost,
            ..Self::default()
        };
        for entry in entries {
            if let ForegroundJournalEntry::Event(event) = entry {
                interval.add(event);
            }
        }
        interval
    }

    fn add(&mut self, event: &ForegroundEvent) {
        match event {
            ForegroundEvent::Draw(frame) => {
                let draw = frame.draw_duration();
                self.frames += 1;
                self.over_budget += u32::from(draw > frame.budget());
                self.draw += draw;
                self.longest_draw = self.longest_draw.max(draw);
                if let Some(stall) = frame.dirty_to_draw_duration() {
                    self.longest_stall = self.longest_stall.max(stall);
                }
                self.request_layout += frame.phases.request_layout;
                self.prepaint += frame.phases.prepaint;
                self.paint += frame.phases.paint;
            }
            ForegroundEvent::View(view) => self
                .views
                .entry((view.name, view.phase.as_str()))
                .or_default()
                .add(view.self_duration),
            ForegroundEvent::Input(input) => {
                self.inputs += 1;
                self.longest_input = self.longest_input.max(input.end - input.start);
            }
            ForegroundEvent::TaskPoll(task) => {
                let poll = task.poll_duration();
                self.tasks.entry(task.location).or_default().add(poll);
                self.longest_task = self.longest_task.max(poll);
            }
            ForegroundEvent::Action(_)
            | ForegroundEvent::Present(_)
            | ForegroundEvent::SmallPolls(_) => {}
        }
    }

    /// Describes the interval when a frame exceeded its budget or a task
    /// held the main thread, and nothing otherwise.
    fn report(&self, elapsed: Duration) -> Option<String> {
        if self.over_budget == 0 && self.longest_task < SLOW_TASK {
            return None;
        }
        let ms = |time: Duration| time.as_millis();
        let mut report = format!(
            "interval_ms={} frames={} over_budget={} draw_ms={} longest_draw_ms={} longest_stall_ms={} request_layout_ms={} prepaint_ms={} paint_ms={} inputs={} longest_input_ms={} longest_task_ms={} lost={}",
            ms(elapsed),
            self.frames,
            self.over_budget,
            ms(self.draw),
            ms(self.longest_draw),
            ms(self.longest_stall),
            ms(self.request_layout),
            ms(self.prepaint),
            ms(self.paint),
            self.inputs,
            ms(self.longest_input),
            ms(self.longest_task),
            self.lost,
        );
        report.push_str(" views=");
        list(
            &mut report,
            self.views
                .iter()
                .map(|((name, phase), tally)| (format!("{}/{phase}", type_name(name)), *tally)),
        );
        report.push_str(" tasks=");
        list(
            &mut report,
            self.tasks
                .iter()
                .map(|(location, tally)| (source(location), *tally)),
        );
        Some(report)
    }
}

fn list(report: &mut String, tallies: impl Iterator<Item = (String, Tally)>) {
    let mut tallies: Vec<_> = tallies.collect();
    tallies.sort_by(|a, b| b.1.time.cmp(&a.1.time).then_with(|| a.0.cmp(&b.0)));
    for (index, (name, tally)) in tallies.into_iter().take(LISTED).enumerate() {
        let separator = if index == 0 { "" } else { "," };
        let _ = write!(
            report,
            "{separator}{name}:{}x/{}ms",
            tally.count,
            tally.time.as_millis()
        );
    }
}

/// `a::b::Sidebar<c::Section>` becomes `Sidebar<Section>`.
fn type_name(name: &str) -> String {
    let mut short = String::with_capacity(name.len());
    let mut segment_start = 0;
    for (index, character) in name.char_indices() {
        if character.is_alphanumeric() || character == '_' {
            continue;
        }
        if character == ':' {
            segment_start = index + 1;
            continue;
        }
        short.push_str(&name[segment_start..index]);
        short.push(character);
        segment_start = index + character.len_utf8();
    }
    short.push_str(&name[segment_start..]);
    short
}

/// `apps/desktop/src/app.rs:270` becomes `desktop/app.rs:270`.
fn source(location: &Location) -> String {
    let file = location.file();
    let file = match file.rfind("/src/") {
        Some(at) => {
            let package = file[..at].rsplit('/').next().unwrap_or_default();
            format!("{package}/{}", &file[at + "/src/".len()..])
        }
        None => file.to_owned(),
    };
    format!("{file}:{}", location.line())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{
        DrawPhases, FrameTiming, ViewPhase, ViewTiming, WindowId, journal::InputTiming,
    };

    const REFRESH: Duration = Duration::from_nanos(8_333_333);

    fn frame(start: Instant, draw: Duration, stall: Duration) -> ForegroundJournalEntry {
        ForegroundJournalEntry::Event(ForegroundEvent::Draw(FrameTiming {
            window_id: WindowId::from(1),
            dirty_at: Some(start + draw - stall),
            invalidations: 1,
            draw_start: start,
            draw_end: start + draw,
            refresh_interval: Some(REFRESH),
            signal_at: None,
            phases: DrawPhases {
                request_layout: draw / 4,
                prepaint: draw / 2,
                paint: draw / 4,
            },
        }))
    }

    fn view(name: &'static str, start: Instant, time: Duration) -> ForegroundJournalEntry {
        ForegroundJournalEntry::Event(ForegroundEvent::View(ViewTiming {
            name,
            phase: ViewPhase::Prepaint,
            start,
            end: start + time,
            self_duration: time,
        }))
    }

    #[test]
    fn slow_intervals_name_the_views_that_occupied_their_frames() {
        let start = Instant::now();
        let entries = [
            frame(start, Duration::from_millis(30), Duration::from_millis(45)),
            view(
                "agent_desktop::app::view::sidebar::SidebarView",
                start,
                Duration::from_millis(12),
            ),
            view(
                "agent_desktop::app::view::composer::ComposerContent",
                start,
                Duration::from_millis(3),
            ),
            frame(start, Duration::from_millis(2), Duration::from_millis(2)),
            view(
                "agent_desktop::app::view::sidebar::SidebarView",
                start,
                Duration::from_millis(9),
            ),
            ForegroundJournalEntry::Event(ForegroundEvent::Input(InputTiming {
                kind: "mouse_move",
                start,
                end: start + Duration::from_millis(1),
                caused_invalidation: true,
            })),
            ForegroundJournalEntry::Discontinuity { lost: 3 },
        ];
        let report = Interval::of(&entries, 3)
            .report(Duration::from_secs(5))
            .unwrap();
        assert_eq!(
            report,
            "interval_ms=5000 frames=2 over_budget=1 draw_ms=32 longest_draw_ms=30 \
             longest_stall_ms=45 request_layout_ms=8 prepaint_ms=16 paint_ms=8 inputs=1 \
             longest_input_ms=1 longest_task_ms=0 lost=3 \
             views=SidebarView/prepaint:2x/21ms,ComposerContent/prepaint:1x/3ms tasks="
        );
    }

    #[test]
    fn intervals_within_budget_are_not_reported() {
        let start = Instant::now();
        let entries = [
            frame(start, REFRESH, REFRESH),
            view("agent_desktop::activity::ActivityWindow", start, REFRESH),
        ];
        assert_eq!(Interval::of(&entries, 0).report(INTERVAL), None);
        assert_eq!(Interval::of(&[], 0).report(INTERVAL), None);
    }

    #[test]
    fn names_drop_module_paths_but_keep_generic_structure() {
        assert_eq!(
            type_name(
                "gpui_component::sidebar::Sidebar<agent_desktop::app::view::sidebar::SidebarEntry>"
            ),
            "Sidebar<SidebarEntry>"
        );
        assert_eq!(type_name("gpui::Entity<(a::B, c::D)>"), "Entity<(B, D)>");
        assert_eq!(type_name("Plain"), "Plain");
        let here = Location::caller();
        assert_eq!(
            source(here),
            format!("desktop/frame_diagnostics.rs:{}", here.line())
        );
    }
}
