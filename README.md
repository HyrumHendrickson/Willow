<p align="center"><img src="assets/logo.png" alt="Willow" width="320"></p>

# Willow

A visual editor for static websites hosted on GitHub Pages. Arrange text, images, video,
tables, buttons and embeds on a canvas; Willow writes plain HTML, CSS and JavaScript that you
commit and publish.

    cargo run --release

Requires Rust 1.82+. On Linux, file dialogs need an XDG desktop portal.

## Getting started

- **File > New site...** creates the folder structure; **File > Open site folder...** opens an existing one.
- Double-click text to edit; drag to move; drag the bottom-right handle to resize.
- Click empty space in the header, page or footer to choose where new items are added.
- **Ctrl+S** saves; **File > Save and preview in browser** serves the site locally over HTTP.
- Commit the folder and enable GitHub Pages (deploy from branch, root).

## Features

- **Text:** headings, bullets (start a line with `- `, indent two spaces per level) and a second style
  (text in `{{ }}`) with its own font, size and colour. Cut, copy and paste inside the box.
- **Tables:** grid, striped, horizontal lines, borderless and rounded card, plus presets such as weekly schedule and pricing.
- **Elements:** images, video, shapes, buttons, navigation, and embeds for YouTube, Vimeo, Google Maps, Calendar and Forms.
- **Sections:** hero, feature cards, call to action, contact block (Insert > Section).
- **Layout:** snapping to elements, page edges, centre lines and a grid (toolbar **Snap**; hold Alt to bypass), align-to-page, undo and redo.
- **Clipboard:** Ctrl+C, Ctrl+X, Ctrl+V and Ctrl+D copy, cut, paste and duplicate elements, also between sites.
- **SEO:** page and site descriptions, site icon, language.

## Security

Generated pages use a strict Content-Security-Policy, link schemes are restricted, embeds are
sandboxed, and the preview server is loopback-only with host and path checks. See
[SECURITY.md](SECURITY.md).

## Development

See [AGENTS.md](AGENTS.md) and [CONTRIBUTING.md](CONTRIBUTING.md).

    cargo fmt --all -- --check
    cargo clippy --all-targets -- -D warnings
    cargo test

## License

MIT. See [LICENSE](LICENSE).
