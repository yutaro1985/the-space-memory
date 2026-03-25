use rusqlite::Connection;

use crate::db;
use crate::entity;
use crate::tokenizer::wakachi;

/// Query type determined by rule-based classification.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum QueryType {
    /// Short query (1-2 tokens), technical terms. FTS5-heavy.
    Keyword,
    /// Question form ("〜とは", "どうやって", etc.). Vector-heavy.
    Question,
    /// Query matches known entities in the graph.
    EntityFocused,
    /// Long natural language (>10 tokens). Vector-only.
    Fuzzy,
    /// Default hybrid search.
    Default,
}

/// Result of query classification with RRF weights.
#[derive(Debug, Clone)]
pub struct QueryClassification {
    pub query_type: QueryType,
    pub fts_weight: f64,
    pub vec_weight: f64,
    /// Entity IDs matched in the query (reusable by entity search).
    pub matched_entity_ids: Vec<i64>,
}

impl QueryClassification {
    fn new(query_type: QueryType, entity_ids: Vec<i64>) -> Self {
        let (fts_weight, vec_weight) = match query_type {
            QueryType::Keyword => (1.5, 0.5),
            QueryType::Question => (0.5, 1.5),
            QueryType::EntityFocused => (1.2, 0.8),
            QueryType::Fuzzy => (0.0, 1.0),
            QueryType::Default => (1.0, 1.0),
        };
        Self {
            query_type,
            fts_weight,
            vec_weight,
            matched_entity_ids: entity_ids,
        }
    }
}

/// Classify a query into a QueryType with associated RRF weights.
/// Also returns matched entity IDs for reuse by downstream search logic.
pub fn classify(conn: &Connection, query: &str) -> QueryClassification {
    let query = query.trim();
    if query.is_empty() {
        return QueryClassification::new(QueryType::Default, Vec::new());
    }

    // Entity lookup (results cached for reuse)
    let entity_ids = find_matching_entity_ids(conn, query);

    // 1. Entity-focused: query matches a known entity name
    if !entity_ids.is_empty() {
        return QueryClassification::new(QueryType::EntityFocused, entity_ids);
    }

    // Token count (lindera runs once here; wakachi result is not reused elsewhere)
    let token_count = count_tokens(query);

    // Guard: 0 tokens → Default
    if token_count == 0 {
        return QueryClassification::new(QueryType::Default, Vec::new());
    }

    // 2. Short query (1-2 tokens) → Keyword
    if token_count <= 2 {
        return QueryClassification::new(QueryType::Keyword, Vec::new());
    }

    // 3. Question patterns
    if is_question(query) {
        return QueryClassification::new(QueryType::Question, Vec::new());
    }

    // 4. Long query (>10 tokens) → Fuzzy
    if token_count > 10 {
        return QueryClassification::new(QueryType::Fuzzy, Vec::new());
    }

    // 5. Default
    QueryClassification::new(QueryType::Default, Vec::new())
}

/// Count tokens using lindera morphological analysis.
fn count_tokens(query: &str) -> usize {
    wakachi(query).split_whitespace().count()
}

