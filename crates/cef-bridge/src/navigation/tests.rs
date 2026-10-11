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
