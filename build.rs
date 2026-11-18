fn main() {
    println!("cargo:rerun-if-changed=app_icon.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default() == "windows" {
        let mut res = winres::WindowsResource::new();
        res.set_icon("app_icon.ico");
        res.set("ProductName", "NotDynamicIsland");
        res.set("FileDescription", "NotDynamicIsland - Windows Native Desktop Island");
        res.set("LegalCopyright", "Copyright (c) 2026 Sarashinaa");
        if let Err(e) = res.compile() {
            eprintln!("Warning: failed to compile Windows resources: {}", e);
        }
    }
}
