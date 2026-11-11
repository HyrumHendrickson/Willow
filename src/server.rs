//! Minimal local static file server so the site can be previewed over HTTP
//! (header/footer are loaded with `fetch`, which does not work from `file://`).
//!
//! The server is meant for one person on one machine and is hardened accordingly:
//!
//! * binds to the loopback interface only;
//! * rejects requests whose `Host` header is not `127.0.0.1:<port>` or `localhost:<port>`,
//!   which defeats DNS-rebinding attacks from web pages open in the same browser;
//! * answers `GET` and `HEAD` only;
//! * confines every path to the site folder (the path is canonicalised, so symlinks
//!   cannot lead out of it) and hides dot-files such as `.git`;
//! * bounds header size, read/write time and the number of simultaneous connections;
//! * streams files instead of loading them into memory.

use std::fs;
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const MAX_CONNECTIONS: usize = 32;
const MAX_HEADER_BYTES: u64 = 16 * 1024;
const MAX_HEADER_LINES: usize = 100;
const MAX_RANGE_CHUNK: u64 = 8 * 1024 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(10);

const COMMON_HEADERS: &str = "X-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\n\
Cross-Origin-Resource-Policy: same-origin\r\nCache-Control: no-store\r\nConnection: close\r\n";

pub struct Server {
    pub port: u16,
    root: Arc<Mutex<PathBuf>>,
}

impl Server {
    pub fn start() -> io::Result<Server> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        let root = Arc::new(Mutex::new(PathBuf::new()));
        let shared = root.clone();
        let active = Arc::new(AtomicUsize::new(0));
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                if active.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
                    active.fetch_sub(1, Ordering::SeqCst);
                    let mut s = stream;
                    let _ = respond(&mut s, "503 Service Unavailable", "", b"busy");
                    continue;
                }
                let root = shared.lock().map(|r| r.clone()).unwrap_or_default();
                let counter = active.clone();
                thread::spawn(move || {
                    let _ = handle(stream, &root, port);
                    counter.fetch_sub(1, Ordering::SeqCst);
                });
            }
        });
        Ok(Server { port, root })
    }

    pub fn set_root(&self, path: &Path) {
        if let Ok(mut r) = self.root.lock() {
            *r = path.to_path_buf();
        }
    }
}

