//! Embeds the icon and the version info into `Rain.exe`.

fn main() {
    let icon = "../../assets/icon/rain.ico";
    println!("cargo:rerun-if-changed={icon}");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon(icon)
        .set("ProductName", "Rain")
        .set("FileDescription", "Rain — code review")
        .set("CompanyName", "Sumit Patel")
        .set("InternalName", "Rain")
        .set("OriginalFilename", "Rain.exe");
    if let Err(e) = res.compile() {
        panic!("embedding the Windows resources failed: {e}");
    }
}
