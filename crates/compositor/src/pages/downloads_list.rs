// Ctrl+J downloads: live "Active" downloads above the persisted "Completed"
// list. Actions are `download://…` (→ PENDING_DOWNLOAD_ACTION); while the
// tile is open, pending_actions::download_page pushes `live_json` to it and
// SCRIPT below redraws both sections in place — no reload mid-download.

use super::ui::{self, html, Icon, Markup};
use crate::output::Theme;
use crate::persistence::bookmarks::host_of;
use crate::persistence::downloads::DownloadRecord;
use cef_bridge::{DownloadProgress, DownloadState};
use serde::Serialize;

/// What a file looks like on the page: its row icon, and the key the page
/// script picks that icon by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FileKind {
    File,
    Document,
    Image,
    Video,
    Audio,
    Archive,
    Code,
    /// Executable, script or unknown binary: "Open" reveals its folder.
    Program,
}

const DOCUMENT_EXTENSIONS: &[&str] = &[
    "pdf", "txt", "md", "rtf", "doc", "docx", "odt", "xls", "xlsx", "ods", "csv", "tsv", "ppt",
    "pptx", "odp", "epub", "djvu",
];
const IMAGE_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "svg", "bmp", "ico", "tif", "tiff", "heic", "avif",
];
const VIDEO_EXTENSIONS: &[&str] = &["mp4", "mkv", "webm", "mov", "avi", "m4v", "wmv", "flv"];
const AUDIO_EXTENSIONS: &[&str] = &["mp3", "flac", "ogg", "oga", "opus", "wav", "m4a", "aac"];
const ARCHIVE_EXTENSIONS: &[&str] = &[
    "zip", "tar", "gz", "tgz", "bz2", "xz", "zst", "7z", "rar", "iso",
];
const CODE_EXTENSIONS: &[&str] = &[
    "json", "xml", "html", "htm", "css", "js", "ts", "rs", "c", "h", "cpp", "go", "java", "toml",
    "yaml", "yml", "sql", "log",
];

impl FileKind {
    const ALL: [FileKind; 8] = [
        FileKind::File,
        FileKind::Document,
        FileKind::Image,
        FileKind::Video,
        FileKind::Audio,
        FileKind::Archive,
        FileKind::Code,
        FileKind::Program,
    ];

    /// By extension first (what a file manager shows), then MIME type.
    pub(crate) fn of(name: &str, mime: Option<&str>) -> Self {
        let ext = std::path::Path::new(name)
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase);
        // Risky extensions, and no extension at all.
        if cef_bridge::is_risky_download(name, None) {
            return FileKind::Program;
        }
        if let Some(ext) = ext.as_deref() {
            for (list, kind) in [
                (DOCUMENT_EXTENSIONS, FileKind::Document),
                (IMAGE_EXTENSIONS, FileKind::Image),
                (VIDEO_EXTENSIONS, FileKind::Video),
                (AUDIO_EXTENSIONS, FileKind::Audio),
                (ARCHIVE_EXTENSIONS, FileKind::Archive),
                (CODE_EXTENSIONS, FileKind::Code),
            ] {
                if list.contains(&ext) {
                    return kind;
                }
            }
        }
        let mime = mime.unwrap_or("").trim().to_ascii_lowercase();
        if mime.starts_with("image/") {
            FileKind::Image
        } else if mime.starts_with("video/") {
            FileKind::Video
        } else if mime.starts_with("audio/") {
            FileKind::Audio
        } else if mime.starts_with("text/") {
            FileKind::Document
        } else if cef_bridge::is_risky_download(name, Some(&mime)) {
            FileKind::Program
        } else {
            FileKind::File
        }
    }

    fn key(self) -> &'static str {
        match self {
            FileKind::File => "file",
            FileKind::Document => "document",
            FileKind::Image => "image",
            FileKind::Video => "video",
            FileKind::Audio => "audio",
            FileKind::Archive => "archive",
            FileKind::Code => "code",
            FileKind::Program => "program",
        }
    }

    fn icon(self) -> Icon {
        match self {
            FileKind::File => Icon::File,
            FileKind::Document => Icon::Document,
            FileKind::Image => Icon::Image,
            FileKind::Video => Icon::Movie,
            FileKind::Audio => Icon::Music,
            FileKind::Archive => Icon::Archive,
            FileKind::Code => Icon::Code,
            FileKind::Program => Icon::Shield,
        }
    }

    fn by_key(key: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|k| k.key() == key)
            .unwrap_or(FileKind::File)
    }
}

