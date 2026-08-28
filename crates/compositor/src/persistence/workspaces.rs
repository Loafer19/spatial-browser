// Live workspace slots: always one active, lazy-loaded, autosaved.
// Default three virgin slots (1–3). Ctrl+N / HUD chips switch:
// save current → load target (empty canvas if never visited). `+` adds
// a fresh empty slot and switches to it.
//
// Within one process run, pages of a visited slot stay alive in
// `WorkspaceRuntime` when you leave it — switching back restores the
// same CEF browsers (no reload). Disk still stores URL/rect snapshots
// for the next launch.
//
// File: ~/.config/spatial-browser/workspaces.json (version 2).
// Legacy version-1 `{entries:[…]}` snapshots migrate into visited slots.

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
    #[allow(dead_code)]
    name: String,
    viewport: Viewport,
    theme: String,
    pages: Vec<WorkspacePage>,
}

fn path() -> PathBuf {
    let home = std::env::var_os("HOME").expect("HOME not set");
    PathBuf::from(home).join(".config/spatial-browser/workspaces.json")
}

impl WorkspaceStore {
    pub fn fresh() -> Self {
        Self {
            version: 2,
            active: 1,
            slots: (1..=DEFAULT_SLOT_COUNT)
                .map(|id| WorkspaceSlot {
                    id,
                    visited: false,
                    viewport: Viewport::default(),
                    theme: default_theme_name(),
                    pages: Vec::new(),
                })
                .collect(),
        }
    }

    pub fn load() -> Self {
        let Some(bytes) = std::fs::read(path()).ok() else {
            return Self::fresh();
        };
        if let Ok(store) = serde_json::from_slice::<WorkspaceStore>(&bytes) {
            if store.version >= 2 && !store.slots.is_empty() {
                return store.ensure_invariants();
            }
        }
        if let Ok(legacy) = serde_json::from_slice::<LegacyFile>(&bytes) {
            return Self::from_legacy(legacy).ensure_invariants();
        }
        Self::fresh()
    }

    fn from_legacy(legacy: LegacyFile) -> Self {
        let mut slots: Vec<WorkspaceSlot> = legacy
            .entries
            .into_iter()
            .enumerate()
            .map(|(i, e)| WorkspaceSlot {
                id: (i as u32) + 1,
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
        let path = path();
        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                log::warn!("workspaces save: couldn't create {parent:?}: {e}");
                return;
            }
        }
        match serde_json::to_vec_pretty(self) {
            Ok(bytes) => {
                if let Err(e) = std::fs::write(&path, bytes) {
                    log::warn!("workspaces save: couldn't write {path:?}: {e}");
                }
            }
            Err(e) => log::warn!("workspaces save: serialize failed: {e}"),
        }
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
            visited: false,
            viewport: Viewport::default(),
            theme: default_theme_name(),
            pages: Vec::new(),
        });
        id
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
        let Some(pos) = self.slots.iter().position(|s| s.id == id) else {
            return None;
        };
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

fn restore_parked(session: &mut Session, parked: ParkedSlot) {
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
            session.add_page(crate::browser::spawn(
                gpu,
                &gpu.window,
                &p.url,
                p.rect,
                false,
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
