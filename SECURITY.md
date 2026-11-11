# Security policy

## Reporting a vulnerability

Please report vulnerabilities privately through GitHub: **Security > Report a vulnerability** on
this repository. Do not open a public issue. Include steps to reproduce and the affected version.
You can expect an acknowledgement within a few days.

## Supported versions

Only the latest release receives fixes.

## Threat model

Willow runs locally and writes static files. The main risks are a hostile site folder or
clipboard, and a hostile web page talking to the preview server.

| Risk | Mitigation |
| --- | --- |
| Tampered `site.json` writing or deleting outside the site folder | Page folder names are validated on load (`security::valid_slug`) |
| Script injection through text, links or attributes | All output is escaped; `javascript:`, `data:` and other schemes are refused |
| Hostile elements in files or on the clipboard | Values are clamped and media paths, links and embeds validated on load and paste |
| Third-party embeds | Allow-list of services, `sandbox` attribute, per-site `frame-src` |
| Injected scripts or external requests on published pages | Strict Content-Security-Policy, no external resources |
| DNS rebinding against the preview server | Host header must be `127.0.0.1:<port>` or `localhost:<port>` |
| Path traversal or symlink escape in preview | Paths are canonicalised and confined; dot-files hidden |
| Slow or oversized requests | Header size, time and connection limits |
| Corrupted files after a crash | Atomic write-then-rename |

Out of scope: content the author deliberately writes into custom CSS or JS outside the generated
markers, and the security of embedded third-party services.

## Dependencies

CI runs `cargo audit` and Dependabot proposes updates weekly.
