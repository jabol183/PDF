fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    // Embed the icon and version information into the Windows executable.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set("FileDescription", "PDFMark - PDF Reader & Annotator");
        res.set("ProductName", "PDFMark");
        res.set("CompanyName", "PDFMark contributors");
        res.set("LegalCopyright", "MIT License");
        res.set("OriginalFilename", "PDFMark.exe");
        if let Err(e) = res.compile() {
            println!("cargo:warning=Windows resources were not embedded: {e}");
        }
    }
}
