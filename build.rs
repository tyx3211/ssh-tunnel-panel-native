use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build/icon.ico");
    let version = env::var("CARGO_PKG_VERSION").expect("Cargo always provides package version");
    let numeric_version = numeric_version(&version);
    let resource = format!(
        r#"1 ICON "build/icon.ico"

1 VERSIONINFO
FILEVERSION {numeric_version}
PRODUCTVERSION {numeric_version}
FILEFLAGSMASK 0x3fL
FILEFLAGS 0x0L
FILEOS 0x40004L
FILETYPE 0x1L
FILESUBTYPE 0x0L
BEGIN
    BLOCK "StringFileInfo"
    BEGIN
        BLOCK "040904b0"
        BEGIN
            VALUE "CompanyName", "tyx3211\0"
            VALUE "FileDescription", "SSH Tunnel Panel\0"
            VALUE "FileVersion", "{version}\0"
            VALUE "InternalName", "ssh-tunnel-panel\0"
            VALUE "LegalCopyright", "Copyright (c) 2026 tyx3211\0"
            VALUE "OriginalFilename", "ssh-tunnel-panel.exe\0"
            VALUE "ProductName", "SSH Tunnel Panel\0"
            VALUE "ProductVersion", "{version}\0"
        END
    END
    BLOCK "VarFileInfo"
    BEGIN
        VALUE "Translation", 0x0409, 1200
    END
END
"#
    );
    let resource_path = PathBuf::from(
        env::var_os("OUT_DIR").expect("Cargo always provides the build output directory"),
    )
    .join("app.rc");
    fs::write(&resource_path, resource).expect("failed to write Windows application resources");
    embed_resource::compile(resource_path, embed_resource::NONE)
        .manifest_required()
        .expect("failed to embed Windows application resources");
}

fn numeric_version(version: &str) -> String {
    let mut parts = version
        .split_once('-')
        .map_or(version, |(stable, _)| stable)
        .split('.')
        .map(|part| part.parse::<u16>().unwrap_or(0));
    format!(
        "{},{},{},0",
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0)
    )
}
