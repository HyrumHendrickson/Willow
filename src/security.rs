//! Input validation shared by the model loader, the exporter and the editor.
//!
//! Everything that ends up in generated HTML or in a file-system path passes through
//! one of these functions. They are deliberately conservative: when in doubt, reject.

/// Slugs that would collide with the generated site structure.
pub const RESERVED_SLUGS: [&str; 8] = ["header", "footer", "media", "config", "index", "assets", "css", "404"];

/// Largest JSON document the editor will read, in bytes.
pub const MAX_JSON_BYTES: u64 = 20 * 1024 * 1024;

/// Largest amount of text accepted in a single text element or table cell.
pub const MAX_TEXT_CHARS: usize = 100_000;

/// A page folder name: lower-case ASCII letters, digits and single dashes.
pub fn valid_slug(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 60
        && !s.starts_with('-')
        && !s.ends_with('-')
        && !s.contains("--")
        && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !RESERVED_SLUGS.contains(&s)
}

/// A BCP 47 style language tag, such as `en` or `pt-BR`.
pub fn valid_lang(s: &str) -> bool {
    !s.is_empty() && s.len() <= 12 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// A site-relative media reference such as `media/photo.png`.
pub fn safe_media_path(s: &str) -> bool {
    let Some(rest) = s.strip_prefix("media/") else {
        return false;
    };
    !rest.is_empty()
        && !rest.contains('/')
        && !rest.contains('\\')
        && !rest.contains(':')
        && rest != "."
        && rest != ".."
        && !rest.starts_with('.')
        && !s.chars().any(|c| c.is_control())
}

/// Splits `scheme:` off the front of a URL, if present.
fn scheme_of(s: &str) -> Option<&str> {
    let colon = s.find(':')?;
    let scheme = &s[..colon];
    let first_ok = scheme.chars().next().is_some_and(|c| c.is_ascii_alphabetic());
    if first_ok && scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')) {
        Some(scheme)
    } else {
        None
    }
}

/// Validates a link target. Allowed: http(s), mailto, tel, in-page anchors and
/// relative paths. Rejects `javascript:`, `data:`, `vbscript:`, `file:` and
/// protocol-relative URLs.
pub fn safe_href(raw: &str) -> Result<String, String> {
    let s = raw.trim();
    if s.is_empty() {
        return Err("empty link".into());
    }
    if s.len() > 2048 {
        return Err("link is too long".into());
    }
    if s.chars().any(|c| c.is_control()) {
        return Err("link contains control characters".into());
    }
    if s.starts_with("//") || s.starts_with('\\') {
        return Err("protocol-relative links are not allowed; use https://".into());
    }
    if let Some(scheme) = scheme_of(s) {
        let scheme = scheme.to_ascii_lowercase();
        if !matches!(scheme.as_str(), "http" | "https" | "mailto" | "tel") {
            return Err(format!("'{scheme}:' links are not allowed"));
        }
    }
    Ok(s.to_string())
}

/// True for links that leave the site and should carry `rel="noopener noreferrer"`.
pub fn is_external(href: &str) -> bool {
    scheme_of(href).is_some_and(|s| s.eq_ignore_ascii_case("http") || s.eq_ignore_ascii_case("https"))
}

// ---------------------------------------------------------------- embeds

struct Parsed<'a> {
    host: String,
    path: &'a str,
    query: &'a str,
}

fn parse_https(url: &str) -> Result<Parsed<'_>, String> {
    let lower_prefix = url.get(..8).map(|p| p.to_ascii_lowercase());
    if lower_prefix.as_deref() != Some("https://") {
        return Err("embed addresses must start with https://".into());
    }
    let rest = &url[8..];
    let host_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let host = rest[..host_end].to_ascii_lowercase();
    if host.is_empty()
        || host.contains('@')
        || host.contains(':')
        || !host.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-')
    {
        return Err("invalid host".into());
    }
    let tail = &rest[host_end..];
    let tail = tail.split('#').next().unwrap_or("");
    let (path, query) = match tail.split_once('?') {
        Some((p, q)) => (p, q),
        None => (tail, ""),
    };
    let path = if path.is_empty() { "/" } else { path };
    Ok(Parsed { host, path, query })
}

fn query_param<'a>(query: &'a str, key: &str) -> Option<&'a str> {
    query.split('&').find_map(|kv| kv.split_once('=').filter(|(k, _)| *k == key).map(|(_, v)| v))
}

fn video_id(s: &str) -> Option<&str> {
    let ok = (6..=20).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    ok.then_some(s)
}

/// Pulls the `src` out of pasted `<iframe ...>` markup, otherwise returns the input.
fn unwrap_iframe(raw: &str) -> &str {
    let t = raw.trim();
    if t.len() > 7 && t[..7].eq_ignore_ascii_case("<iframe") {
        if let Some(i) = t.find("src=\"") {
            let rest = &t[i + 5..];
            if let Some(j) = rest.find('"') {
                return &rest[..j];
            }
        }
    }
    t
}

