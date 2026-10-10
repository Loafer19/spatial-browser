// Drain cef-bridge PENDING_* queues every loop tick and refresh list pages.

use crate::app::{ContextMenuState, PendingSaveOffer};
use crate::autofill_bridge;
use crate::browser;
use crate::clipboard_bridge;
use crate::file_dialog::{self, PickKind, PickOptions};
use crate::hotkeys;
use crate::output::{GpuState, Rect, THEMES};
use crate::pages;
use crate::pages::context_menu::MenuContext;
use crate::persistence::{
    bookmarks::{self, Bookmark},
    bookmarks_chrome,
    downloads::{self, DownloadRecord},
    history::{self, HistoryEntry},
    settings::{self, AppSettings},
    typed_history,
    vault::{self, VaultEntry, VaultSession},
    vault_csv,
    workspaces::{self, WorkspaceRuntime, WorkspaceStore},
};
use crate::session::Session;
use crate::userscripts::{self, RunAt, UserScript};
use crate::userstyles::{self, UserStyle};
use cef::{ImplBrowser, ImplBrowserHost, ImplFrame};
use cef_bridge::{
    BookmarkAction, ContextAction, DownloadPageAction, FindAction, HistoryPageAction, PasswordAction,
    SettingsPageAction, UserscriptsPageAction, WorkspacePageAction, PENDING_BOOKMARK,
    PENDING_CONTEXT_ACTION, PENDING_DOWNLOADS, PENDING_FIND_ACTION, PENDING_FIND_RESULTS, PENDING_DOWNLOAD_ACTION, PENDING_HISTORY_ACTION,
    PENDING_LOAD_PROGRESS, PENDING_LOAD_START, PENDING_LOAD_STATE, PENDING_OMNIBOX,
    PENDING_PASSWORD_ACTION, PENDING_POPUPS, PENDING_SETTINGS_ACTION, PENDING_SWITCH,
    PENDING_USERSCRIPT_ACTION, PENDING_VISITS, PENDING_WORKSPACE_ACTION,
};
use std::cell::Cell;
use std::path::PathBuf;
use zeroize::{Zeroize, Zeroizing};

mod bookmark_page;
mod context;
mod download_page;
mod find;
mod history_page;
mod loads;
mod passwords;
mod scripts_page;
mod settings_page;
mod spawning;
mod workspace_page;

use passwords::*;

// Used from app.rs / hotkeys.rs.
pub(crate) use context::{dismiss_context_menu, open_context_menu, ContextHitRequest};
pub(crate) use passwords::request_vault_lock;
pub(crate) use settings_page::sync_blocklist_settings;

/// The app state `apply` works on, borrowed from `App` for one tick.
pub struct Stores<'a> {
    pub bookmarks: &'a mut Vec<Bookmark>,
    pub typed_history: &'a mut Vec<String>,
    pub downloads: &'a mut Vec<DownloadRecord>,
    pub history: &'a mut Vec<HistoryEntry>,
    pub workspaces: &'a mut WorkspaceStore,
    pub workspace_runtime: &'a mut WorkspaceRuntime,
    pub settings: &'a mut AppSettings,
    pub userscripts: &'a mut Vec<UserScript>,
    pub userstyles: &'a mut Vec<UserStyle>,
    pub vault: &'a mut Option<VaultSession>,
    pub generated_password: &'a mut Option<String>,
    pub pending_save_offer: &'a mut Option<PendingSaveOffer>,
    pub context_menu: &'a mut Option<ContextMenuState>,
    pub pending_context_hit: &'a mut Option<ContextHitRequest>,
}

/// Who sent a `scheme://` action. Built-in pages are ephemeral tiles on a
/// `spatial://page/…` URL whose token the compositor issued to that very
/// browser; anything else is web content, which can navigate to these
/// schemes too and must not drive the vault or settings.
enum ActionSource {
    Internal,
    Web { origin: String },
}

fn action_source(session: &Session, browser_id: i32) -> Option<ActionSource> {
    let page = session
        .pages()
        .iter()
        .find(|p| p.browser.identifier() == browser_id)?;
    let url = page.url();
    if is_internal_page(page.ephemeral, &url, browser_id) {
        Some(ActionSource::Internal)
    } else {
        Some(ActionSource::Web {
            origin: vault::normalize_origin(&url),
        })
    }
}

fn is_internal_page(ephemeral: bool, url: &str, browser_id: i32) -> bool {
    ephemeral && cef_bridge::spatial::issued_to(url, browser_id)
}

/// `Option::filter` predicate for actions only built-in pages may send.
fn from_internal(session: &Session, browser_id: i32, scheme: &str) -> bool {
    let internal = matches!(
        action_source(session, browser_id),
        Some(ActionSource::Internal)
    );
    if !internal {
        log::warn!("dropped {scheme} action from non-internal browser {browser_id}");
    }
    internal
}

