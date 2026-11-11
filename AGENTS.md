# AGENTS.md

Guidance for AI coding agents and human contributors working on Willow, a desktop editor
(Rust, egui) that generates static websites for GitHub Pages.

## Commands

```
cargo run --release                          # start the editor
cargo fmt --all -- --check                   # formatting (rustfmt.toml: max_width 120)
cargo clippy --all-targets -- -D warnings    # lints; unwrap/expect are allowed in tests only
cargo test                                   # all tests; they run headless
```

All three checks must pass before a change is considered done. Rust 1.82 or newer.

## Layout

| File | Responsibility |
| --- | --- |
| `src/model.rs` | Data model, JSON (de)serialisation, load-time validation and sanitising |
| `src/export.rs` | Generates HTML, CSS, JS and JSON from a `Project`; atomic writes |
| `src/security.rs` | Every validator for URLs, slugs, media paths and embeds |
| `src/richtext.rs` | Text markup (`- ` bullets, `{{ }}` secondary style) and its editing helpers |
| `src/snap.rs` | Pure snapping geometry, no UI types |
| `src/presets.rs` | Table presets and section templates |
| `src/server.rs` | Loopback preview server |
| `src/app.rs` | The egui editor; its tests drive the real UI headlessly |

## Invariants. Do not break these

1. **Nothing reaches HTML or a file path unvalidated.** Links go through `security::safe_href`,
   media through `safe_media_path`, embeds through `normalize_embed`, folder names through
   `valid_slug`. Add a validator to `security.rs` rather than checking inline.
2. **All output text is escaped** with `esc()` in `export.rs`. Never build markup from raw strings.
3. **Untrusted content is sanitised.** Anything read from disk or pasted from the clipboard calls
   `sanitize()` before use. New fields on `Element` kinds must be clamped or validated there.
4. **Old sites must keep opening.** Add new serialised fields with `#[serde(default)]`. The test
   `sites_saved_by_the_previous_version_still_open` guards this.
5. **No external resources on generated pages.** The Content-Security-Policy allows only the site
   itself plus embed hosts in use. Do not add CDNs, fonts or analytics.
6. **The preview server stays on loopback** and keeps its Host check and path confinement.
7. **No `unsafe`** (`unsafe_code = "forbid"`).

## Conventions

- New element kinds need: a `Kind` variant and `sanitize` arm in `model.rs`; `element_html` and
  `element_css` arms in `export.rs`; drawing in `draw_element` and a properties panel in
  `element_props` in `app.rs`; tests for each.
- Generated CSS and JS sit between marker comments, and user code outside them is preserved.
  Never write generated content outside the markers.
- Write tests with the change. Pure logic gets unit tests; interaction gets a headless test in
  `app.rs` using the `Harness`.
- Keep user-facing text plain and professional. Prefer small, reviewable commits.
