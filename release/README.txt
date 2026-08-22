Lihati / MarkdownEditor - a minimal Markdown editor for Windows
================================================================

Contents:
  MarkdownEditor.exe   The complete application (release build, self-contained)

This executable is fully self-contained: no Rust, Cargo, Node.js, Python,
WebView, or any other runtime is required. Fonts are embedded. Everything
is in this single file.

Run
---
  Double-click MarkdownEditor.exe
      Starts with an empty document.

  MarkdownEditor.exe "C:\path\to\my notes.md"
      Opens the given file. Paths with spaces and non-ASCII characters
      are supported. This also works from Explorer's "Open with".

File association (optional, per-user, no admin required)
--------------------------------------------------------
  MarkdownEditor.exe --register      adds "Open with -> Lihati" for .md files
  MarkdownEditor.exe --unregister    removes it again

After registering: right-click any .md file > Open with > Lihati.
Your default association is never changed automatically.

Data written by the app
-----------------------
  %APPDATA%\lihati\app.ron   window state, panel layout, recent files

Keyboard shortcuts
------------------
  Ctrl+O        open        Ctrl+E     source/preview toggle
  Ctrl+S        save        Ctrl+F     find (Enter = next, Esc = close)
  Ctrl+Shift+S  save as     Ctrl+=/-/0 zoom in/out/reset
  Ctrl+N        new         Ctrl+Z / Ctrl+Y or Ctrl+Shift+Z   undo / redo
