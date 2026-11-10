//! Minimal local static file server so the site can be previewed over HTTP
//! (header/footer are loaded with `fetch`, which does not work from `file://`).

use std::fs;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;

pub struct Server {
    pub port: u16,
    root: Arc<Mutex<PathBuf>>,
}

impl Server {
    pub fn start() -> std::io::Result<Server> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        let root = Arc::new(Mutex::new(PathBuf::new()));
        let shared = root.clone();
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let root = shared.lock().map(|r| r.clone()).unwrap_or_default();
                thread::spawn(move || {
                    let _ = handle(stream, &root);
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
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
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

fn respond(stream: &mut TcpStream, status: &str, headers: &str, body: &[u8]) -> std::io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {status}\r\n{headers}Content-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)
}

fn handle(mut stream: TcpStream, root: &Path) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let mut range: Option<String> = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 || line == "\r\n" || line == "\n" {
            break;
        }
        if let Some(v) = line.to_lowercase().strip_prefix("range:") {
            range = Some(v.trim().to_string());
        }
    }
    let mut parts = request_line.split_whitespace();
    let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or("/"));
    if method != "GET" && method != "HEAD" {
        return respond(&mut stream, "405 Method Not Allowed", "", b"");
    }
    let path_part = percent_decode(target.split(['?', '#']).next().unwrap_or("/"));

    // Resolve safely inside root.
    let mut full = root.to_path_buf();
    for c in Path::new(path_part.trim_start_matches('/')).components() {
        match c {
            Component::Normal(n) => full.push(n),
            Component::CurDir => {}
            _ => return respond(&mut stream, "403 Forbidden", "", b"forbidden"),
        }
    }
    if full.is_dir() {
        if !path_part.ends_with('/') {
            return respond(&mut stream, "301 Moved Permanently", &format!("Location: {path_part}/\r\n"), b"");
        }
        full.push("index.html");
    }
    let Ok(mut file) = fs::File::open(&full) else {
        return respond(&mut stream, "404 Not Found", "Content-Type: text/plain\r\n", b"not found");
    };
    let len = file.metadata()?.len();
    let ctype = format!("Content-Type: {}\r\nAccept-Ranges: bytes\r\n", mime(&full));

    // Byte ranges (needed for video seeking).
    if let Some(r) = range.as_deref().and_then(|r| r.strip_prefix("bytes=")) {
        let mut it = r.splitn(2, '-');
        let start: u64 = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        let end: u64 = it.next().and_then(|s| s.parse().ok()).unwrap_or(len.saturating_sub(1)).min(len.saturating_sub(1));
        if start <= end && start < len {
            file.seek(SeekFrom::Start(start))?;
            let mut buf = vec![0u8; (end - start + 1) as usize];
            file.read_exact(&mut buf)?;
            let h = format!("{ctype}Content-Range: bytes {start}-{end}/{len}\r\n");
            return respond(&mut stream, "206 Partial Content", &h, if method == "HEAD" { b"" } else { &buf });
        }
    }
    let mut buf = Vec::with_capacity(len as usize);
    file.read_to_end(&mut buf)?;
    respond(&mut stream, "200 OK", &ctype, if method == "HEAD" { b"" } else { &buf })
}
