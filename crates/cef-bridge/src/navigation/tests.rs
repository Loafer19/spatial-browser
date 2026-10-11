use super::query::percent_decode;
use super::*;

#[test]
fn percent_decode_handles_plus_utf8_and_malformed() {
    assert_eq!(percent_decode("a+b%20c%2B"), "a b c+");
    assert_eq!(percent_decode("%D0%BF%D1%80%D0%B8"), "при");
    assert_eq!(percent_decode("100%zz"), "100%zz");
    assert_eq!(percent_decode("tail%4"), "tail%4");
}

#[test]
fn query_param_finds_exact_key() {
    assert_eq!(query_param("a=1&ab=2&b=x%20y", "b").as_deref(), Some("x y"));
    assert_eq!(query_param("a=1", "missing"), None);
}

#[test]
fn password_query_accepts_go_and_legacy_forms() {
    for url in [
        "password://go/query?origin=https%3A%2F%2Fa.test",
        "password://query?origin=https%3A%2F%2Fa.test",
        "password://query/?origin=https%3A%2F%2Fa.test",
    ] {
        assert!(
            matches!(parse_password_action(url), Some(PasswordAction::Query { origin }) if origin == "https://a.test"),
            "{url}"
        );
    }
}

#[test]
fn password_save_offer_is_not_parsed_as_save() {
    assert!(matches!(
        parse_password_action("password://go/save-offer?origin=o&username=u&password=p"),
        Some(PasswordAction::SaveOffer { .. })
    ));
    assert!(matches!(
        parse_password_action("password://go/save?origin=o&username=u&password=p&id="),
        Some(PasswordAction::Save { id: None, .. })
    ));
}

#[test]
fn password_unlock_flags() {
    let Some(PasswordAction::Unlock {
        password,
        create,
        confirm,
    }) = parse_password_action("password://go/unlock?create=1&password=a%2Bb&confirm=a%2Bb")
    else {
        panic!("expected unlock");
    };
    assert_eq!(password, "a+b");
    assert!(create);
    assert_eq!(confirm.as_deref(), Some("a+b"));
}

#[test]
fn password_required_params_and_unknown_actions() {
    assert!(parse_password_action("password://go/fill?nope=1").is_none());
    assert!(parse_password_action("password://go/explode?x=1").is_none());
    assert!(matches!(
        parse_password_action("password://go/lock"),
        Some(PasswordAction::Lock)
    ));
    assert!(parse_password_action("password://go/locked").is_none());
    assert!(parse_password_action("https://password.test/go/fill?id=1").is_none());
}

#[test]
fn password_signals_only_from_main_or_bridge_iframe() {
    assert!(signal_frame_ok(true, false, "https://bank.test/"));
    assert!(signal_frame_ok(false, true, "about:blank"));
    assert!(signal_frame_ok(false, true, ""));
    // Ad iframe navigating itself.
    assert!(!signal_frame_ok(false, true, "https://ads.test/slot"));
    // Iframe created inside an ad iframe.
    assert!(!signal_frame_ok(false, false, "about:blank"));
}

#[test]
fn find_actions() {
    assert!(matches!(
        parse_find_action("find://go/search?q=foo%20bar&dir=prev&case=1"),
        Some(FindAction::Search { text, forward: false, match_case: true }) if text == "foo bar"
    ));
    assert!(matches!(
        parse_find_action("find://go/search/?q=x"),
        Some(FindAction::Search {
            forward: true,
            match_case: false,
            ..
        })
    ));
    assert!(matches!(
        parse_find_action("find://go/close"),
        Some(FindAction::Close)
    ));
    assert!(parse_find_action("find://go/explode").is_none());
    assert!(parse_find_action("https://find.test/go/close").is_none());
}

#[test]
fn clipboard_copy_decodes_text() {
    assert_eq!(
        parse_clipboard_copy("clipboard://copy?text=hello%20%F0%9F%91%8B").as_deref(),
        Some("hello 👋")
    );
    assert_eq!(parse_clipboard_copy("clipboard://paste?text=x"), None);
}

#[test]
fn history_search_action() {
    for url in [
        "history://search?q=rust%20book&offset=40&collapse=1",
        "history://search/?q=rust+book&offset=40&collapse=1",
    ] {
        assert!(
            matches!(parse_history_action(url), Some(HistoryPageAction::Search { query, offset: 40, collapse: true }) if query == "rust book"),
            "{url}"
        );
    }
    assert!(matches!(
        parse_history_action("history://search?offset=x"),
        Some(HistoryPageAction::Search {
            offset: 0,
            collapse: false,
            ..
        })
    ));
    assert!(matches!(
        parse_history_action("history://remove-all/3"),
        Some(HistoryPageAction::RemoveAll(3))
    ));
    assert!(parse_history_action("history://searching").is_none());
}

#[test]
fn omnibox_suggest_action() {
    assert_eq!(
        parse_omnibox_suggest("omnibox://suggest?q=gith%20ub").as_deref(),
        Some("gith ub")
    );
    assert_eq!(
        parse_omnibox_suggest("omnibox://suggest/?q=x").as_deref(),
        Some("x")
    );
    assert!(parse_omnibox_suggest("omnibox://go?q=x&url=y").is_none());
    assert!(parse_omnibox_submit("omnibox://suggest?q=x").is_none());
}

#[test]
fn palette_actions_parse_and_reject_garbage() {
    for url in ["palette://go/42", "palette://go/42/"] {
        assert!(
            matches!(parse_palette_action(url), Some(PaletteAction::Go(42))),
            "{url}"
        );
    }
    for url in ["palette://run/new-page", "palette://run/new-page/"] {
        assert!(
            matches!(parse_palette_action(url), Some(PaletteAction::Run(id)) if id == "new-page"),
            "{url}"
        );
    }
    for url in [
        "palette://open?url=https%3A%2F%2Fa.test%2F%3Fx%3D1",
        "palette://open/?url=https%3A%2F%2Fa.test%2F%3Fx%3D1",
    ] {
        assert!(
            matches!(parse_palette_action(url), Some(PaletteAction::Open(u)) if u == "https://a.test/?x=1"),
            "{url}"
        );
    }
    for url in ["palette://close", "palette://close/"] {
        assert!(
            matches!(parse_palette_action(url), Some(PaletteAction::Close)),
            "{url}"
        );
    }
    let long = format!("palette://run/{}", "a".repeat(41));
    for url in [
        "palette://go/x",
        "palette://run/",
        "palette://run/New-Page",
        "palette://run/a%2Fb",
        "palette://run/../settings",
        long.as_str(),
        "palette://open?url=",
        "palette://open?url=%20",
        "palette://open",
        "palette://nope",
        "switcher://go/1",
    ] {
        assert!(parse_palette_action(url).is_none(), "{url}");
    }
}
