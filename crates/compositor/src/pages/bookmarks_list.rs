// Ctrl+B bookmarks manager (`bookmark://…` → PENDING_BOOKMARK): folder tree
// (`/`-separated paths), search, inline rename, multi-select to delete or
// move. Folder selection and checkboxes are client-side; every change is a
// `bookmark://` action that refreshes this page in place.

use super::ui::{self, html, Icon, Markup};
use crate::output::Theme;
use crate::persistence::bookmarks::{self, Bookmark};

const CSS: &str = r#"
.layout { display: grid; grid-template-columns: 210px 1fr; gap: 18px; align-items: start; }
@media (max-width: 640px) { .layout { grid-template-columns: 1fr; } .tree { display: none; } }
.tree { position: sticky; top: 12px; display: flex; flex-direction: column; gap: 2px; }
.tree a { display: flex; align-items: center; gap: 6px; padding: 5px 8px; border-radius: var(--radius-sm);
  color: var(--fg); text-decoration: none; font-size: 13px; min-width: 0; }
.tree a svg { width: 16px; height: 16px; flex-shrink: 0; opacity: .7; }
.tree a .name { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.tree a .count { font-size: 11px; opacity: .6; }
.tree a:hover { background: var(--surface); }
.tree a.on { background: var(--accent); color: var(--accent-fg); }
.row.out { display: none; }
.row .sel { width: 16px; height: 16px; flex-shrink: 0; accent-color: var(--accent); }
.row .folder-tag { font-size: 11px; }
.edit { display: flex; align-items: center; gap: 4px; margin: 0; }
.edit input { height: 28px; font-size: 13px; min-width: 0; }
.edit input[name=title] { flex: 2; }
.edit input[name=folder] { flex: 1; }
.row.renaming .view { display: none; }
.row:not(.renaming) .edit { display: none; }
.bar { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; margin: 0 0 10px; }
.bar input { height: 30px; min-width: 0; }
.bar.selection:not(.any) { display: none; }
.folder-bar:not(.shown) { display: none; }
"#;

const SCRIPT: &str = r#"
(function () {
  var cur = '';            // '' all · '\u0000' unfiled · path
  var armed = 0, armTimer = 0;
  function enc(s) { return encodeURIComponent(s); }
  function inFolder(f, root) { return f === root || f.indexOf(root + '/') === 0; }
  function selected() {
    return Array.prototype.map.call(document.querySelectorAll('.row .sel:checked'),
      function (c) { return c.closest('.row').dataset.id; });
  }
  function syncSel() {
    var ids = selected();
    document.querySelector('.selection').classList.toggle('any', ids.length > 0);
    document.querySelectorAll('[data-sel-count]').forEach(function (e) { e.textContent = ids.length; });
    document.querySelectorAll('.row').forEach(function (r) {
      var c = r.querySelector('.sel'); if (c) r.classList.toggle('is-selected', c.checked);
    });
  }
  function show(folder) {
    cur = folder;
    document.querySelectorAll('.tree a').forEach(function (a) { a.classList.toggle('on', a.dataset.folder === folder); });
    document.querySelectorAll('.list .row').forEach(function (r) {
      var f = r.dataset.folder || '';
      var hit = folder === '' || (folder === '\u0000' ? f === '' : inFolder(f, folder));
      r.classList.toggle('out', !hit);
    });
    var bar = document.querySelector('.folder-bar');
    var real = folder !== '' && folder !== '\u0000';
    bar.classList.toggle('shown', real);
    if (real) { bar.querySelector('[name=from]').value = folder; bar.querySelector('[name=to]').value = folder; }
    var move = document.querySelector('.selection [name=folder]');
    if (move && real && !move.value) move.value = folder;
    try { sessionStorage.setItem('bm-folder', folder); } catch (e) {}
    ui.setActive(0, false);
  }
  document.addEventListener('click', function (e) {
    var t = e.target.closest && e.target.closest('.tree a');
    if (t) { e.preventDefault(); show(t.dataset.folder); return; }
    var r = e.target.closest && e.target.closest('[data-rename]');
    if (r) { e.preventDefault(); startRename(r.closest('.row')); return; }
    var del = e.target.closest && e.target.closest('[data-delete-selected]');
    if (del) { e.preventDefault(); deleteSelected(true); }
  });
  document.addEventListener('change', function (e) { if (e.target.classList.contains('sel')) syncSel(); });
  function startRename(row) {
    if (!row) return;
    row.classList.add('renaming');
    var i = row.querySelector('.edit input'); i.focus(); i.select();
  }
  document.addEventListener('focusout', function (e) {
    var form = e.target.closest && e.target.closest('.edit');
    if (!form) return;
    setTimeout(function () { if (!form.contains(document.activeElement)) form.closest('.row').classList.remove('renaming'); }, 0);
  });
  function deleteSelected(now) {
    var ids = selected();
    if (!ids.length) return false;
    if (!now && armed !== ids.length) {
      armed = ids.length; clearTimeout(armTimer);
      armTimer = setTimeout(function () { armed = 0; }, 2500);
      ui.toast('Delete ' + ids.length + ' bookmarks \u2014 press Delete again');
      return true;
    }
    ui.go('bookmark://delete-many?ids=' + ids.join(','));
    return true;
  }
  var moveForm = document.querySelector('.selection form');
  if (moveForm) moveForm.addEventListener('submit', function (e) {
    e.preventDefault();
    var ids = selected(); if (!ids.length) return;
    ui.go('bookmark://move?ids=' + ids.join(',') + '&folder=' + enc(moveForm.querySelector('[name=folder]').value));
  });
  // Capture phase: runs before ui.js so Delete acts on the selection.
  window.addEventListener('keydown', function (e) {
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    if (/^(INPUT|TEXTAREA|SELECT)$/.test(e.target.tagName)) return;
    var row = ui.active();
    if (e.key === 'Delete' && selected().length) { e.preventDefault(); e.stopPropagation(); deleteSelected(false); }
    else if ((e.key === ' ' || e.key === 'x') && row) {
      var c = row.querySelector('.sel'); if (!c) return;
      e.preventDefault(); c.checked = !c.checked; syncSel();
    }
    else if ((e.key === 'r' || e.key === 'F2') && row) { e.preventDefault(); startRename(row); }
    else if (e.key === 'a' && !e.shiftKey) {
      e.preventDefault();
      var vis = ui.rows(); var all = vis.every(function (r) { var c = r.querySelector('.sel'); return !c || c.checked; });
      vis.forEach(function (r) { var c = r.querySelector('.sel'); if (c) c.checked = !all; });
      syncSel();
    }
  }, true);
  var saved = '';
  try { saved = sessionStorage.getItem('bm-folder') || ''; } catch (e) {}
  if (saved && !document.querySelector('.tree a[data-folder="' + CSS.escape(saved) + '"]')) saved = '';
  window.addEventListener('load', function () { show(saved); syncSel(); });
})();
"#;

fn tree_item(folder: &str, label: &str, depth: usize, count: usize, icon: Icon) -> Markup {
    html! {
        a href="#" data-folder=(folder) title=(label)
            style=(format!("padding-left:{}px", 8 + depth * 14)) {
            (icon) span class="name" { (label) } span class="count" { (count) }
        }
    }
}

fn row(index: usize, b: &Bookmark) -> Markup {
    let host = bookmarks::host_of(&b.url);
    let label = bookmarks::display_label(b);
    let folder = b.folder.as_deref().unwrap_or("");
    let open = format!("bookmark://open/{index}");
    let delete = format!("bookmark://delete/{index}");
    html! {
        div class="row card" data-id=(index) data-open=(open) data-delete=(delete)
            data-folder=(folder) data-text=(format!("{label} {} {folder}", b.url)) {
            input class="sel" type="checkbox" tabindex="-1" aria-label="Select";
            (ui::favicon(host, label))
            div class="main" {
                div class="view" {
                    span class="title" { (label) }
                    span class="sub" {
                        (b.url)
                        @if !folder.is_empty() { " · " span class="badge folder-tag" { (folder) } }
                    }
                }
                form class="edit" method="get" action=(format!("bookmark://rename/{index}")) {
                    input class="input" type="text" name="title" value=(b.title.as_deref().unwrap_or(""))
                        placeholder=(host) aria-label="Title" autocomplete="off" spellcheck="false";
                    input class="input" type="text" name="folder" value=(folder)
                        placeholder="Folder/Sub" aria-label="Folder" autocomplete="off" spellcheck="false";
                    (ui::icon_submit("Save", Icon::Check))
                }
            }
            div class="actions" {
                button type="button" class="icon-btn" data-rename title="Rename / move" aria-label="Rename" { (Icon::Edit) }
                (ui::icon_link(&delete, "Delete", Icon::Trash, true))
            }
        }
    }
}

/// Bookmarks page URL (`bookmark://` actions).
pub(crate) fn page_url(theme: &Theme, marks: &[Bookmark], status: Option<&str>) -> String {
    let tree = bookmarks::folder_tree(marks);
    let unfiled = marks.iter().filter(|b| b.folder.is_none()).count();
    let body = html! {
        main class="page wide" {
            (ui::head("Bookmarks", Some(html! {
                (marks.len()) " bookmarks · " kbd { "Space" } " select · " kbd { "a" } " all · "
                kbd { "r" } " rename · " kbd { "Del" } " delete"
            }), html! {
                (ui::search("q", "", "Search bookmarks", Some(".list")))
                a class="btn sm" href="bookmark://go/import-browse" title="Chrome / Chromium / Brave / Edge Bookmarks file" {
                    (Icon::Download) "Import…"
                }
            }))
            @if let Some(status) = status.filter(|s| !s.is_empty()) {
                (ui::banner(if status.contains("failed") { ui::Tone::Danger } else { ui::Tone::Success }, status))
            }
            div class="layout" {
                nav class="tree" aria-label="Folders" {
                    (tree_item("", "All bookmarks", 0, marks.len(), Icon::Star))
                    (tree_item("\u{0}", "Unfiled", 0, unfiled, Icon::File))
                    @for (path, count) in &tree {
                        @let depth = path.matches('/').count();
                        @let name = path.rsplit('/').next().unwrap_or(path);
                        (tree_item(path, name, depth, *count, Icon::Folder))
                    }
                }
                div {
                    div class="bar selection" {
                        span { strong data-sel-count { "0" } " selected" }
                        button type="button" class="btn sm danger" data-delete-selected { (Icon::Trash) "Delete" }
                        form class="cluster" {
                            input class="input" type="text" name="folder" placeholder="Folder (empty = unfiled)"
                                aria-label="Move to folder" autocomplete="off" spellcheck="false";
                            button class="btn sm" type="submit" { (Icon::Folder) "Move" }
                        }
                    }
                    form class="bar folder-bar" method="get" action="bookmark://rename-folder" {
                        input type="hidden" name="from";
                        input class="input" type="text" name="to" aria-label="Folder name"
                            autocomplete="off" spellcheck="false";
                        button class="btn sm" type="submit" { (Icon::Edit) "Rename folder" }
                        span class="small muted" { "Empty name unfiles it" }
                    }
                    @if marks.is_empty() {
                        (ui::empty(Icon::Star, html! { "No bookmarks yet — " kbd { "Ctrl+D" } " on a page, or Import from a browser." }))
                    }
                    div class="list" {
                        @for (index, b) in marks.iter().enumerate() { (row(index, b)) }
                    }
                    div class="empty hidden" data-filter-empty { (Icon::Search) div { "No bookmark matches." } }
                }
            }
        }
    };
    let html = ui::Doc::new(theme, "Bookmarks")
        .keys(&[
            ("Space / x", "Select row"),
            ("a", "Select all shown"),
            ("r / F2", "Rename / move"),
        ])
        .css(CSS)
        .script(SCRIPT)
        .render(body);
    super::serve(super::kind::BOOKMARKS, html)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::THEMES;
    use crate::pages::tests::served_html;

    #[test]
    fn bookmark_label_folder_and_host_are_escaped() {
        let marks = [
            Bookmark {
                url: "https://a.test/".into(),
                title: Some("<img src=x onerror=alert(1)>".into()),
                folder: Some("</h2><script>f()</script>".into()),
            },
            // No title: label falls back to the host of a web-chosen URL.
            Bookmark {
                url: "javascript:\"><svg onload=alert(2)>".into(),
                title: None,
                folder: None,
            },
        ];
        let html = served_html(&page_url(&THEMES[0], &marks, Some("<b>x</b>")));
        for raw in ["<img src=x", "<script>f()", "<svg onload", "<b>x</b>"] {
            assert!(!html.contains(raw), "{raw} in {html}");
        }
        assert!(html.contains("&lt;img src=x onerror=alert(1)&gt;"));
    }

    #[test]
    fn tree_rows_and_manager_actions() {
        let marks: Vec<Bookmark> = serde_json::from_value(serde_json::json!([
            { "url": "https://a.test/", "folder": "Rust/Async" },
            { "url": "https://b.test/" },
        ]))
        .unwrap();
        let html = served_html(&page_url(&THEMES[0], &marks, None));
        assert!(html.contains("data-folder=\"Rust\""));
        assert!(html.contains("data-folder=\"Rust/Async\""));
        assert!(html.contains("data-open=\"bookmark://open/1\""));
        assert!(html.contains("data-delete=\"bookmark://delete/0\""));
        assert!(html.contains("action=\"bookmark://rename/0\""));
        assert!(html.contains("action=\"bookmark://rename-folder\""));
        assert!(html.contains("bookmark://delete-many?ids="));
    }
}
