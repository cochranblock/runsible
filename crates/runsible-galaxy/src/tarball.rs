use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use tar::Archive;

use crate::errors::{GalaxyError, Result};

/// Build a `.runsible-pkg` tarball from `src_dir`.
///
/// The tarball is written to `out_dir/<name>-<version>.runsible-pkg`.
/// Returns the path to the written file and its SHA-256 hex checksum.
pub fn build_package(
    src_dir: &Path,
    name: &str,
    version: &str,
    out_dir: &Path,
) -> Result<(PathBuf, String)> {
    fs::create_dir_all(out_dir)?;

    let pkg_filename = format!("{}-{}.runsible-pkg", name, version);
    let out_path = out_dir.join(&pkg_filename);

    // Collect all files in the source directory, relative paths.
    let mut file_paths: Vec<PathBuf> = Vec::new();
    collect_files(src_dir, src_dir, &mut file_paths)?;
    file_paths.sort();

    // Compute SHA-256 for each file and build SHA256SUMS content.
    let mut sha256sums = String::new();
    let mut checksums: Vec<(PathBuf, String)> = Vec::new();
    for rel in &file_paths {
        let full = src_dir.join(rel);
        let hex = sha256_file(&full)?;
        sha256sums.push_str(&format!("{}  {}\n", hex, rel.display()));
        checksums.push((rel.clone(), hex));
    }

    // Build FILES list.
    let files_content: String = file_paths
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";

    // Write the tarball.
    let out_file = fs::File::create(&out_path)?;
    let enc = GzEncoder::new(out_file, Compression::default());
    let mut tar = tar::Builder::new(enc);

    // Archive all source files.
    for rel in &file_paths {
        let full = src_dir.join(rel);
        let mut f = fs::File::open(&full)?;
        let meta = f.metadata()?;
        let mut header = tar::Header::new_gnu();
        header.set_size(meta.len());
        header.set_mode(0o644);
        header.set_mtime(0); // deterministic
        header.set_uid(0);
        header.set_gid(0);
        header.set_cksum();
        tar.append_data(&mut header, rel, &mut f)?;
    }

    // Append FILES entry.
    {
        let data = files_content.as_bytes();
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_mtime(0);
        header.set_uid(0);
        header.set_gid(0);
        header.set_cksum();
        tar.append_data(&mut header, "FILES", data)?;
    }

    // Append SHA256SUMS entry.
    {
        let data = sha256sums.as_bytes();
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_mtime(0);
        header.set_uid(0);
        header.set_gid(0);
        header.set_cksum();
        tar.append_data(&mut header, "SHA256SUMS", data)?;
    }

    let enc = tar.into_inner()?;
    enc.finish()?;

    // Compute checksum of the tarball itself.
    let pkg_checksum = sha256_file(&out_path)?;

    Ok((out_path, format!("sha256:{}", pkg_checksum)))
}

/// Extract a `.runsible-pkg` tarball into `dest_dir`.
/// Returns the list of extracted relative paths and validates SHA256SUMS.
pub fn extract_package(pkg_path: &Path, dest_dir: &Path) -> Result<Vec<PathBuf>> {
    fs::create_dir_all(dest_dir)?;

    // Extract everything.
    let file = fs::File::open(pkg_path)?;
    let dec = GzDecoder::new(file);
    let mut archive = Archive::new(dec);
    archive.unpack(dest_dir)?;

    // Read FILES.
    let files_path = dest_dir.join("FILES");
    let files_content = fs::read_to_string(&files_path).map_err(|e| {
        GalaxyError::Tarball(format!("cannot read FILES from tarball: {}", e))
    })?;

    let extracted: Vec<PathBuf> = files_content
        .lines()
        .filter(|l| !l.is_empty())
        .map(PathBuf::from)
        .collect();

    // Read and validate SHA256SUMS.
    let sums_path = dest_dir.join("SHA256SUMS");
    if sums_path.exists() {
        let sums_content = fs::read_to_string(&sums_path)?;
        for line in sums_content.lines() {
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.splitn(2, "  ").collect();
            if parts.len() != 2 {
                continue;
            }
            let expected_hex = parts[0];
            let rel_path = PathBuf::from(parts[1]);
            let full = dest_dir.join(&rel_path);
            if full.exists() {
                let actual_hex = sha256_file(&full)?;
                if actual_hex != expected_hex {
                    return Err(GalaxyError::ChecksumMismatch {
                        path: rel_path.display().to_string(),
                        expected: expected_hex.to_string(),
                        actual: actual_hex,
                    });
                }
            }
        }
    }

    Ok(extracted)
}

