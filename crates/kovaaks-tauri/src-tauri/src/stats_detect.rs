//! KovaaK's stats-folder auto-detection across every Steam library / disk.
//!
//! Resolution order (when no stats-dir override is set in settings):
//! 1. Steam install dir from the registry (`Software\Valve\Steam`)
//! 2. Every library listed in `<steam>\config\libraryfolders.vdf` — the
//!    authoritative list of all configured Steam libraries on any disk
//! 3. Per-drive conventional Steam locations (`X:\SteamLibrary`, …)
//!
//! The first existing `<lib>\steamapps\common\FPSAimTrainer\FPSAimTrainer\stats`
//! wins. Pure path logic is cross-platform and unit-tested; registry/drive
//! enumeration is Windows-only (on other platforms detection yields `None`
//! and the caller falls back to the historical default path).

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Candidate stats dirs under one Steam library root. The standard KovaaK's
/// layout nests the game folder inside itself; a legacy flat layout is
/// probed too.
fn candidates_under_library(root: &Path) -> Vec<PathBuf> {
    let game = root.join("steamapps").join("common").join("FPSAimTrainer");
    vec![game.join("FPSAimTrainer").join("stats"), game.join("stats")]
}

/// Parse Steam's `libraryfolders.vdf` and return every configured library
/// path (any disk). Handles the VDF `\\` escaping.
/// Platform-neutral pure logic; the production caller is Windows-only
/// (`library_roots`), but the unit tests exercise it on every platform —
/// hence the dead-code allow on non-Windows builds.
#[cfg_attr(not(windows), allow(dead_code))]
fn library_paths_from_vdf(text: &str) -> Vec<PathBuf> {
    // Splitting on '"' yields [outside, inside, between, inside, …]. A
    // "path" key's value sits two chunks after the key chunk.
    let chunks: Vec<&str> = text.split('"').collect();
    let mut out = Vec::new();
    for (i, chunk) in chunks.iter().enumerate() {
        if *chunk == "path" {
            if let Some(raw) = chunks.get(i + 2) {
                let p = raw.trim().replace("\\\\", "\\");
                if !p.is_empty() {
                    out.push(PathBuf::from(p));
                }
            }
        }
    }
    out
}

/// First existing stats dir among the given library roots, in order.
fn detect_in_roots(roots: &[PathBuf]) -> Option<PathBuf> {
    roots
        .iter()
        .flat_map(|r| candidates_under_library(r))
        .find(|p| p.is_dir())
}

/// Conventional per-drive Steam locations (Windows only).
#[cfg(windows)]
fn drive_roots() -> Vec<PathBuf> {
    let mut out = Vec::new();
    for c in b'A'..=b'Z' {
        let letter = c as char;
        for tail in [
            r"SteamLibrary",
            r"Steam",
            r"Program Files (x86)\Steam",
            r"Program Files\Steam",
            r"Games\SteamLibrary",
        ] {
            out.push(PathBuf::from(format!("{letter}:\\{tail}")));
        }
    }
    out
}

/// Steam install dir from the registry, if present. `SteamPath` (HKCU) is
/// the canonical value; `InstallPath` (HKLM) covers all-user installs.
#[cfg(windows)]
fn registry_steam_dir() -> Option<PathBuf> {
    for (hive, value) in [
        (r"HKCU\Software\Valve\Steam", "SteamPath"),
        (r"HKLM\SOFTWARE\WOW6432Node\Valve\Steam", "InstallPath"),
    ] {
        let Ok(out) = std::process::Command::new("reg")
            .args(["query", hive, "/v", value])
            .output()
        else {
            continue;
        };
        let text = String::from_utf8_lossy(&out.stdout);
        for line in text.lines() {
            if line.contains(value) {
                // "    SteamPath    REG_SZ    c:/program files (x86)/steam"
                if let Some(v) = line.rsplit("REG_SZ").next() {
                    let v = v.trim();
                    if !v.is_empty() {
                        return Some(PathBuf::from(v));
                    }
                }
            }
        }
    }
    None
}

/// All candidate library roots, most-likely first.
#[cfg(windows)]
fn library_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(steam) = registry_steam_dir() {
        roots.push(steam.clone());
        // libraryfolders.vdf lists every configured Steam library on any disk.
        let vdf = steam.join("config").join("libraryfolders.vdf");
        if let Ok(text) = std::fs::read_to_string(&vdf) {
            roots.extend(library_paths_from_vdf(&text));
        }
    }
    roots.extend(drive_roots());
    roots
}

#[cfg(not(windows))]
fn library_roots() -> Vec<PathBuf> {
    Vec::new()
}

/// Detected stats dir, cached for the process lifetime (the Steam install
/// layout does not change mid-session; a settings override always wins and
/// is resolved by the caller without touching this cache).
pub fn detect_stats_dir() -> Option<PathBuf> {
    static CACHE: OnceLock<Option<PathBuf>> = OnceLock::new();
    CACHE
        .get_or_init(|| detect_in_roots(&library_roots()))
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vdf_parse_extracts_all_libraries_with_escapes() {
        let vdf = "\"libraryfolders\"\n{\n\t\"0\"\n\t{\n\t\t\"path\"\t\t\"C:\\\\Program Files (x86)\\\\Steam\"\n\t}\n\t\"1\"\n\t{\n\t\t\"path\"\t\t\"D:\\\\SteamLibrary\"\n\t}\n\t\"2\"\n\t{\n\t\t\"path\"\t\t\"E:\\\\Games\\\\Steam\"\n\t}\n}\n";
        let libs = library_paths_from_vdf(vdf);
        assert_eq!(libs.len(), 3, "{libs:?}");
        assert_eq!(libs[0], PathBuf::from("C:\\Program Files (x86)\\Steam"));
        assert_eq!(libs[1], PathBuf::from("D:\\SteamLibrary"));
        assert_eq!(libs[2], PathBuf::from("E:\\Games\\Steam"));
    }

    #[test]
    fn vdf_parse_ignores_other_keys_and_blank_paths() {
        let vdf =
            "\"path\"\t\"D:\\\\SteamLibrary\"\n\t\"addStoreLibrary\"\t\"X\"\n\"path\"\t\"\"\n";
        let libs = library_paths_from_vdf(vdf);
        assert_eq!(libs, vec![PathBuf::from("D:\\SteamLibrary")]);
    }

    #[test]
    fn detect_finds_stats_dir_on_a_secondary_disk_library() {
        let base = std::env::temp_dir().join(format!(
            "kairos-detect-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        let stats = base
            .join("SteamLibrary")
            .join("steamapps")
            .join("common")
            .join("FPSAimTrainer")
            .join("FPSAimTrainer")
            .join("stats");
        std::fs::create_dir_all(&stats).unwrap();
        let found = detect_in_roots(&[base.join("SteamLibrary")]);
        assert_eq!(found.as_deref(), Some(stats.as_path()));
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn detect_is_none_when_no_library_has_the_game() {
        assert_eq!(
            detect_in_roots(&[PathBuf::from("/nonexistent-kairos-probe-root")]),
            None
        );
    }

    #[test]
    fn candidates_cover_standard_and_legacy_layouts() {
        let c = candidates_under_library(Path::new("/lib"));
        assert_eq!(c.len(), 2);
        let last: Vec<String> = c
            .iter()
            .map(|p| p.iter().next_back().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(last, vec!["stats", "stats"]);
        // Standard layout nests FPSAimTrainer twice.
        assert!(c[0].iter().filter(|s| *s == "FPSAimTrainer").count() == 2);
        assert!(c[1].iter().filter(|s| *s == "FPSAimTrainer").count() == 1);
    }
}
