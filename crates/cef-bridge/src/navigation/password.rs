// `password://` vault/autofill actions and which frames may send them.

use super::query::query_param;
use std::cell::RefCell;

/// Vault / autofill actions from `password://...`.
pub enum PasswordAction {
    /// Master password for unlock or create (confirm also set for create).
    Unlock {
        password: String,
        create: bool,
        confirm: Option<String>,
    },
    Query {
        origin: String,
    },
    Fill {
        id: String,
    },
    SaveOffer {
        origin: String,
        username: String,
        password: String,
    },
    Save {
        origin: String,
        username: String,
        password: String,
        id: Option<String>,
    },
    Never {
        origin: String,
    },
    /// Buttons of the compositor's save-prompt tile.
    PromptSave,
    PromptNever,
    PromptDismiss,
    /// Generator page's Copy: the compositor copies its own generated
    /// password (marked sensitive), so it never rides in a URL.
    CopyGenerated,
    Delete {
        id: String,
    },
    Upsert {
        id: Option<String>,
        origin: String,
        username: String,
        password: String,
        email: Option<String>,
    },
    RemoveNever {
        origin: String,
    },
    Generate {
        length: usize,
        symbols: bool,
    },
    /// Open a native file picker and import Chrome/Bitwarden CSV.
    ImportBrowse,
    /// Switch passwords UI tab (`saved` / `add` / `generator` / `never`).
    Tab {
        id: String,
    },
    /// Open the passwords list after a successful unlock.
    OpenList,
    /// Lock the vault now (drop the key; list pages fall back to unlock).
    Lock,
}

thread_local! {
    pub static PENDING_PASSWORD_ACTION: RefCell<Option<(i32, PasswordAction)>> =
        const { RefCell::new(None) };
}

/// Accept `action?…` or CEF-canonicalized `action/?…`.
pub(super) fn password_action_query<'a>(rest: &'a str, action: &str) -> Option<&'a str> {
    let with_q = format!("{action}?");
    let with_slash_q = format!("{action}/?");
    rest.strip_prefix(&with_q)
        .or_else(|| rest.strip_prefix(&with_slash_q))
}

pub(super) fn parse_password_action(url: &str) -> Option<PasswordAction> {
    let rest = url.strip_prefix("password://")?;
    // Prefer `password://go/<action>?…` so CEF does not turn `query?…` into
    // `query/?…`. Legacy `password://<action>?…` still accepted.
    let rest = rest.strip_prefix("go/").unwrap_or(rest);
    let rest = rest.strip_prefix('/').unwrap_or(rest);
    match rest {
        "prompt-save" => return Some(PasswordAction::PromptSave),
        "prompt-never" => return Some(PasswordAction::PromptNever),
        "prompt-dismiss" => return Some(PasswordAction::PromptDismiss),
        "copy-generated" => return Some(PasswordAction::CopyGenerated),
        _ => {}
    }
    if let Some(query) = password_action_query(rest, "unlock") {
        let password = query_param(query, "password")?;
        let create = query_param(query, "create").as_deref() == Some("1");
        let confirm = query_param(query, "confirm");
        return Some(PasswordAction::Unlock {
            password,
            create,
            confirm,
        });
    }
    if let Some(query) = password_action_query(rest, "query") {
        return Some(PasswordAction::Query {
            origin: query_param(query, "origin")?,
        });
    }
    if let Some(query) = password_action_query(rest, "fill") {
        return Some(PasswordAction::Fill {
            id: query_param(query, "id")?,
        });
    }
    if let Some(query) = password_action_query(rest, "save-offer") {
        return Some(PasswordAction::SaveOffer {
            origin: query_param(query, "origin").unwrap_or_default(),
            username: query_param(query, "username").unwrap_or_default(),
            password: query_param(query, "password").unwrap_or_default(),
        });
    }
    if let Some(query) = password_action_query(rest, "save") {
        let id = query_param(query, "id").filter(|s| !s.is_empty());
        return Some(PasswordAction::Save {
            origin: query_param(query, "origin").unwrap_or_default(),
            username: query_param(query, "username").unwrap_or_default(),
            password: query_param(query, "password").unwrap_or_default(),
            id,
        });
    }
    if let Some(query) = password_action_query(rest, "never") {
        return Some(PasswordAction::Never {
            origin: query_param(query, "origin")?,
        });
    }
    if let Some(query) = password_action_query(rest, "delete") {
        return Some(PasswordAction::Delete {
            id: query_param(query, "id")?,
        });
    }
    if let Some(query) = password_action_query(rest, "upsert") {
        return Some(PasswordAction::Upsert {
            id: query_param(query, "id").filter(|s| !s.is_empty()),
            origin: query_param(query, "origin").unwrap_or_default(),
            username: query_param(query, "username").unwrap_or_default(),
            password: query_param(query, "password").unwrap_or_default(),
            email: query_param(query, "email").filter(|s| !s.is_empty()),
        });
    }
    if let Some(query) = password_action_query(rest, "remove-never") {
        return Some(PasswordAction::RemoveNever {
            origin: query_param(query, "origin")?,
        });
    }
    if let Some(query) = password_action_query(rest, "generate") {
        let length = query_param(query, "length")
            .and_then(|s| s.parse().ok())
            .unwrap_or(20);
        let symbols = query_param(query, "symbols").as_deref() != Some("0");
        return Some(PasswordAction::Generate { length, symbols });
    }
    if rest == "import-browse" || rest == "import" {
        return Some(PasswordAction::ImportBrowse);
    }
    if let Some(id) = rest.strip_prefix("tab/") {
        return Some(PasswordAction::Tab { id: id.to_string() });
    }
    if rest == "open-list" {
        return Some(PasswordAction::OpenList);
    }
    if rest == "lock" || rest == "lock/" {
        return Some(PasswordAction::Lock);
    }
    None
}

/// Which frames may send `password://`: the main frame (built-in pages,
/// the find/save tiles) or the autofill bridge's hidden iframe — a fresh,
/// still-blank direct child of the main frame. A cross-origin ad iframe has
/// its own URL, and any iframe it creates has the ad as its parent, so
/// neither can offer or fill logins in the name of the top-level site.
pub(super) fn signal_frame_ok(is_main: bool, parent_is_main: bool, current_url: &str) -> bool {
    // A fresh iframe may report its pending password:// URL instead of the
    // initial about:blank; either way it has never held web content.
    is_main
        || (parent_is_main
            && (matches!(current_url, "" | "about:blank")
                || current_url.starts_with("password://")))
}
