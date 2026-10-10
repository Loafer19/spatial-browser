// Favicons for the built-in pages, captured from the browser's own cache
// when a page announces its icon, so lists never request
// https://<host>/favicon.ico (which told every listed site it was listed).

use cef::{self, rc::Rc, *};
use std::cell::RefCell;

/// Largest icon side requested from CEF (lists draw 20 CSS px; 2x for HiDPI).
const MAX_ICON_PX: u32 = 64;

thread_local! {
    // (browser_id, icon URL) from on_favicon_urlchange.
    pub static PENDING_FAVICON_URLS: RefCell<Vec<(i32, String)>> = const { RefCell::new(Vec::new()) };
    // (cache key, PNG bytes) from finished downloads.
    pub static PENDING_FAVICON_IMAGES: RefCell<Vec<(String, Vec<u8>)>> = const { RefCell::new(Vec::new()) };
}

/// Fetch `icon_url` through `browser`'s cache (no bypass) as PNG; the result
/// lands in PENDING_FAVICON_IMAGES under `key`.
pub fn fetch_favicon(browser: &Browser, icon_url: &str, key: String) {
    let Some(host) = browser.host() else { return };
    let mut callback = FaviconCallbackBuilder::build(FaviconCallback { key });
    host.download_image(
        Some(&icon_url.into()),
        true as _,
        MAX_ICON_PX,
        false as _,
        Some(&mut callback),
    );
}

#[derive(Clone)]
pub struct FaviconCallback {
    key: String,
}

wrap_download_image_callback! {
    pub struct FaviconCallbackBuilder {
        handler: FaviconCallback,
    }

    impl DownloadImageCallback {
        fn on_download_image_finished(
            &self,
            _image_url: Option<&CefString>,
            http_status_code: ::std::os::raw::c_int,
            image: Option<&mut Image>,
        ) {
            let key = &self.handler.key;
            let Some(image) = image else {
                log::debug!("favicon {key}: no image (HTTP {http_status_code})");
                return;
            };
            if !(200..300).contains(&http_status_code) && http_status_code != 0 {
                log::debug!("favicon {key}: HTTP {http_status_code}");
                return;
            }
            let Some(png) = image.as_png(1.0, true as _, None, None) else {
                log::debug!("favicon {key}: image has no PNG representation");
                return;
            };
            let mut bytes = vec![0u8; png.size()];
            let read = png.data(Some(&mut bytes), 0);
            bytes.truncate(read);
            if !bytes.is_empty() {
                PENDING_FAVICON_IMAGES.with_borrow_mut(|q| q.push((key.clone(), bytes)));
            }
        }
    }
}

impl FaviconCallbackBuilder {
    pub fn build(handler: FaviconCallback) -> DownloadImageCallback {
        Self::new(handler)
    }
}
