// Vault + autofill: `password://` actions, the save-prompt tile, CSV import, locking.

use super::*;

thread_local! {
    /// Set by the Ctrl+Shift+L hotkey; applied at the top of the next `apply`.
    static VAULT_LOCK_REQUESTED: Cell<bool> = const { Cell::new(false) };
}

pub(crate) fn request_vault_lock() {
    VAULT_LOCK_REQUESTED.with(|c| c.set(true));
}

/// Web content only gets the autofill subset, always bound to its own
/// main-frame origin — never one it names in the URL. Saving is the
/// compositor's prompt tile's job (`PromptSave`), never the page's.
pub(super) fn web_password_action(
    vault: Option<&VaultSession>,
    origin: &str,
    action: PasswordAction,
) -> Option<PasswordAction> {
    // Every autofill action is a no-op while locked or on public HTTP;
    // stay silent (focus on any field sends a query).
    if !vault::is_autofill_origin(origin) {
        return None;
    }
    let v = vault?;
    let owned_by_origin = |id: &str| {
        v.get(id)
            .is_some_and(|e| vault::host_key(&e.origin) == vault::host_key(origin))
    };
    let origin = origin.to_string();
    match action {
        PasswordAction::Query { .. } => Some(PasswordAction::Query { origin }),
        PasswordAction::Fill { id } if owned_by_origin(&id) => Some(PasswordAction::Fill { id }),
        PasswordAction::SaveOffer {
            username, password, ..
        } => Some(PasswordAction::SaveOffer {
            origin,
            username,
            password,
        }),
        _ => {
            log::warn!("dropped password:// action from web origin {origin}");
            None
        }
    }
}

