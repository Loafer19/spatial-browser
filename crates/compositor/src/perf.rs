// F3 performance overlay data: canvas frame rate, CPU time per frame, CEF
// paints, tiles by state and the memory of the browser and its CEF child
// processes. Sampled once per SAMPLE_EVERY while shown; hidden, the only cost
// is timing each presented frame.

use std::cell::Cell;
use std::collections::HashMap;
use std::time::{Duration, Instant};

const SAMPLE_EVERY: Duration = Duration::from_secs(1);

thread_local! {
    static SHOWN: Cell<bool> = const { Cell::new(false) };
}

/// F3 / palette: show or hide the overlay.
pub fn toggle() {
    SHOWN.set(!SHOWN.get());
}

pub fn shown() -> bool {
    SHOWN.get()
}

/// Session tiles by state, as the frame pacer sees them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TileCounts {
    pub on_screen: usize,
    pub off_screen: usize,
    /// Loading or not painted yet.
    pub loading: usize,
    /// In other workspaces (alive, hidden).
    pub parked: usize,
}

/// Resident memory (KiB) of this process and its CEF descendants.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MemoryUsage {
    pub browser_kib: u64,
    pub renderers: (usize, u64),
    pub gpu_kib: u64,
    /// Utility / zygote / other helpers.
    pub other: (usize, u64),
}

