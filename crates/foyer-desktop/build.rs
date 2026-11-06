//! Embeds the icon and version info into the Windows executable.

fn main() {
    println!("cargo:rerun-if-changed=packaging/windows/foyer.rc");
    println!("cargo:rerun-if-changed=packaging/icon.ico");
    println!("cargo:rerun-if-env-changed=FOYER_UPDATE_PUBLIC_KEY");

    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let version = env!("CARGO_PKG_VERSION");
        let numbers: Vec<&str> = version
            .split(['.', '-', '+'])
            .take(3)
            .chain(std::iter::repeat("0"))
            .take(4)
            .collect();
        let icon = concat!(env!("CARGO_MANIFEST_DIR"), "/packaging/icon.ico").replace('\\', "/");
        let macros = [
            format!("FOYER_ICON=\"{icon}\""),
            format!("FOYER_VERSION=\"{version}\""),
            format!("FOYER_VERSION_NUMBERS={}", numbers.join(",")),
        ];
        embed_resource::compile("packaging/windows/foyer.rc", macros)
            .manifest_optional()
            .expect("failed to embed the Windows resources");
    }
}
