// OSR: embedder applies cursor (CEF has no native window). Load progress
// comes from DisplayHandler, not LoadHandler.

use cef::{self, rc::Rc, *};
use std::cell::RefCell;
use winit::window::CursorIcon;

thread_local! {
    // Global slot is fine: only the page under the mouse gets cursor events.
    pub static CURSOR: RefCell<Option<CursorIcon>> = const { RefCell::new(None) };
    // (browser_id, progress 0..1)
    pub static PENDING_LOAD_PROGRESS: RefCell<Vec<(i32, f64)>> = const { RefCell::new(Vec::new()) };
}

#[derive(Clone)]
pub struct OsrDisplayHandler {}

wrap_display_handler! {
    pub struct DisplayHandlerBuilder {
        handler: OsrDisplayHandler,
    }

    impl DisplayHandler {
        fn on_cursor_change(
            &self,
            _browser: Option<&mut Browser>,
            _cursor: ::std::os::raw::c_ulong,
            type_: CursorType,
            _custom_cursor_info: Option<&CursorInfo>,
        ) -> ::std::os::raw::c_int {
            CURSOR.with_borrow_mut(|cursor| {
                cursor.replace(cef_cursor_to_winit(type_));
            });
            true as _
        }

        fn on_loading_progress_change(
            &self,
            browser: Option<&mut Browser>,
            progress: f64,
        ) {
            let Some(browser) = browser else {
                return;
            };
            PENDING_LOAD_PROGRESS
                .with_borrow_mut(|q| q.push((browser.identifier(), progress)));
        }
    }
}

impl DisplayHandlerBuilder {
    pub fn build(handler: OsrDisplayHandler) -> DisplayHandler {
        Self::new(handler)
    }
}

// CEF CT_* → winit CursorIcon (closest match when winit has no 1:1 icon).
fn cef_cursor_to_winit(type_: CursorType) -> CursorIcon {
    match type_ {
        CursorType::POINTER => CursorIcon::Default,
        CursorType::CROSS => CursorIcon::Crosshair,
        CursorType::HAND => CursorIcon::Pointer,
        CursorType::IBEAM => CursorIcon::Text,
        CursorType::WAIT => CursorIcon::Wait,
        CursorType::HELP => CursorIcon::Help,
        CursorType::EASTRESIZE => CursorIcon::EResize,
        CursorType::NORTHRESIZE => CursorIcon::NResize,
        CursorType::NORTHEASTRESIZE => CursorIcon::NeResize,
        CursorType::NORTHWESTRESIZE => CursorIcon::NwResize,
        CursorType::SOUTHRESIZE => CursorIcon::SResize,
        CursorType::SOUTHEASTRESIZE => CursorIcon::SeResize,
        CursorType::SOUTHWESTRESIZE => CursorIcon::SwResize,
        CursorType::WESTRESIZE => CursorIcon::WResize,
        CursorType::NORTHSOUTHRESIZE => CursorIcon::NsResize,
        CursorType::EASTWESTRESIZE => CursorIcon::EwResize,
        CursorType::NORTHEASTSOUTHWESTRESIZE => CursorIcon::NeswResize,
        CursorType::NORTHWESTSOUTHEASTRESIZE => CursorIcon::NwseResize,
        CursorType::COLUMNRESIZE => CursorIcon::ColResize,
        CursorType::ROWRESIZE => CursorIcon::RowResize,
        CursorType::MIDDLEPANNING
        | CursorType::EASTPANNING
        | CursorType::NORTHPANNING
        | CursorType::NORTHEASTPANNING
        | CursorType::NORTHWESTPANNING
        | CursorType::SOUTHPANNING
        | CursorType::SOUTHEASTPANNING
        | CursorType::SOUTHWESTPANNING
        | CursorType::WESTPANNING
        | CursorType::MIDDLE_PANNING_VERTICAL
        | CursorType::MIDDLE_PANNING_HORIZONTAL => CursorIcon::AllScroll,
        CursorType::MOVE => CursorIcon::Move,
        CursorType::VERTICALTEXT => CursorIcon::VerticalText,
        CursorType::CELL => CursorIcon::Cell,
        CursorType::CONTEXTMENU => CursorIcon::ContextMenu,
        CursorType::ALIAS => CursorIcon::Alias,
        CursorType::PROGRESS => CursorIcon::Progress,
        CursorType::NODROP => CursorIcon::NoDrop,
        CursorType::COPY | CursorType::DND_COPY => CursorIcon::Copy,
        CursorType::NONE => CursorIcon::Default,
        CursorType::NOTALLOWED => CursorIcon::NotAllowed,
        CursorType::ZOOMIN => CursorIcon::ZoomIn,
        CursorType::ZOOMOUT => CursorIcon::ZoomOut,
        CursorType::GRAB | CursorType::DND_MOVE => CursorIcon::Grab,
        CursorType::GRABBING => CursorIcon::Grabbing,
        CursorType::DND_NONE => CursorIcon::NoDrop,
        CursorType::DND_LINK => CursorIcon::Alias,
        _ => CursorIcon::Default,
    }
}