/// Find entity IDs matching the query (full name match + NER extraction).
fn find_matching_entity_ids(conn: &Connection, query: &str) -> Vec<i64> {
    if !db::has_entity_tables(conn) {
        return Vec::new();
    }

    let mut ids = Vec::new();
    let normalized = query.trim().to_lowercase();

    // Full query as entity name
    if normalized.chars().count() >= 2 {
        if let Ok(id) = conn.query_row(
            "SELECT id FROM entities WHERE name = ?",
            rusqlite::params![normalized],
            |row| row.get::<_, i64>(0),
        ) {
            ids.push(id);
        }
    }

    // NER extraction
    for e in &entity::extract_entities(query) {
        if let Ok(id) = conn.query_row(
            "SELECT id FROM entities WHERE name = ?",
            rusqlite::params![e.name],
            |row| row.get::<_, i64>(0),
        ) {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }

    ids
}

/// Check if the query matches Japanese question patterns.
fn is_question(query: &str) -> bool {
    // Trailing question mark is always a question
    let trimmed = query.trim();
    if trimmed.ends_with('？') || trimmed.ends_with('?') {
        return true;
    }

    const PATTERNS: &[&str] = &[
        "とは",
        "の方法",
        "のやり方",
        "どうやって",
        "どうすれば",
        "どのように",
        "なぜ",
        "どうして",
        "ですか",
        "でしょうか",
        "って何",
        "とは何",
        "について",
        "の違い",
        "の比較",
    ];
    PATTERNS.iter().any(|p| query.contains(p))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    #[test]
    fn test_classify_empty() {
        let conn = db::get_memory_connection().unwrap();
        assert_eq!(classify(&conn, "").query_type, QueryType::Default);
        assert_eq!(classify(&conn, "   ").query_type, QueryType::Default);
    }

    #[test]
    fn test_classify_keyword_short() {
        let conn = db::get_memory_connection().unwrap();
        let cls = classify(&conn, "rust");
        assert_eq!(cls.query_type, QueryType::Keyword);
        assert_eq!(cls.fts_weight, 1.5);
        assert_eq!(cls.vec_weight, 0.5);
    }

    #[test]
    fn test_classify_keyword_two_tokens() {
        let conn = db::get_memory_connection().unwrap();
        assert_eq!(
            classify(&conn, "射撃 ルール").query_type,
            QueryType::Keyword
        );
    }

    #[test]
    fn test_classify_question_toha() {
        let conn = db::get_memory_connection().unwrap();
        let cls = classify(&conn, "FTS5とは何ですか");
        assert_eq!(cls.query_type, QueryType::Question);
        assert_eq!(cls.fts_weight, 0.5);
        assert_eq!(cls.vec_weight, 1.5);
    }

    #[test]
    fn test_classify_question_douyatte() {
        let conn = db::get_memory_connection().unwrap();
        assert_eq!(
            classify(&conn, "どうやってベクトル検索を実装する").query_type,
            QueryType::Question
        );
    }

    #[test]
    fn test_classify_question_naze() {
        let conn = db::get_memory_connection().unwrap();
        assert_eq!(
            classify(&conn, "なぜこの設計にしたのか説明").query_type,
            QueryType::Question
        );
    }

    #[test]
    fn test_classify_fuzzy_long() {
        let conn = db::get_memory_connection().unwrap();
        let cls = classify(
            &conn,
            "アウトドア製品の市場規模と ハンター向けの IoT デバイスの需要と供給のバランスを知りたい 特に地方の状況",
        );
        assert_eq!(cls.query_type, QueryType::Fuzzy);
        assert_eq!(cls.fts_weight, 0.0);
        assert_eq!(cls.vec_weight, 1.0);
    }

    #[test]
    fn test_classify_default_medium() {
        let conn = db::get_memory_connection().unwrap();
        let cls = classify(&conn, "LoRa 通信 山間部 到達距離");
        assert_eq!(cls.query_type, QueryType::Default);
        assert_eq!(cls.fts_weight, 1.0);
        assert_eq!(cls.vec_weight, 1.0);
    }

    #[test]
    fn test_classify_entity_focused() {
        let conn = db::get_memory_connection().unwrap();
        conn.execute(
            "INSERT INTO entities (name, entity_type) VALUES ('rust', 'tag')",
            [],
        )
        .unwrap();

        let cls = classify(&conn, "rust");
        assert_eq!(cls.query_type, QueryType::EntityFocused);
        assert_eq!(cls.fts_weight, 1.2);
        assert_eq!(cls.vec_weight, 0.8);
        assert!(!cls.matched_entity_ids.is_empty());
    }

    #[test]
    fn test_classify_entity_beats_question() {
        let conn = db::get_memory_connection().unwrap();
        conn.execute(
            "INSERT INTO entities (name, entity_type) VALUES ('rust', 'tag')",
            [],
        )
        .unwrap();

        // "rustとは" matches both entity AND question — entity should win
        let cls = classify(&conn, "rustとは");
        assert_eq!(cls.query_type, QueryType::EntityFocused);
    }

    #[test]
    fn test_classify_entity_no_tables() {
        let conn = db::get_memory_connection().unwrap();
        conn.execute_batch(
            "DROP TABLE IF EXISTS entity_edges; DROP TABLE IF EXISTS chunk_entities; DROP TABLE IF EXISTS entities;",
        ).unwrap();

        let cls = classify(&conn, "rust");
        assert_eq!(cls.query_type, QueryType::Keyword);
    }

    #[test]
    fn test_is_question() {
        assert!(is_question("FTS5とは"));
        assert!(is_question("どうやって実装する"));
        assert!(is_question("なぜRustを使うのか"));
        assert!(is_question("ベクトル検索の方法"));
        assert!(is_question("SQLiteについて教えて"));
        assert!(is_question("RustとGoの違い"));
        assert!(!is_question("rust sqlite"));
        assert!(!is_question("射撃ルール"));
        // Question marks
        assert!(is_question("猟に行った記録はある？"));
        assert!(is_question("検索できる?"));
        assert!(!is_question("探して"));
    }

    #[test]
    fn test_count_tokens() {
        assert_eq!(count_tokens("rust"), 1);
        assert_eq!(count_tokens("射撃 ルール"), 2);
        assert_eq!(count_tokens(""), 0);
    }

    #[test]
    fn test_weights_consistency() {
        let types = [
            QueryType::Keyword,
            QueryType::Question,
            QueryType::EntityFocused,
            QueryType::Fuzzy,
            QueryType::Default,
        ];
        for qt in types {
            let cls = QueryClassification::new(qt, Vec::new());
            assert!(cls.fts_weight >= 0.0);
            assert!(cls.vec_weight >= 0.0);
            assert!(
                cls.fts_weight + cls.vec_weight > 0.0,
                "at least one weight must be positive"
            );
        }
    }
}
