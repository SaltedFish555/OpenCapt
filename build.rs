fn main() {
    println!("cargo:rerun-if-changed=assets/icons/tray.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/icons/tray.ico");
    res.set("FileDescription", "OpenCapt");
    res.set("ProductName", "OpenCapt");
    res.compile().expect("failed to compile windows resources");
}
