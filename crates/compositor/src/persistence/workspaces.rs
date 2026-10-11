// Live workspace slots (workspaces.json v2). Switch parks browsers in RAM;
// disk holds URL/rect for next launch. Legacy v1 snapshots migrate in.

use crate::browser::Page;
use crate::output::{Rect, Theme, THEMES};
use crate::session::Session;
use crate::viewport::Viewport;
use cef::{ImplBrowser, ImplBrowserHost};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

pub const DEFAULT_SLOT_COUNT: u32 = 3;

#[derive(Clone, Serialize, Deserialize)]
pub struct WorkspacePage {
    pub url: String,
    pub rect: Rect,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct WorkspaceSlot {
    pub id: u32,
    /// User-given name; empty = "Workspace <id>" (see `label`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// False until first activated — no pages spawned until then.
    #[serde(default)]
    pub visited: bool,
    #[serde(default)]
    pub viewport: Viewport,
    #[serde(default = "default_theme_name")]
    pub theme: String,
    #[serde(default)]
    pub pages: Vec<WorkspacePage>,
}

/// Longest workspace name kept (chars); longer renames are cut.
pub const MAX_NAME_CHARS: usize = 40;

impl WorkspaceSlot {
    /// Display name: the user's name, else "Workspace <id>".
    pub fn label(&self) -> String {
        if self.name.is_empty() {
            format!("Workspace {}", self.id)
        } else {
            self.name.clone()
        }
    }
}

/// A rename as typed → stored name: control characters dropped, runs of
/// whitespace collapsed, trimmed, capped at `MAX_NAME_CHARS`.
fn clean_name(raw: &str) -> String {
    raw.split(|c: char| c.is_whitespace() || c.is_control())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(MAX_NAME_CHARS)
        .collect::<String>()
        .trim_end()
        .to_string()
}

fn default_theme_name() -> String {
    THEMES[0].name.to_string()
}

#[derive(Clone, Serialize, Deserialize)]
pub struct WorkspaceStore {
    #[serde(default = "default_version")]
    pub version: u32,
    pub active: u32,
    pub slots: Vec<WorkspaceSlot>,
}

fn default_version() -> u32 {
    2
}

/// Legacy v1 file shape (named snapshots).
#[derive(Deserialize)]
struct LegacyFile {
    entries: Vec<LegacyEntry>,
}

#[derive(Deserialize)]
struct LegacyEntry {
    name: String,
    viewport: Viewport,
    theme: String,
    pages: Vec<WorkspacePage>,
}

fn path() -> PathBuf {
    crate::persistence::config_path("workspaces.json")
}

impl WorkspaceStore {
    pub fn fresh() -> Self {
        Self {
            version: 2,
            active: 1,
            slots: (1..=DEFAULT_SLOT_COUNT)
                .map(|id| WorkspaceSlot {
                    id,
                    name: String::new(),
                    visited: false,
                    viewport: Viewport::default(),
                    theme: default_theme_name(),
                    pages: Vec::new(),
                })
                .collect(),
        }
    }

    pub fn load() -> Self {
        super::store::load_with(&path(), "workspaces", |bytes| {
            if let Ok(store) = serde_json::from_slice::<WorkspaceStore>(bytes) {
                if store.version >= 2 && !store.slots.is_empty() {
                    return Some(store.ensure_invariants());
                }
            }
            serde_json::from_slice::<LegacyFile>(bytes)
                .ok()
                .map(|legacy| Self::from_legacy(legacy).ensure_invariants())
        })
        .unwrap_or_else(Self::fresh)
    }

    fn from_legacy(legacy: LegacyFile) -> Self {
        let mut slots: Vec<WorkspaceSlot> = legacy
            .entries
            .into_iter()
            .enumerate()
            .map(|(i, e)| WorkspaceSlot {
                id: (i as u32) + 1,
                name: clean_name(&e.name),
                visited: true,
                viewport: e.viewport,
                theme: e.theme,
                pages: e.pages,
            })
            .collect();
        while slots.len() < DEFAULT_SLOT_COUNT as usize {
            let id = slots.len() as u32 + 1;
            slots.push(WorkspaceSlot {
                id,
                name: String::new(),
                visited: false,
                viewport: Viewport::default(),
                theme: default_theme_name(),
                pages: Vec::new(),
            });
        }
        Self {
            version: 2,
            active: 1,
            slots,
        }
    }

