use regex::Regex;
use std::sync::LazyLock;

static FM_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)\A---\s*\n(.*?)\n---\s*\n").unwrap());

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Frontmatter {
    pub status: Option<String>,
    pub created: Option<String>,
    pub updated: Option<String>,
    pub tags: Vec<String>,
    pub superseded_by: Option<String>,
}

/// Parse YAML frontmatter from text. Returns (Frontmatter, remaining body).
pub fn parse(text: &str) -> (Frontmatter, &str) {
    let Some(caps) = FM_PATTERN.captures(text) else {
        return (Frontmatter::default(), text);
    };

    let yaml_str = &caps[1];
    let body_start = caps.get(0).unwrap().end();
    let body = &text[body_start..];

    let yaml: serde_yaml::Value = match serde_yaml::from_str(yaml_str) {
        Ok(v) => v,
        Err(_) => return (Frontmatter::default(), body),
    };

    let map = match yaml {
        serde_yaml::Value::Mapping(m) => m,
        serde_yaml::Value::Null => return (Frontmatter::default(), body),
        _ => return (Frontmatter::default(), body),
    };

    let fm = Frontmatter {
        status: value_to_string(map.get("status")),
        created: value_to_string(map.get("created")),
        updated: value_to_string(map.get("updated")),
        tags: extract_tags(map.get("tags")),
        superseded_by: value_to_string(map.get("superseded_by")),
    };

    (fm, body)
}

fn value_to_string(val: Option<&serde_yaml::Value>) -> Option<String> {
    match val? {
        serde_yaml::Value::Null => None,
        serde_yaml::Value::String(s) => Some(s.clone()),
        serde_yaml::Value::Bool(b) => Some(b.to_string()),
        serde_yaml::Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn extract_tags(val: Option<&serde_yaml::Value>) -> Vec<String> {
    let Some(serde_yaml::Value::Sequence(seq)) = val else {
        return Vec::new();
    };
    seq.iter()
        .filter_map(|v| match v {
            serde_yaml::Value::String(s) => Some(s.clone()),
            serde_yaml::Value::Number(n) => Some(n.to_string()),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_full_frontmatter() {
        let text = "---\nstatus: current\ncreated: 2026-01-01\nupdated: 2026-03-24\ntags: [検索, Rust]\nsuperseded_by: other.md\n---\n\n# Title\n\nBody text.\n";
        let (fm, body) = parse(text);
        assert_eq!(fm.status.as_deref(), Some("current"));
        assert_eq!(fm.created.as_deref(), Some("2026-01-01"));
        assert_eq!(fm.updated.as_deref(), Some("2026-03-24"));
        assert_eq!(fm.tags, vec!["検索", "Rust"]);
        assert_eq!(fm.superseded_by.as_deref(), Some("other.md"));
        assert!(body.contains("# Title"));
    }

    #[test]
    fn test_no_frontmatter() {
        let text = "# Just a title\n\nSome text.\n";
        let (fm, body) = parse(text);
        assert_eq!(fm, Frontmatter::default());
        assert_eq!(body, text);
    }

    #[test]
    fn test_partial_frontmatter() {
        let text = "---\nstatus: outdated\n---\n\nBody.\n";
        let (fm, body) = parse(text);
        assert_eq!(fm.status.as_deref(), Some("outdated"));
        assert!(fm.created.is_none());
        assert!(fm.updated.is_none());
        assert!(fm.tags.is_empty());
        assert!(body.contains("Body."));
    }

    #[test]
    fn test_superseded_by() {
        let text = "---\nstatus: superseded\nsuperseded_by: new-doc.md\n---\n\nOld content.\n";
        let (fm, _body) = parse(text);
        assert_eq!(fm.status.as_deref(), Some("superseded"));
        assert_eq!(fm.superseded_by.as_deref(), Some("new-doc.md"));
    }

    #[test]
    fn test_empty_frontmatter() {
        let text = "---\n---\n\nBody.\n";
        let (fm, body) = parse(text);
        assert_eq!(fm, Frontmatter::default());
        assert!(body.contains("Body."));
    }

    #[test]
    fn test_date_type_converted_to_string() {
        // serde_yaml parses bare dates like 2026-01-01 as strings already,
        // but if tagged, they should still become strings
        let text = "---\ncreated: 2026-01-15\nupdated: 2026-03-24\n---\n\nText.\n";
        let (fm, _body) = parse(text);
        assert!(fm.created.is_some());
        assert!(fm.created.unwrap().contains("2026"));
    }

    #[test]
    fn test_tags_null() {
        let text = "---\nstatus: current\ntags:\n---\n\nBody.\n";
        let (fm, _body) = parse(text);
        assert!(fm.tags.is_empty());
    }
}
