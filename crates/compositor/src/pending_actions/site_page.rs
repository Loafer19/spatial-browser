// `site://` from the built-in site-info page.

use super::*;

pub(super) fn drain(session: &mut Session, gpu: &GpuState, s: &mut Stores<'_>) {
    let Some((browser_id, action)) = PENDING_SITE_ACTION
        .with_borrow_mut(|pending| pending.take())
        .filter(|(id, _)| from_internal(session, *id, "site://"))
    else {
        return;
    };
    let url = match action {
        SiteAction::ForgetHistory(url) => {
            let host = bookmarks::host_of(&url).to_string();
            let removed = history::remove_host(s.history, &host);
            log::info!("forgot {removed} history visits on {host}");
            history::save(s.history);
            url
        }
        SiteAction::ToggleBookmark { url, title } => {
            if let Some(i) = s.bookmarks.iter().position(|b| b.url == url) {
                s.bookmarks.remove(i);
            } else {
                s.bookmarks.push(Bookmark {
                    url: url.clone(),
                    title: (!title.trim().is_empty()).then(|| title.trim().to_string()),
                    folder: None,
                });
            }
            bookmarks::save(s.bookmarks);
            url
        }
    };
    let page = pages::site_info::page_url(&session.theme(), &url, &site_context(s));
    show_in_place(session, gpu, browser_id, &page);
}
