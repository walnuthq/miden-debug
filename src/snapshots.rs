//! Locating recorded replay snapshots by transaction ID.
//!
//! `miden-debug --trace <REF>` and `--replay <REF>` accept either a snapshot file path or a
//! transaction ID (optionally a unique prefix). A transaction ID resolves to a snapshot named
//! `<tx_id>.mdsnap` in the snapshot directories, which is where `miden-client ... --record`
//! stores recordings keyed by the executed transaction's ID.
//!
//! Note that only *locally recorded* transactions can be resolved this way: a transaction ID on
//! its own is not enough to re-execute a transaction, since the chain does not carry the
//! execution inputs (advice data, signatures, resolved code) that the snapshot captures.

use std::path::{Path, PathBuf};

use miden_assembly_syntax::diagnostics::Report;

/// Overrides the snapshot search directory when set.
pub const SNAPSHOT_DIR_ENV: &str = "MIDEN_DEBUG_SNAPSHOTS";

/// The file extension of replay snapshots.
const SNAPSHOT_EXT: &str = "mdsnap";

/// The directories searched when resolving a transaction ID, in order: the
/// [`SNAPSHOT_DIR_ENV`] override, `./.miden/debug-snapshots`, and `~/.miden/debug-snapshots`.
pub fn snapshot_search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(dir) = std::env::var_os(SNAPSHOT_DIR_ENV) {
        dirs.push(PathBuf::from(dir));
    }
    dirs.push(PathBuf::from(".miden").join("debug-snapshots"));
    if let Some(home) = std::env::home_dir() {
        dirs.push(home.join(".miden").join("debug-snapshots"));
    }
    dirs
}

/// Resolve a `--trace`/`--replay` argument to a snapshot file path.
///
/// An existing file path is used as-is. Otherwise, if the argument looks like a transaction ID
/// (a `0x`-prefixed hex string, or a unique prefix of one), the snapshot directories are searched
/// for a matching `<tx_id>.mdsnap`.
pub fn resolve_snapshot_ref(reference: &Path) -> Result<PathBuf, Report> {
    resolve_snapshot_ref_in(reference, &snapshot_search_dirs())
}

fn resolve_snapshot_ref_in(reference: &Path, dirs: &[PathBuf]) -> Result<PathBuf, Report> {
    if reference.is_file() {
        return Ok(reference.to_path_buf());
    }

    let reference_str = reference.to_string_lossy();
    if !looks_like_transaction_id(&reference_str) {
        return Err(Report::msg(format!("snapshot file not found: {}", reference.display())));
    }

    let needle = reference_str.to_ascii_lowercase();
    let mut matches = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let is_snapshot =
                path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case(SNAPSHOT_EXT));
            let stem_matches = path
                .file_stem()
                .map(|stem| stem.to_string_lossy().to_ascii_lowercase())
                .is_some_and(|stem| stem.starts_with(&needle));
            if is_snapshot && stem_matches {
                matches.push(path);
            }
        }
        // Prefer the first directory that has any match: later directories are fallbacks, and
        // mixing them could report a stale copy of the same transaction as ambiguous.
        if !matches.is_empty() {
            break;
        }
    }

    match matches.len() {
        1 => Ok(matches.remove(0)),
        0 => Err(Report::msg(format!(
            "no recorded snapshot found for transaction '{reference_str}' (searched: {}). Record \
             one with `miden-client ... --record`.",
            dirs.iter().map(|d| d.display().to_string()).collect::<Vec<_>>().join(", ")
        ))),
        _ => {
            matches.sort();
            Err(Report::msg(format!(
                "transaction ID prefix '{reference_str}' is ambiguous; it matches: {}",
                matches.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ")
            )))
        }
    }
}

/// Returns true when `value` looks like a (prefix of a) transaction ID: `0x` followed by hex
/// digits.
fn looks_like_transaction_id(value: &str) -> bool {
    value
        .strip_prefix("0x")
        .is_some_and(|hex| !hex.is_empty() && hex.chars().all(|c| c.is_ascii_hexdigit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup(dir: &Path, names: &[&str]) {
        std::fs::create_dir_all(dir).unwrap();
        for name in names {
            std::fs::write(dir.join(name), b"stub").unwrap();
        }
    }

    #[test]
    fn resolves_ids_and_prefixes() {
        let root =
            std::env::temp_dir().join(format!("miden-snapshot-resolver-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dir = root.join("snaps");
        setup(&dir, &["0xabc123.mdsnap", "0xabd999.mdsnap", "notes.txt"]);
        let dirs = vec![dir.clone()];

        // An existing file path wins outright.
        let literal = dir.join("0xabc123.mdsnap");
        assert_eq!(resolve_snapshot_ref_in(&literal, &dirs).unwrap(), literal);

        // Full ID and unique prefix resolve; case is ignored.
        assert_eq!(
            resolve_snapshot_ref_in(Path::new("0xabc123"), &dirs).unwrap(),
            dir.join("0xabc123.mdsnap")
        );
        assert_eq!(
            resolve_snapshot_ref_in(Path::new("0xAbC1"), &dirs).unwrap(),
            dir.join("0xabc123.mdsnap")
        );

        // An ambiguous prefix is rejected.
        let err = resolve_snapshot_ref_in(Path::new("0xab"), &dirs).unwrap_err();
        assert!(err.to_string().contains("ambiguous"), "unexpected error: {err}");

        // Unknown IDs and non-ID missing paths get distinct errors.
        let err = resolve_snapshot_ref_in(Path::new("0x999999"), &dirs).unwrap_err();
        assert!(err.to_string().contains("no recorded snapshot"), "unexpected error: {err}");
        let err = resolve_snapshot_ref_in(Path::new("missing.mdsnap"), &dirs).unwrap_err();
        assert!(err.to_string().contains("snapshot file not found"), "unexpected error: {err}");

        let _ = std::fs::remove_dir_all(&root);
    }
}
