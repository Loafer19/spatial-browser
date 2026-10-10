// Hand-offs to the rest of the desktop: open a URL in the system browser
// (sites that need H.264/AAC, which the minimal CEF build lacks).

/// http(s) only: anything else could make xdg-open run a local handler.
pub fn is_web_url(url: &str) -> bool {
    let lower = url.trim_start().to_ascii_lowercase();
    (lower.starts_with("https://") || lower.starts_with("http://")) && !url.chars().any(char::is_whitespace)
}

/// Open `url` with the desktop's default browser; ignores non-web URLs.
pub fn open_in_system_browser(url: &str) {
    if !is_web_url(url) {
        log::warn!("not opening non-web URL externally: {url}");
        return;
    }
    if let Err(e) = std::process::Command::new("xdg-open").arg(url).spawn() {
        log::warn!("xdg-open {url}: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::is_web_url;

    #[test]
    fn only_plain_web_urls() {
        assert!(is_web_url("https://twitch.tv/x"));
        assert!(is_web_url("HTTP://a.test"));
        assert!(!is_web_url("file:///etc/passwd"));
        assert!(!is_web_url("javascript:alert(1)"));
        assert!(!is_web_url("-https://a.test"));
        assert!(!is_web_url("https://a.test/ --flag"));
        assert!(!is_web_url(""));
    }
}
