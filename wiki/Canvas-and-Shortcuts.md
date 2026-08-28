# Canvas and shortcuts

Pages live in **world space** on a pannable/zoomable canvas. Hit-testing and drawing map through the viewport (`offset` + `zoom`).

## Mouse / trackpad

| Gesture | Action |
|---------|--------|
| Middle-drag | Pan canvas |
| Shift+Left-drag | Pan canvas (trackpad-friendly) |
| Ctrl+Scroll | Zoom canvas toward cursor |
| Alt+Left-drag | Move a page |
| Drag bottom-right corner | Resize a page |

## Touch

| Gesture | Action |
|---------|--------|
| One finger on empty canvas | Pan |
| Two-finger pinch / drag | Zoom + pan |
| One finger on a page | Forwarded as CEF touch events |

(macOS trackpad `PinchGesture` also zooms the canvas when the platform emits it.)

## Keyboard

### Pages

| Shortcut | Action |
|----------|--------|
| `Ctrl+T` | New page (omnibox) |
| `Ctrl+Shift+T` | Reopen closed page |
| `Ctrl+W` | Close page |
| `Ctrl+R` | Reload |
| `Ctrl+V` | Paste from clipboard |
| `Ctrl+Shift+C` | Copy page URL |
| `Ctrl+Shift+R` | Toggle reader mode |
| `Alt+Left` / `Alt+Right` | Back / forward |
| `Ctrl+=` / `Ctrl+-` | Zoom in / out (page) |
| `Ctrl+0` | Reset page zoom |

### Lists

| Shortcut | Action |
|----------|--------|
| `Ctrl+D` | Bookmark page |
| `Ctrl+B` | Bookmarks list |
| `Ctrl+J` | Downloads |
| `Ctrl+H` | History |
| `Ctrl+K` | Switcher (open pages) |
| `Ctrl+Shift+W` | Workspace slot list |
| `Ctrl+N` | New workspace slot |
| `Ctrl+1`…`Ctrl+9` | Switch to workspace slot N |
| `Ctrl+Shift+U` | Scripts & styles |
| `Ctrl+Shift+P` | Passwords / vault |
| `Ctrl+,` | Settings |

### Canvas

| Shortcut | Action |
|----------|--------|
| `Ctrl+G` | Auto-layout into a grid |
| `Ctrl+Tab` / `Ctrl+Shift+Tab` | Next / previous page |
| `Ctrl+Space` | Zoom focused page to canvas (temporary; saved layout keeps the pre-zoom rect) |
| Top-edge hover | Show workspace chip strip |
| Zoom out below ~0.55 | Minimap (bottom-right, ~22% of shorter side): page rects + viewfinder; drag/click to pan |
| `Ctrl+Shift+0` | Reset canvas view |

### Other

| Shortcut | Action |
|----------|--------|
| `Ctrl+Shift+Space` | Cycle UI theme |
| `F1` / `Ctrl+/` | This help list (in-app) |

Same table is always available in-app via **F1**. Live workspace slots: see [[Features]].
