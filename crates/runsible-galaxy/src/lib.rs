pub mod errors;
pub mod manifest;
pub mod registry;
pub mod resolver;
pub mod lockfile;
pub mod tarball;
pub mod init;

/// Smoke gate: exercise the full scaffold → parse → tarball → extract → re-parse
/// loop end-to-end against the public API surface (`init::init_package`,
/// `manifest::parse_manifest_file`, `tarball::build_package`,
/// `tarball::extract_package`). Returns 0 only on full success; distinct
/// non-zero codes for each stage failure. Cleans up its tempdirs.
pub fn f30() -> i32 {
    use std::path::PathBuf;

    // Stage 1: unique tempdir layout.
    let tag = format!(
        "rsl-f30-runsible-galaxy-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    let root: PathBuf = std::env::temp_dir().join(&tag);
    let scaffold_base = root.join("scaffold");
    let out_dir = root.join("out");
    let extract_dir = root.join("extract");

    let cleanup = |root: &std::path::Path| {
        let _ = std::fs::remove_dir_all(root);
    };

    if std::fs::create_dir_all(&scaffold_base).is_err() {
        cleanup(&root);
        return 1;
    }

    // Stage 2: scaffold a package via the public init API.
    if init::init_package("f30pkg", Some(&scaffold_base)).is_err() {
        cleanup(&root);
        return 2;
    }

    let pkg_dir = scaffold_base.join("f30pkg");
    let manifest_path = pkg_dir.join("runsible.toml");
    if !manifest_path.exists() {
        cleanup(&root);
        return 3;
    }

    // Stage 3: parse the scaffold manifest.
    let original = match manifest::parse_manifest_file(&manifest_path) {
        Ok(m) => m,
        Err(_) => {
            cleanup(&root);
            return 4;
        }
    };
    if original.package.name != "f30pkg" {
        cleanup(&root);
        return 5;
    }
    // Scaffold version must be valid semver — `parse_manifest_file` already
    // calls validate(), so we just lock in the expected default.
    if original.package.version != "0.1.0" {
        cleanup(&root);
        return 6;
    }

    // Stage 4: build a tarball.
    let (pkg_path, checksum) =
        match tarball::build_package(&pkg_dir, "f30pkg", &original.package.version, &out_dir) {
            Ok(t) => t,
            Err(_) => {
                cleanup(&root);
                return 7;
            }
        };
    if !pkg_path.exists() {
        cleanup(&root);
        return 8;
    }
    let pkg_meta = match std::fs::metadata(&pkg_path) {
        Ok(m) => m,
        Err(_) => {
            cleanup(&root);
            return 9;
        }
    };
    if pkg_meta.len() == 0 {
        cleanup(&root);
        return 10;
    }
    if !checksum.starts_with("sha256:") {
        cleanup(&root);
        return 11;
    }

    // Stage 5: extract into a fresh dir.
    let extracted = match tarball::extract_package(&pkg_path, &extract_dir) {
        Ok(e) => e,
        Err(_) => {
            cleanup(&root);
            return 12;
        }
    };
    if !extracted
        .iter()
        .any(|p| p.to_string_lossy() == "runsible.toml")
    {
        cleanup(&root);
        return 13;
    }

    // Stage 6: re-parse the extracted manifest and confirm it matches the original.
    let extracted_manifest_path = extract_dir.join("runsible.toml");
    if !extracted_manifest_path.exists() {
        cleanup(&root);
        return 14;
    }
    let round_trip = match manifest::parse_manifest_file(&extracted_manifest_path) {
        Ok(m) => m,
        Err(_) => {
            cleanup(&root);
            return 15;
        }
    };
    if round_trip.package.name != original.package.name {
        cleanup(&root);
        return 16;
    }
    if round_trip.package.version != original.package.version {
        cleanup(&root);
        return 17;
    }
    if round_trip != original {
        cleanup(&root);
        return 18;
    }

    cleanup(&root);
    0
}
