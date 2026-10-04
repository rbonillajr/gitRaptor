//! Secret redaction for values that leave the layer (SEC-05).

/// Remove the userinfo (`user`, `user:token`) from a remote URL.
///
/// Handles `scheme://userinfo@host/...` and the scp-like `user@host:path`. Local paths are
/// returned unchanged.
pub fn remote_url(url: &str) -> String {
    if let Some(scheme_end) = url.find("://") {
        let (scheme, rest) = url.split_at(scheme_end + 3);
        let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let (authority, tail) = rest.split_at(authority_end);
        return match authority.rfind('@') {
            Some(at) => format!("{scheme}{}{tail}", &authority[at + 1..]),
            None => url.to_owned(),
        };
    }
    // scp-like syntax: `[user@]host:path`, with no slash before the colon.
    if let Some(colon) = url.find(':') {
        let head = &url[..colon];
        if !head.contains('/')
            && !head.contains('\\')
            && let Some(at) = head.rfind('@')
        {
            return url[at + 1..].to_owned();
        }
    }
    url.to_owned()
}

#[cfg(test)]
mod tests {
    use super::remote_url;

    #[test]
    fn strips_userinfo() {
        assert_eq!(
            remote_url("https://user:ghp_secret@github.com/o/r.git"),
            "https://github.com/o/r.git"
        );
        assert_eq!(remote_url("https://tok@example.com"), "https://example.com");
        assert_eq!(remote_url("ssh://git@host:22/o/r"), "ssh://host:22/o/r");
        assert_eq!(remote_url("git@github.com:o/r.git"), "github.com:o/r.git");
        assert_eq!(
            remote_url("https://a:b@c@host/p?x=@y"),
            "https://host/p?x=@y"
        );
    }

    #[test]
    fn leaves_urls_without_userinfo() {
        for url in [
            "https://github.com/o/r.git",
            "/srv/git/r.git",
            "C:\\repos\\r",
            "file:///srv/r",
            "../r",
        ] {
            assert_eq!(remote_url(url), url);
        }
    }
}
