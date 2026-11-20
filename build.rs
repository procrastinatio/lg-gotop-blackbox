fn main() {
    println!("cargo:rerun-if-changed=assets/gotop-blackbox.rc");
    println!("cargo:rerun-if-changed=assets/gotop-blackbox.manifest");

    // Embed the application manifest (Common Controls v6) into the MSVC build.
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        embed_resource::compile("assets/gotop-blackbox.rc", embed_resource::NONE)
            .manifest_required()
            .expect("failed to embed the application manifest");
    }
}