pub(super) fn handle_password_action(
    session: &mut Session,
    gpu: &GpuState,
    s: &mut Stores<'_>,
    browser_id: i32,
    source: ActionSource,
    action: PasswordAction,
) {
    let vault = &mut *s.vault;
    let generated_password = &mut *s.generated_password;
    let pending_save_offer = &mut *s.pending_save_offer;
    let internal = matches!(source, ActionSource::Internal);
    let action = match source {
        ActionSource::Internal => action,
        ActionSource::Web { origin } => {
            match web_password_action(vault.as_ref(), &origin, action) {
                Some(action) => action,
                None => return,
            }
        }
    };
    // Idle timer: built-in page actions and user clicks on web autofill UI
    // count as use; focus queries and save offers fire on their own.
    if let Some(v) = vault.as_ref() {
        if internal
            || matches!(
                action,
                PasswordAction::Fill { .. }
                    | PasswordAction::Save { .. }
                    | PasswordAction::Never { .. }
            )
        {
            v.touch();
        }
    }
    match action {
        PasswordAction::Unlock {
            password,
            create,
            confirm,
        } => {
            // Scrubbed on drop, on every path out of this arm. The URL CEF
            // parsed this from is beyond reach.
            let password = Zeroizing::new(password);
            let confirm = confirm.map(Zeroizing::new);
            let result = if create {
                if confirm.as_deref().map(String::as_str) != Some(password.as_str()) {
                    Err("Passwords do not match".into())
                } else if password.len() < 8 {
                    Err("Use at least 8 characters".into())
                } else {
                    vault::create(&password).map_err(|e| e.to_string())
                }
            } else {
                VaultSession::unlock(&password).map_err(|e| e.to_string())
            };
            match result {
                Ok(session_vault) => {
                    *vault = Some(session_vault);
                    // Turn the unlock tile into the list (opens one if gone).
                    refresh_passwords_page(
                        session,
                        gpu,
                        vault.as_ref(),
                        None,
                        None,
                        "saved",
                        browser_id,
                    );
                    // Re-inject autofill on open pages so query works immediately.
                    reinject_autofill(session);
                }
                Err(msg) => {
                    refresh_unlock_page(session, gpu, browser_id, create, Some(&msg));
                }
            }
        }
        PasswordAction::OpenList => {
            hotkeys::open_passwords(session, gpu, vault.as_ref());
        }
        PasswordAction::Lock => {
            lock_vault(
                session,
                gpu,
                vault,
                generated_password,
                pending_save_offer,
                Some(browser_id),
            );
        }
        PasswordAction::Query { origin } => {
            // Focus queries: stay silent if locked (don't yank open unlock UI).
            // Context-menu Fill still opens the vault via FillPassword.
            let Some(v) = vault.as_ref() else {
                return;
            };
            let matches = v.entries_for_origin(&origin);
            if let Some(page) = session
                .pages()
                .iter()
                .find(|p| p.browser.identifier() == browser_id)
            {
                if let Some(frame) = page.browser.main_frame() {
                    if let Some(js) = suggest_fill_js(&matches) {
                        frame.execute_java_script(Some(&js.as_str().into()), Some(&"".into()), 0);
                    }
                }
            }
        }
        PasswordAction::Fill { id } => {
            let Some(v) = vault.as_ref() else {
                return;
            };
            let Some(entry) = v.get(&id).cloned() else {
                return;
            };
            // Prefer the page that requested fill; else focused non-ephemeral.
            let target = session
                .pages()
                .iter()
                .find(|p| p.browser.identifier() == browser_id && !p.ephemeral)
                .or_else(|| session.pages().iter().rev().find(|p| !p.ephemeral));
            let Some(page) = target else {
                return;
            };
            // The page owns `__spatialAutofillFill` and can read whatever
            // it is handed — only ever fill the entry's own site.
            let page_url = page.url();
            if !fill_allowed(&entry.origin, &page_url) {
                log::warn!(
                    "refused fill of {} entry into {}",
                    entry.origin,
                    vault::normalize_origin(&page_url)
                );
                if internal {
                    let status = format!("Open {} first", entry.origin);
                    refresh_passwords_page(
                        session,
                        gpu,
                        vault.as_ref(),
                        generated_password.as_deref(),
                        Some(&status),
                        "saved",
                        browser_id,
                    );
                }
                return;
            }
            if let Some(frame) = page.browser.main_frame() {
                let js = fill_entry_js(&entry);
                frame.execute_java_script(Some(&js.as_str().into()), Some(&"".into()), 0);
            }
        }
        PasswordAction::SaveOffer {
            origin,
            username,
            password,
        } => {
            let Some(v) = vault.as_ref() else {
                return;
            };
            if origin.is_empty() || password.is_empty() || v.is_never_save(&origin) {
                return;
            }
            let existing = v.entries_for_origin(&origin);
            // Same credentials already stored → no prompt (also covers empty
            // username when the only distinguishing field is the password).
            if existing
                .iter()
                .any(|e| e.password == password && (username.is_empty() || e.username == username))
            {
                return;
            }
            let id = existing
                .iter()
                .find(|e| e.username == username)
                .map(|e| e.id.clone())
                .unwrap_or_default();
            close_save_prompt(session, pending_save_offer);
            let Some(target_rect) = session
                .pages()
                .iter()
                .find(|p| p.browser.identifier() == browser_id)
                .map(|p| p.rect)
            else {
                return;
            };
            let url = pages::save_prompt::page_url(
                &session.theme(),
                vault::host_key(&origin).as_str(),
                &username,
                !id.is_empty(),
            );
            let prompt = browser::spawn(
                gpu,
                &gpu.window,
                &url,
                crate::session::save_prompt_rect(target_rect, gpu.window.scale_factor()),
                true,
            );
            let prompt_id = prompt.browser.identifier();
            session.add_page(prompt);
            *pending_save_offer = Some(PendingSaveOffer {
                origin,
                username,
                password,
                id,
                target_id: browser_id,
                prompt_id,
            });
        }
        PasswordAction::PromptSave => {
            if let (Some(v), Some(offer)) = (vault.as_mut(), pending_save_offer.as_ref()) {
                let entry = VaultEntry {
                    id: offer.id.clone(),
                    origin: vault::normalize_origin(&offer.origin),
                    username: offer.username.clone(),
                    password: offer.password.clone(),
                    email: None,
                    address_line1: None,
                    city: None,
                    postal_code: None,
                    country: None,
                    notes: None,
                    updated_at: vault::now_unix(),
                };
                if let Err(e) = v.upsert(entry) {
                    log::warn!("vault save: {e}");
                }
            }
            close_save_prompt(session, pending_save_offer);
        }
        PasswordAction::PromptNever => {
            if let (Some(v), Some(offer)) = (vault.as_mut(), pending_save_offer.as_ref()) {
                let _ = v.add_never_save(&offer.origin);
            }
            close_save_prompt(session, pending_save_offer);
        }
        PasswordAction::PromptDismiss => {
            close_save_prompt(session, pending_save_offer);
        }
        PasswordAction::CopyGenerated => {
            if let Some(pw) = generated_password.as_deref() {
                cef_bridge::clipboard::copy_text(pw, true);
            }
        }
        PasswordAction::Save {
            origin,
            username,
            password,
            id,
        } => {
            let Some(v) = vault.as_mut() else {
                return;
            };
            let entry = VaultEntry {
                id: id.unwrap_or_default(),
                origin: vault::normalize_origin(&origin),
                username,
                password,
                email: None,
                address_line1: None,
                city: None,
                postal_code: None,
                country: None,
                notes: None,
                updated_at: vault::now_unix(),
            };
            if let Err(e) = v.upsert(entry) {
                log::warn!("vault save: {e}");
            }
            close_save_prompt(session, pending_save_offer);
        }
        PasswordAction::Never { origin } => {
            if let Some(v) = vault.as_mut() {
                let _ = v.add_never_save(&origin);
            }
            close_save_prompt(session, pending_save_offer);
        }
        PasswordAction::Delete { id } => {
            if let Some(v) = vault.as_mut() {
                let _ = v.remove(&id);
            }
            refresh_passwords_page(
                session,
                gpu,
                vault.as_ref(),
                generated_password.as_deref(),
                None,
                "saved",
                browser_id,
            );
        }
        PasswordAction::Upsert {
            id,
            origin,
            username,
            password,
            email,
        } => {
            if let Some(v) = vault.as_mut() {
                let entry = VaultEntry {
                    id: id.unwrap_or_default(),
                    origin: vault::normalize_origin(&origin),
                    username,
                    password,
                    email,
                    address_line1: None,
                    city: None,
                    postal_code: None,
                    country: None,
                    notes: None,
                    updated_at: vault::now_unix(),
                };
                let _ = v.upsert(entry);
            }
            refresh_passwords_page(
                session,
                gpu,
                vault.as_ref(),
                generated_password.as_deref(),
                None,
                "saved",
                browser_id,
            );
        }
        PasswordAction::RemoveNever { origin } => {
            if let Some(v) = vault.as_mut() {
                let _ = v.remove_never_save(&origin);
            }
            refresh_passwords_page(
                session,
                gpu,
                vault.as_ref(),
                generated_password.as_deref(),
                None,
                "never",
                browser_id,
            );
        }
        PasswordAction::Generate { length, symbols } => {
            if let Some(mut old) =
                generated_password.replace(vault::generate_password(length, symbols))
            {
                old.zeroize();
            }
            refresh_passwords_page(
                session,
                gpu,
                vault.as_ref(),
                generated_password.as_deref(),
                None,
                "generator",
                browser_id,
            );
        }
        PasswordAction::ImportBrowse => {
            if vault.is_none() {
                refresh_passwords_page(
                    session,
                    gpu,
                    None,
                    generated_password.as_deref(),
                    Some("Unlock the vault first"),
                    "add",
                    browser_id,
                );
            } else {
                start_csv_file_dialog(browser_id);
            }
        }
        PasswordAction::Tab { id } => {
            refresh_passwords_page(
                session,
                gpu,
                vault.as_ref(),
                generated_password.as_deref(),
                None,
                pages::passwords_list::normalize_tab(&id),
                browser_id,
            );
        }
    }
}

