// Right-click menu: open/dismiss and `context://` actions.

use super::*;

/// A right-click on a page, waiting for that page's `context://hit` reply.
pub struct ContextHitRequest {
    pub browser_id: i32,
    pub screen_pos: (f32, f32),
    pub at: std::time::Instant,
}

impl ContextHitRequest {
    /// The hit script answers within a frame or two; anything later (or
    /// from another page) was not caused by this click.
    const MAX_DELAY: std::time::Duration = std::time::Duration::from_secs(1);

    pub(super) fn accepts(&self, browser_id: i32, now: std::time::Instant) -> bool {
        self.browser_id == browser_id && now.duration_since(self.at) <= Self::MAX_DELAY
    }
}

/// Close+respawn bookmarks list at same rect (load_url after cancel is unreliable).
pub(crate) fn dismiss_context_menu(
    session: &mut Session,
    context_menu: &mut Option<ContextMenuState>,
) {
    let Some(cm) = context_menu.take() else {
        return;
    };
    if let Some(index) = session
        .pages()
        .iter()
        .position(|p| p.browser.identifier() == cm.menu_browser_id)
    {
        let _ = session.close_at(index);
    }
}

pub(crate) fn open_context_menu(
    session: &mut Session,
    gpu: &GpuState,
    context_menu: &mut Option<ContextMenuState>,
    screen_pos: (f32, f32),
    ctx: MenuContext,
) {
    dismiss_context_menu(session, context_menu);
    let url = pages::context_menu::page_url(&session.theme(), &ctx);
    let (css_w, css_h) = pages::context_menu::menu_css_size(&ctx);
    // CEF logical size = world_rect / scale_factor (see browser::Page::set_rect).
    // Want logical == css_* at current zoom → world = css * scale / zoom.
    let scale = gpu.window.scale_factor() as f32;
    let zoom = session.viewport().zoom.max(0.05);
    let viewport = session.viewport();
    let world = viewport.screen_to_world(screen_pos);
    let rect = Rect {
        x: world.0,
        y: world.1,
        w: css_w * scale / zoom,
        h: css_h * scale / zoom,
    };
    session.add_page(browser::spawn(gpu, &gpu.window, &url, rect, true));
    let menu_id = session.pages().last().map(|p| p.browser.identifier());
    if let Some(menu_browser_id) = menu_id {
        *context_menu = Some(ContextMenuState {
            menu_browser_id,
            target_browser_id: ctx.target_browser_id,
            screen_pos,
        });
    }
}

