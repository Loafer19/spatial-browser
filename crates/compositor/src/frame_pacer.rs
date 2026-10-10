// Decides when the window redraws and which pages get CEF begin frames, so
// an unchanged canvas costs (almost) nothing. Pure: no CEF, no wgpu — app.rs
// feeds it page snapshots and acts on the returned plan.
//
// Begin frames (external_begin_frame_enabled: CEF paints only when sent one):
// - on-screen page that is busy (loading / no texture yet), painted within
//   ACTIVE_HOLD, or saw input within ACTIVE_HOLD → target fps;
// - on-screen idle page → IDLE_BEGIN_FRAME_HZ, so JS timers / carets / GIFs
//   still get a frame and promote the page back to full rate on their paint;
// - off-screen page in the visible workspace → OFFSCREEN_BEGIN_FRAME_HZ;
// - parked workspace pages are not in the session (and are was_hidden).
// Redraw: input, a change in what would be drawn (signature: paints, rects,
// viewport, load bars, HUD…), or a time-based shader animation, capped at
// target fps.

use crate::output::Rect;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Begin-frame rate for visible pages that have not painted recently.
pub const IDLE_BEGIN_FRAME_HZ: u32 = 10;
/// Begin-frame rate for pages outside the window (keeps textures fresh-ish).
pub const OFFSCREEN_BEGIN_FRAME_HZ: u32 = 1;
/// How long a paint or input keeps a page at full rate.
pub const ACTIVE_HOLD: Duration = Duration::from_millis(500);
/// Wake-up timer slop: treat a deadline this close as reached.
const SLOP: Duration = Duration::from_millis(1);
/// Interval for the RUST_LOG=debug rate line.
const STATS_WINDOW: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Visibility {
    OnScreen,
    OffScreen,
}

/// Does a page's screen-space rect overlap the window at all?
/// An unknown (zero) canvas counts as on-screen — never starve a page.
pub fn classify(screen_rect: Rect, canvas: (f32, f32)) -> Visibility {
    if canvas.0 <= 0.0 || canvas.1 <= 0.0 {
        return Visibility::OnScreen;
    }
    let overlaps = screen_rect.w > 0.0
        && screen_rect.h > 0.0
        && screen_rect.x < canvas.0
        && screen_rect.y < canvas.1
        && screen_rect.x + screen_rect.w > 0.0
        && screen_rect.y + screen_rect.h > 0.0;
    if overlaps {
        Visibility::OnScreen
    } else {
        Visibility::OffScreen
    }
}

/// Per-tick snapshot of one session page.
#[derive(Clone, Copy, Debug)]
pub struct PageFrameInfo {
    pub id: i32,
    pub visibility: Visibility,
    /// Changes whenever CEF delivered a new paint for this page.
    pub paint_token: u64,
    /// Loading or still without a texture: always full rate when visible.
    pub busy: bool,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct TickPlan {
    /// Browser ids to call `send_external_begin_frame` on now.
    pub begin_frames: Vec<i32>,
    /// Call `window.request_redraw()` now.
    pub redraw: bool,
}

struct PageClock {
    paint_token: u64,
    visibility: Visibility,
    active_until: Instant,
    last_begin_frame: Option<Instant>,
}

#[derive(Default)]
struct Stats {
    window_start: Option<Instant>,
    renders: u32,
    begin_frames: u32,
    ticks: u32,
}

pub struct FramePacer {
    pages: HashMap<i32, PageClock>,
    last_input: Option<Instant>,
    last_signature: Option<u64>,
    dirty: bool,
    animating: bool,
    redraw_pending: bool,
    last_render: Option<Instant>,
    frame: Duration,
    next_begin_frame: Option<Instant>,
    stats: Stats,
}

impl Default for FramePacer {
    fn default() -> Self {
        Self::new()
    }
}

impl FramePacer {
    pub fn new() -> Self {
        Self {
            pages: HashMap::new(),
            last_input: None,
            last_signature: None,
            dirty: true,
            animating: false,
            redraw_pending: false,
            last_render: None,
            frame: frame_interval(60),
            next_begin_frame: None,
            stats: Stats::default(),
        }
    }

    /// Any window event: redraw and run visible pages at full rate.
    pub fn note_input(&mut self, now: Instant) {
        self.last_input = Some(now);
        self.dirty = true;
    }

