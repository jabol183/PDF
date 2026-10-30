//! End-to-end checks through the real PDF worker: open, geometry, text,
//! export of every mark type and form field, and re-rendering of the result.
//!
//! Needs the pdfium library: set PDFMARK_PDFIUM, or run scripts/fetch-pdfium
//! so it is found in vendor/pdfium/<platform>/.

use pdfmark::engine::{DocInfo, Engine, PageGeom, RenderKind, Request, Response, TextChar};
use pdfmark::model::*;
use std::sync::Arc;
use std::time::Duration;

fn setup() {
    if std::env::var_os("PDFMARK_PDFIUM").is_some() {
        return;
    }
    let dir = if cfg!(windows) {
        "win-x64/pdfium.dll"
    } else if cfg!(target_os = "macos") {
        "mac/libpdfium.dylib"
    } else {
        "linux-x64/libpdfium.so"
    };
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("vendor/pdfium").join(dir);
    assert!(path.exists(), "pdfium not found at {} — run scripts/fetch-pdfium or set PDFMARK_PDFIUM", path.display());
    // SAFETY: set before any worker thread starts.
    unsafe { std::env::set_var("PDFMARK_PDFIUM", path) };
}

struct Client {
    engine: Engine,
    epoch: u64,
}

impl Client {
    fn new() -> Self {
        setup();
        Client { engine: Engine::start(|| {}), epoch: 0 }
    }
    fn recv(&self) -> Response {
        let r = self.engine.rx.recv_timeout(Duration::from_secs(60)).expect("worker response");
        if let Response::Fatal(e) = &r {
            panic!("engine failed: {e}");
        }
        r
    }
    fn open(&mut self, bytes: Vec<u8>, password: Option<&str>) -> Result<DocInfo, (bool, String)> {
        self.epoch += 1;
        self.engine.tx.send(Request::Open { epoch: self.epoch, bytes: Arc::new(bytes), password: password.map(String::from) }).unwrap();
        match self.recv() {
            Response::Opened { info, .. } => Ok(info),
            Response::NeedPassword { incorrect, .. } => Err((true, format!("password needed (incorrect: {incorrect})"))),
            Response::OpenFailed { error, .. } => Err((false, error)),
            _ => panic!("unexpected response"),
        }
    }
    fn text(&self, page: usize) -> (PageGeom, Vec<TextChar>) {
        self.engine.tx.send(Request::Text { epoch: self.epoch, page }).unwrap();
        match self.recv() {
            Response::Text { geom, chars, .. } => (geom, chars),
            _ => panic!("unexpected response"),
        }
    }
    fn render(&self, page: usize, w: u32, h: u32) -> pdfmark::engine::RgbaImage {
        self.engine.tx.send(Request::Render { epoch: self.epoch, page, kind: RenderKind::Main, width: w, height: h }).unwrap();
        match self.recv() {
            Response::Rendered { image, .. } => image,
            _ => panic!("unexpected response"),
        }
    }
    fn export(&self, edits: Edits) -> Result<Vec<u8>, String> {
        self.engine.tx.send(Request::Export { epoch: self.epoch, edits }).unwrap();
        match self.recv() {
            Response::Exported { result, .. } => result,
            _ => panic!("unexpected response"),
        }
    }
}

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)).unwrap()
}

fn mark(id: String, page: u32, color: &str, shape: Shape) -> Mark {
    Mark { id, page, color: color.into(), shape }
}