/// Turns a pasted address into a safe, canonical embed URL. Only a short list of
/// well-known services is accepted; everything else is refused.
pub fn normalize_embed(raw: &str) -> Result<String, String> {
    let url = unwrap_iframe(raw);
    if url.is_empty() {
        return Err("enter an address".into());
    }
    if url.len() > 2048
        || url.chars().any(|c| c.is_control() || c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>'))
    {
        return Err("address contains invalid characters".into());
    }
    let u = parse_https(url)?;
    let canonical = |host: &str, path: &str, query: &str| {
        if query.is_empty() {
            format!("https://{host}{path}")
        } else {
            format!("https://{host}{path}?{query}")
        }
    };
    match u.host.as_str() {
        "www.youtube.com" | "youtube.com" | "m.youtube.com" => {
            let id = if u.path == "/watch" {
                query_param(u.query, "v")
            } else {
                u.path.strip_prefix("/embed/").or_else(|| u.path.strip_prefix("/shorts/"))
            };
            let id = id.and_then(video_id).ok_or("could not find a YouTube video id")?;
            Ok(format!("https://www.youtube-nocookie.com/embed/{id}"))
        }
        "youtu.be" => {
            let id = u.path.trim_start_matches('/');
            let id = video_id(id).ok_or("could not find a YouTube video id")?;
            Ok(format!("https://www.youtube-nocookie.com/embed/{id}"))
        }
        "www.youtube-nocookie.com" => {
            let id = u.path.strip_prefix("/embed/").and_then(video_id).ok_or("could not find a YouTube video id")?;
            Ok(format!("https://www.youtube-nocookie.com/embed/{id}"))
        }
        "vimeo.com" | "www.vimeo.com" | "player.vimeo.com" => {
            let id = u.path.strip_prefix("/video/").unwrap_or(u.path.trim_start_matches('/'));
            if id.is_empty() || id.len() > 15 || !id.bytes().all(|b| b.is_ascii_digit()) {
                return Err("could not find a Vimeo video id".into());
            }
            Ok(format!("https://player.vimeo.com/video/{id}"))
        }
        "www.google.com" | "google.com" if u.path.starts_with("/maps/embed") => {
            Ok(canonical("www.google.com", u.path, u.query))
        }
        "calendar.google.com" if u.path.starts_with("/calendar/embed") => Ok(canonical(&u.host, u.path, u.query)),
        "docs.google.com" if u.path.starts_with("/forms/") => Ok(canonical(&u.host, u.path, u.query)),
        "www.openstreetmap.org" if u.path == "/export/embed.html" => Ok(canonical(&u.host, u.path, u.query)),
        _ => Err("supported: YouTube, Vimeo, Google Maps (Share > Embed a map), Google Calendar, Google Forms, OpenStreetMap".into()),
    }
}

/// `https://host` for a normalised embed URL, used to build `frame-src`.
pub fn origin_of(url: &str) -> Option<String> {
    let u = parse_https(url).ok()?;
    Some(format!("https://{}", u.host))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs() {
        assert!(valid_slug("about-us"));
        assert!(valid_slug("2026"));
        for bad in ["", "../x", "a/b", "A", "-a", "a-", "a--b", "media", "404", "a b", "é", &"x".repeat(61)] {
            assert!(!valid_slug(bad), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn hrefs() {
        for ok in [
            "https://example.com/a?b=c",
            "http://x.org",
            "mailto:a@b.co",
            "tel:+1555",
            "#top",
            "about/",
            "./",
            "a/b.html",
        ] {
            assert!(safe_href(ok).is_ok(), "{ok}");
        }
        for bad in [
            "javascript:alert(1)",
            "JaVaScRiPt:alert(1)",
            " javascript:alert(1)",
            "data:text/html,x",
            "vbscript:x",
            "file:///etc/passwd",
            "//evil.example",
            "\\\\evil",
            "java\tscript:alert(1)",
            "",
        ] {
            assert!(safe_href(bad).is_err(), "{bad:?}");
        }
        assert!(is_external("https://a.b") && !is_external("mailto:a@b.c") && !is_external("about/"));
    }

    #[test]
    fn media() {
        assert!(safe_media_path("media/a.png"));
        for bad in [
            "a.png",
            "media/",
            "media/../x",
            "media/a/b.png",
            "/media/a.png",
            "media/.hidden",
            "media/a:b",
            "media\\a.png",
        ] {
            assert!(!safe_media_path(bad), "{bad}");
        }
    }

    #[test]
    fn embeds() {
        let yt = "https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ";
        assert_eq!(normalize_embed("https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=3").unwrap(), yt);
        assert_eq!(normalize_embed("https://youtu.be/dQw4w9WgXcQ").unwrap(), yt);
        assert_eq!(normalize_embed("https://vimeo.com/123456789").unwrap(), "https://player.vimeo.com/video/123456789");
        let map = "https://www.google.com/maps/embed?pb=!1m18!1m12";
        assert_eq!(normalize_embed(map).unwrap(), map);
        assert_eq!(normalize_embed(&format!("<iframe src=\"{map}\" width=\"600\"></iframe>")).unwrap(), map);
        for bad in [
            "http://www.youtube.com/watch?v=dQw4w9WgXcQ",
            "https://evil.example/embed",
            "https://www.google.com/search?q=x",
            "https://www.youtube.com.evil.example/watch?v=dQw4w9WgXcQ",
            "https://user@www.youtube.com/watch?v=dQw4w9WgXcQ",
            "https://www.youtube.com:8080/watch?v=dQw4w9WgXcQ",
            "https://www.youtube.com/watch?v=x\"onload=\"alert(1)",
            "javascript:alert(1)",
            "",
        ] {
            assert!(normalize_embed(bad).is_err(), "{bad:?}");
        }
        assert_eq!(origin_of(yt).as_deref(), Some("https://www.youtube-nocookie.com"));
    }
}