pub(super) fn start_csv_file_dialog(browser_id: i32) {
    let downloads = std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join("Downloads"))
        .filter(|p| p.is_dir());
    file_dialog::start(
        PickKind::PasswordsCsv,
        browser_id,
        PickOptions {
            title: "Import passwords CSV".into(),
            start_dir: downloads,
            file_name: None,
            filters: vec![("CSV".into(), vec!["csv".into()])],
        },
    );
}

pub(super) fn poll_csv_file_pick(
    session: &mut Session,
    gpu: &GpuState,
    vault: &mut Option<VaultSession>,
    generated_password: &mut Option<String>,
) {
    let Some((browser_id, path)) = file_dialog::poll(PickKind::PasswordsCsv) else {
        return;
    };

    let status = match (path, vault.as_mut()) {
        (None, _) => "Import cancelled".to_string(),
        (_, None) => "Unlock the vault first".to_string(),
        (Some(path), Some(v)) => match vault_csv::import_path(v, &path.to_string_lossy()) {
            Ok(stats) => format!(
                "Import done: {} added, {} updated, {} skipped",
                stats.added, stats.updated, stats.skipped
            ),
            Err(e) => format!("Import failed: {e}"),
        },
    };
    refresh_passwords_page(
        session,
        gpu,
        vault.as_ref(),
        generated_password.as_deref(),
        Some(&status),
        "add",
        browser_id,
    );
}

