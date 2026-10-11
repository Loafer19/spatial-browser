// Settings page actions and blocklist/filter sync.

use super::*;

/// Push blocklist/filter settings into cef-bridge live state (IO-thread statics).
pub(crate) fn sync_blocklist_settings(settings: &AppSettings) {
    let master = settings.ad_block_enabled;
    let network_on = master && settings.filter_network_enabled;
    let cosmetic_on = master && settings.filter_cosmetic_enabled;
    let scriptlets_on = master && settings.filter_scriptlets_enabled;
    // Engine needed for any layer that reads EasyList/EasyPrivacy rules.
    let need_engine = network_on || cosmetic_on || scriptlets_on;

    cef_bridge::set_enabled(network_on);
    cef_bridge::set_peter_lowe_enabled(settings.filter_lists.peter_lowe);
    cef_bridge::set_custom_hosts(settings.custom_blocked_hosts.clone());
    cef_bridge::set_cosmetic_enabled(cosmetic_on);
    cef_bridge::set_scriptlets_enabled(scriptlets_on);

    // Background rebuild; a no-op unless the EasyList config itself changed
    // (custom hosts / Peter Lowe toggles above never reparse the lists).
    let filters_dir = ensure_filter_lists_dir();
    cef_bridge::rebuild_filter_engine(&cef_bridge::FilterEngineConfig {
        easylist: need_engine && settings.filter_lists.easylist,
        easyprivacy: need_engine && settings.filter_lists.easyprivacy,
        load_scriptlets: scriptlets_on,
        filters_dir,
    });
}

/// Prefer `~/.config/spatial-browser/filters/`; seed from bundle/repo on first run.
pub(super) fn ensure_filter_lists_dir() -> std::path::PathBuf {
    let dest = crate::persistence::config_path("filters");
    if let Err(e) = std::fs::create_dir_all(&dest) {
        log::warn!("filters dir {}: {e}", dest.display());
        return dest;
    }
    for name in ["easylist.txt", "easyprivacy.txt", "scriptlets.js"] {
        let dest_file = dest.join(name);
        if dest_file.exists() {
            continue;
        }
        let mut candidates: Vec<std::path::PathBuf> = Vec::new();
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                candidates.push(dir.join("filters").join(name));
            }
        }
        candidates.push(std::path::PathBuf::from("data/filters").join(name));
        candidates.push(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../data/filters")
                .join(name),
        );
        for src in candidates {
            if src.is_file() {
                match std::fs::copy(&src, &dest_file) {
                    Ok(_) => {
                        log::info!("seeded filter list {} ← {}", name, src.display());
                        break;
                    }
                    Err(e) => log::warn!("copy {} → {}: {e}", src.display(), dest_file.display()),
                }
            }
        }
        if !dest_file.exists() {
            log::warn!(
                "filter list {name} missing — enable EasyList files under {}",
                dest.display()
            );
        }
    }
    dest
}

/// Like `refresh_bookmarks_page`, for settings.
pub(super) fn refresh_settings_page(
    session: &mut Session,
    gpu: &GpuState,
    settings: &AppSettings,
    browser_id: i32,
) {
    let url = pages::settings_list::page_url(&session.theme(), settings);
    show_in_place(session, gpu, browser_id, &url);
}