// ---- formatting ---------------------------------------------------------------

/// "512 B", "1.5 KB", "12.3 MB", "48 MB", "120 MB" (binary units, one
/// decimal below 100, none above, ".0" dropped).
pub(crate) fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["KB", "MB", "GB", "TB", "PB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    // 1023.6 KB would print as "1024 KB": move up a unit first.
    while value >= 1023.95 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    let text = if value >= 99.95 {
        format!("{value:.0}")
    } else {
        format!("{value:.1}")
    };
    let text = text.strip_suffix(".0").unwrap_or(&text);
    format!("{text} {}", UNITS[unit])
}

/// "2.1 MB/s".
pub(crate) fn format_speed(bytes_per_sec: u64) -> String {
    format!("{}/s", format_bytes(bytes_per_sec))
}

/// "15 s left", "3 min left", "2 h 5 min left" (minutes rounded up).
pub(crate) fn format_eta(secs: u64) -> String {
    if secs < 60 {
        return format!("{} s left", secs.max(1));
    }
    let minutes = secs.div_ceil(60);
    if minutes < 60 {
        return format!("{minutes} min left");
    }
    match (minutes / 60, minutes % 60) {
        (h, 0) => format!("{h} h left"),
        (h, m) => format!("{h} h {m} min left"),
    }
}

/// Seconds to go at the current speed; `None` without a total or a speed.
pub(crate) fn eta_secs(received: u64, total: Option<u64>, speed: u64) -> Option<u64> {
    let total = total?;
    (speed > 0 && total > received).then(|| (total - received).div_ceil(speed))
}

/// "12.3 MB of 48 MB · 2.1 MB/s · 15 s left", "Paused · 3 MB of 10 MB", …
pub(crate) fn status_line(p: &DownloadProgress) -> String {
    let size = match p.total_bytes {
        Some(total) => format!(
            "{} of {}",
            format_bytes(p.received_bytes),
            format_bytes(total)
        ),
        None => format_bytes(p.received_bytes),
    };
    match p.state {
        DownloadState::InProgress => {
            let mut parts = vec![size];
            if p.speed > 0 {
                parts.push(format_speed(p.speed));
                if let Some(eta) = eta_secs(p.received_bytes, p.total_bytes, p.speed) {
                    parts.push(format_eta(eta));
                }
            }
            parts.join(" · ")
        }
        DownloadState::Paused => format!("Paused · {size}"),
        DownloadState::Interrupted => format!("Failed · {size}"),
        DownloadState::Complete => format!(
            "Done · {}",
            format_bytes(p.total_bytes.unwrap_or(p.received_bytes))
        ),
        DownloadState::Canceled => "Canceled".into(),
    }
}

/// Where a download came from, for the row's second line. Only real hosts:
/// a `data:` URL would otherwise show its payload.
pub(crate) fn source_host(url: &str) -> &str {
    let url = url.strip_prefix("blob:").unwrap_or(url);
    let lower = url.get(..8).unwrap_or(url).to_ascii_lowercase();
    if lower.starts_with("https://") || lower.starts_with("http://") || lower.starts_with("ftp://")
    {
        host_of(url)
    } else if lower.starts_with("data:") {
        "Embedded file"
    } else {
        ""
    }
}

// ---- views (one shape for the first render and for live pushes) ---------------

#[derive(Debug, Serialize)]
struct ActiveView {
    id: u32,
    name: String,
    host: String,
    icon: &'static str,
    state: &'static str,
    /// Only with a known total (else the bar is indeterminate).
    percent: Option<u8>,
    status: String,
}

impl ActiveView {
    fn of(p: &DownloadProgress) -> Self {
        let name = p.file_name().to_string();
        let mime = Some(p.mime.as_str()).filter(|m| !m.is_empty());
        Self {
            id: p.id,
            icon: FileKind::of(&name, mime).key(),
            host: source_host(&p.url).to_string(),
            name,
            state: p.state.as_str(),
            percent: p.total_bytes.and(p.percent),
            status: status_line(p),
        }
    }

    fn running(&self) -> bool {
        self.state == DownloadState::InProgress.as_str()
            || self.state == DownloadState::Paused.as_str()
    }

