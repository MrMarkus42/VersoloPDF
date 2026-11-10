//! Signs release packages for the update check of VersoloPDF.
//!
//!     cargo run --release --example sign_release -- new <key file>
//!         creates a private key (only once; keep it safe and private) and
//!         prints the public key for `UPDATE_KEYS` in src/update.rs
//!     cargo run --release --example sign_release -- sign <key file> <package.zip>...
//!         writes "<package.zip>.sig" next to every package
//!     cargo run --release --example sign_release -- verify <package.zip>...
//!         checks "<package.zip>.sig" with the keys built into VersoloPDF
//!
//! The key file holds the 32-byte private key as hex. It never goes into the
//! repository or to GitHub.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use versolopdf::update;

fn read_key(path: &Path) -> Result<Vec<u8>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let hex = text.trim();
    let bytes: Option<Vec<u8>> =
        (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok()).collect();
    match bytes {
        Some(b) if b.len() == 32 => Ok(b),
        _ => Err(format!("{}: no valid key", path.display())),
    }
}

fn file_name(path: &Path) -> Result<String, String> {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| format!("{}: no file name", path.display()))
}

fn sig_path(package: &Path) -> PathBuf {
    let mut name = package.as_os_str().to_os_string();
    name.push(".sig");
    PathBuf::from(name)
}

fn new_key(path: &Path) -> Result<(), String> {
    if path.exists() {
        return Err(format!("{} exists already – a key is created only once", path.display()));
    }
    let mut secret = [0u8; 32];
    getrandom::fill(&mut secret).map_err(|e| e.to_string())?;
    let public = update::public_key_hex(&secret).map_err(|e| e.to_string())?;
    let hex: String = secret.iter().map(|b| format!("{b:02x}")).collect();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    std::io::Write::write_all(&mut file, format!("{hex}\n").as_bytes()).map_err(|e| e.to_string())?;
    println!("private key: {}", path.display());
    println!("public key (UPDATE_KEYS): {public}");
    Ok(())
}

fn sign(key: &Path, packages: &[PathBuf]) -> Result<(), String> {
    let secret = read_key(key)?;
    let public = update::public_key_hex(&secret).map_err(|e| e.to_string())?;
    if !update::UPDATE_KEYS.contains(&public.as_str()) {
        return Err("this key is not built into VersoloPDF (UPDATE_KEYS)".into());
    }
    for package in packages {
        let name = file_name(package)?;
        let sha = update::file_sha256(package).map_err(|e| e.to_string())?;
        let text = update::sign_package(&secret, &name, &sha).map_err(|e| e.to_string())?;
        // Checked before it is written.
        update::verify_signature(&text, &name, update::UPDATE_KEYS).map_err(|e| e.to_string())?;
        let out = sig_path(package);
        std::fs::write(&out, text).map_err(|e| format!("{}: {e}", out.display()))?;
        println!("signed: {}", out.display());
    }
    Ok(())
}

fn verify(packages: &[PathBuf]) -> Result<(), String> {
    for package in packages {
        let name = file_name(package)?;
        let path = sig_path(package);
        let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let signed =
            update::verify_signature(&text, &name, update::UPDATE_KEYS).map_err(|e| e.to_string())?;
        let actual = update::file_sha256(package).map_err(|e| e.to_string())?;
        if signed != actual {
            return Err(format!("{name}: the file does not match its signature"));
        }
        println!("ok: {name}");
    }
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let paths = |from: usize| args.iter().skip(from).map(PathBuf::from).collect::<Vec<_>>();
    let result = match args.first().map(String::as_str) {
        Some("new") if args.len() == 2 => new_key(Path::new(&args[1])),
        Some("sign") if args.len() >= 3 => sign(Path::new(&args[1]), &paths(2)),
        Some("verify") if args.len() >= 2 => verify(&paths(1)),
        _ => Err("usage: sign_release new <key> | sign <key> <zip>... | verify <zip>...".into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