fn all_marks(pages: u32) -> Vec<Mark> {
    let mut marks = Vec::new();
    for p in 1..=pages {
        marks.push(mark(format!("rect-{p}"), p, "#ff0000", Shape::Rectangle { x: 30.0, y: 40.0, w: 100.0, h: 55.0, width: 3.0 }));
        let lines = vec!["Zażółć gęślą jaźń".to_string(), "Added selectable text".to_string()];
        marks.push(mark(
            format!("text-{p}"),
            p,
            "#0000ff",
            Shape::Text { x: 35.0, y: 115.0, w: 240.0, h: 42.0, size: 16.0, text: lines.join("\n"), lines },
        ));
        marks.push(mark(format!("hl-{p}"), p, "#ffff00", Shape::Highlight { x: 30.0, y: 180.0, w: 200.0, h: 20.0, width: 1.0 }));
        marks.push(mark(
            format!("pen-{p}"),
            p,
            "#00aa00",
            Shape::Pen { points: vec![Pt::new(30.0, 225.0), Pt::new(60.0, 245.0), Pt::new(100.0, 220.0)], width: 3.0 },
        ));
        marks.push(mark(format!("arrow-{p}"), p, "#aa00aa", Shape::Arrow { x: 180.0, y: 225.0, x2: 240.0, y2: 255.0, width: 2.0 }));
        marks.push(mark(format!("ellipse-{p}"), p, "#00aaaa", Shape::Ellipse { x: 280.0, y: 220.0, w: 45.0, h: 30.0, width: 2.0 }));
    }
    marks
}

#[test]
fn export_preserves_geometry_text_and_forms() {
    let mut c = Client::new();
    let input = fixture("sample.pdf");
    let info = c.open(input.clone(), None).expect("open sample");
    assert_eq!(info.page_count, 6);
    assert!(!info.encrypted);

    // Viewport geometry: rotation, crop box and UserUnit like PDF.js.
    let expected = [(420.0, 520.0), (520.0, 420.0), (420.0, 520.0), (520.0, 420.0), (430.0, 350.0), (840.0, 1040.0)];
    for (i, (w, h)) in expected.iter().enumerate() {
        let (g, chars) = c.text(i);
        assert!((g.width - w).abs() < 0.01 && (g.height - h).abs() < 0.01, "page {} size {}x{}", i + 1, g.width, g.height);
        let s: String = chars.iter().map(|c| c.text.as_str()).collect();
        assert!(s.contains("Original PDF page"), "page {} text: {s}", i + 1);
    }
    // PDF.js viewport transform for an unrotated 420x520 page is [1,0,0,-1,0,520].
    let (g0, _) = c.text(0);
    assert_eq!(g0.transform, [1.0, 0.0, 0.0, -1.0, 0.0, 520.0]);

    let mut edits = Edits { marks: all_marks(6), ..Default::default() };
    edits.fields.insert("customer.name".into(), FieldValue::Text("Łukasz Żółć".into()));
    edits.fields.insert("approved".into(), FieldValue::Bool(true));
    edits.fields.insert("service".into(), FieldValue::Text("Install".into()));
    edits.fields.insert("priority".into(), FieldValue::Text("Urgent".into()));
    edits.fields.insert("extras".into(), FieldValue::List(vec!["Warranty".into(), "Setup".into()]));
    edits.fields.insert("notes".into(), FieldValue::Text("Line one\nLine two".into()));
    let edits = validate_edits(edits, 6).unwrap();
    let output = c.export(edits.clone()).expect("export");
    let out_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/output");
    std::fs::create_dir_all(&out_dir).unwrap();
    std::fs::write(out_dir.join("exported.pdf"), &output).unwrap();

    // Form values were written.
    let (forms, _) = pdfmark::forms::parse(&output, None).unwrap();
    let value = |name: &str| forms.page(0).iter().find(|w| w.field == name).map(|w| w.initial.clone()).unwrap();
    assert_eq!(value("customer.name"), FieldValue::Text("Łukasz Żółć".into()));
    assert_eq!(value("approved"), FieldValue::Bool(true));
    assert_eq!(value("service"), FieldValue::Text("Install".into()));
    // "Urgent" is the /Opt export value of appearance state "1".
    assert_eq!(value("priority"), FieldValue::Text("1".into()));
    assert_eq!(value("extras"), FieldValue::List(vec!["Warranty".into(), "Setup".into()]));
    assert_eq!(value("notes"), FieldValue::Text("Line one\nLine two".into()));

    // Re-open the export and check every page.
    let info = c.open(output.clone(), None).expect("open export");
    assert_eq!(info.page_count, 6);
    for p in 0..6 {
        let (g, chars) = c.text(p);
        let s: String = chars.iter().map(|c| c.text.as_str()).collect();
        assert!(s.contains("Zażółć gęślą jaźń"), "page {}: Polish text must be selectable: {s}", p + 1);
        assert!(s.contains("Added selectable text"), "page {}", p + 1);
        assert!(s.contains("Original PDF page"), "page {}: original text preserved", p + 1);

        let img = c.render(p, g.width.round() as u32, g.height.round() as u32);
        let mut red = (u32::MAX, u32::MAX, 0u32, 0u32, 0usize);
        for y in 0..img.height {
            for x in 0..img.width {
                let i = (y * img.width + x) * 4;
                let px = &img.pixels[i..i + 4];
                if px[0] > 210 && px[1] < 80 && px[2] < 80 {
                    red.0 = red.0.min(x as u32);
                    red.1 = red.1.min(y as u32);
                    red.2 = red.2.max(x as u32);
                    red.3 = red.3.max(y as u32);
                    red.4 += 1;
                }
            }
        }
        assert!(red.4 > 300, "page {}: rectangle must render", p + 1);
        let close = |a: u32, b: i64| (a as i64 - b).abs() <= 2;
        assert!(
            close(red.0, 29) && close(red.1, 39) && close(red.2, 131) && close(red.3, 96),
            "page {}: viewport export drift {:?}",
            p + 1,
            red
        );
        // Highlight is translucent yellow (multiply) over white.
        let hi = (190 * img.width + 100) * 4;
        let px = &img.pixels[hi..hi + 3];
        assert!(px[0] > 200 && px[1] > 200 && px[2] < 220 && px[2] > 120, "page {}: highlight pixel {:?}", p + 1, px);
        let png = image::RgbaImage::from_raw(img.width as u32, img.height as u32, img.pixels).unwrap();
        png.save(out_dir.join(format!("page-{}.png", p + 1))).unwrap();
    }

    // Unsupported glyphs are rejected instead of silently replaced.
    c.open(input, None).unwrap();
    for p in 0..6 {
        c.text(p);
    }
    let bad = Edits {
        marks: vec![mark(
            "x".into(),
            1,
            "#000000",
            Shape::Text { x: 1.0, y: 1.0, w: 10.0, h: 10.0, size: 12.0, text: "龘".into(), lines: vec!["龘".into()] },
        )],
        ..Default::default()
    };
    let err = c.export(bad).unwrap_err();
    assert!(err.contains("cannot display"), "{err}");
    // Unknown form fields are reported.
    let mut missing = Edits::default();
    missing.fields.insert("nope".into(), FieldValue::Text("x".into()));
    assert!(c.export(missing).unwrap_err().contains("no longer exists"));
}