    fn interrupted(&self) -> bool {
        self.state == DownloadState::Interrupted.as_str()
    }
}

#[derive(Debug, Serialize)]
struct DoneView {
    index: usize,
    name: String,
    host: String,
    icon: &'static str,
}

impl DoneView {
    fn of(index: usize, record: &DownloadRecord) -> Self {
        let name = std::path::Path::new(&record.path)
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or(&record.path)
            .to_string();
        Self {
            index,
            icon: FileKind::of(&name, record.mime.as_deref()).key(),
            host: source_host(&record.url).to_string(),
            name,
        }
    }
}

/// Push payload for `ui.on('downloads')`: the active list, plus the
/// completed list when it changed (a download finished, an entry was
/// removed) — its indices shift, so it is always sent whole.
pub(crate) fn live_json(
    active: &[DownloadProgress],
    completed: Option<&[DownloadRecord]>,
) -> serde_json::Value {
    let mut value = serde_json::json!({
        "active": active.iter().map(ActiveView::of).collect::<Vec<_>>(),
    });
    if let Some(records) = completed {
        value["completed"] = serde_json::json!(records
            .iter()
            .enumerate()
            .map(|(i, r)| DoneView::of(i, r))
            .collect::<Vec<_>>());
    }
    value
}

// ---- markup ---------------------------------------------------------------------

fn file_icon(key: &str) -> Markup {
    html! { span class="dl-icon" data-icon=(key) { (FileKind::by_key(key).icon()) } }
}

/// `<a class="icon-btn">` that the page script finds by `data-act`.
fn act(act: &str, href: &str, title: &str, icon: Icon, danger: bool, show: bool) -> Markup {
    html! {
        a class={ "icon-btn" @if danger { " danger" } @if !show { " hidden" } }
            data-act=(act) href=(href) title=(title) aria-label=(title) { (icon) }
    }
}

/// Keep in step with `fillActive` in SCRIPT. Delete only dismisses a failed
/// download: cancelling a running one takes its button (OSR tiles have no
/// confirm() dialog to guard a stray key).
fn active_row(v: &ActiveView) -> Markup {
    let id = v.id;
    let indeterminate = v.percent.is_none() && v.state == DownloadState::InProgress.as_str();
    let width = v.percent.map(|p| format!("width:{p}%"));
    html! {
        div class="row dl-live" data-nav data-id=(id) data-state=(v.state)
            data-delete=[v.interrupted().then(|| format!("download://cancel/{id}"))] {
            (file_icon(v.icon))
            div class="main" {
                span class="title" title=(v.name) { (v.name) }
                span class="sub" { (v.host) }
                div class={ "progress" @if indeterminate { " indeterminate" } } { i style=[width] {} }
                span class="dl-status small muted" { (v.status) }
            }
            span class="actions" {
                (act("pause", &format!("download://pause/{id}"), "Pause", Icon::Pause, false,
                    v.state == DownloadState::InProgress.as_str()))
                (act("resume", &format!("download://resume/{id}"),
                    if v.interrupted() { "Retry" } else { "Resume" }, Icon::Play, false,
                    v.state == DownloadState::Paused.as_str() || v.interrupted()))
                (act("cancel", &format!("download://cancel/{id}"),
                    if v.running() { "Cancel" } else { "Dismiss" }, Icon::Close, true,
                    v.running() || v.interrupted()))
            }
        }
    }
}

/// Keep in step with `doneRow` in SCRIPT.
fn done_row(v: &DoneView) -> Markup {
    let open = format!("download://open/{}", v.index);
    let remove = format!("download://remove/{}", v.index);
    html! {
        div class="row" data-open=(open) data-delete=(remove) {
            (file_icon(v.icon))
            span class="main" {
                span class="title" title=(v.name) { (v.name) }
                span class="sub" { (v.host) }
            }
            span class="actions" {
                (act("open", &open, "Open", Icon::Open, false, true))
                (act("reveal", &format!("download://reveal/{}", v.index), "Show in folder", Icon::Folder, false, true))
                (act("remove", &remove, "Remove from list", Icon::Trash, true, true))
            }
        }
    }
}

