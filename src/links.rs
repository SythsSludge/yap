//! Finding links in chat text and deciding which hosts we trust enough to fetch from.

use url::Url;

/// A link found in a piece of text, as a byte range into that text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub start: usize,
    pub end: usize,
    pub url: String,
}

/// Find http(s) links. Like the web client, bare domains without a scheme are not linked.
pub fn find_links(text: &str) -> Vec<Link> {
    let mut finder = linkify::LinkFinder::new();
    finder.kinds(&[linkify::LinkKind::Url]);
    finder
        .links(text)
        .filter(|l| {
            let s = l.as_str();
            s.starts_with("http://") || s.starts_with("https://")
        })
        .map(|l| Link { start: l.start(), end: l.end(), url: l.as_str().to_owned() })
        .collect()
}

/// Image hosts that are trusted out of the box. All serve direct image files over HTTPS.
pub const DEFAULT_TRUSTED_DOMAINS: &[&str] = &[
    "static1.e621.net",
    "static1.e926.net",
    "d.furaffinity.net",
    "i.imgur.com",
    "cdn.discordapp.com",
    "media.discordapp.net",
    "pbs.twimg.com",
    "files.catbox.moe",
    "i.redd.it",
    "cdn.bsky.app",
];

/// Normalise a user-entered domain: lowercase, no scheme, path, port, wildcard prefix or
/// trailing dot. Returns `None` for input that can't be a hostname.
pub fn normalize_domain(input: &str) -> Option<String> {
    let mut s = input.trim().to_ascii_lowercase();
    if let Some(rest) = s.split_once("://").map(|(_, r)| r.to_owned()) {
        s = rest;
    }
    s = s.split(['/', '?', '#']).next().unwrap_or_default().to_owned();
    s = s.rsplit_once('@').map_or(s.clone(), |(_, h)| h.to_owned());
    if let Some((host, port)) = s.rsplit_once(':')
        && port.chars().all(|c| c.is_ascii_digit())
    {
        s = host.to_owned();
    }
    let s = s.trim_start_matches("*.").trim_start_matches('.').trim_end_matches('.');
    // Let the URL parser do IDN → punycode and reject garbage.
    let url = Url::parse(&format!("https://{s}/")).ok()?;
    let host = url.host_str()?.to_owned();
    (!host.is_empty() && host.contains('.')).then_some(host)
}

/// True when `host` is `domain` or a subdomain of it. `evile621.net` and
/// `e621.net.attacker.com` must not match `e621.net`.
pub fn host_matches(host: &str, domain: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let domain = domain.to_ascii_lowercase();
    host == domain || host.strip_suffix(&domain).is_some_and(|prefix| prefix.ends_with('.'))
}

/// Why a URL may or may not be fetched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trust {
    Trusted,
    UntrustedHost,
    InsecureScheme,
    NotHttp,
}

pub fn check_trust(url: &Url, trusted: &[String], https_only: bool) -> Trust {
    match url.scheme() {
        "https" => {}
        "http" if !https_only => {}
        "http" => return Trust::InsecureScheme,
        _ => return Trust::NotHttp,
    }
    // Userinfo in an image URL is never legitimate and is a classic spoofing trick.
    if !url.username().is_empty() || url.password().is_some() {
        return Trust::UntrustedHost;
    }
    match url.host_str() {
        Some(host) if trusted.iter().any(|d| host_matches(host, d)) => Trust::Trusted,
        _ => Trust::UntrustedHost,
    }
}

/// Whether the URL path looks like a direct image file.
pub fn looks_like_image(url: &Url) -> bool {
    let path = url.path().to_ascii_lowercase();
    [".png", ".jpg", ".jpeg", ".gif", ".webp"].iter().any(|ext| path.ends_with(ext))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trusted() -> Vec<String> {
        vec!["e621.net".into(), "i.imgur.com".into()]
    }

    #[test]
    fn finds_http_links_only() {
        let text = "see https://e621.net/posts/1 and http://x.com/a?b=c, not ftp://nope or example.com";
        let links = find_links(text);
        let urls: Vec<_> = links.iter().map(|l| l.url.as_str()).collect();
        assert_eq!(urls, vec!["https://e621.net/posts/1", "http://x.com/a?b=c"]);
        assert_eq!(&text[links[0].start..links[0].end], "https://e621.net/posts/1");
    }

    #[test]
    fn link_offsets_survive_multibyte_text() {
        let text = "🦊 ref → https://i.imgur.com/a.png ✨";
        let l = &find_links(text)[0];
        assert_eq!(&text[l.start..l.end], "https://i.imgur.com/a.png");
    }

    #[test]
    fn host_matching_resists_lookalikes() {
        assert!(host_matches("e621.net", "e621.net"));
        assert!(host_matches("static1.e621.net", "e621.net"));
        assert!(host_matches("STATIC1.E621.NET.", "e621.net"));
        assert!(!host_matches("evile621.net", "e621.net"));
        assert!(!host_matches("e621.net.attacker.com", "e621.net"));
        assert!(!host_matches("net", "e621.net"));
    }

    #[test]
    fn trust_checks_scheme_host_and_userinfo() {
        let t = trusted();
        let check = |u: &str, https_only| check_trust(&Url::parse(u).unwrap(), &t, https_only);
        assert_eq!(check("https://static1.e621.net/data/a.png", true), Trust::Trusted);
        assert_eq!(check("http://static1.e621.net/data/a.png", true), Trust::InsecureScheme);
        assert_eq!(check("http://static1.e621.net/data/a.png", false), Trust::Trusted);
        assert_eq!(check("https://evil.com/a.png", true), Trust::UntrustedHost);
        assert_eq!(check("https://e621.net@evil.com/a.png", true), Trust::UntrustedHost);
        assert_eq!(check("https://user@e621.net/a.png", true), Trust::UntrustedHost);
        assert_eq!(check("file:///etc/passwd", true), Trust::NotHttp);
    }

    #[test]
    fn normalizes_user_entered_domains() {
        assert_eq!(normalize_domain("E621.net").as_deref(), Some("e621.net"));
        assert_eq!(normalize_domain("*.e621.net").as_deref(), Some("e621.net"));
        assert_eq!(normalize_domain("https://i.imgur.com/abc.png").as_deref(), Some("i.imgur.com"));
        assert_eq!(normalize_domain("cdn.example.com:8443").as_deref(), Some("cdn.example.com"));
        assert_eq!(normalize_domain("bücher.example").as_deref(), Some("xn--bcher-kva.example"));
        assert_eq!(normalize_domain("localhost"), None);
        assert_eq!(normalize_domain("  "), None);
        assert_eq!(normalize_domain("has space.com"), None);
    }

    #[test]
    fn default_domains_are_normalized() {
        for d in DEFAULT_TRUSTED_DOMAINS {
            assert_eq!(normalize_domain(d).as_deref(), Some(*d));
        }
    }

    #[test]
    fn detects_image_paths() {
        let is_img = |u: &str| looks_like_image(&Url::parse(u).unwrap());
        assert!(is_img("https://i.imgur.com/abc.PNG"));
        assert!(is_img("https://cdn.discordapp.com/x/y.webp?ex=1&is=2"));
        assert!(!is_img("https://e621.net/posts/123"));
        assert!(!is_img("https://example.com/png"));
    }
}
