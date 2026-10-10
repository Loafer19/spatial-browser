// Finished downloads and the downloads list.

use super::*;

/// Like `refresh_bookmarks_page`, for downloads.
pub(super) fn refresh_downloads_page(
    session: &mut Session,
    gpu: &GpuState,
    downloads: &[DownloadRecord],
    browser_id: i32,
) {
    let url = pages::downloads_list::page_url(&session.theme(), downloads);
    show_in_place(session, gpu, browser_id, &url);
}

/// Finished downloads (record + notification) and `download://` list actions.
pub(super) fn drain(session: &mut Session, gpu: &GpuState, downloads: &mut Vec<DownloadRecord>) {
    // Completed downloads → downloads.json + desktop notification.
    let completed = PENDING_DOWNLOADS.with_borrow_mut(std::mem::take);
    for download in completed {
        let filename = std::path::Path::new(&download.path)
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or(&download.path)
            .to_string();
        downloads::record(
            downloads,
            DownloadRecord {
                url: download.url,
                path: download.path,
            },
        );
        if let Err(e) = std::process::Command::new("notify-send")
            .arg("Download complete")
            .arg(&filename)
            .spawn()
        {
            log::warn!("notify-send failed (download {filename:?} still saved): {e}");
        }
    }

    // download://…
    if let Some((browser_id, action)) = PENDING_DOWNLOAD_ACTION
        .with_borrow_mut(|pending| pending.take())
        .filter(|(id, _)| from_internal(session, *id, "download://"))
    {
        match action {
            DownloadPageAction::Open(index) => {
                if let Some(download) = downloads.get(index) {
                    // File for safe types, its folder for executables/scripts/
                    // unknown; None if the record points outside ~/Downloads.
                    match cef_bridge::download_open_target(&download.path) {
                        Some(target) => {
                            if let Err(e) =
                                std::process::Command::new("xdg-open").arg(&target).spawn()
                            {
                                log::warn!("xdg-open failed for {target:?}: {e}");
                            }
                        }
                        None => log::warn!(
                            "refused to open download {:?}: missing or outside downloads dir",
                            download.path
                        ),
                    }
                }
            }
            DownloadPageAction::Remove(index) => {
                if index < downloads.len() {
                    downloads.remove(index);
                    downloads::save(downloads);
                }
                refresh_downloads_page(session, gpu, downloads, browser_id);
            }
        }
    }
}
