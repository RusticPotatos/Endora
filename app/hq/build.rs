//! Embeds Endora's public records and plans (`docs/adr/*.md`, `docs/plans/*.md`, top level
//! only) in HQ, for the CEO's `read_doc` (ADR 0111). They are public on GitHub already; HQ
//! reads nothing from the disk at run time, so no path a turn gives can reach a file.

use std::fmt::Write as _;
use std::path::Path;

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut found: Vec<(String, std::path::PathBuf)> = Vec::new();
    for folder in ["docs/adr", "docs/plans"] {
        let dir = root.join(folder);
        println!("cargo:rerun-if-changed={}", dir.display());
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if !path.is_file() || !name.ends_with(".md") {
                continue;
            }
            println!("cargo:rerun-if-changed={}", path.display());
            found.push((format!("{folder}/{name}"), path));
        }
    }
    found.sort();
    let mut out = String::from("/// Endora's public records and plans, as `(path, text)`.\n");
    out.push_str("pub static PUBLIC_DOCS: &[(&str, &str)] = &[\n");
    for (published, path) in &found {
        let full = path.canonicalize().unwrap_or_else(|_| path.clone());
        let _ = writeln!(
            out,
            "    ({published:?}, include_str!({:?})),",
            full.display().to_string()
        );
    }
    out.push_str("];\n");
    let dest =
        Path::new(&std::env::var("OUT_DIR").expect("cargo sets OUT_DIR")).join("public_docs.rs");
    std::fs::write(dest, out).expect("the docs' index is written");
}
