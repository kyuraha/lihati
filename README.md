# Lihati

A fast, minimal Markdown reader and editor for Windows, written in Rust.

Single ~9 MB executable. No webviews, no Electron — pure native rendering with
[egui/eframe](https://github.com/emilk/egui), Markdown via
[pulldown-cmark](https://github.com/pulldown-cmark/pulldown-cmark), and syntax
highlighting for code blocks via [syntect](https://github.com/trishume/syntect).

![layout](https://img.shields.io/badge/layout-directory_%C2%B7_editor_%C2%B7_outline-5b9dff)

## Features

- **Three-panel workspace**: directory browser, editor, outline — each panel can be hidden
  independently for a distraction-free writing mode.
- **Source / Split / Preview** toggle (Ctrl+E cycles Source ↔ Preview).
  - Live preview renders headings, lists, task checkboxes, tables, blockquotes,
    code blocks (syntax highlighted), links, images, horizontal rules.
  - Local images are resolved relative to the document; remote images show a placeholder.
- **Outline** generated from `#` headings; click to jump to the section.
- **Editing**: monospace JetBrains Mono editor, soft wrap, undo/redo, drag-select, IME support.
- **Files**: open/save/save-as (Ctrl+O / Ctrl+S / Ctrl+Shift+S), drag-and-drop onto the window,
  external-modification detection with reload/conflict banner.
- **Windows integration**: pass a file on the command line or use right-click → *Open with*.
  Registering adds Lihati to the *Open with* menu for `.md` files (HKCU only, never hijacks
  your default association).
- **State persistence**: window size/position, open panels, view mode, theme, zoom,
  last file and recent files are remembered between launches.

## Keyboard shortcuts

| Keys | Action |
| --- | --- |
| `Ctrl+O` | Open file |
| `Ctrl+N` | New document |
| `Ctrl+S` | Save |
| `Ctrl+Shift+S` | Save as |
| `Ctrl+E` | Toggle source / preview |
| `Ctrl+F` | Find in document (Enter = next, Shift+Enter = previous, Esc = close) |
| `Ctrl+=` / `Ctrl+-` / `Ctrl+0` | Zoom in / out / reset |
| `Ctrl+Z` / `Ctrl+Y`, `Ctrl+Shift+Z` | Undo / redo |

## Performance notes

- The Markdown preview parses the document **once per edit**, not once per frame;
  results are cached against a document revision counter.
- Outline, word count and line count are computed only when the text changes.
- When idle the app does not repaint at all unless it is watching an open file
  (1 Hz focused, 5 Hz in background); external-change detection is that timer.

## Build

Requires Rust 1.80+ (`winget install Rustlang.Rustup`) with the MSVC toolchain.

```powershell
cargo build --release
# binary: target\release\lihati.exe  (~9 MB, self-contained)
```

A ready-to-run copy is provided at `release\MarkdownEditor.exe` along with
run instructions in `release\README.txt`.

## Build

Requires Rust 1.80+ (`winget install Rustlang.Rustup`) with the MSVC toolchain.

```powershell
cargo build --release
# binary: target\release\lihati.exe
```

## Install & file association

Option A — portable registration (no installer needed):

```powershell
target\release\lihati.exe --register     # adds "Open with -> Lihati" for .md files
target\release\lihati.exe --unregister   # removes it again
```

Then right-click any `.md` file → **Open with** → **Lihati**
(tick *Always* to make it the default).

Option B — installer: build `packaging\Lihati-setup-0.1.0.exe` with
[Inno Setup](https://jrsoftware.org/isinfo.php):

```powershell
iscc packaging\lihati-setup.iss
```

The installer copies the exe to `%LocalAppData%\Programs\Lihati`, creates shortcuts,
and runs `lihati --register`; uninstalling runs `--unregister`.

## Architecture

```
src/
├── main.rs       entry point, CLI parsing (--register/--unregister/file arg)
├── app.rs        UI shell: toolbar, panels, layout, shortcuts, modals, toasts
├── document.rs   document state: load/save, dirty flag, external-change detection
├── markdown.rs   heading-outline extraction, word count
├── preview.rs    pulldown-cmark events → egui widgets (incl. syntect + image cache)
├── fs_tree.rs    directory listing for the Files panel
├── state.rs      persisted UI state (serde JSON via eframe storage)
├── theme.rs      bundled fonts (Inter + JetBrains Mono), palette, egui styling
└── registry.rs   Windows file association under HKCU (winreg)
```

UI state lives in `app.rs`, document/file I/O in `document.rs`, rendering in
`preview.rs`, and Windows integration in `registry.rs` — kept deliberately small
and free of extra abstraction layers.

## License

MIT. Bundled fonts: Inter and JetBrains Mono (SIL OFL 1.1).