const CSS: &str = r#"
#dl:not([data-has-active]) #dl-active-sec,
#dl:not([data-has-done]) #dl-done-sec,
#dl:not([data-has-done]) .page-head .search,
#dl[data-has-active] #dl-empty,
#dl[data-has-done] #dl-empty { display: none; }
#dl-active-sec + #dl-done-sec { margin-top: 26px; }
.dl-icon {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  width: 32px;
  height: 32px;
  border-radius: var(--radius-sm);
  background: var(--surface-2);
  color: var(--muted);
}
.dl-icon svg { width: 18px; height: 18px; }
.dl-live { align-items: flex-start; padding-top: 10px; padding-bottom: 10px; }
.dl-live .dl-icon { background: var(--accent-soft); color: var(--accent); }
.dl-live .main { gap: 3px; }
.dl-live .progress { margin: 4px 0 2px; }
.dl-live .dl-status { font-variant-numeric: tabular-nums; overflow-wrap: anywhere; }
.dl-live .actions { opacity: 1; align-self: center; }
.dl-live[data-state=paused] .progress > i { background: var(--muted); }
.dl-live[data-state=complete] .progress > i { background: var(--success); }
.dl-live[data-state=interrupted] .progress > i { background: var(--danger); }
.dl-live[data-state=interrupted] .dl-status { color: var(--danger); }
.dl-live[data-state=interrupted] .dl-icon { background: var(--danger-soft); color: var(--danger); }
"#;

/// Live redraw from `ui.on('downloads')`. Rows come from the server-rendered
/// templates and are filled with textContent / numeric ids only, so pushed
/// names can never become markup or a different action URL.
const SCRIPT: &str = r#"
(function () {
  'use strict';
  var main = document.getElementById('dl');
  var activeList = document.getElementById('dl-active');
  var doneList = document.getElementById('dl-done');
  var icons = {};
  document.getElementById('dl-icons').content.querySelectorAll('[data-icon]').forEach(function (el) {
    icons[el.dataset.icon] = el.firstElementChild;
  });

  function clone(id) { return document.getElementById(id).content.firstElementChild.cloneNode(true); }
  function num(n) { return String(Math.max(0, Math.floor(Number(n) || 0))); }
  function setText(el, text) { text = String(text == null ? '' : text); if (el.textContent !== text) el.textContent = text; }
  function setIcon(row, key) {
    var slot = row.querySelector('.dl-icon');
    if (slot.dataset.icon === key) return;
    var svg = icons[key] || icons.file;
    slot.dataset.icon = icons[key] ? key : 'file';
    slot.replaceChildren(svg.cloneNode(true));
  }
  function setTitle(row, d) {
    var title = row.querySelector('.title');
    setText(title, d.name);
    title.title = String(d.name == null ? '' : d.name);
    setText(row.querySelector('.sub'), d.host);
  }
  function setAct(row, name, href, show, label) {
    var a = row.querySelector('[data-act="' + name + '"]');
    a.setAttribute('href', href);
    a.classList.toggle('hidden', !show);
    if (label) { a.title = label; a.setAttribute('aria-label', label); }
  }
  function toggleData(el, key, value) {
    if (value) el.dataset[key] = value; else delete el.dataset[key];
  }

  function fillActive(row, d) {
    var id = num(d.id);
    var running = d.state === 'in-progress' || d.state === 'paused';
    var failed = d.state === 'interrupted';
    row.dataset.id = id;
    row.dataset.state = String(d.state);
    toggleData(row, 'delete', failed ? 'download://cancel/' + id : '');
    setIcon(row, d.icon);
    setTitle(row, d);
    var bar = row.querySelector('.progress');
    var known = typeof d.percent === 'number';
    bar.classList.toggle('indeterminate', !known && d.state === 'in-progress');
    bar.firstElementChild.style.width = known ? Math.min(100, Math.max(0, d.percent)) + '%' : '';
    setText(row.querySelector('.dl-status'), d.status);
    setAct(row, 'pause', 'download://pause/' + id, d.state === 'in-progress');
    setAct(row, 'resume', 'download://resume/' + id, d.state === 'paused' || failed, failed ? 'Retry' : 'Resume');
    setAct(row, 'cancel', 'download://cancel/' + id, running || failed, running ? 'Cancel' : 'Dismiss');
  }

  function doneRow(d) {
    var row = clone('dl-tpl-done');
    var i = num(d.index);
    row.dataset.open = 'download://open/' + i;
    row.dataset.delete = 'download://remove/' + i;
    setIcon(row, d.icon);
    setTitle(row, d);
    setAct(row, 'open', 'download://open/' + i, true);
    setAct(row, 'reveal', 'download://reveal/' + i, true);
    setAct(row, 'remove', 'download://remove/' + i, true);
    return row;
  }

  function renderActive(items) {
    var keep = {};
    items.forEach(function (d, i) {
      var id = num(d.id);
      keep[id] = true;
      var row = activeList.querySelector('.row[data-id="' + id + '"]') || clone('dl-tpl-active');
      fillActive(row, d);
      if (activeList.children[i] !== row) activeList.insertBefore(row, activeList.children[i] || null);
    });
    Array.prototype.slice.call(activeList.children).forEach(function (row) {
      if (!keep[row.dataset.id]) row.remove();
    });
    main.toggleAttribute('data-has-active', items.length > 0);
  }

  function renderDone(items) {
    var frag = document.createDocumentFragment();
    items.forEach(function (d) { frag.appendChild(doneRow(d)); });
    doneList.replaceChildren(frag);
    main.toggleAttribute('data-has-done', items.length > 0);
    var box = document.querySelector('input[data-filter]');
    if (box && box.value) ui.filter(box);
  }

  // Keep the keyboard selection on the same row (or the same slot when
  // that row is gone) across a redraw.
  function keepSelection(update) {
    var before = document.querySelector('.row.is-active');
    var index = before ? ui.rows().indexOf(before) : -1;
    update();
    var rows = ui.rows();
    var at = before ? rows.indexOf(before) : -1;
    if (at >= 0) ui.setActive(at, false);
    else if (index >= 0 && rows.length) ui.setActive(Math.min(index, rows.length - 1), false);
  }

  ui.on('downloads', function (data) {
    if (!data || typeof data !== 'object') return;
    keepSelection(function () {
      if (Array.isArray(data.active)) renderActive(data.active);
      if (Array.isArray(data.completed)) renderDone(data.completed);
    });
  });
})();
"#;