    fn ensure_invariants(mut self) -> Self {
        self.version = 2;
        if self.slots.is_empty() {
            return Self::fresh();
        }
        self.slots.sort_by_key(|s| s.id);
        // Re-number contiguous from 1 if needed
        for (i, slot) in self.slots.iter_mut().enumerate() {
            slot.id = (i as u32) + 1;
        }
        if !self.slots.iter().any(|s| s.id == self.active) {
            self.active = self.slots[0].id;
        }
        self
    }

    pub fn save(&self) {
        super::store::save_json_bg(&path(), "workspaces", self, super::store::Backup::Keep);
    }

    pub fn active_slot(&self) -> &WorkspaceSlot {
        self.slots
            .iter()
            .find(|s| s.id == self.active)
            .expect("active slot missing")
    }

    pub fn active_slot_mut(&mut self) -> &mut WorkspaceSlot {
        let id = self.active;
        self.slots
            .iter_mut()
            .find(|s| s.id == id)
            .expect("active slot missing")
    }

    pub fn slot_count(&self) -> u32 {
        self.slots.len() as u32
    }

    /// Snapshot live session into the active slot and mark visited.
    pub fn capture_active_from_session(&mut self, session: &Session) {
        let slot = self.active_slot_mut();
        slot.visited = true;
        // Pre-Ctrl+Space geometry — see Session::layout_viewport / Page::layout_rect.
        slot.viewport = session.layout_viewport();
        slot.theme = session.theme().name.to_string();
        slot.pages = session
            .pages()
            .iter()
            .filter(|p| !p.ephemeral)
            .map(|p| WorkspacePage {
                url: p.url(),
                rect: p.layout_rect(),
            })
            .collect();
    }

    /// Add a new empty slot and return its id (does not switch).
    pub fn add_fresh_slot(&mut self) -> u32 {
        let id = self.slots.iter().map(|s| s.id).max().unwrap_or(0) + 1;
        self.slots.push(WorkspaceSlot {
            id,
            name: String::new(),
            visited: false,
            viewport: Viewport::default(),
            theme: default_theme_name(),
            pages: Vec::new(),
        });
        id
    }

    /// Ensure contiguous slots `1..=id` exist (append empty unvisited
    /// ones). No-op when `id` is already present.
    pub fn ensure_slot(&mut self, id: u32) {
        if id == 0 {
            return;
        }
        while self.slot_count() < id {
            self.add_fresh_slot();
        }
    }

    /// Rename the slot at list `index`; a blank name restores the default
    /// label. Returns whether anything changed (caller saves).
    pub fn rename(&mut self, index: usize, raw: &str) -> bool {
        let name = clean_name(raw);
        match self.slots.get_mut(index) {
            Some(slot) if slot.name != name => {
                slot.name = name;
                true
            }
            _ => false,
        }
    }

    pub fn slot(&self, id: u32) -> Option<&WorkspaceSlot> {
        self.slots.iter().find(|s| s.id == id)
    }

    /// Remove a slot by id. Keeps at least one; renumbers 1..N afterward.
    /// Returns the new active id when the removed slot was active.
    pub fn remove_slot(&mut self, id: u32) -> Option<u32> {
        if self.slots.len() <= 1 {
            return None;
        }
        let pos = self.slots.iter().position(|s| s.id == id)?;
        let was_active = self.active == id;
        self.slots.remove(pos);
        for (i, slot) in self.slots.iter_mut().enumerate() {
            slot.id = (i as u32) + 1;
        }
        if was_active {
            let idx = pos.min(self.slots.len() - 1);
            self.active = self.slots[idx].id;
            Some(self.active)
        } else {
            if self.active > id {
                self.active -= 1;
            }
            None
        }
    }
}

/// In-process parked browsers for slots that aren't currently shown.
#[derive(Default)]
pub struct WorkspaceRuntime {
    parked: HashMap<u32, ParkedSlot>,
}

struct ParkedSlot {
    pages: Vec<Page>,
    viewport: Viewport,
    theme: String,
}

impl WorkspaceRuntime {
    pub fn new() -> Self {
        Self::default()
    }

