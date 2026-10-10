// Bookmarks list actions and Chrome bookmark import.

use super::*;

pub(super) fn start_bookmark_file_dialog(browser_id: i32) {
    let (start_dir, file_name) = bookmarks_chrome::dialog_start();
    file_dialog::start(
        PickKind::Bookmarks,
        browser_id,
        PickOptions {
            title: "Import bookmarks (Chrome / Chromium / Brave / Edge)".into(),
            start_dir,
            file_name,
            filters: Vec::new(),
        },
    );
}

pub(super) fn poll_bookmark_file_pick(
    session: &mut Session,
    gpu: &GpuState,
    bookmarks: &mut Vec<Bookmark>,
) {
    let Some((browser_id, path)) = file_dialog::poll(PickKind::Bookmarks) else {
        return;
    };

    let status = match path {
        None => "Import cancelled".to_string(),
        Some(path) => match bookmarks_chrome::import_path(bookmarks, &path.to_string_lossy()) {
            Ok(stats) => {
                bookmarks::save(bookmarks);
                format!(
                    "Import done: {} added, {} skipped — from {}",
                    stats.added,
                    stats.skipped,
                    path.display()
                )
            }
            Err(e) => format!("Import failed: {e}"),
        },
    };
    if session
        .pages()
        .iter()
        .any(|p| p.browser.identifier() == browser_id)
    {
        refresh_bookmarks_page(session, gpu, bookmarks, Some(&status), browser_id);
    } else {
        hotkeys::open_bookmarks(session, gpu, bookmarks);
        if let Some(page) = session.pages().last() {
            let id = page.browser.identifier();
            refresh_bookmarks_page(session, gpu, bookmarks, Some(&status), id);
        }
    }
}

pub(super) fn refresh_bookmarks_page(
    session: &mut Session,
    gpu: &GpuState,
    bookmarks: &[Bookmark],
    status: Option<&str>,
    browser_id: i32,
) {
    let url = pages::bookmarks_list::page_url(&session.theme(), bookmarks, status);
    show_in_place(session, gpu, browser_id, &url);
}

/// `bookmark://` from the bookmarks list.
pub(super) fn drain(session: &mut Session, gpu: &GpuState, bookmarks: &mut Vec<Bookmark>) {
    // bookmark://… → act; refresh the requesting list page in place.
    if let Some((browser_id, action)) = PENDING_BOOKMARK
        .with_borrow_mut(|pending| pending.take())
        .filter(|(id, _)| from_internal(session, *id, "bookmark://"))
    {
        match action {
            BookmarkAction::Open(index) => {
                if let Some(bookmark) = bookmarks.get(index) {
                    let size = gpu.window.inner_size();
                    let rect = session.cascade_rect((size.width as f32, size.height as f32));
                    session.add_page(browser::spawn(gpu, &gpu.window, &bookmark.url, rect, false));
                }
            }
            BookmarkAction::Delete(index) => {
                if index < bookmarks.len() {
                    bookmarks.remove(index);
                    bookmarks::save(bookmarks);
                }
                refresh_bookmarks_page(session, gpu, bookmarks, None, browser_id);
            }
            BookmarkAction::Rename(index, title, folder) => {
                if let Some(bookmark) = bookmarks.get_mut(index) {
                    bookmark.title = (!title.is_empty()).then_some(title);
                    bookmark.folder = (!folder.is_empty()).then_some(folder);
                }
                bookmarks::save(bookmarks);
                refresh_bookmarks_page(session, gpu, bookmarks, None, browser_id);
            }
            BookmarkAction::ImportBrowse => {
                // Keep page open (browser_id); dialog is off-thread for portal.
                start_bookmark_file_dialog(browser_id);
            }
        }
    }
}
