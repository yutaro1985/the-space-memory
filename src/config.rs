use std::path::PathBuf;

pub const PROJECT_ROOT: &str = "/workspaces";
pub const MAX_CHUNK_CHARS: usize = 800;
pub const RRF_K: f64 = 60.0;
pub const SCORE_THRESHOLD: f64 = 0.005;
pub const MAX_RESULTS: usize = 5;
pub const EMBEDDING_DIM: usize = 256;
pub const SOCKET_PATH: &str = "/tmp/knowledge-search-embedder.sock";
pub const DEFAULT_HALF_LIFE_DAYS: f64 = 90.0;
pub const SNIPPET_MAX_CHARS: usize = 200;
pub const MIN_SESSION_MESSAGE_LEN: usize = 10;
pub const BACKFILL_BATCH_SIZE: usize = 8;
pub const MAX_QUERY_EXPANSIONS: usize = 5;
pub const RECENT_DAYS: i64 = 30;
pub const EMBEDDER_IDLE_TIMEOUT_SECS: u64 = 600;
pub const DICT_CANDIDATE_FREQ_THRESHOLD: i64 = 5;

/// Content directories with score weights. (directory, weight)
pub const CONTENT_DIRS: &[(&str, f64)] = &[
    // daily
    ("daily/notes", 1.0),
    ("daily/daily/research", 1.1),
    ("daily/daily/intel", 0.7),
    // company
    ("company/knowledge", 1.3),
    ("company/ideas", 0.9),
    ("company/updates", 0.8),
    ("company/research", 1.2),
    ("company/products", 1.2),
    ("company/decisions", 1.1),
    ("company/retrospectives", 0.9),
    // novels
    ("novels/novels", 1.0),
    ("novels/references", 1.0),
    ("novels/memory", 0.8),
    ("novels/styles", 0.8),
];
pub const SESSION_WEIGHT: f64 = 0.3;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub fn db_path() -> PathBuf {
    repo_root().join("data").join("knowledge-rust.db")
}

pub fn user_dict_path() -> PathBuf {
    repo_root().join("data").join("user_dict.csv")
}

pub fn custom_terms_path() -> PathBuf {
    repo_root().join("data").join("custom_terms.toml")
}

pub fn status_penalty(status: Option<&str>) -> f64 {
    match status {
        Some("superseded") => 0.2,
        Some("rejected") | Some("dropped") => 0.3,
        Some("outdated") => 0.4,
        _ => 1.0,
    }
}

pub fn half_life_days(source_type: &str) -> f64 {
    match source_type {
        "note" => 120.0,
        "research" => 60.0,
        "session" => 30.0,
        _ => DEFAULT_HALF_LIFE_DAYS,
    }
}

pub fn source_type_from_dir(directory: &str) -> String {
    let last = directory.rsplit('/').next().unwrap_or(directory);
    match last {
        "notes" => "note",
        "research" => "research",
        "intel" => "intel",
        "knowledge" => "knowledge",
        "ideas" => "idea",
        "updates" => "update",
        "products" => "product",
        "decisions" => "decision",
        "retrospectives" => "retrospective",
        "novels" => "novel",
        "references" => "reference",
        "memory" => "memory",
        "styles" => "style",
        other => other,
    }
    .to_string()
}

/// Score weight based on directory prefix of file_path.
pub fn directory_weight(file_path: &str) -> f64 {
    if file_path.starts_with("session:") {
        return SESSION_WEIGHT;
    }
    for &(dir, weight) in CONTENT_DIRS {
        if file_path.starts_with(dir) && file_path.as_bytes().get(dir.len()) == Some(&b'/') {
            return weight;
        }
    }
    1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_content_dirs_count() {
        assert_eq!(CONTENT_DIRS.len(), 14);
    }

    #[test]
    fn test_directory_weight_known() {
        assert_eq!(directory_weight("company/knowledge/foo.md"), 1.3);
        assert_eq!(directory_weight("daily/daily/intel/2026-01.md"), 0.7);
        assert_eq!(directory_weight("company/products/ks.md"), 1.2);
        assert_eq!(directory_weight("daily/notes/test.md"), 1.0);
    }

    #[test]
    fn test_directory_weight_session() {
        assert_eq!(directory_weight("session:abc123"), SESSION_WEIGHT);
    }

    #[test]
    fn test_directory_weight_unknown() {
        assert_eq!(directory_weight("unknown/path/file.md"), 1.0);
    }

    #[test]
    fn test_directory_weight_boundary() {
        // "daily/notes_extra/foo.md" must NOT match "daily/notes"
        assert_eq!(directory_weight("daily/notes_extra/foo.md"), 1.0);
    }

    #[test]
    fn test_no_prefix_shadowing() {
        for (i, &(a, _)) in CONTENT_DIRS.iter().enumerate() {
            for (j, &(b, _)) in CONTENT_DIRS.iter().enumerate() {
                if i != j {
                    assert!(
                        !b.starts_with(a) || !a.starts_with(b),
                        "CONTENT_DIRS[{i}]=\"{a}\" and [{j}]=\"{b}\" overlap — reorder longest-first"
                    );
                }
            }
        }
    }

    #[test]
    fn test_status_penalty_values() {
        assert_eq!(status_penalty(None), 1.0);
        assert_eq!(status_penalty(Some("current")), 1.0);
        assert_eq!(status_penalty(Some("outdated")), 0.4);
        assert_eq!(status_penalty(Some("rejected")), 0.3);
        assert_eq!(status_penalty(Some("dropped")), 0.3);
        assert_eq!(status_penalty(Some("superseded")), 0.2);
    }

    #[test]
    fn test_half_life_days_values() {
        assert_eq!(half_life_days("note"), 120.0);
        assert_eq!(half_life_days("research"), 60.0);
        assert_eq!(half_life_days("session"), 30.0);
        assert_eq!(half_life_days("unknown"), DEFAULT_HALF_LIFE_DAYS);
    }

    #[test]
    fn test_source_type_from_dir() {
        assert_eq!(source_type_from_dir("daily/notes"), "note");
        assert_eq!(source_type_from_dir("company/knowledge"), "knowledge");
        assert_eq!(source_type_from_dir("company/products"), "product");
        assert_eq!(source_type_from_dir("novels/novels"), "novel");
        assert_eq!(
            source_type_from_dir("company/retrospectives"),
            "retrospective"
        );
        assert_eq!(source_type_from_dir("unknown_dir"), "unknown_dir");
    }

    #[test]
    fn test_user_dict_path() {
        let path = user_dict_path();
        assert!(path.to_string_lossy().ends_with("data/user_dict.csv"));
    }

    #[test]
    fn test_dict_candidate_freq_threshold() {
        assert_eq!(DICT_CANDIDATE_FREQ_THRESHOLD, 5);
    }

    #[test]
    fn test_constants() {
        assert_eq!(MAX_CHUNK_CHARS, 800);
        assert_eq!(RRF_K, 60.0);
        assert_eq!(SCORE_THRESHOLD, 0.005);
        assert_eq!(MAX_RESULTS, 5);
        assert_eq!(EMBEDDING_DIM, 256);
    }
}
