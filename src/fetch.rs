//! Downloading a PDF from a web address (replaces "open this tab's PDF").

use crate::model::MAX_PDF_BYTES;
use std::time::Duration;

pub fn filename_from_url(url: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or("");
    let last = path.rsplit('/').next().unwrap_or("");
    let decoded = percent_decode(last);
    if decoded.trim().is_empty() || decoded.contains("://") { "document.pdf".into() } else { decoded }
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Some(v) = s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn content_disposition_name(value: &str) -> Option<String> {
    for part in value.split(';') {
        let part = part.trim();
        if let Some(v) = part.strip_prefix("filename*=") {
            let v = v.trim_matches('"');
            let v = v.splitn(3, '\'').last().unwrap_or(v);
            return Some(percent_decode(v));
        }
    }
    for part in value.split(';') {
        if let Some(v) = part.trim().strip_prefix("filename=") {
            return Some(v.trim_matches('"').to_string());
        }
    }
    None
}

/// Downloads a PDF (max 100 MB, 60 s). Returns (bytes, filename).
pub fn download(url: &str) -> Result<(Vec<u8>, String), String> {
    let url = url.trim();
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("https://") || lower.starts_with("http://")) {
        return Err("Enter a web address that starts with https:// or http://.".into());
    }
    let agent: ureq::Agent =
        ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(60))).user_agent("PDFMark/1.1").build().into();
    let mut response = agent.get(url).call().map_err(|e| match e {
        ureq::Error::StatusCode(code) => format!("The server returned HTTP {code}. Download the file and use Open PDF."),
        other => format!("Could not download this PDF: {other}"),
    })?;
    if let Some(len) = response.headers().get("content-length").and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u64>().ok())
        && len > MAX_PDF_BYTES as u64
    {
        return Err("PDFs up to 100 MB are supported.".into());
    }
    let name = response
        .headers()
        .get("content-disposition")
        .and_then(|v| v.to_str().ok())
        .and_then(content_disposition_name)
        .unwrap_or_else(|| filename_from_url(url));
    let bytes = response.body_mut().with_config().limit(MAX_PDF_BYTES as u64).read_to_vec().map_err(|e| match e {
        ureq::Error::BodyExceedsLimit(_) => "PDFs up to 100 MB are supported.".to_string(),
        other => format!("The download was interrupted: {other}"),
    })?;
    let name = if name.to_lowercase().ends_with(".pdf") { name } else { format!("{name}.pdf") };
    Ok((bytes, name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_from_urls() {
        assert_eq!(filename_from_url("https://x.org/a/My%20File.pdf?x=1"), "My File.pdf");
        assert_eq!(filename_from_url("https://x.org/"), "document.pdf");
        assert_eq!(content_disposition_name("attachment; filename=\"r.pdf\"").as_deref(), Some("r.pdf"));
        assert_eq!(content_disposition_name("attachment; filename*=UTF-8''%C5%81.pdf").as_deref(), Some("Ł.pdf"));
        assert!(download("ftp://x").is_err());
    }
}
