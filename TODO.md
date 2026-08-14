# Known gaps

- **Cursor doesn't change shape on hover** (e.g. link hover should show a
  pointer). Clicks and scroll work — CEF just doesn't drive the OS
  cursor itself in windowless/OSR mode, since it doesn't own a native
  window. Needs `RenderHandler::on_cursor_change` implemented in
  `cef-bridge` (receives the CEF cursor type/handle on change) wired to
  `window.set_cursor(...)` in `compositor`.