/// `settings://` from the settings page.
pub(super) fn drain(session: &mut Session, gpu: &GpuState, stores: &mut Stores<'_>) {
    // settings://…
    if let Some((browser_id, action)) = PENDING_SETTINGS_ACTION
        .with_borrow_mut(|pending| pending.take())
        .filter(|(id, _)| from_internal(session, *id, "settings://"))
    {
        // `Open` hands the stores to `commands::run`; everything else edits settings.
        if let SettingsPageAction::Open(page) = &action {
            let command = match page.as_str() {
                "scripts" => crate::commands::Command::Scripts,
                _ => crate::commands::Command::Help,
            };
            crate::commands::run(command, session, gpu, stores);
            return;
        }
        let settings = &mut *stores.settings;
        match action {
            SettingsPageAction::Open(_) => {}
            SettingsPageAction::SetGpu(id) => {
                if let Some(gpu_power) = settings::GpuPower::from_id(&id) {
                    // Startup-only: saved now, used by the next launch.
                    settings.gpu = gpu_power;
                    settings.settings_tab = "performance".into();
                    settings::save(settings);
                }
                refresh_settings_page(session, gpu, settings, browser_id);
            }
            SettingsPageAction::SetOsr(id) => {
                if let Some(osr) = settings::OsrMode::from_id(&id) {
                    settings.osr = osr;
                    settings.settings_tab = "performance".into();
                    settings::save(settings);
                }
                refresh_settings_page(session, gpu, settings, browser_id);
            }
            SettingsPageAction::ToggleAdBlock => {
                settings.ad_block_enabled = !settings.ad_block_enabled;
                settings.settings_tab = "blocking".into();
                settings::save(settings);
                sync_blocklist_settings(settings);
                refresh_settings_page(session, gpu, settings, browser_id);
            }
            SettingsPageAction::ToggleCleanUrls => {
                settings.clean_urls_enabled = !settings.clean_urls_enabled;
                settings.settings_tab = "general".into();
                settings::save(settings);
                cef_bridge::set_clean_urls_enabled(settings.clean_urls_enabled);
                refresh_settings_page(session, gpu, settings, browser_id);
            }
            SettingsPageAction::ToggleFilterNetwork => {
                settings.filter_network_enabled = !settings.filter_network_enabled;
                settings.settings_tab = "blocking".into();
                settings::save(settings);
                sync_blocklist_settings(settings);
                refresh_settings_page(session, gpu, settings, browser_id);
            }
            SettingsPageAction::ToggleFilterCosmetic => {
                settings.filter_cosmetic_enabled = !settings.filter_cosmetic_enabled;
                settings.settings_tab = "blocking".into();
                settings::save(settings);
                sync_blocklist_settings(settings);
                refresh_settings_page(session, gpu, settings, browser_id);
            }
            SettingsPageAction::ToggleFilterScriptlets => {
                settings.filter_scriptlets_enabled = !settings.filter_scriptlets_enabled;
                settings.settings_tab = "blocking".into();
                settings::save(settings);
                sync_blocklist_settings(settings);
                refresh_settings_page(session, gpu, settings, browser_id);
            }
            SettingsPageAction::ToggleFilterList(id) => {
                match id.as_str() {
                    "peter_lowe" => {
                        settings.filter_lists.peter_lowe = !settings.filter_lists.peter_lowe;
                    }
                    "easylist" => {
                        settings.filter_lists.easylist = !settings.filter_lists.easylist;
                    }
                    "easyprivacy" => {
                        settings.filter_lists.easyprivacy = !settings.filter_lists.easyprivacy;
                    }
                    _ => {}
                }
                settings.settings_tab = "blocking".into();
                settings::save(settings);
                sync_blocklist_settings(settings);
                refresh_settings_page(session, gpu, settings, browser_id);
            }
            SettingsPageAction::SetTab(tab) => {
                settings.settings_tab = AppSettings::normalize_tab(&tab).to_string();
                settings::save(settings);
                refresh_settings_page(session, gpu, settings, browser_id);
            }
            SettingsPageAction::SetSearchEngine(engine) => {
                match settings::valid_search_engine(&engine) {
                    Some(engine) => {
                        settings.default_search_engine = engine.to_string();
                        settings.settings_tab = "general".into();
                        settings::save(settings);
                    }
                    None => log::warn!("rejected search engine URL {engine:?}"),
                }
                refresh_settings_page(session, gpu, settings, browser_id);
            }
            SettingsPageAction::SetTheme(index) => {
                settings.settings_tab = "appearance".into();
                if let Some(theme) = THEMES.get(index) {
                    session.set_theme(*theme);
                }
                settings::save(settings);
                refresh_settings_page(session, gpu, settings, browser_id);
            }
            SettingsPageAction::SetReaderTheme(index) => {
                if index < crate::reader_mode::READER_THEMES.len() {
                    settings.reader_theme = index;
                    settings.settings_tab = "appearance".into();
                    settings::save(settings);
                }
                refresh_settings_page(session, gpu, settings, browser_id);
            }
            SettingsPageAction::AddBlockedHost(host) => {
                let host = host.trim();
                if !host.is_empty() && !settings.custom_blocked_hosts.iter().any(|h| h == host) {
                    settings.custom_blocked_hosts.push(host.to_string());
                    settings.settings_tab = "blocking".into();
                    settings::save(settings);
                    sync_blocklist_settings(settings);
                }
                refresh_settings_page(session, gpu, settings, browser_id);
            }
            SettingsPageAction::RemoveBlockedHost(index) => {
                if index < settings.custom_blocked_hosts.len() {
                    settings.custom_blocked_hosts.remove(index);
                    settings.settings_tab = "blocking".into();
                    settings::save(settings);
                    sync_blocklist_settings(settings);
                }
                refresh_settings_page(session, gpu, settings, browser_id);
            }
            SettingsPageAction::SetFrameRate(fps) => {
                if !settings::FRAME_RATES.contains(&fps) {
                    refresh_settings_page(session, gpu, settings, browser_id);
                    return;
                }
                settings.target_fps = fps;
                settings.settings_tab = "performance".into();
                settings::save(settings);
                browser::set_target_frame_rate(fps);
                // Apply live to every open host, not only future spawns.
                for page in session.pages() {
                    if let Some(host) = page.browser.host() {
                        host.set_windowless_frame_rate(fps as _);
                    }
                }
                refresh_settings_page(session, gpu, settings, browser_id);
            }
        }
    }
}