/// Show `url` (a fresh `pages::serve` URL) in the built-in tile
/// `browser_id`: same CEF browser, same tile, the tile's own token — no
/// close/respawn flicker. Falls back to respawning at the same rect.
fn show_in_place(session: &mut Session, gpu: &GpuState, browser_id: i32, url: &str) {
    let Some(index) = session
        .pages()
        .iter()
        .position(|p| p.browser.identifier() == browser_id)
    else {
        return;
    };
    let page = &session.pages()[index];
    if page.ephemeral {
        if let Some(frame) = page.browser.main_frame() {
            if let Some(url) = cef_bridge::spatial::adopt(browser_id, url) {
                frame.load_url(Some(&url.as_str().into()));
                return;
            }
        }
    }
    let Some(rect) = session.close_at(index) else {
        return;
    };
    session.add_page(browser::spawn(gpu, &gpu.window, url, rect, true));
}

fn now_unix_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

/// Drain PENDING_* from App::tick (after CEF work, before pacing/redraw).
pub fn apply(session: &mut Session, gpu: &GpuState, mut s: Stores<'_>) {
    passwords::auto_lock(session, gpu, s.vault, s.generated_password, s.pending_save_offer);

    // Complete background file dialogs and Ctrl+V clipboard reads.
    bookmark_page::poll_bookmark_file_pick(session, gpu, s.bookmarks);
    passwords::poll_csv_file_pick(session, gpu, s.vault, s.generated_password);
    clipboard_bridge::deliver_paste(session);

    loads::apply_load_progress(session);

    bookmark_page::drain(session, gpu, s.bookmarks);

    spawning::drain_omnibox(session, gpu, s.typed_history);

    spawning::drain_switcher(session, gpu);

    download_page::drain(session, gpu, s.downloads);

    spawning::drain_new_pages(session, gpu);

    loads::inject_document_start(session, s.userscripts, s.userstyles);

    find::apply_find(session, gpu);
    passwords::apply_save_prompt(session, gpu, s.pending_save_offer);

    loads::inject_load_end(session, s.history, s.userscripts, s.userstyles);

    scripts_page::drain(session, gpu, s.userscripts, s.userstyles);

    passwords::drain(session, gpu, s.vault, s.generated_password, s.pending_save_offer);

    context::drain(session, gpu, &mut s);

    history_page::drain(session, gpu, s.history);

    workspace_page::drain(session, gpu, s.workspaces, s.workspace_runtime);

    settings_page::drain(session, gpu, s.settings);
}

#[cfg(test)]
mod tests {
    use super::loads::same_origin;
    use super::*;
    use crate::persistence::vault::test_entry;

    fn vault() -> VaultSession {
        VaultSession::for_test(vec![
            test_entry("mine", "https://www.site.test"),
            test_entry("bank", "https://bank.test"),
        ])
    }

    fn web(v: Option<&VaultSession>, action: PasswordAction) -> Option<PasswordAction> {
        web_password_action(v, "https://site.test", action)
    }

    #[test]
    fn web_query_uses_page_origin_not_requested_one() {
        let v = vault();
        let out = web(Some(&v), PasswordAction::Query { origin: "https://bank.test".into() });
        assert!(matches!(out, Some(PasswordAction::Query { origin }) if origin == "https://site.test"));
    }

    #[test]
    fn web_fill_only_for_own_origin() {
        let v = vault();
        assert!(web(Some(&v), PasswordAction::Fill { id: "bank".into() }).is_none());
        assert!(web(Some(&v), PasswordAction::Fill { id: "missing".into() }).is_none());
        assert!(matches!(
            web(Some(&v), PasswordAction::Fill { id: "mine".into() }),
            Some(PasswordAction::Fill { id }) if id == "mine"
        ));
    }

    #[test]
    fn web_cannot_save_or_answer_the_prompt() {
        let v = vault();
        let save = PasswordAction::Save {
            origin: "https://site.test".into(),
            username: "u".into(),
            password: "p".into(),
            id: None,
        };
        assert!(web(Some(&v), save).is_none());
        assert!(web(Some(&v), PasswordAction::Never { origin: String::new() }).is_none());
        assert!(web(Some(&v), PasswordAction::PromptSave).is_none());
        assert!(web(Some(&v), PasswordAction::PromptNever).is_none());
        assert!(web(Some(&v), PasswordAction::PromptDismiss).is_none());
        assert!(web(Some(&v), PasswordAction::CopyGenerated).is_none());
    }

    #[test]
    fn fill_only_into_entry_site() {
        assert!(fill_allowed("https://www.site.test", "https://site.test/login"));
        assert!(fill_allowed("https://site.test", "https://www.site.test/"));
        assert!(fill_allowed("http://192.168.1.1", "http://192.168.1.1/admin"));
        assert!(!fill_allowed("https://bank.test", "https://evil.test/"));
        assert!(!fill_allowed("https://bank.test", "https://bank.test.evil.test/"));
        assert!(!fill_allowed("https://bank.test", "https://bank.test@evil.test/"));
        assert!(!fill_allowed("https://bank.test", "https://login.bank.test/"));
        // Same host, but public HTTP: MITM-able, no autofill.
        assert!(!fill_allowed("https://bank.test", "http://bank.test/"));
        assert!(!fill_allowed("https://bank.test", "data:text/html,https://bank.test"));
        assert!(!fill_allowed("", "https://site.test/"));
    }

