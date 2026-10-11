// `context://` context-menu actions.

use super::query::query_param;
use std::cell::RefCell;

/// Context-menu actions (`context://...`).
pub enum ContextAction {
    /// Element hit-test result from a page (async after right-click).
    Hit {
        link: Option<String>,
        image: Option<String>,
        password_field: bool,
        page_url: String,
    },
    OpenLink(String),
    /// Open in the desktop's default browser (xdg-open).
    OpenExternal(String),
    Copy(String),
    SaveImage(String),
    GenPassword,
    FillPassword,
    ClosePage,
    NewPage,
    SaveWorkspace,
    Reader,
    Dismiss,
}

thread_local! {
    pub static PENDING_CONTEXT_ACTION: RefCell<Option<(i32, ContextAction)>> =
        const { RefCell::new(None) };
}

pub(super) fn parse_context_action(url: &str) -> Option<ContextAction> {
    let rest = url.strip_prefix("context://")?;
    if let Some(query) = rest.strip_prefix("hit?") {
        return Some(ContextAction::Hit {
            link: query_param(query, "link").filter(|s| !s.is_empty()),
            image: query_param(query, "image").filter(|s| !s.is_empty()),
            password_field: query_param(query, "pwd").as_deref() == Some("1"),
            page_url: query_param(query, "page").unwrap_or_default(),
        });
    }
    if let Some(query) = rest.strip_prefix("open-link?") {
        return Some(ContextAction::OpenLink(query_param(query, "url")?));
    }
    if let Some(query) = rest.strip_prefix("external?") {
        return Some(ContextAction::OpenExternal(query_param(query, "url")?));
    }
    if let Some(query) = rest.strip_prefix("copy?") {
        return Some(ContextAction::Copy(query_param(query, "text")?));
    }
    if let Some(query) = rest.strip_prefix("save-image?") {
        return Some(ContextAction::SaveImage(query_param(query, "url")?));
    }
    Some(match rest {
        "gen-password" => ContextAction::GenPassword,
        "fill-password" => ContextAction::FillPassword,
        "close-page" => ContextAction::ClosePage,
        "new-page" => ContextAction::NewPage,
        "save-workspace" => ContextAction::SaveWorkspace,
        "reader" => ContextAction::Reader,
        "dismiss" => ContextAction::Dismiss,
        _ => return None,
    })
}
