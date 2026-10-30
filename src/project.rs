//! Editable `.pdfmark` projects (same JSON format as the PDFMark browser
//! extension) and the local recovery draft.

use crate::model::{Edits, MAX_PDF_BYTES, MAX_PROJECT_BYTES};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Sender, channel};

#[derive(Serialize, Deserialize)]
struct ProjectFile {
    format: String,
    version: u32,
    name: String,
    #[serde(rename = "createdAt", default)]
    created_at: String,
    pdf: String,
    edits: Edits,
    #[serde(default)]
    page: serde_json::Value,
}

pub struct Project {
    pub name: String,
    pub pdf: Vec<u8>,
    /// Not yet validated against the page count.
    pub edits: Edits,
    pub page: usize,
}

pub fn now_iso() -> String {
    // Minimal UTC ISO-8601 timestamp without extra dependencies.
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Civil-from-days (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.000Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

pub fn project_json(name: &str, pdf: &[u8], edits: &Edits, page: usize) -> Result<Vec<u8>, String> {
    let p = ProjectFile {
        format: "pdfmark".into(),
        version: 1,
        name: name.into(),
        created_at: now_iso(),
        pdf: B64.encode(pdf),
        edits: edits.clone(),
        page: serde_json::Value::from(page),
    };
    serde_json::to_vec(&p).map_err(|e| e.to_string())
}

pub fn parse_project(bytes: &[u8]) -> Result<Project, String> {
    if bytes.len() > MAX_PROJECT_BYTES {
        return Err("Project files up to 150 MB are supported.".into());
    }
    let raw: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| "This is not a valid PDFMark project file.".to_string())?;
    if raw.get("format").and_then(|v| v.as_str()) != Some("pdfmark")
        || raw.get("version").and_then(|v| v.as_u64()) != Some(1)
        || !raw.get("name").is_some_and(|v| v.is_string())
    {
        return Err("Unsupported PDFMark project format.".into());
    }
    let p: ProjectFile = serde_json::from_value(raw).map_err(|e| format!("This project file is damaged ({e})."))?;
    let b64 = p.pdf.as_bytes();
    if b64.len() > MAX_PDF_BYTES.div_ceil(3) * 4 + 4 {
        return Err("PDF is larger than the 100 MB limit.".into());
    }
    let pdf = B64.decode(b64).map_err(|_| "Invalid PDF data in project.".to_string())?;
    if pdf.len() > MAX_PDF_BYTES {
        return Err("PDF is larger than the 100 MB limit.".into());
    }
    let page = p.page.as_f64().filter(|v| v.is_finite() && *v >= 1.0).map(|v| v as usize).unwrap_or(1);
    Ok(Project { name: p.name, pdf, edits: p.edits, page })
}

// ---------------------------------------------------------------- recovery draft

pub fn data_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_DATA_HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("share")))
        .unwrap_or_else(std::env::temp_dir);
    base.join("PDFMark")
}

fn draft_dir() -> PathBuf {
    std::env::var_os("PDFMARK_DATA_DIR").map(PathBuf::from).unwrap_or_else(data_dir).join("recovery")
}

#[derive(Serialize, Deserialize)]
struct DraftState {
    version: u32,
    name: String,
    edits: Edits,
    page: usize,
    #[serde(rename = "updatedAt")]
    updated_at: u64,
    #[serde(rename = "sourceLen")]
    source_len: usize,
}

pub struct Draft {
    pub name: String,
    pub pdf: Vec<u8>,
    pub edits: Edits,
    pub page: usize,
}

fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, path)
}

pub fn draft_exists() -> bool {
    draft_dir().join("state.json").exists() && draft_dir().join("source.pdf").exists()
}

pub fn read_draft() -> Result<Option<Draft>, String> {
    let dir = draft_dir();
    let state = match std::fs::read(dir.join("state.json")) {
        Ok(s) => s,
        Err(_) => return Ok(None),
    };
    let state: DraftState = serde_json::from_slice(&state).map_err(|_| "The local draft is damaged.".to_string())?;
    let pdf = std::fs::read(dir.join("source.pdf")).map_err(|_| "The local draft's PDF is missing.".to_string())?;
    if pdf.len() != state.source_len {
        return Err("The local draft is incomplete.".into());
    }
    Ok(Some(Draft { name: state.name, pdf, edits: state.edits, page: state.page }))
}

pub fn forget_draft() -> Result<(), String> {
    let dir = draft_dir();
    for f in ["state.json", "source.pdf"] {
        let p = dir.join(f);
        if p.exists() {
            std::fs::remove_file(&p).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

pub enum DraftJob {
    Save { source: Option<Arc<Vec<u8>>>, name: String, edits: Edits, page: usize, source_len: usize },
    Forget,
}

/// Background writer for the recovery draft. Results come back as `Ok/Err` messages.
pub fn start_draft_writer(done: impl Fn(Result<(), String>) + Send + 'static) -> Sender<DraftJob> {
    let (tx, rx) = channel::<DraftJob>();
    std::thread::Builder::new()
        .name("draft-writer".into())
        .spawn(move || {
            while let Ok(mut job) = rx.recv() {
                // Coalesce: only the most recent state matters, but keep any new source bytes.
                let mut pending_source = None;
                loop {
                    if let DraftJob::Save { source: Some(s), .. } = &job {
                        pending_source = Some(s.clone());
                    }
                    if let DraftJob::Forget = job {
                        pending_source = None;
                    }
                    match rx.try_recv() {
                        Ok(next) => job = next,
                        Err(_) => break,
                    }
                }
                let result = match job {
                    DraftJob::Forget => forget_draft(),
                    DraftJob::Save { name, edits, page, source_len, .. } => (|| {
                        let dir = draft_dir();
                        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                        if let Some(src) = pending_source.take() {
                            write_atomic(&dir.join("source.pdf"), &src).map_err(|e| e.to_string())?;
                        }
                        let state = DraftState {
                            version: 1,
                            name,
                            edits,
                            page,
                            updated_at: std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|d| d.as_millis() as u64)
                                .unwrap_or(0),
                            source_len,
                        };
                        let json = serde_json::to_vec(&state).map_err(|e| e.to_string())?;
                        write_atomic(&dir.join("state.json"), &json).map_err(|e| e.to_string())
                    })(),
                };
                done(result);
            }
        })
        .expect("start draft writer");
    tx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_round_trip() {
        let pdf = b"%PDF-1.4 fake".to_vec();
        let json = project_json("a.pdf", &pdf, &Edits::default(), 3).unwrap();
        let p = parse_project(&json).unwrap();
        assert_eq!(p.pdf, pdf);
        assert_eq!(p.name, "a.pdf");
        assert_eq!(p.page, 3);
        assert!(parse_project(b"{}").is_err());
        assert!(parse_project(b"nope").is_err());
        assert!(parse_project(br#"{"format":"pdfmark","version":1,"name":"x","pdf":"!!","edits":{"marks":[],"fields":{}}}"#).is_err());
    }

    #[test]
    fn iso_time_shape() {
        let t = now_iso();
        assert_eq!(t.len(), 24);
        assert!(t.ends_with('Z'));
    }
}
