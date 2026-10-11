// `find://` find-bar actions.

use super::query::query_param;
use std::cell::RefCell;

/// Find bar actions (`find://go/search?q=..&dir=next|prev&case=0|1`,
/// `find://go/close`). Path-style host avoids CEF's `x?` → `x/?` rewrite.
pub enum FindAction {
    Search {
        text: String,
        forward: bool,
        match_case: bool,
    },
    Close,
}

thread_local! {
    pub static PENDING_FIND_ACTION: RefCell<Option<(i32, FindAction)>> = const { RefCell::new(None) };
}

pub(super) fn parse_find_action(url: &str) -> Option<FindAction> {
    let rest = url.strip_prefix("find://go/")?;
    if rest == "close" || rest == "close/" {
        return Some(FindAction::Close);
    }
    let query = rest
        .strip_prefix("search?")
        .or_else(|| rest.strip_prefix("search/?"))?;
    Some(FindAction::Search {
        text: query_param(query, "q").unwrap_or_default(),
        forward: query_param(query, "dir").as_deref() != Some("prev"),
        match_case: query_param(query, "case").as_deref() == Some("1"),
    })
}
