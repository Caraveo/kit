//! Embeds the Kit icon and version info into the Windows executable.

fn main() {
    println!("cargo:rerun-if-changed=../packaging/icons/kit.ico");
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../packaging/icons/kit.ico");
        res.set("ProductName", "Kit");
        res.set("FileDescription", env!("CARGO_PKG_DESCRIPTION"));
        if let Err(e) = res.compile() {
            println!("cargo:warning=could not embed Windows resources: {e}");
        }
    }
}