/// Compute SHA-256 of a file; returns lowercase hex string.
pub fn sha256_file(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Compute SHA-256 of raw bytes.
pub fn sha256_bytes(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

/// Recursively collect all files under `base`, returning paths relative to `base`.
fn collect_files(base: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let rel = path.strip_prefix(base).map_err(|e| {
            GalaxyError::Tarball(format!("strip_prefix error: {}", e))
        })?;
        if path.is_dir() {
            collect_files(base, &path, out)?;
        } else {
            out.push(rel.to_path_buf());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn build_and_extract_roundtrip() {
        let tmp = std::env::temp_dir();
        let src = tmp.join(format!("runsible-src-{}", std::process::id()));
        let out = tmp.join(format!("runsible-out-{}", std::process::id()));
        let dst = tmp.join(format!("runsible-dst-{}", std::process::id()));

        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("runsible.toml"), b"[package]\nname = \"test\"\nversion = \"0.1.0\"\n").unwrap();
        fs::create_dir_all(src.join("tasks")).unwrap();
        fs::write(src.join("tasks").join("main.toml"), b"# tasks\n").unwrap();

        let (pkg_path, _checksum) = build_package(&src, "test", "0.1.0", &out).unwrap();
        assert!(pkg_path.exists());

        let extracted = extract_package(&pkg_path, &dst).unwrap();
        let names: HashSet<String> = extracted.iter().map(|p| p.display().to_string()).collect();
        assert!(names.contains("runsible.toml"), "runsible.toml should be extracted");
        assert!(
            names.iter().any(|n| n.contains("tasks")),
            "tasks/main.toml should be extracted"
        );

        // Cleanup
        let _ = fs::remove_dir_all(&src);
        let _ = fs::remove_dir_all(&out);
        let _ = fs::remove_dir_all(&dst);
    }

    #[test]
    fn checksum_in_tarball() {
        let tmp = std::env::temp_dir();
        let src = tmp.join(format!("runsible-csum-src-{}", std::process::id()));
        let out = tmp.join(format!("runsible-csum-out-{}", std::process::id()));
        let dst = tmp.join(format!("runsible-csum-dst-{}", std::process::id()));

        fs::create_dir_all(&src).unwrap();
        let content = b"hello world\n";
        fs::write(src.join("hello.txt"), content).unwrap();

        build_package(&src, "csum-test", "0.0.1", &out).unwrap();
        let pkg_path = out.join("csum-test-0.0.1.runsible-pkg");

        extract_package(&pkg_path, &dst).unwrap();

        // Read SHA256SUMS from extracted dir.
        let sums_content = fs::read_to_string(dst.join("SHA256SUMS")).unwrap();
        let mut found = false;
        for line in sums_content.lines() {
            if line.contains("hello.txt") {
                let parts: Vec<&str> = line.splitn(2, "  ").collect();
                let recorded_hex = parts[0];
                let actual_hex = sha256_bytes(content);
                assert_eq!(recorded_hex, actual_hex, "SHA256 mismatch for hello.txt");
                found = true;
            }
        }
        assert!(found, "hello.txt not found in SHA256SUMS");

        let _ = fs::remove_dir_all(&src);
        let _ = fs::remove_dir_all(&out);
        let _ = fs::remove_dir_all(&dst);
    }
}