/// Always suggest (picker) — never silent-fill — when ≥1 match.
pub(super) fn suggest_fill_js(matches: &[&VaultEntry]) -> Option<String> {
    if matches.is_empty() {
        return None;
    }
    let items: Vec<String> = matches
        .iter()
        .map(|e| {
            format!(
                "{{id:{},username:{}}}",
                serde_json::to_string(&e.id).unwrap_or_default(),
                serde_json::to_string(&e.username).unwrap_or_default()
            )
        })
        .collect();
    Some(format!(
        "window.__spatialAutofillShowPicker && window.__spatialAutofillShowPicker([{}]);",
        items.join(",")
    ))
}

/// Drop the pending offer (wiping it) and close its prompt tile.
pub(super) fn close_save_prompt(session: &mut Session, offer: &mut Option<PendingSaveOffer>) {
    let Some(offer) = offer.take() else { return };
    if let Some(index) = session
        .pages()
        .iter()
        .position(|p| p.browser.identifier() == offer.prompt_id)
    {
        session.close_at(index);
    }
}

/// Keep the save prompt above its page; drop it when either tile is gone.
pub(super) fn apply_save_prompt(
    session: &mut Session,
    gpu: &GpuState,
    offer: &mut Option<PendingSaveOffer>,
) {
    let Some((target_id, prompt_id)) = offer.as_ref().map(|o| (o.target_id, o.prompt_id)) else {
        return;
    };
    let rect_of = |id: i32| {
        session
            .pages()
            .iter()
            .find(|p| p.browser.identifier() == id)
            .map(|p| p.rect)
    };
    match (rect_of(target_id), rect_of(prompt_id)) {
        (Some(target), Some(_)) => {
            let scale = gpu.window.scale_factor();
            session.place_page(
                prompt_id,
                crate::session::save_prompt_rect(target, scale),
                scale,
            );
        }
        _ => close_save_prompt(session, offer),
    }
}

/// A vault entry may only be filled into a page on its own site (same
/// `host_key`) that is also an autofill origin (HTTPS / local HTTP).
pub(super) fn fill_allowed(entry_origin: &str, page_url: &str) -> bool {
    let key = vault::host_key(entry_origin);
    !key.is_empty() && vault::is_autofill_origin(page_url) && key == vault::host_key(page_url)
}

pub(super) fn fill_entry_js(entry: &VaultEntry) -> String {
    let obj = serde_json::json!({
        "username": entry.username,
        "password": entry.password,
        "email": entry.email,
        "address_line1": entry.address_line1,
        "city": entry.city,
        "postal_code": entry.postal_code,
        "country": entry.country,
    });
    format!(
        "window.__spatialAutofillFill && window.__spatialAutofillFill({});",
        obj
    )
}

