//! Windows builds: puts the application icon and version information into
//! the program file (shown by the Explorer, the taskbar and "Eigenschaften").
//! Uses the resource compiler of the MinGW tool chain; without it the
//! program is built without icon resource.

use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/versolopdf.ico");
    let windows = std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "windows");
    let gnu = std::env::var("CARGO_CFG_TARGET_ENV").is_ok_and(|env| env == "gnu");
    if !windows || !gnu {
        return;
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let icon = root.join("assets").join("versolopdf.ico");
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let parts: Vec<u32> = version.split('.').filter_map(|p| p.parse().ok()).collect();
    let v = |i: usize| parts.get(i).copied().unwrap_or(0);
    let rc = format!(
        r#"1 ICON "{icon}"
1 VERSIONINFO
FILEVERSION {a},{b},{c},0
PRODUCTVERSION {a},{b},{c},0
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040704B0"
    BEGIN
      VALUE "FileDescription", "VersoloPDF"
      VALUE "ProductName", "VersoloPDF"
      VALUE "FileVersion", "{version}"
      VALUE "ProductVersion", "{version}"
      VALUE "OriginalFilename", "VersoloPDF.exe"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x0407, 1200
  END
END
"#,
        icon = icon.display().to_string().replace('\\', "/"),
        a = v(0),
        b = v(1),
        c = v(2),
    );
    let rc_path = out.join("versolopdf.rc");
    std::fs::write(&rc_path, rc).expect("write resource script");
    let obj = out.join("versolopdf-resources.o");
    let tool = std::env::var("WINDRES").unwrap_or_else(|_| "x86_64-w64-mingw32-windres".into());
    let status = Command::new(&tool).arg(&rc_path).args(["-O", "coff", "-o"]).arg(&obj).status();
    match status {
        Ok(s) if s.success() => println!("cargo:rustc-link-arg-bins={}", obj.display()),
        _ => println!("cargo:warning=no Windows resource compiler ({tool}); VersoloPDF.exe gets no icon"),
    }
}