    /// Plan this loop iteration. `signature` hashes everything the next
    /// frame would draw; `animating` = a shader animation is on screen.
    pub fn tick(
        &mut self,
        now: Instant,
        fps: u32,
        pages: &[PageFrameInfo],
        signature: u64,
        animating: bool,
    ) -> TickPlan {
        self.frame = frame_interval(fps);
        self.animating = animating;
        self.stats.ticks += 1;

        let mut plan = TickPlan::default();
        self.pages.retain(|id, _| pages.iter().any(|p| p.id == *id));
        let input_active = self
            .last_input
            .is_some_and(|t| now < t + ACTIVE_HOLD);

        let mut next_begin_frame: Option<Instant> = None;
        for page in pages {
            let clock = self.pages.entry(page.id).or_insert_with(|| PageClock {
                paint_token: page.paint_token,
                visibility: page.visibility,
                // New to the canvas (spawned, restored from a parked slot):
                // get it painting right away.
                active_until: now + ACTIVE_HOLD,
                last_begin_frame: None,
            });
            if clock.paint_token != page.paint_token {
                clock.paint_token = page.paint_token;
                clock.active_until = now + ACTIVE_HOLD;
            }
            if page.visibility == Visibility::OnScreen
                && clock.visibility == Visibility::OffScreen
            {
                // Panned into view: refresh the (≤1 s old) texture now.
                clock.active_until = now + ACTIVE_HOLD;
                clock.last_begin_frame = None;
            }
            clock.visibility = page.visibility;

            let active = page.busy || now < clock.active_until || input_active;
            let interval = begin_frame_interval(page.visibility, active, self.frame);
            let due = clock
                .last_begin_frame
                .is_none_or(|last| now + SLOP >= last + interval);
            if due {
                plan.begin_frames.push(page.id);
                // Woken a hair early → keep the cadence instead of drifting;
                // late → restart from now (no catch-up bursts).
                let on_time = clock.last_begin_frame.map_or(now, |last| last + interval);
                clock.last_begin_frame = Some(on_time.max(now));
            }
            let next = clock.last_begin_frame.map_or(now, |last| last + interval);
            next_begin_frame = Some(next_begin_frame.map_or(next, |n: Instant| n.min(next)));
        }
        self.next_begin_frame = next_begin_frame;

        if self.last_signature != Some(signature) {
            self.last_signature = Some(signature);
            self.dirty = true;
        }
        if (self.dirty || self.animating) && !self.redraw_pending && self.render_allowed(now) {
            plan.redraw = true;
            self.redraw_pending = true;
        }

        self.stats.begin_frames += plan.begin_frames.len() as u32;
        self.log_stats(now, pages.len());
        plan
    }

    /// A frame was presented (RedrawRequested, ours or the system's).
    pub fn rendered(&mut self, now: Instant) {
        self.dirty = false;
        self.redraw_pending = false;
        self.last_render = Some(now);
        self.stats.renders += 1;
    }

    /// The surface had no frame for us (occluded/timeout): retry next frame.
    pub fn render_skipped(&mut self, now: Instant) {
        self.redraw_pending = false;
        self.last_render = Some(now);
    }

    /// When the loop must wake for pacing work (None = sleep until an event).
    pub fn next_deadline(&self) -> Option<Instant> {
        let redraw = if (self.dirty || self.animating) && !self.redraw_pending {
            Some(self.last_render.map_or_else(Instant::now, |t| t + self.frame))
        } else {
            None
        };
        min_instant(redraw, self.next_begin_frame)
    }

    fn render_allowed(&self, now: Instant) -> bool {
        self.last_render
            .is_none_or(|last| now + SLOP >= last + self.frame)
    }