impl MemoryUsage {
    pub fn total_kib(&self) -> u64 {
        self.browser_kib + self.renderers.1 + self.gpu_kib + self.other.1
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sample {
    pub fps: f32,
    pub frame_ms_avg: f32,
    pub frame_ms_max: f32,
    pub paints_per_s: f32,
    pub tiles: TileCounts,
    pub memory: Option<MemoryUsage>,
}

pub struct PerfMonitor {
    window_start: Instant,
    frames: u32,
    frame_cpu_total: Duration,
    frame_cpu_max: Duration,
    paints: u64,
    last_paint_counts: HashMap<i32, u64>,
    sample: Option<Sample>,
    /// Bumped per sample; part of the frame signature so a new sample redraws.
    generation: u64,
}

impl PerfMonitor {
    pub fn new(now: Instant) -> Self {
        Self {
            window_start: now,
            frames: 0,
            frame_cpu_total: Duration::ZERO,
            frame_cpu_max: Duration::ZERO,
            paints: 0,
            last_paint_counts: HashMap::new(),
            sample: None,
            generation: 0,
        }
    }

    /// A frame was presented; `cpu` = time spent encoding and submitting it.
    pub fn frame_presented(&mut self, cpu: Duration) {
        self.frames += 1;
        self.frame_cpu_total += cpu;
        self.frame_cpu_max = self.frame_cpu_max.max(cpu);
    }

    /// Once per loop iteration. `paint_counts`: (browser id, CEF paint
    /// count) per session page. Takes a sample when the window is full.
    pub fn tick(&mut self, now: Instant, paint_counts: &[(i32, u64)], tiles: TileCounts) {
        if !shown() {
            // Start fresh when shown again, without a stale first window.
            self.window_start = now;
            self.frames = 0;
            self.frame_cpu_total = Duration::ZERO;
            self.frame_cpu_max = Duration::ZERO;
            self.paints = 0;
            self.last_paint_counts.clear();
            self.sample = None;
            return;
        }
        let mut seen = HashMap::with_capacity(paint_counts.len());
        for &(id, count) in paint_counts {
            // A page new to the window counts from its next paint.
            let before = self.last_paint_counts.get(&id).copied().unwrap_or(count);
            self.paints += count.saturating_sub(before);
            seen.insert(id, count);
        }
        self.last_paint_counts = seen;

        let elapsed = now.duration_since(self.window_start);
        if elapsed < SAMPLE_EVERY && self.sample.is_some() {
            return;
        }
        let secs = elapsed.as_secs_f32().max(0.001);
        let avg = if self.frames == 0 {
            0.0
        } else {
            self.frame_cpu_total.as_secs_f32() * 1000.0 / self.frames as f32
        };
        self.sample = Some(Sample {
            fps: self.frames as f32 / secs,
            frame_ms_avg: avg,
            frame_ms_max: self.frame_cpu_max.as_secs_f32() * 1000.0,
            paints_per_s: self.paints as f32 / secs,
            tiles,
            memory: process_memory(),
        });
        self.generation += 1;
        self.window_start = now;
        self.frames = 0;
        self.frame_cpu_total = Duration::ZERO;
        self.frame_cpu_max = Duration::ZERO;
        self.paints = 0;
    }

    /// When the loop must wake for the next sample (None while hidden).
    pub fn next_deadline(&self) -> Option<Instant> {
        shown().then(|| self.window_start + SAMPLE_EVERY)
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Overlay text, one entry per line (ASCII: the HUD font is font8x8).
    pub fn lines(&self, paint_path: &str) -> Vec<String> {
        let Some(s) = &self.sample else {
            return vec!["PERF  (F3 hides)".into(), "sampling...".into()];
        };
        let t = s.tiles;
        let mut lines = vec![
            "PERF  (F3 hides)".to_string(),
            format!("fps   {:>5.1}  {paint_path}", s.fps),
            format!(
                "cpu   {:>4.1} ms  max {:.1}",
                s.frame_ms_avg, s.frame_ms_max
            ),
            format!("paint {:>5.0}/s", s.paints_per_s),
            format!(
                "tiles {:>3}  on {} off {}",
                t.on_screen + t.off_screen,
                t.on_screen,
                t.off_screen
            ),
            format!("      load {} parked {}", t.loading, t.parked),
        ];
        match s.memory {
            Some(m) => {
                lines.push(format!("ram   {}", fmt_kib(m.total_kib())));
                lines.push(format!("  app     {}", fmt_kib(m.browser_kib)));
                lines.push(format!(
                    "  render  {}x {}",
                    m.renderers.0,
                    fmt_kib(m.renderers.1)
                ));
                lines.push(format!("  gpu     {}", fmt_kib(m.gpu_kib)));
                if m.other.0 > 0 {
                    lines.push(format!("  other   {}x {}", m.other.0, fmt_kib(m.other.1)));
                }
            }
            None => lines.push("ram   n/a".into()),
        }
        lines
    }
}

/// `1.84 GB` / `312 MB` / `640 KB` (binary units, short labels).
pub fn fmt_kib(kib: u64) -> String {
    const MIB: u64 = 1024;
    const GIB: u64 = 1024 * 1024;
    if kib >= GIB {
        format!("{:.2} GB", kib as f64 / GIB as f64)
    } else if kib >= MIB {
        format!("{} MB", (kib as f64 / MIB as f64).round() as u64)
    } else {
        format!("{kib} KB")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProcessKind {
    Renderer,
    Gpu,
    Other,
}

/// Chromium helpers name their role in `--type=…`.
fn kind_from_cmdline(cmdline: &[u8]) -> ProcessKind {
    for arg in cmdline.split(|&b| b == 0) {
        match arg {
            b"--type=renderer" => return ProcessKind::Renderer,
            b"--type=gpu-process" => return ProcessKind::Gpu,
            _ => {}
        }
    }
    ProcessKind::Other
}

/// `VmRSS:  123456 kB` from /proc/<pid>/status.
fn rss_kib(status: &str) -> Option<u64> {
    status
        .lines()
        .find_map(|l| l.strip_prefix("VmRSS:"))
        .and_then(|v| v.trim().trim_end_matches("kB").trim().parse().ok())
}

/// Parent pid from /proc/<pid>/stat (the comm field may contain spaces and
/// parentheses, so parse after the last `)`).
fn ppid_from_stat(stat: &str) -> Option<u32> {
    let rest = &stat[stat.rfind(')')? + 1..];
    rest.split_whitespace().nth(1)?.parse().ok()
}

/// Every pid descending from `root`, given (pid, ppid) pairs.
fn descendants(root: u32, parents: &[(u32, u32)]) -> Vec<u32> {
    let mut out = Vec::new();
    let mut frontier = vec![root];
    while let Some(parent) = frontier.pop() {
        for &(pid, ppid) in parents {
            if ppid == parent && pid != root && !out.contains(&pid) {
                out.push(pid);
                frontier.push(pid);
            }
        }
    }
    out
}

#[cfg(target_os = "linux")]
fn process_memory() -> Option<MemoryUsage> {
    use std::fs;
    let me = std::process::id();
    let parents: Vec<(u32, u32)> = fs::read_dir("/proc")
        .ok()?
        .filter_map(|e| e.ok()?.file_name().to_str()?.parse::<u32>().ok())
        .filter_map(|pid| {
            let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
            Some((pid, ppid_from_stat(&stat)?))
        })
        .collect();
    let mut usage = MemoryUsage {
        browser_kib: rss_kib(&fs::read_to_string("/proc/self/status").ok()?)?,
        ..Default::default()
    };
    for pid in descendants(me, &parents) {
        // Gone between the scan and now: skip it.
        let Some(rss) = fs::read_to_string(format!("/proc/{pid}/status"))
            .ok()
            .and_then(|s| rss_kib(&s))
        else {
            continue;
        };
        let cmdline = fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
        match kind_from_cmdline(&cmdline) {
            ProcessKind::Renderer => {
                usage.renderers.0 += 1;
                usage.renderers.1 += rss;
            }
            ProcessKind::Gpu => usage.gpu_kib += rss,
            ProcessKind::Other => {
                usage.other.0 += 1;
                usage.other.1 += rss;
            }
        }
    }
    Some(usage)
}

#[cfg(not(target_os = "linux"))]
fn process_memory() -> Option<MemoryUsage> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_proc_fields() {
        assert_eq!(
            rss_kib("Name:\tx\nVmRSS:\t  123456 kB\nVmSwap: 0 kB\n"),
            Some(123_456)
        );
        assert_eq!(rss_kib("Name:\tkthreadd\n"), None);
        assert_eq!(ppid_from_stat("42 (we (ird) name) S 7 42 42 0"), Some(7));
        assert_eq!(ppid_from_stat("garbage"), None);
        assert_eq!(
            kind_from_cmdline(b"/opt/cef/compositor\0--type=renderer\0--lang=en\0"),
            ProcessKind::Renderer
        );
        assert_eq!(
            kind_from_cmdline(b"x\0--type=gpu-process\0"),
            ProcessKind::Gpu
        );
        assert_eq!(kind_from_cmdline(b"x\0--type=zygote\0"), ProcessKind::Other);
        // A URL argument that merely contains the flag text is not the flag.
        assert_eq!(
            kind_from_cmdline(b"x\0https://a.test/?--type=renderer\0"),
            ProcessKind::Other
        );
    }

    #[test]
    fn finds_the_whole_process_tree() {
        let parents = [(10, 1), (11, 10), (12, 11), (13, 10), (20, 1), (21, 20)];
        let mut tree = descendants(10, &parents);
        tree.sort_unstable();
        assert_eq!(tree, vec![11, 12, 13]);
        assert!(descendants(99, &parents).is_empty());
    }

    #[test]
    fn formats_memory() {
        assert_eq!(fmt_kib(640), "640 KB");
        assert_eq!(fmt_kib(312 * 1024), "312 MB");
        assert_eq!(fmt_kib(1_929_380), "1.84 GB");
    }

    #[test]
    fn samples_rates_over_the_window_only_while_shown() {
        SHOWN.set(false);
        let t0 = Instant::now();
        let mut perf = PerfMonitor::new(t0);
        perf.frame_presented(Duration::from_millis(2));
        perf.tick(t0, &[(1, 5)], TileCounts::default());
        assert!(perf.sample.is_none());
        assert!(perf.next_deadline().is_none());

        toggle();
        // First tick while shown takes a placeholder sample at once.
        perf.tick(t0, &[(1, 5)], TileCounts::default());
        let first = perf.generation();
        for i in 0..30 {
            perf.frame_presented(Duration::from_millis(if i == 0 { 9 } else { 3 }));
        }
        let tiles = TileCounts {
            on_screen: 2,
            off_screen: 1,
            loading: 1,
            parked: 4,
        };
        perf.tick(t0 + Duration::from_millis(500), &[(1, 25), (2, 0)], tiles);
        assert_eq!(perf.generation(), first, "no sample before a full window");
        perf.tick(t0 + SAMPLE_EVERY, &[(1, 45), (2, 10)], tiles);
        assert_eq!(perf.generation(), first + 1);
        let s = perf.sample.clone().expect("sample");
        assert!((s.fps - 30.0).abs() < 0.1, "{}", s.fps);
        assert!((s.frame_ms_avg - 3.2).abs() < 0.01, "{}", s.frame_ms_avg);
        assert!((s.frame_ms_max - 9.0).abs() < 0.01);
        // 40 paints on page 1 + 10 on page 2 (new page counts from its first count).
        assert!((s.paints_per_s - 50.0).abs() < 0.1, "{}", s.paints_per_s);
        assert_eq!(s.tiles, tiles);
        let text = perf.lines("gpu").join("\n");
        assert!(text.contains("fps    30.0  gpu"), "{text}");
        assert!(text.is_ascii());
        toggle();
        SHOWN.set(false);
    }
}