#[test]
fn password_pdf_opens_and_blocks_export() {
    let mut c = Client::new();
    let bytes = fixture("password.pdf");
    let (needs, msg) = c.open(bytes.clone(), None).unwrap_err();
    assert!(needs && msg.contains("incorrect: false"), "{msg}");
    let (needs, msg) = c.open(bytes.clone(), Some("wrong")).unwrap_err();
    assert!(needs && msg.contains("incorrect: true"), "{msg}");
    let info = c.open(bytes, Some("test-password")).expect("correct password opens");
    assert!(info.encrypted);
    let err = c.export(Edits::default()).unwrap_err();
    assert!(err.contains("Encrypted"), "{err}");
}

#[test]
fn rejects_non_pdf_and_project_round_trip() {
    let mut c = Client::new();
    let (needs, _) = c.open(b"not a pdf at all".to_vec(), None).unwrap_err();
    assert!(!needs);
    let pdf = fixture("sample.pdf");
    let edits = Edits { marks: all_marks(2), ..Default::default() };
    let json = pdfmark::project::project_json("sample.pdf", &pdf, &edits, 2).unwrap();
    let p = pdfmark::project::parse_project(&json).unwrap();
    assert_eq!(p.pdf, pdf);
    assert_eq!(validate_edits(p.edits, 6).unwrap(), edits);
}