    #[test]
    fn web_save_offer_is_rebound() {
        let v = vault();
        let offer = PasswordAction::SaveOffer {
            origin: "https://bank.test".into(),
            username: "u".into(),
            password: "p".into(),
        };
        assert!(matches!(web(Some(&v), offer), Some(PasswordAction::SaveOffer { origin, .. }) if origin == "https://site.test"));
    }

    #[test]
    fn web_cannot_manage_vault() {
        let v = vault();
        let denied = [
            PasswordAction::Unlock { password: "x".repeat(8), create: true, confirm: Some("x".repeat(8)) },
            PasswordAction::Delete { id: "bank".into() },
            PasswordAction::Upsert {
                id: Some("bank".into()),
                origin: "https://bank.test".into(),
                username: "u".into(),
                password: "p".into(),
                email: None,
            },
            PasswordAction::RemoveNever { origin: "https://bank.test".into() },
            PasswordAction::Generate { length: 20, symbols: true },
            PasswordAction::ImportBrowse,
            PasswordAction::OpenList,
            PasswordAction::Lock,
        ];
        for action in denied {
            assert!(web(Some(&v), action).is_none());
        }
    }

    #[test]
    fn public_http_gets_no_autofill_local_http_does() {
        let v = VaultSession::for_test(vec![
            test_entry("plain", "http://site.test"),
            test_entry("router", "http://192.168.1.1"),
        ]);
        let http = |action| web_password_action(Some(&v), "http://site.test", action);
        assert!(http(PasswordAction::Query { origin: String::new() }).is_none());
        assert!(http(PasswordAction::Fill { id: "plain".into() }).is_none());
        assert!(http(PasswordAction::SaveOffer {
            origin: String::new(),
            username: "u".into(),
            password: "p".into(),
        })
        .is_none());

        let lan = |action| web_password_action(Some(&v), "http://192.168.1.1", action);
        assert!(matches!(
            lan(PasswordAction::Fill { id: "router".into() }),
            Some(PasswordAction::Fill { .. })
        ));
    }

    #[test]
    fn context_hit_needs_the_clicked_page_and_a_fresh_click() {
        let at = std::time::Instant::now();
        let request = ContextHitRequest { browser_id: 7, screen_pos: (1.0, 2.0), at };
        assert!(request.accepts(7, at + std::time::Duration::from_millis(50)));
        assert!(!request.accepts(8, at));
        assert!(!request.accepts(7, at + std::time::Duration::from_secs(2)));
    }

    #[test]
    fn same_origin_for_injection() {
        assert!(same_origin("https://a.test/x?y#z", "https://A.test/other"));
        assert!(!same_origin("https://a.test/", "https://b.test/"));
        assert!(!same_origin("http://a.test/", "https://a.test/"));
        assert!(same_origin("spatial://page/help/a", "spatial://page/settings/b"));
        assert!(!same_origin("spatial://page/help/a", "https://a.test/"));
        assert!(!same_origin("https://page/", "spatial://page/help/a"));
        assert!(!same_origin("about:blank", "javascript:void(0)"));
    }

    #[test]
    fn internal_only_with_issued_token_in_ephemeral_tile() {
        let url = pages::serve(pages::kind::SETTINGS, String::new());
        // Published but not yet loaded by any browser: nobody is trusted.
        assert!(!is_internal_page(true, &url, 31_337));
        assert!(cef_bridge::spatial::bind(&url, 31_337));
        assert!(is_internal_page(true, &url, 31_337));
        // Not ephemeral (a web tile that somehow shows it), or another browser.
        assert!(!is_internal_page(false, &url, 31_337));
        assert!(!is_internal_page(true, &url, 31_338));
        // Same token under another kind, unknown token, and the old data: path.
        let (_, token) = cef_bridge::spatial::parse_page_url(&url).unwrap();
        let token = token.to_string();
        assert!(!is_internal_page(true, &format!("spatial://page/vault-list/{token}"), 31_337));
        assert!(!is_internal_page(
            true,
            "spatial://page/settings/00000000000000000000000000000000",
            31_337
        ));
        assert!(!is_internal_page(true, "data:text/html;base64,PGgxPg==", 31_337));
        // Closing the tile revokes it.
        cef_bridge::spatial::forget_browser(31_337);
        assert!(!is_internal_page(true, &url, 31_337));
    }

    #[test]
    fn web_actions_are_silent_while_locked() {
        assert!(web(None, PasswordAction::Query { origin: String::new() }).is_none());
    }
}
