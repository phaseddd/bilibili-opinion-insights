fn main() {
    println!("cargo:rerun-if-changed=assets/app-icon/windows/app.rc");
    println!("cargo:rerun-if-changed=assets/app-icon/windows/app.ico");

    #[cfg(windows)]
    {
        embed_resource::compile_for(
            "assets/app-icon/windows/app.rc",
            ["bili-opinion-gui"],
            embed_resource::NONE,
        )
        .manifest_optional()
        .unwrap();
    }
}