    fn log_stats(&mut self, now: Instant, pages: usize) {
        let start = *self.stats.window_start.get_or_insert(now);
        let elapsed = now.duration_since(start);
        if elapsed < STATS_WINDOW {
            return;
        }
        let secs = elapsed.as_secs_f32();
        log::debug!(
            "frame pacer: {:.1} renders/s, {:.1} begin frames/s, {:.1} loop ticks/s, {} pages",
            self.stats.renders as f32 / secs,
            self.stats.begin_frames as f32 / secs,
            self.stats.ticks as f32 / secs,
            pages,
        );
        self.stats = Stats {
            window_start: Some(now),
            ..Stats::default()
        };
    }
}

pub fn min_instant(a: Option<Instant>, b: Option<Instant>) -> Option<Instant> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

fn frame_interval(fps: u32) -> Duration {
    Duration::from_secs(1) / fps.clamp(1, 1000)
}

fn begin_frame_interval(visibility: Visibility, active: bool, frame: Duration) -> Duration {
    match visibility {
        Visibility::OffScreen => Duration::from_secs(1) / OFFSCREEN_BEGIN_FRAME_HZ,
        Visibility::OnScreen if active => frame,
        Visibility::OnScreen => Duration::from_secs(1) / IDLE_BEGIN_FRAME_HZ,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FPS: u32 = 60;

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }

    fn page(id: i32, visibility: Visibility, paint_token: u64) -> PageFrameInfo {
        PageFrameInfo {
            id,
            visibility,
            paint_token,
            busy: false,
        }
    }

    /// Settle a page into its idle state: render, let hold windows expire.
    fn settled(pages: &[PageFrameInfo]) -> (FramePacer, Instant) {
        let mut pacer = FramePacer::new();
        let t0 = Instant::now();
        pacer.tick(t0, FPS, pages, 1, false);
        pacer.rendered(t0);
        let t = t0 + ACTIVE_HOLD + Duration::from_millis(10);
        pacer.tick(t, FPS, pages, 1, false);
        (pacer, t)
    }

    #[test]
    fn classify_overlap() {
        let canvas = (800.0, 600.0);
        assert_eq!(classify(rect(10.0, 10.0, 100.0, 100.0), canvas), Visibility::OnScreen);
        // Partially visible counts as on screen.
        assert_eq!(classify(rect(-50.0, -50.0, 60.0, 60.0), canvas), Visibility::OnScreen);
        assert_eq!(classify(rect(790.0, 590.0, 100.0, 100.0), canvas), Visibility::OnScreen);
        // Fully outside on each side.
        assert_eq!(classify(rect(-200.0, 0.0, 100.0, 100.0), canvas), Visibility::OffScreen);
        assert_eq!(classify(rect(800.0, 0.0, 100.0, 100.0), canvas), Visibility::OffScreen);
        assert_eq!(classify(rect(0.0, 600.0, 100.0, 100.0), canvas), Visibility::OffScreen);
        assert_eq!(classify(rect(0.0, -100.0, 100.0, 100.0), canvas), Visibility::OffScreen);
        // Degenerate rect, unknown canvas.
        assert_eq!(classify(rect(10.0, 10.0, 0.0, 100.0), canvas), Visibility::OffScreen);
        assert_eq!(classify(rect(5000.0, 0.0, 10.0, 10.0), (0.0, 0.0)), Visibility::OnScreen);
    }

    #[test]
    fn idle_canvas_does_not_redraw() {
        let pages = [page(1, Visibility::OnScreen, 0)];
        let (mut pacer, t) = settled(&pages);
        pacer.rendered(t);
        for i in 1..=100 {
            let now = t + Duration::from_millis(i * 7);
            assert!(!pacer.tick(now, FPS, &pages, 1, false).redraw, "tick {i}");
        }
    }

    #[test]
    fn first_tick_redraws_and_begins_frames() {
        let mut pacer = FramePacer::new();
        let now = Instant::now();
        let plan = pacer.tick(now, FPS, &[page(1, Visibility::OnScreen, 0)], 9, false);
        assert!(plan.redraw);
        assert_eq!(plan.begin_frames, vec![1]);
    }

    #[test]
    fn signature_change_redraws_once() {
        let pages = [page(1, Visibility::OnScreen, 0)];
        let (mut pacer, t) = settled(&pages);
        pacer.rendered(t);
        let t1 = t + Duration::from_millis(50);
        assert!(pacer.tick(t1, FPS, &pages, 2, false).redraw);
        pacer.rendered(t1);
        let t2 = t1 + Duration::from_millis(50);
        assert!(!pacer.tick(t2, FPS, &pages, 2, false).redraw);
    }

    #[test]
    fn redraw_capped_at_frame_rate_and_not_rerequested_while_pending() {
        let pages = [page(1, Visibility::OnScreen, 0)];
        let (mut pacer, t) = settled(&pages);
        pacer.rendered(t);
        // 2 ms after a render: too soon even though dirty.
        let t1 = t + Duration::from_millis(2);
        pacer.note_input(t1);
        assert!(!pacer.tick(t1, FPS, &pages, 1, false).redraw);
        let deadline = pacer.next_deadline().unwrap();
        assert!(deadline <= t + frame_interval(FPS));
        let t2 = t + frame_interval(FPS);
        assert!(pacer.tick(t2, FPS, &pages, 1, false).redraw);
        // Still pending (RedrawRequested not yet delivered): no second request.
        assert!(!pacer.tick(t2 + Duration::from_millis(30), FPS, &pages, 1, false).redraw);
    }

    #[test]
    fn animation_redraws_every_frame() {
        let pages = [page(1, Visibility::OnScreen, 0)];
        let mut pacer = FramePacer::new();
        let mut now = Instant::now();
        let mut redraws = 0;
        for _ in 0..60 {
            if pacer.tick(now, FPS, &pages, 1, true).redraw {
                redraws += 1;
                pacer.rendered(now);
            }
            now += frame_interval(FPS);
        }
        assert_eq!(redraws, 60);
    }

    #[test]
    fn begin_frame_rates_by_state() {
        let on = page(1, Visibility::OnScreen, 0);
        let off = page(2, Visibility::OffScreen, 0);
        let busy = PageFrameInfo {
            busy: true,
            ..page(3, Visibility::OnScreen, 0)
        };
        let pages = [on, off, busy];
        let (mut pacer, start) = settled(&pages);

        let mut counts: HashMap<i32, u32> = HashMap::new();
        let step = Duration::from_millis(1);
        let mut now = start + step;
        while now < start + Duration::from_secs(2) {
            for id in pacer.tick(now, FPS, &pages, 1, false).begin_frames {
                *counts.entry(id).or_default() += 1;
            }
            now += step;
        }
        let per_sec = |id| counts.get(&id).copied().unwrap_or(0) as f32 / 2.0;
        assert!((9.0..=11.0).contains(&per_sec(1)), "idle on-screen {}", per_sec(1));
        assert!((0.5..=1.5).contains(&per_sec(2)), "off-screen {}", per_sec(2));
        assert!((55.0..=62.0).contains(&per_sec(3)), "busy {}", per_sec(3));
    }

    #[test]
    fn paint_promotes_idle_page_to_full_rate() {
        let pages = [page(1, Visibility::OnScreen, 0)];
        let (mut pacer, t) = settled(&pages);
        // Paint arrives (e.g. video started): token changes.
        let painted = [page(1, Visibility::OnScreen, 1)];
        let t1 = t + Duration::from_millis(200);
        let plan = pacer.tick(t1, FPS, &painted, 2, false);
        assert!(plan.redraw);
        let next = pacer.next_deadline().unwrap();
        assert!(next <= t1 + frame_interval(FPS), "full-rate begin frame scheduled");
    }

    #[test]
    fn input_promotes_all_visible_pages() {
        let pages = [page(1, Visibility::OnScreen, 0), page(2, Visibility::OffScreen, 0)];
        let (mut pacer, t) = settled(&pages);
        let t1 = t + Duration::from_millis(150);
        pacer.note_input(t1);
        pacer.tick(t1, FPS, &pages, 1, false);
        let t2 = t1 + frame_interval(FPS);
        let plan = pacer.tick(t2, FPS, &pages, 1, false);
        assert_eq!(plan.begin_frames, vec![1], "off-screen page stays slow");
    }

    #[test]
    fn panned_into_view_gets_immediate_begin_frame() {
        let off = [page(1, Visibility::OffScreen, 0)];
        let (mut pacer, t) = settled(&off);
        let on = [page(1, Visibility::OnScreen, 0)];
        let plan = pacer.tick(t + Duration::from_millis(5), FPS, &on, 1, false);
        assert_eq!(plan.begin_frames, vec![1]);
    }

    #[test]
    fn removed_pages_are_forgotten_and_new_pages_start_active() {
        let (mut pacer, t) = settled(&[page(1, Visibility::OnScreen, 0)]);
        // Workspace switch: page 1 parked, page 2 restored.
        let restored = [page(2, Visibility::OnScreen, 0)];
        let plan = pacer.tick(t + Duration::from_millis(5), FPS, &restored, 3, false);
        assert_eq!(plan.begin_frames, vec![2]);
        assert!(!pacer.pages.contains_key(&1));
    }

    #[test]
    fn idle_deadline_is_far_and_none_without_pages() {
        let pages = [page(1, Visibility::OnScreen, 0)];
        let (mut pacer, t) = settled(&pages);
        pacer.rendered(t);
        pacer.tick(t, FPS, &pages, 1, false);
        let next = pacer.next_deadline().unwrap();
        assert!(next >= t + Duration::from_millis(90), "idle page wakes at ~10 Hz");

        let mut empty = FramePacer::new();
        let now = Instant::now();
        empty.tick(now, FPS, &[], 1, false);
        empty.rendered(now);
        empty.tick(now, FPS, &[], 1, false);
        assert_eq!(empty.next_deadline(), None);
    }

    #[test]
    fn skipped_render_retries_next_frame() {
        let pages = [page(1, Visibility::OnScreen, 0)];
        let mut pacer = FramePacer::new();
        let t = Instant::now();
        assert!(pacer.tick(t, FPS, &pages, 1, false).redraw);
        pacer.render_skipped(t);
        assert!(!pacer.tick(t, FPS, &pages, 1, false).redraw);
        assert!(pacer
            .tick(t + frame_interval(FPS), FPS, &pages, 1, false)
            .redraw);
    }
}
