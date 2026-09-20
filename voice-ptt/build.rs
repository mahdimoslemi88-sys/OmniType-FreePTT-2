fn main() {
    #[cfg(windows)]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set("FileDescription", "OmniType — AI Voice Typing & Industrial Speech Routing");
        res.set("ProductName", "OmniType");
        res.set("LegalCopyright", "Copyright (c) 2026");
        if let Err(e) = res.compile() {
            eprintln!("cargo:warning=Failed to compile Windows resource icon: {}", e);
        }
    }
}
