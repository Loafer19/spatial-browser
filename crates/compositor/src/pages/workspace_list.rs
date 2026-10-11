// Ctrl+Shift+W workspace-slot list: switch / rename / delete / add. Live
// slots (not named snapshots). See workspaces.rs and PENDING_WORKSPACE_ACTION.

use super::ui::{self, html, Icon};
use crate::output::Theme;
use crate::persistence::workspaces::{WorkspaceStore, MAX_NAME_CHARS};

const CSS: &str = r#"
.slot {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  width: 30px;
  height: 30px;
  border: 1px solid var(--border);
  border-radius: var(--radius-sm);
  background: var(--surface-2);
  color: var(--fg);
  font-family: var(--mono);
  font-size: 13px;
  font-weight: 700;
}
.row.current .slot { background: var(--accent); border-color: var(--accent); color: var(--accent-fg); }
.name-line { display: flex; align-items: center; gap: 8px; min-width: 0; }
.rename { display: flex; align-items: center; gap: 4px; margin: 0; }
.rename input { flex: 1; height: 28px; font-size: 13.5px; }
.row.renaming .name-line { display: none; }
.row:not(.renaming) .rename { display: none; }
.row .sub { font-family: var(--font); }
/* Same width whether or not Switch / Delete apply, so the kbd column lines up. */
.row .actions { min-width: 88px; justify-content: flex-end; }
@media (max-width: 480px) { .row .meta { display: none; } }
"#;

// Rename in place: the pencil (or r / F2 on the selected row) swaps the
// name for a GET form to workspace://rename/<i>; Esc or leaving the field
// puts the name back.
const SCRIPT: &str = r#"
(function () {
  function startRename(row) {
    if (!row || !row.querySelector('.rename')) return;
    row.classList.add('renaming');
    var input = row.querySelector('.rename input');
    input.focus();
    input.select();
  }
  document.addEventListener('click', function (e) {
    var btn = e.target.closest && e.target.closest('[data-rename]');
    if (!btn) return;
    e.preventDefault();
    startRename(btn.closest('.row'));
  });
  document.addEventListener('focusout', function (e) {
    var form = e.target.closest && e.target.closest('.rename');
    if (!form) return;
    setTimeout(function () {
      if (!form.contains(document.activeElement)) form.closest('.row').classList.remove('renaming');
    }, 0);
  });
  document.addEventListener('keydown', function (e) {
    if (e.defaultPrevented || e.ctrlKey || e.metaKey || e.altKey) return;
    if (/^(INPUT|TEXTAREA|SELECT)$/.test(e.target.tagName)) return;
    if (e.key === 'r' || e.key === 'F2') { e.preventDefault(); startRename(ui.active()); }
  });
})();
"#;

fn page_count(visited: bool, pages: usize) -> String {
    match (visited, pages) {
        (false, _) => "Not opened yet".into(),
        (true, 0) => "No pages".into(),
        (true, 1) => "1 page".into(),
        (true, n) => format!("{n} pages"),
    }
}

pub(crate) fn page_url(theme: &Theme, store: &WorkspaceStore) -> String {
    let deletable = store.slots.len() > 1;
    let body = html! {
        main class="page" {
            (ui::head("Workspaces", Some(html! {
                "Hover the top edge for chips · " kbd { "Ctrl+1–9" } " switch · " kbd { "Ctrl+N" } " new"
            }), html! {
                a class="btn primary sm" href="workspace://save" { (Icon::Add) "New workspace" }
            }))
            @if store.slots.is_empty() {
                (ui::empty(Icon::Layers, html! { "No workspaces. " a href="workspace://save" { "Create one" } }))
            }
            div class="list" {
                @for (index, slot) in store.slots.iter().enumerate() {
                    @let active = slot.id == store.active;
                    @let label = slot.label();
                    @let load = format!("workspace://load/{index}");
                    @let delete = format!("workspace://delete/{index}");
                    div class={ "row card" @if active { " current" } }
                        data-open=(load)
                        data-delete=[deletable.then_some(&delete)]
                        data-text=(label) {
                        span class="slot" aria-hidden="true" { (slot.id) }
                        div class="main" {
                            div class="name-line" {
                                span class="title" { (label) }
                                @if active { span class="badge accent" { "active" } }
                            }
                            form class="rename" method="get" action=(format!("workspace://rename/{index}")) {
                                input type="text" name="name" value=(slot.name)
                                    placeholder=(format!("Workspace {}", slot.id))
                                    maxlength=(MAX_NAME_CHARS) aria-label="Workspace name"
                                    autocomplete="off" spellcheck="false";
                                (ui::icon_submit("Save name", Icon::Check))
                            }
                            span class="sub" {
                                (page_count(slot.visited, slot.pages.len()))
                                @if slot.visited { " · " (slot.theme) }
                            }
                        }
                        @if slot.id <= 9 {
                            span class="meta" { kbd { "Ctrl+" (slot.id) } }
                        }
                        div class="actions" {
                            button type="button" class="icon-btn" data-rename title="Rename" aria-label="Rename" {
                                (Icon::Edit)
                            }
                            @if !active {
                                (ui::icon_link(&load, "Switch", Icon::Play, false))
                            }
                            @if deletable {
                                (ui::icon_link(&delete, "Delete", Icon::Trash, true))
                            }
                        }
                    }
                }
            }
        }
    };
    let html = ui::Doc::new(theme, "Workspaces")
        .keys(&[("r / F2", "Rename selected")])
        .css(CSS)
        .script(SCRIPT)
        .render(body);
    super::serve(super::kind::WORKSPACES, html)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::THEMES;
    use crate::pages::tests::served_html;

    fn store(json: &str) -> WorkspaceStore {
        serde_json::from_str(json).expect("store")
    }

    #[test]
    fn rows_switch_rename_and_delete() {
        let s = store(
            r#"{"active":2,"slots":[{"id":1,"visited":true,"name":"Ops <b>x</b>"},{"id":2,"visited":true}]}"#,
        );
        let html = served_html(&page_url(&THEMES[0], &s));
        assert!(html.contains("data-open=\"workspace://load/0\""));
        assert!(html.contains("data-delete=\"workspace://delete/1\""));
        assert!(html.contains("action=\"workspace://rename/0\""));
        assert!(html.contains("href=\"workspace://save\""));
        assert!(
            html.contains("Ops &lt;b&gt;x&lt;/b&gt;"),
            "names are escaped"
        );
        assert!(!html.contains("<b>x</b>"));
        assert_eq!(html.matches("class=\"badge accent\">active").count(), 1);
    }

    #[test]
    fn last_workspace_cannot_be_deleted() {
        let s = store(r#"{"active":1,"slots":[{"id":1}]}"#);
        let html = served_html(&page_url(&THEMES[0], &s));
        assert!(!html.contains("workspace://delete/"));
        assert!(html.contains("Not opened yet"));
    }
}
