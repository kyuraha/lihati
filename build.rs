fn main() {
    // Build tag for the status bar (set LIHATI_BUILD_TAG env when building,
    // e.g. date-time). Lets users confirm exactly which build they run.
    let tag = std::env::var("LIHATI_BUILD_TAG").unwrap_or_else(|_| "dev".to_string());
    println!("cargo:rustc-env=LIHATI_BUILD_TAG={tag}");
    println!("cargo:rerun-if-env-changed=LIHATI_BUILD_TAG");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=assets/app.ico");
        winresource::WindowsResource::new()
            .set_icon("assets/app.ico")
            .set("FileDescription", "Lihati - Markdown Editor")
            .set("ProductName", "Lihati")
            .set("OriginalFilename", "MarkdownEditor.exe")
            .set("LegalCopyright", "MIT License")
            .compile()
            .expect("failed to compile Windows resource");
    }
}
