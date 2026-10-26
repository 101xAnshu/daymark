//! Domain-only site names. This module never keeps a path, query, title or
//! full URL; it extracts the registrable host from a web URL and rejects
//! non-web schemes and local addresses.

/// Extract the host from an http(s) URL. Returns `None` for non-web schemes,
/// malformed input, localhost and IP literals.
pub fn domain_from_url(url: &str) -> Option<String> {
    let url = url.trim();
    let (scheme, rest) = url.split_once("://")?;
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return None;
    }
    let authority = rest.split(['/', '?', '#']).next()?;
    if authority.is_empty() {
        return None;
    }
    let host = authority.rsplit('@').next()?.trim();
    let host = host.split(':').next()?.trim_end_matches('.');
    if host.is_empty() {
        return None;
    }
    let host = host.to_ascii_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host);
    if host == "localhost"
        || host.parse::<std::net::IpAddr>().is_ok()
        || !host.contains('.')
        || host.starts_with('.')
        || host.ends_with('.')
        || host.contains("..")
    {
        return None;
    }
    Some(host.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_only_the_host() {
        assert_eq!(
            domain_from_url("https://www.github.com/slint-ui/slint?tab=readme#top").as_deref(),
            Some("github.com")
        );
        assert_eq!(
            domain_from_url("http://Sub.Example.co.uk:8080/path").as_deref(),
            Some("sub.example.co.uk")
        );
        assert_eq!(
            domain_from_url("https://user:pass@Example.COM./").as_deref(),
            Some("example.com")
        );
    }

    #[test]
    fn rejects_non_web_and_local_addresses() {
        for url in [
            "file:///tmp/notes.txt",
            "about:blank",
            "chrome://settings",
            "moz-extension://abc/page.html",
            "data:text/html,hello",
            "javascript:alert(1)",
            "https://localhost:3000/",
            "https://127.0.0.1/",
            "https://[::1]/",
            "https://intranet/",
            "not a url",
            "",
        ] {
            assert_eq!(domain_from_url(url), None, "{url}");
        }
    }

    #[test]
    fn is_case_insensitive_and_strips_a_trailing_dot() {
        assert_eq!(
            domain_from_url("HTTPS://WWW.Example.ORG./x").as_deref(),
            Some("example.org")
        );
    }
}