pub(super) fn reinject_autofill(session: &Session) {
    let bridge = autofill_bridge::script(&session.theme());
    for page in session.pages() {
        if page.ephemeral {
            continue;
        }
        if let Some(frame) = page.browser.main_frame() {
            frame.execute_java_script(Some(&bridge.as_str().into()), Some(&"".into()), 0);
        }
    }
}

pub(super) fn refresh_unlock_page(
    session: &mut Session,
    gpu: &GpuState,
    browser_id: i32,
    create: bool,
    error: Option<&str>,
) {
    let url = pages::passwords_list::unlock_url(&session.theme(), create, error);
    show_in_place(session, gpu, browser_id, &url);
}

/// Drop the unlocked vault (key and entries zeroize on drop) and the secrets
/// cached beside it, then swap every open passwords list — plus `sender`, the
/// page that asked — for the unlock form.
pub(super) fn lock_vault(
    session: &mut Session,
    gpu: &GpuState,
    vault: &mut Option<VaultSession>,
    generated_password: &mut Option<String>,
    pending_save_offer: &mut Option<PendingSaveOffer>,
    sender: Option<i32>,
) {
    if vault.take().is_some() {
        log::info!("vault locked");
    }
    if let Some(mut pw) = generated_password.take() {
        pw.zeroize();
    }
    close_save_prompt(session, pending_save_offer);
    let mut list_pages: Vec<i32> = session
        .pages()
        .iter()
        .filter(|p| p.ephemeral && pages::passwords_list::is_list_url(&p.url()))
        .map(|p| p.browser.identifier())
        .collect();
    if let Some(id) = sender.filter(|id| !list_pages.contains(id)) {
        list_pages.push(id);
    }
    for id in list_pages {
        refresh_passwords_page(session, gpu, None, None, None, "saved", id);
    }
    // Open lists now hold the unlock form; wipe any list HTML still around
    // (e.g. published for a spawn that never happened).
    cef_bridge::spatial::remove_kind(pages::kind::VAULT_LIST);
}

pub(super) fn refresh_passwords_page(
    session: &mut Session,
    gpu: &GpuState,
    vault: Option<&VaultSession>,
    generated: Option<&str>,
    status: Option<&str>,
    active_tab: &str,
    browser_id: i32,
) {
    if !session
        .pages()
        .iter()
        .any(|p| p.browser.identifier() == browser_id)
    {
        // Action came from a normal page (e.g. generate from nowhere) — open list.
        hotkeys::open_passwords(session, gpu, vault);
        return;
    }
    let url = match vault {
        Some(v) => pages::passwords_list::page_url(
            &session.theme(),
            &v.data.entries,
            &v.data.never_save,
            generated,
            status,
            active_tab,
        ),
        None => pages::passwords_list::unlock_url(&session.theme(), !vault::exists(), None),
    };
    show_in_place(session, gpu, browser_id, &url);
}

/// Lock before anything else this frame can read the vault: Ctrl+Shift+L,
/// the Lock button (via `request_vault_lock`) or the idle timeout.
pub(super) fn auto_lock(
    session: &mut Session,
    gpu: &GpuState,
    vault: &mut Option<VaultSession>,
    generated_password: &mut Option<String>,
    pending_save_offer: &mut Option<PendingSaveOffer>,
) {
    // Lock before anything below can read the vault this frame.
    let lock_requested = VAULT_LOCK_REQUESTED.with(|c| c.replace(false));
    let idle = vault
        .as_ref()
        .is_some_and(|v| v.idle_for() >= vault::AUTO_LOCK_AFTER);
    if lock_requested || idle {
        lock_vault(
            session,
            gpu,
            vault,
            generated_password,
            pending_save_offer,
            None,
        );
    }
}

/// `password://` from built-in pages, tiles and the web autofill bridge.
pub(super) fn drain(session: &mut Session, gpu: &GpuState, s: &mut Stores<'_>) {
    if let Some((browser_id, action)) =
        PENDING_PASSWORD_ACTION.with_borrow_mut(|pending| pending.take())
    {
        if let Some(source) = action_source(session, browser_id) {
            handle_password_action(session, gpu, s, browser_id, source, action);
        }
    }
}