const KEYS: &[(&str, &str)] = &[
    ("Enter", "Open completed file"),
    ("Del", "Remove from list / dismiss failed download"),
];

/// Downloads page URL. `active` = `cef_bridge::active_downloads()` at render
/// time; later changes arrive by push.
pub(crate) fn page_url(
    theme: &Theme,
    downloads: &[DownloadRecord],
    active: &[DownloadProgress],
) -> String {
    let active: Vec<ActiveView> = active.iter().map(ActiveView::of).collect();
    let done: Vec<DoneView> = downloads
        .iter()
        .enumerate()
        .map(|(i, r)| DoneView::of(i, r))
        .collect();
    let template_active = ActiveView {
        id: 0,
        name: String::new(),
        host: String::new(),
        icon: FileKind::File.key(),
        state: DownloadState::InProgress.as_str(),
        percent: None,
        status: String::new(),
    };
    let template_done = DoneView {
        index: 0,
        name: String::new(),
        host: String::new(),
        icon: FileKind::File.key(),
    };
    let body = html! {
        main id="dl" class="page" data-has-active[!active.is_empty()] data-has-done[!done.is_empty()] {
            (ui::head("Downloads", Some(html! { "Saved to " code { "~/Downloads" } }), html! {
                (ui::search("q", "", "Filter completed", Some("#dl-done")))
            }))
            section id="dl-active-sec" {
                h2 { "Active" }
                div id="dl-active" class="list" {
                    @for view in &active { (active_row(view)) }
                }
            }
            section id="dl-done-sec" {
                h2 { "Completed" }
                div id="dl-done" class="list" {
                    @for view in &done { (done_row(view)) }
                }
                div class="empty hidden" data-filter-empty {
                    (Icon::Search) div { "No download matches." }
                }
            }
            div id="dl-empty" {
                (ui::empty(Icon::Download, html! {
                    "No downloads yet."
                    div class="small faint" { "Files you download appear here while they arrive." }
                }))
            }
            template id="dl-tpl-active" { (active_row(&template_active)) }
            template id="dl-tpl-done" { (done_row(&template_done)) }
            template id="dl-icons" {
                @for kind in FileKind::ALL {
                    span data-icon=(kind.key()) { (kind.icon()) }
                }
            }
        }
    };
    let html = ui::Doc::new(theme, "Downloads")
        .css(CSS)
        .script(SCRIPT)
        .keys(KEYS)
        .render(body);
    super::serve(super::kind::DOWNLOADS, html)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::THEMES;
    use crate::pages::tests::served_html;

    fn progress(state: DownloadState) -> DownloadProgress {
        DownloadProgress {
            id: 7,
            url: "https://files.example.test/big/archive.tar.gz?token=x".into(),
            suggested_name: "archive.tar.gz".into(),
            full_path: "/home/u/Downloads/archive.tar.gz".into(),
            received_bytes: 12_897_485, // 12.3 MB
            total_bytes: Some(48 * 1024 * 1024),
            speed: 2_202_009, // 2.1 MB/s
            percent: Some(25),
            state,
            mime: "application/gzip".into(),
            started_at: 0,
        }
    }

    #[test]
    fn bytes_format() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(1023), "1023 B");
        assert_eq!(format_bytes(1024), "1 KB");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(12_897_485), "12.3 MB");
        assert_eq!(format_bytes(48 * 1024 * 1024), "48 MB");
        assert_eq!(format_bytes(120 * 1024 * 1024 + 300_000), "120 MB");
        // Rounds up into the next unit instead of "1024 KB".
        assert_eq!(format_bytes(1024 * 1024 - 10), "1 MB");
        assert_eq!(format_bytes(3 * 1024 * 1024 * 1024), "3 GB");
        assert_eq!(format_bytes(u64::MAX), "16384 PB");
        assert_eq!(format_speed(2_202_009), "2.1 MB/s");
    }

    #[test]
    fn eta_format() {
        assert_eq!(format_eta(0), "1 s left");
        assert_eq!(format_eta(15), "15 s left");
        assert_eq!(format_eta(59), "59 s left");
        assert_eq!(format_eta(60), "1 min left");
        assert_eq!(format_eta(61), "2 min left");
        assert_eq!(format_eta(3599), "1 h left");
        assert_eq!(format_eta(3600), "1 h left");
        assert_eq!(format_eta(3600 + 5 * 60), "1 h 5 min left");
        assert_eq!(eta_secs(10, Some(100), 30), Some(3));
        assert_eq!(eta_secs(10, Some(100), 0), None);
        assert_eq!(eta_secs(10, None, 30), None);
        assert_eq!(eta_secs(100, Some(100), 30), None);
    }

    #[test]
    fn status_lines() {
        use DownloadState::*;
        let p = progress(InProgress);
        // 35.7 MB to go at 2.1 MB/s: 17.0… s, rounded up.
        assert_eq!(status_line(&p), "12.3 MB of 48 MB · 2.1 MB/s · 18 s left");
        let mut stalled = progress(InProgress);
        stalled.speed = 0;
        assert_eq!(status_line(&stalled), "12.3 MB of 48 MB");
        let mut unknown = progress(InProgress);
        unknown.total_bytes = None;
        unknown.percent = None;
        assert_eq!(status_line(&unknown), "12.3 MB · 2.1 MB/s");
        assert_eq!(status_line(&progress(Paused)), "Paused · 12.3 MB of 48 MB");
        assert_eq!(
            status_line(&progress(Interrupted)),
            "Failed · 12.3 MB of 48 MB"
        );
        assert_eq!(status_line(&progress(Complete)), "Done · 48 MB");
        assert_eq!(status_line(&progress(Canceled)), "Canceled");
    }

    #[test]
    fn file_kinds() {
        assert_eq!(FileKind::of("a.PDF", None), FileKind::Document);
        assert_eq!(FileKind::of("a.tar.gz", None), FileKind::Archive);
        assert_eq!(FileKind::of("p.jpeg", None), FileKind::Image);
        assert_eq!(FileKind::of("m.mkv", None), FileKind::Video);
        assert_eq!(FileKind::of("s.flac", None), FileKind::Audio);
        assert_eq!(FileKind::of("c.json", None), FileKind::Code);
        assert_eq!(FileKind::of("setup.sh", None), FileKind::Program);
        assert_eq!(FileKind::of("payload", None), FileKind::Program);
        // Extension wins over a generic server type.
        assert_eq!(
            FileKind::of("r.pdf", Some("application/octet-stream")),
            FileKind::Document
        );
        assert_eq!(FileKind::of("x.dat", Some("image/png")), FileKind::Image);
        assert_eq!(
            FileKind::of("x.dat", Some("application/octet-stream")),
            FileKind::Program
        );
        assert_eq!(FileKind::of("x.dat", None), FileKind::File);
        for kind in FileKind::ALL {
            assert_eq!(FileKind::by_key(kind.key()), kind);
        }
        assert_eq!(FileKind::by_key("nope"), FileKind::File);
    }

    #[test]
    fn source_hosts() {
        assert_eq!(source_host("https://a.test/x"), "a.test");
        assert_eq!(source_host("HTTP://a.test:8080/x"), "a.test:8080");
        assert_eq!(source_host("blob:https://a.test/uuid"), "a.test");
        assert_eq!(
            source_host("data:application/pdf;base64,AAAA"),
            "Embedded file"
        );
        assert_eq!(source_host("file:///etc/passwd"), "");
        assert_eq!(source_host(""), "");
    }

    #[test]
    fn live_json_shapes() {
        let records = [DownloadRecord {
            url: "https://a.test/r.pdf".into(),
            path: "/d/r.pdf".into(),
            mime: None,
        }];
        let mut unknown = progress(DownloadState::InProgress);
        unknown.total_bytes = None;
        let json = live_json(&[progress(DownloadState::Paused), unknown], Some(&records));
        let active = json["active"].as_array().unwrap();
        assert_eq!(active.len(), 2);
        assert_eq!(active[0]["id"], 7);
        assert_eq!(active[0]["state"], "paused");
        assert_eq!(active[0]["icon"], "archive");
        assert_eq!(active[0]["name"], "archive.tar.gz");
        assert_eq!(active[0]["host"], "files.example.test");
        assert_eq!(active[0]["percent"], 25);
        // No total: no percent, whatever CEF said.
        assert!(active[1]["percent"].is_null());
        assert_eq!(json["completed"][0]["index"], 0);
        assert_eq!(json["completed"][0]["icon"], "document");
        assert!(live_json(&[], None).get("completed").is_none());
    }

    #[test]
    fn page_renders_states_and_escapes_names() {
        let theme = THEMES[0];
        let mut evil = progress(DownloadState::InProgress);
        evil.full_path = "/d/<img src=x onerror=alert(1)>.zip".into();
        evil.url = "https://a.test\"><script>alert(1)</script>/f".into();
        let mut indeterminate = progress(DownloadState::InProgress);
        indeterminate.id = 8;
        indeterminate.total_bytes = None;
        let records = [DownloadRecord {
            url: "https://a.test/r.pdf".into(),
            path: "/d/\"><b>r.pdf".into(),
            mime: Some("application/pdf".into()),
        }];
        let html = served_html(&page_url(&theme, &records, &[evil, indeterminate]));
        assert!(
            !html.contains("<img src=x") && !html.contains("<b>r.pdf"),
            "{html}"
        );
        assert!(!html.contains("<script>alert(1)"));
        assert!(html.contains("data-has-active") && html.contains("data-has-done"));
        assert!(html.contains("href=\"download://pause/7\""));
        // Del never cancels a running download (no confirm() in OSR).
        assert!(!html.contains("data-delete=\"download://cancel/7\""));
        assert!(html.contains("style=\"width:25%\""));
        assert!(html.contains("class=\"progress indeterminate\""));
        assert!(html.contains("data-open=\"download://open/0\""));
        assert!(html.contains("href=\"download://reveal/0\""));
        assert!(html.contains("ui.on('downloads'"));

        let empty = served_html(&page_url(&theme, &[], &[]));
        assert!(!empty.contains("<main id=\"dl\" class=\"page\" data-has"));
        assert!(empty.contains("No downloads yet."));
    }

    #[test]
    fn paused_and_failed_rows_offer_the_right_controls() {
        let paused = active_row(&ActiveView::of(&progress(DownloadState::Paused))).into_string();
        assert!(paused.contains("class=\"icon-btn hidden\" data-act=\"pause\""));
        assert!(paused.contains("class=\"icon-btn\" data-act=\"resume\""));
        assert!(paused.contains("title=\"Cancel\""));
        let failed =
            active_row(&ActiveView::of(&progress(DownloadState::Interrupted))).into_string();
        assert!(failed.contains("title=\"Retry\""));
        assert!(failed.contains("title=\"Dismiss\""));
        assert!(failed.contains("data-delete=\"download://cancel/7\""));
        assert!(!paused.contains("data-delete"));
    }
}
