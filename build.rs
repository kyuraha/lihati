fn main() {
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
