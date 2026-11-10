# Microschool Site Builder

Visual editor for a static site hosted on GitHub Pages.

    cargo run --release

Requires Rust 1.81+. On Linux, file dialogs need an XDG desktop portal.

- File > New site... creates the folder structure; File > Open site folder... opens an existing one.
- Double-click text to edit; drag to move; drag the bottom-right handle to resize.
- Click empty space in header, page or footer to choose where new items are added.
- Ctrl+S saves; File > Save and preview in browser serves the site locally over HTTP.
- Commit the chosen folder and enable GitHub Pages (deploy from branch, root).