fn mime(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase()).as_deref() {
        Some("html") | Some("htm") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("json") => "application/json",
        Some("txt") => "text/plain; charset=utf-8",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("svg") => "image/svg+xml",
        Some("mp4") | Some("m4v") => "video/mp4",
        Some("webm") => "video/webm",
        Some("ogv") | Some("ogg") => "video/ogg",
        Some("mov") => "video/quicktime",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    }
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok());
            if let Some(v) = hex {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn respond(stream: &mut TcpStream, status: &str, headers: &str, body: &[u8]) -> io::Result<()> {
    write!(stream, "HTTP/1.1 {status}\r\n{headers}{COMMON_HEADERS}Content-Length: {}\r\n\r\n", body.len())?;
    stream.write_all(body)
}

#[derive(Debug, PartialEq)]
enum Resolved {
    File(PathBuf),
    /// Directory requested without a trailing slash.
    Redirect(String),
}

#[derive(Debug, PartialEq)]
enum Reject {
    Forbidden,
    NotFound,
}

/// Maps a request target onto a file inside `root`, or says why it cannot.
fn resolve(root: &Path, target: &str) -> Result<Resolved, Reject> {
    let path_part = percent_decode(target.split(['?', '#']).next().unwrap_or("/"));
    if path_part.chars().any(|c| c.is_control()) || !path_part.starts_with('/') {
        return Err(Reject::Forbidden);
    }
    let mut full = root.to_path_buf();
    let mut rel: Vec<String> = Vec::new();
    for c in Path::new(path_part.trim_start_matches('/')).components() {
        match c {
            Component::Normal(n) => {
                let n = n.to_string_lossy();
                if n.starts_with('.') {
                    return Err(Reject::NotFound); // .git, .env, ...
                }
                full.push(n.as_ref());
                rel.push(n.into_owned());
            }
            Component::CurDir => {}
            _ => return Err(Reject::Forbidden),
        }
    }
    let root_canon = root.canonicalize().map_err(|_| Reject::NotFound)?;
    if fs::metadata(&full).map_err(|_| Reject::NotFound)?.is_dir() {
        if !path_part.ends_with('/') {
            // Built from the validated components, never from raw input.
            return Ok(Resolved::Redirect(format!("/{}/", rel.join("/"))));
        }
        full.push("index.html");
    }
    let canon = full.canonicalize().map_err(|_| Reject::NotFound)?;
    if !canon.starts_with(&root_canon) {
        return Err(Reject::Forbidden);
    }
    if !canon.is_file() {
        return Err(Reject::NotFound);
    }
    Ok(Resolved::File(canon))
}

/// Parses a single `bytes=` range. `None` means "no usable range, send the whole file".
fn parse_range(spec: &str, len: u64) -> Option<Result<(u64, u64), ()>> {
    let r = spec.strip_prefix("bytes=")?;
    if r.contains(',') {
        return None;
    }
    let (a, b) = r.split_once('-')?;
    let last = len.checked_sub(1)?;
    let (start, end) = if a.is_empty() {
        let n: u64 = b.parse().ok()?;
        (len.saturating_sub(n), last)
    } else {
        let start: u64 = a.parse().ok()?;
        let end = if b.is_empty() { last } else { b.parse::<u64>().ok()?.min(last) };
        (start, end)
    };
    if start > end || start >= len {
        return Some(Err(()));
    }
    Some(Ok((start, end.min(start + MAX_RANGE_CHUNK - 1))))
}

fn handle(mut stream: TcpStream, root: &Path, port: u16) -> io::Result<()> {
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;
    let mut reader = BufReader::new(stream.try_clone()?.take(MAX_HEADER_BYTES));
    let mut request_line = String::new();
    if reader.read_line(&mut request_line)? == 0 {
        return Ok(());
    }
    let (mut range, mut host): (Option<String>, Option<String>) = (None, None);
    let mut complete = false;
    for _ in 0..MAX_HEADER_LINES {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        if line == "\r\n" || line == "\n" {
            complete = true;
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            match name.trim().to_ascii_lowercase().as_str() {
                "range" => range = Some(value.trim().to_ascii_lowercase()),
                "host" => host = Some(value.trim().to_ascii_lowercase()),
                _ => {}
            }
        }
    }
    if !complete {
        return respond(&mut stream, "431 Request Header Fields Too Large", "", b"headers too large");
    }
    let allowed = [format!("127.0.0.1:{port}"), format!("localhost:{port}")];
    if !host.is_some_and(|h| allowed.contains(&h)) {
        return respond(&mut stream, "403 Forbidden", "Content-Type: text/plain\r\n", b"forbidden host");
    }
    let mut parts = request_line.split_whitespace();
    let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or("/"));
    if method != "GET" && method != "HEAD" {
        return respond(&mut stream, "405 Method Not Allowed", "Allow: GET, HEAD\r\n", b"");
    }
    let head_only = method == "HEAD";

    let path = match resolve(root, target) {
        Ok(Resolved::File(p)) => p,
        Ok(Resolved::Redirect(to)) => {
            return respond(&mut stream, "301 Moved Permanently", &format!("Location: {to}\r\n"), b"");
        }
        Err(Reject::Forbidden) => return respond(&mut stream, "403 Forbidden", "", b"forbidden"),
        Err(Reject::NotFound) => {
            return respond(&mut stream, "404 Not Found", "Content-Type: text/plain\r\n", b"not found");
        }
    };
    let Ok(mut file) = fs::File::open(&path) else {
        return respond(&mut stream, "404 Not Found", "Content-Type: text/plain\r\n", b"not found");
    };
    let len = file.metadata()?.len();
    let ctype = format!("Content-Type: {}\r\nAccept-Ranges: bytes\r\n", mime(&path));

    let (status, extra, start, count) = match range.as_deref().and_then(|r| parse_range(r, len)) {
        Some(Ok((s, e))) => ("206 Partial Content", format!("Content-Range: bytes {s}-{e}/{len}\r\n"), s, e - s + 1),
        Some(Err(())) => {
            return respond(
                &mut stream,
                "416 Range Not Satisfiable",
                &format!("Content-Range: bytes */{len}\r\n"),
                b"",
            );
        }
        None => ("200 OK", String::new(), 0, len),
    };
    write!(stream, "HTTP/1.1 {status}\r\n{ctype}{extra}{COMMON_HEADERS}Content-Length: {count}\r\n\r\n")?;
    if !head_only {
        file.seek(SeekFrom::Start(start))?;
        io::copy(&mut file.take(count), &mut stream)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("willow_srv_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(d.join("sub")).unwrap();
        fs::write(d.join("index.html"), "home").unwrap();
        fs::write(d.join("sub/index.html"), "sub").unwrap();
        fs::write(d.join("a b.txt"), "spaced").unwrap();
        fs::create_dir_all(d.join(".git")).unwrap();
        fs::write(d.join(".git/config"), "secret").unwrap();
        d
    }

    #[test]
    fn decodes_percent_escapes() {
        assert_eq!(percent_decode("/a%20b"), "/a b");
        assert_eq!(percent_decode("/%2e%2e/x"), "/../x");
        assert_eq!(percent_decode("/100%"), "/100%");
        assert_eq!(percent_decode("/%zz"), "/%zz");
        assert_eq!(percent_decode("a%41"), "aA");
    }

    #[test]
    fn resolves_inside_root_only() {
        let root = tmp("resolve");
        let canon = |p: &str| root.canonicalize().unwrap().join(p);
        assert_eq!(resolve(&root, "/"), Ok(Resolved::File(canon("index.html"))));
        assert_eq!(resolve(&root, "/sub/"), Ok(Resolved::File(canon("sub/index.html"))));
        assert_eq!(resolve(&root, "/sub"), Ok(Resolved::Redirect("/sub/".into())));
        assert_eq!(resolve(&root, "/a%20b.txt"), Ok(Resolved::File(canon("a b.txt"))));
        assert_eq!(resolve(&root, "/index.html?x=1#y"), Ok(Resolved::File(canon("index.html"))));
        assert_eq!(resolve(&root, "/../x"), Err(Reject::Forbidden));
        assert_eq!(resolve(&root, "/%2e%2e/%2e%2e/etc/passwd"), Err(Reject::Forbidden));
        assert_eq!(resolve(&root, "/sub/../../x"), Err(Reject::Forbidden));
        assert_eq!(resolve(&root, "/.git/config"), Err(Reject::NotFound));
        assert_eq!(resolve(&root, "/missing.html"), Err(Reject::NotFound));
        // header injection and open-redirect attempts
        assert_eq!(resolve(&root, "/sub%0d%0aSet-Cookie:x"), Err(Reject::Forbidden));
        assert_eq!(resolve(&root, "//evil.example/"), Err(Reject::NotFound));
        assert_eq!(resolve(&root, "evil"), Err(Reject::Forbidden));
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_cannot_escape_the_root() {
        let root = tmp("symlink");
        let outside = tmp("symlink_outside");
        fs::write(outside.join("secret.txt"), "secret").unwrap();
        std::os::unix::fs::symlink(outside.join("secret.txt"), root.join("link.txt")).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("dir")).unwrap();
        assert_eq!(resolve(&root, "/link.txt"), Err(Reject::Forbidden));
        assert_eq!(resolve(&root, "/dir/secret.txt"), Err(Reject::Forbidden));
    }

    #[test]
    fn parses_ranges() {
        assert_eq!(parse_range("bytes=0-9", 100), Some(Ok((0, 9))));
        assert_eq!(parse_range("bytes=90-", 100), Some(Ok((90, 99))));
        assert_eq!(parse_range("bytes=-10", 100), Some(Ok((90, 99))));
        assert_eq!(parse_range("bytes=0-9999", 100), Some(Ok((0, 99))));
        assert_eq!(parse_range("bytes=100-", 100), Some(Err(())));
        assert_eq!(parse_range("bytes=5-2", 100), Some(Err(())));
        assert_eq!(parse_range("bytes=0-1,5-6", 100), None);
        assert_eq!(parse_range("items=0-1", 100), None);
        assert_eq!(parse_range("bytes=0-", 0), None);
        assert_eq!(parse_range("bytes=0-", 100_000_000), Some(Ok((0, MAX_RANGE_CHUNK - 1))));
    }

    fn request(port: u16, raw: &str) -> String {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        s.write_all(raw.as_bytes()).unwrap();
        let mut out = Vec::new();
        let _ = s.read_to_end(&mut out);
        String::from_utf8_lossy(&out).into_owned()
    }

    #[test]
    fn serves_over_http_with_protections() {
        let root = tmp("http");
        let srv = Server::start().unwrap();
        srv.set_root(&root);
        let p = srv.port;
        let get = |path: &str, host: &str| format!("GET {path} HTTP/1.1\r\nHost: {host}\r\n\r\n");

        let ok = request(p, &get("/", &format!("127.0.0.1:{p}")));
        assert!(ok.starts_with("HTTP/1.1 200 OK"), "{ok}");
        assert!(ok.contains("X-Content-Type-Options: nosniff") && ok.ends_with("home"));
        assert!(request(p, &get("/", &format!("localhost:{p}"))).starts_with("HTTP/1.1 200"));

        // DNS rebinding: a foreign Host header is refused
        assert!(request(p, &get("/", "evil.example")).starts_with("HTTP/1.1 403"));
        assert!(request(p, &get("/", &format!("evil.example:{p}"))).starts_with("HTTP/1.1 403"));
        assert!(request(p, "GET / HTTP/1.0\r\n\r\n").starts_with("HTTP/1.1 403"));

        let host = format!("127.0.0.1:{p}");
        assert!(request(p, &get("/%2e%2e/etc/passwd", &host)).starts_with("HTTP/1.1 403"));
        assert!(request(p, &get("/.git/config", &host)).starts_with("HTTP/1.1 404"));
        assert!(request(p, &format!("POST / HTTP/1.1\r\nHost: {host}\r\n\r\n")).starts_with("HTTP/1.1 405"));
        let head = request(p, &format!("HEAD / HTTP/1.1\r\nHost: {host}\r\n\r\n"));
        assert!(head.starts_with("HTTP/1.1 200") && head.ends_with("\r\n\r\n"));
        let part = request(p, &format!("GET / HTTP/1.1\r\nHost: {host}\r\nRange: bytes=1-2\r\n\r\n"));
        assert!(part.starts_with("HTTP/1.1 206") && part.ends_with("om"), "{part}");
        let redirect = request(p, &get("/sub", &host));
        assert!(redirect.starts_with("HTTP/1.1 301") && redirect.contains("Location: /sub/\r\n"));

        // oversized headers are refused instead of buffered
        let big = format!("GET / HTTP/1.1\r\nHost: {host}\r\nX-Pad: {}\r\n\r\n", "a".repeat(20_000));
        assert!(request(p, &big).starts_with("HTTP/1.1 431"));
    }
}
