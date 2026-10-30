# Third-party components

PDFMark for Windows bundles or links the following components. Everything runs
locally; nothing is downloaded at run time.

| Component | Use | License |
| --- | --- | --- |
| [PDFium](https://pdfium.googlesource.com/pdfium/) (binary build by [bblanchon/pdfium-binaries](https://github.com/bblanchon/pdfium-binaries), chromium/7881) | Page rendering, text extraction | BSD-3-Clause / Apache-2.0, see `licenses/PDFIUM-LICENSE.txt` |
| [DejaVu Sans](https://dejavu-fonts.github.io/) | On-screen and exported typed text | Bitstream Vera / DejaVu license, see `licenses/DejaVuSans-LICENSE.txt` |
| [egui / eframe](https://github.com/emilk/egui) | User interface | MIT / Apache-2.0 |
| [pdfium-render](https://github.com/ajrcarey/pdfium-render) | Rust bindings for PDFium | MIT / Apache-2.0 |
| [lopdf](https://github.com/J-F-Liu/lopdf) | Reading form fields and writing exported PDFs | MIT |
| [subsetter](https://github.com/typst/subsetter), [ttf-parser](https://github.com/harfbuzz/ttf-parser) | Font subsetting and metrics | MIT / Apache-2.0 |
| [rfd](https://github.com/PolyMeilex/rfd) | Native open/save dialogs | MIT |
| [ureq](https://github.com/algesten/ureq), [rustls](https://github.com/rustls/rustls) | Optional "Open from web address" download | MIT / Apache-2.0 / ISC |
| [serde](https://serde.rs), [base64](https://github.com/marshallpierce/rust-base64), [flate2](https://github.com/rust-lang/flate2-rs), [image](https://github.com/image-rs/image) | Project files, compression, icons | MIT / Apache-2.0 |

The full list of Rust crates and their licenses can be produced with
`cargo tree` / `cargo about`.

The PDFMark application code and icons are MIT-licensed; see `LICENSE`.