pub(super) fn handle_context_action(
    session: &mut Session,
    gpu: &GpuState,
    vault: &mut Option<VaultSession>,
    context_menu: &mut Option<ContextMenuState>,
    pending_context_hit: &mut Option<ContextHitRequest>,
    workspaces: &mut WorkspaceStore,
    workspace_runtime: &mut WorkspaceRuntime,
    typed_history: &[String],
    settings: &AppSettings,
    browser_id: i32,
    action: ContextAction,
) {
    match action {
        ContextAction::Hit {
            link,
            image,
            password_field,
            page_url,
        } => {
            // Only the page the user just right-clicked may open the menu;
            // otherwise any page could pop it (UI spoofing) whenever it liked.
            let Some(request) = pending_context_hit
                .take()
                .filter(|r| r.accepts(browser_id, std::time::Instant::now()))
            else {
                log::warn!("dropped context://hit from browser {browser_id} without a right-click");
                return;
            };
            let target_id = request.browser_id;
            let screen_pos = request.screen_pos;
            // The page reports its own URL; use the frame's real one.
            let page_url = session
                .pages()
                .iter()
                .find(|p| p.browser.identifier() == target_id)
                .map(|p| p.url())
                .unwrap_or(page_url);
            open_context_menu(
                session,
                gpu,
                context_menu,
                screen_pos,
                MenuContext {
                    on_canvas: false,
                    page_url: Some(page_url),
                    link,
                    image,
                    password_field,
                    target_browser_id: Some(target_id),
                },
            );
        }
        ContextAction::OpenExternal(url) => {
            dismiss_context_menu(session, context_menu);
            crate::external::open_in_system_browser(&url);
        }
        ContextAction::OpenLink(url) => {
            dismiss_context_menu(session, context_menu);
            let size = gpu.window.inner_size();
            let rect = session.cascade_rect((size.width as f32, size.height as f32));
            session.add_page(browser::spawn(gpu, &gpu.window, &url, rect, false));
        }
        ContextAction::Copy(text) => {
            dismiss_context_menu(session, context_menu);
            cef_bridge::clipboard::copy_text(&text, false);
        }
        ContextAction::SaveImage(url) => {
            let target = context_menu
                .as_ref()
                .and_then(|c| c.target_browser_id)
                .unwrap_or(browser_id);
            dismiss_context_menu(session, context_menu);
            if let Some(page) = session
                .pages()
                .iter()
                .find(|p| p.browser.identifier() == target)
            {
                if let Some(frame) = page.browser.main_frame() {
                    let js = format!(
                        "(function(u){{var a=document.createElement('a');a.href=u;a.download='';\
                         a.rel='noopener';document.documentElement.appendChild(a);a.click();a.remove();}})({});",
                        serde_json::to_string(&url).unwrap_or_default()
                    );
                    frame.execute_java_script(Some(&js.as_str().into()), Some(&"".into()), 0);
                }
            }
        }
        ContextAction::GenPassword => {
            dismiss_context_menu(session, context_menu);
            let pw = vault::generate_password(20, true);
            cef_bridge::clipboard::copy_text(&pw, true);
        }
        ContextAction::FillPassword => {
            let target = context_menu
                .as_ref()
                .and_then(|c| c.target_browser_id);
            dismiss_context_menu(session, context_menu);
            if vault.is_none() {
                hotkeys::open_passwords(session, gpu, None);
                return;
            }
            if let Some(tid) = target {
                if let Some(page) = session.pages().iter().find(|p| p.browser.identifier() == tid)
                {
                    let origin = vault::normalize_origin(&page.url());
                    if let Some(v) = vault.as_ref() {
                        v.touch();
                        let matches = v.entries_for_origin(&origin);
                        if let Some(frame) = page.browser.main_frame() {
                            if let Some(js) = suggest_fill_js(&matches) {
                                frame.execute_java_script(
                                    Some(&js.as_str().into()),
                                    Some(&"".into()),
                                    0,
                                );
                            }
                        }
                    }
                }
            } else {
                hotkeys::open_passwords(session, gpu, vault.as_ref());
            }
        }
        ContextAction::ClosePage => {
            let target = context_menu
                .as_ref()
                .and_then(|c| c.target_browser_id);
            dismiss_context_menu(session, context_menu);
            if let Some(tid) = target {
                if let Some(index) = session
                    .pages()
                    .iter()
                    .position(|p| p.browser.identifier() == tid)
                {
                    let _ = session.close_at(index);
                }
            }
        }
        ContextAction::NewPage => {
            dismiss_context_menu(session, context_menu);
            hotkeys::open_new(session, gpu, typed_history, settings);
        }
        ContextAction::SaveWorkspace => {
            dismiss_context_menu(session, context_menu);
            workspaces::add_and_switch(workspaces, workspace_runtime, session, gpu);
        }
        ContextAction::Reader => {
            let target = context_menu.as_ref().and_then(|c| c.target_browser_id);
            dismiss_context_menu(session, context_menu);
            if let Some(tid) = target {
                if let Some(index) = session
                    .pages()
                    .iter()
                    .position(|p| p.browser.identifier() == tid)
                {
                    session.bring_to_front(index);
                }
            }
            hotkeys::toggle_reader_mode(session, settings);
        }
        ContextAction::Dismiss => {
            dismiss_context_menu(session, context_menu);
        }
    }
}

/// `context://` from the right-click menu (and `hit` from the clicked page).
pub(super) fn drain(
    session: &mut Session,
    gpu: &GpuState,
    s: &mut Stores<'_>,
) {
    if let Some((browser_id, action)) =
        PENDING_CONTEXT_ACTION
        .with_borrow_mut(|pending| pending.take())
        .filter(|(id, action)| {
            matches!(action, ContextAction::Hit { .. }) || from_internal(session, *id, "context://")
        })
    {
        handle_context_action(
            session,
            gpu,
            s.vault,
            s.context_menu,
            s.pending_context_hit,
            s.workspaces,
            s.workspace_runtime,
            s.typed_history,
            s.settings,
            browser_id,
            action,
        );
    }
}
