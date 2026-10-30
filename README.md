# PDFMark for Windows

A fast, private PDF reader and annotator for Windows, written in Rust. It is a
native rewrite of the PDFMark Chrome extension: read, highlight, draw, type,
fill forms and export edited copies. Everything happens on your computer — no
account, no upload, no subscription.

## Install

Download from the repository's **Actions** tab (latest successful *Build* run →
*PDFMark-windows-x64* artifact) or from a **Release**:

- **`PDFMark-Setup-<version>-x64.exe`** — installer. Installs per user (no admin
  needed), adds a Start menu entry, registers `.pdfmark` projects and adds
  PDFMark to the **Open with** menu for PDFs (your default PDF viewer is not
  changed).
- **`PDFMark-<version>-windows-x64.zip`** — portable. Unzip anywhere and run
  `PDFMark.exe`. Keep `pdfium.dll` in the same folder.

Requires 64-bit Windows 10 or 11.

## Use

Open a PDF with **Open PDF**, by dropping it on the window, from Explorer
(**Open with › PDFMark**), or from the **•••** menu › *Open from web address*.

| Tool | What it does |
| --- | --- |
| Read (V) | Select and copy original text, highlight a selection, fill form fields. |
| Move (M) | Drag an added mark. Double-click added text to edit it. |
| Highlight (H) | Drag a translucent highlight over any area. |
| Pen (P) | Draw or handwrite with mouse, touch or pen. |
| Text (T) | Click to place typed text (5–72 pt, any color, multiple lines). |
| Box / Circle / Arrow (R / O / A) | Drag to draw a shape. |
| Eraser (E) | Click an added mark to remove it. |

Undo/redo covers marks and form changes (80 steps). **Pages** shows
thumbnails, **Notes** lists every mark. Search (Ctrl+F) covers the text of all
pages; press Enter for the next match.

### Saving

- **Export PDF** (Ctrl+S) writes a new PDF. Original page content is kept;
  your marks become vector page content, typed text uses an embedded Unicode
  font (DejaVu Sans) and stays selectable, and form fields keep working with
  your values and fresh appearances.
- **Save editable project** (Ctrl+Shift+S) writes a `.pdfmark` file with the
  original PDF, your movable marks and form values. The format is the same as
  the browser extension's, so projects move freely between the two.
- **Recovery draft**: the latest document and edits are saved automatically to
  `%LOCALAPPDATA%\PDFMark\recovery`. Use *Continue your last draft* on the start
  screen or *Restore last local draft* in the ••• menu. *Forget saved draft*
  deletes it.

### Keyboard shortcuts

| Shortcut | Action |
| --- | --- |
| V M H P T R O A E | Tools (see above) |
| Delete / Backspace | Remove the selected mark |
| Ctrl+Z / Ctrl+Shift+Z / Ctrl+Y | Undo / redo |
| Ctrl+S / Ctrl+Shift+S / Ctrl+O | Export PDF / save project / open |
| Ctrl+F | Find in document |
| Ctrl+C | Copy selected original text |
| Page Up / Page Down, Home / End | Change page |
| Ctrl + mouse wheel, Ctrl + / − / 0 | Zoom / fit width |
| Ctrl+Enter | Apply text in the text dialog |
| Escape | Cancel, deselect |

### Limits

- Annotation and form editor: it does not reflow or redact existing PDF text.
- Scanned PDFs can be marked up but have no searchable text (no OCR).
- Editing can invalidate existing digital signatures.
- Password-protected PDFs open and can be annotated and saved as projects;
  exporting them as PDF is not supported (passwords are never stored).
- Up to 100 MB / 3,000 pages per PDF, 150 MB per project, 20,000 marks.
- XFA forms and signing are not supported.

## Build from source

You need [Rust](https://rustup.rs) (stable) and a pdfium library build
matching the `pdfium-render` API (chromium/7881):

```powershell
# Windows (PowerShell)
.\scripts\fetch-pdfium.ps1            # -> vendor\pdfium\win-x64\pdfium.dll
cargo test --release
cargo build --release
copy vendor\pdfium\win-x64\pdfium.dll target\release\
target\release\PDFMark.exe
```

Cross-compiling from Linux:

```sh
sudo apt install mingw-w64
rustup target add x86_64-pc-windows-gnu
scripts/fetch-pdfium.sh linux-x64 win-x64
cargo test                                  # runs against the Linux pdfium
cargo build --release --target x86_64-pc-windows-gnu
```

The GitHub Actions workflow (`.github/workflows/build.yml`) builds and tests on
Windows, smoke-tests the packaged app, and produces the zip and the Inno Setup
installer (`installer/pdfmark.iss`). Pushing a `v*` tag publishes a release.

### Layout

```text
src/main.rs        Entry point, window setup, command-line file argument
src/app.rs         Application state and actions (open, edit, undo, search, save)
src/view.rs        UI: panels, page canvas, tools, form fields, dialogs, shortcuts
src/engine.rs      Background PDF worker (pdfium): render, text, geometry, export
src/export.rs      Writes marks and form values into the PDF (lopdf), font embedding
src/forms.rs       AcroForm field discovery
src/model.rs       Marks, coordinates, hit testing, history, validation
src/project.rs     .pdfmark projects and the recovery draft
src/font.rs        Bundled DejaVu Sans metrics and glyph coverage
src/fetch.rs       Optional download from a web address
src/theme.rs       Colors and fonts;  src/icons.rs  vector toolbar icons
tests/             End-to-end tests (export geometry on rotated/cropped pages,
                   selectable Unicode text, forms, passwords)
```

## Privacy

No analytics, accounts or upload endpoints. The network is used only when you
choose *Open from web address*. PDF JavaScript, links and launch actions are
never executed. Recovery drafts and projects contain the full PDF — handle them
like the PDF itself.

Third-party components and licenses: see `THIRD_PARTY_NOTICES.md`.
