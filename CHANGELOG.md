# Changelog

## Unreleased

### Fixed
- Preview server: after answering `431` to an oversized request it now drains a bounded amount of unread input before closing, so the response is no longer lost to a connection reset (failed intermittently on macOS).

### Security
- `cargo audit` is configured (`.cargo/audit.toml`) to ignore RUSTSEC-2026-0194 and RUSTSEC-2026-0195 (`quick-xml` 0.30, reached only via Linux AccessKit/AT-SPI from the local D-Bus), with the reasoning recorded. Remove when eframe is upgraded.

## 0.2.0

### Added
- Snapping for text boxes (optionally all elements) to elements, page edges, centre lines and a grid; Alt bypasses it.
- Bullet lists with up to three levels in text boxes.
- A secondary text style per text box, applied with `{{ }}`, for a second size, font, colour or weight.
- Cut, copy and paste inside text boxes (floating toolbar) and for whole elements on the page, including across sites.
- Insert menu with five table types, seven presets and a custom table dialog.
- Button and Embed elements (YouTube, Vimeo, Google Maps, Calendar, Forms, OpenStreetMap).
- Ready-made sections: hero, feature cards, call to action, contact block.
- Align-to-page buttons, per-page and site descriptions, site icon, page language.
- `AGENTS.md`, `SECURITY.md`, `CONTRIBUTING.md`, CI, and a headless test suite.

### Security
- Page folder names are validated on load; a tampered config can no longer write outside the site folder.
- Unsafe link schemes are refused; loaded and pasted content is sanitised.
- Generated pages carry a strict Content-Security-Policy; embeds are sandboxed.
- Preview server: Host check, canonical path confinement, request limits, streaming.
- Files are written atomically.

### Changed
- Renamed from Site Builder to Willow. Existing sites open unchanged.
- Minimum Rust version is 1.82.