    fn park(&mut self, id: u32, session: &mut Session) {
        let viewport = session.viewport();
        let theme = session.theme().name.to_string();
        let mut pages = session.take_pages();
        // Utility overlays shouldn't follow the slot into the background.
        let mut kept = Vec::with_capacity(pages.len());
        for page in pages.drain(..) {
            if page.ephemeral {
                close_browser(&page);
            } else {
                // Hidden: CEF stops painting and Chromium throttles timers/rAF.
                set_hidden(&page, true);
                kept.push(page);
            }
        }
        self.parked.insert(
            id,
            ParkedSlot {
                pages: kept,
                viewport,
                theme,
            },
        );
    }

    fn take(&mut self, id: u32) -> Option<ParkedSlot> {
        self.parked.remove(&id)
    }

    /// Close parked browsers for `id` and drop the entry.
    pub fn discard(&mut self, id: u32) {
        if let Some(parked) = self.parked.remove(&id) {
            for page in parked.pages {
                close_browser(&page);
            }
        }
    }

    /// After `WorkspaceStore::remove_slot` renumbers ids, shift parked keys
    /// the same way (`id > removed` → `id - 1`).
    pub fn reindex_after_remove(&mut self, removed_id: u32) {
        let mut next = HashMap::with_capacity(self.parked.len());
        for (id, parked) in self.parked.drain() {
            let new_id = if id > removed_id { id - 1 } else { id };
            next.insert(new_id, parked);
        }
        self.parked = next;
    }
}

fn close_browser(page: &Page) {
    if let Some(host) = page.browser.host() {
        host.close_browser(true as _);
    }
}

fn close_session_browsers(session: &mut Session) {
    for page in session.take_pages() {
        close_browser(&page);
    }
}

fn set_hidden(page: &Page, hidden: bool) {
    if let Some(host) = page.browser.host() {
        host.was_hidden(hidden as _);
    }
}

fn restore_parked(session: &mut Session, parked: ParkedSlot) {
    for page in &parked.pages {
        set_hidden(page, false);
    }
    session.set_theme(theme_from_name(&parked.theme));
    session.set_viewport(parked.viewport);
    session.install_pages(parked.pages);
}

/// Spawn from disk snapshot (or empty canvas). Closes whatever is currently
/// in the session first — for cold start / first visit this process.
pub fn apply_slot_to_session(
    session: &mut Session,
    gpu: &crate::output::GpuState,
    slot: &WorkspaceSlot,
) {
    close_session_browsers(session);
    let theme = theme_from_name(&slot.theme);
    session.set_theme(theme);
    if slot.visited {
        session.set_viewport(slot.viewport);
        for p in &slot.pages {
            session.add_page(crate::browser::spawn_deferred(
                gpu,
                &gpu.window,
                &p.url,
                p.rect,
            ));
        }
    } else {
        session.set_viewport(Viewport::default());
    }
}

fn activate_slot(
    store: &mut WorkspaceStore,
    runtime: &mut WorkspaceRuntime,
    session: &mut Session,
    gpu: &crate::output::GpuState,
    id: u32,
) {
    store.active = id;
    if let Some(parked) = runtime.take(id) {
        // Still in RAM from earlier this run — put the same browsers back.
        close_session_browsers(session); // should already be empty after park
        restore_parked(session, parked);
    } else {
        let slot = store.active_slot().clone();
        apply_slot_to_session(session, gpu, &slot);
    }
    store.active_slot_mut().visited = true;
    store.save();
}

/// Snapshot + park the active slot, then show `id` (from RAM if parked).
pub fn switch_to(
    store: &mut WorkspaceStore,
    runtime: &mut WorkspaceRuntime,
    session: &mut Session,
    gpu: &crate::output::GpuState,
    id: u32,
) {
    if store.active == id || store.slot(id).is_none() {
        return;
    }
    let from = store.active;
    store.capture_active_from_session(session);
    runtime.park(from, session);
    activate_slot(store, runtime, session, gpu, id);
}

/// Snapshot + park current, append an empty slot, switch to it.
pub fn add_and_switch(
    store: &mut WorkspaceStore,
    runtime: &mut WorkspaceRuntime,
    session: &mut Session,
    gpu: &crate::output::GpuState,
) -> u32 {
    let from = store.active;
    store.capture_active_from_session(session);
    runtime.park(from, session);
    let id = store.add_fresh_slot();
    activate_slot(store, runtime, session, gpu, id);
    id
}

/// Move the focused (topmost non-ephemeral) page to slot `id`, creating
/// empty slots up to `id` when needed, then follow into that slot.
/// Keeps the CEF browser alive (park/restore path). No-op when there is
/// no content page to move.
pub fn move_focused_to_slot(
    store: &mut WorkspaceStore,
    runtime: &mut WorkspaceRuntime,
    session: &mut Session,
    gpu: &crate::output::GpuState,
    id: u32,
) {
    if id == 0 {
        return;
    }
    store.ensure_slot(id);
    let scale = gpu.window.scale_factor();
    let Some(page) = session.take_topmost_content_page(scale) else {
        return;
    };
    if store.active == id {
        session.add_page(page);
        store.capture_active_from_session(session);
        store.save();
        return;
    }
    let from = store.active;
    store.capture_active_from_session(session);
    runtime.park(from, session);
    activate_slot(store, runtime, session, gpu, id);
    session.add_page(page);
    store.capture_active_from_session(session);
    store.save();
}

/// Delete a slot; closes its parked (or live) browsers. If it was active,
/// switches to the store's new active slot (RAM restore when possible).
pub fn delete_slot(
    store: &mut WorkspaceStore,
    runtime: &mut WorkspaceRuntime,
    session: &mut Session,
    gpu: &crate::output::GpuState,
    id: u32,
) -> bool {
    if store.slots.len() <= 1 || store.slot(id).is_none() {
        return false;
    }
    let was_active = store.active == id;
    if was_active {
        close_session_browsers(session);
    }
    runtime.discard(id);
    let switched = store.remove_slot(id);
    runtime.reindex_after_remove(id);
    store.save();
    if switched.is_some() {
        let new_id = store.active;
        // Active was deleted — show whatever is now active.
        if let Some(parked) = runtime.take(new_id) {
            restore_parked(session, parked);
        } else {
            let slot = store.active_slot().clone();
            apply_slot_to_session(session, gpu, &slot);
            store.active_slot_mut().visited = true;
            store.save();
        }
    }
    true
}

pub fn theme_from_name(name: &str) -> Theme {
    THEMES
        .iter()
        .find(|t| t.name == name)
        .copied()
        .unwrap_or(THEMES[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rename_cleans_and_caps_names() {
        let mut store = WorkspaceStore::fresh();
        assert!(store.rename(0, "  Client\tA \n\u{7}ops  "));
        assert_eq!(store.slots[0].name, "Client A ops");
        assert_eq!(store.slots[0].label(), "Client A ops");
        assert!(!store.rename(0, "Client A ops"), "unchanged");
        assert!(store.rename(1, &"x".repeat(100)));
        assert_eq!(store.slots[1].name.chars().count(), MAX_NAME_CHARS);
        assert!(store.rename(0, "   "));
        assert_eq!(store.slots[0].label(), "Workspace 1");
        assert!(!store.rename(99, "nope"), "out of range");
    }

    #[test]
    fn names_round_trip_and_stay_optional_on_disk() {
        let mut store = WorkspaceStore::fresh();
        store.rename(1, "Ops");
        let json = serde_json::to_string(&store).expect("serializes");
        assert_eq!(json.matches("\"name\"").count(), 1, "{json}");
        let back: WorkspaceStore = serde_json::from_str(&json).expect("parses");
        assert_eq!(back.slots[1].name, "Ops");
        assert_eq!(back.slots[0].name, "");
    }
}
