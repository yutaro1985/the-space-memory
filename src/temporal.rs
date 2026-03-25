use chrono::{Datelike, Months, NaiveDate, Utc};

use crate::config;
use crate::tokenizer;

/// Time range filter for search queries.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TimeFilter {
    /// Inclusive lower bound (YYYY-MM-DD format).
    pub after: Option<String>,
    /// Exclusive upper bound (YYYY-MM-DD format).
    pub before: Option<String>,
}

/// A query with temporal expressions extracted.
#[derive(Debug, Clone)]
pub struct ParsedQuery {
    /// The query string with temporal expressions removed.
    pub query: String,
    /// Extracted time filter, if any.
    pub filter: Option<TimeFilter>,
}

/// Parse temporal expressions from a query string using morphological analysis.
pub fn parse_temporal(query: &str) -> ParsedQuery {
    let today = Utc::now().date_naive();
    parse_temporal_with_date(query, today)
}

/// Parse temporal expressions using a specific reference date (for testing).
pub fn parse_temporal_with_date(query: &str, today: NaiveDate) -> ParsedQuery {
    let tokens = tokenizer::tokenize(query);
    if tokens.is_empty() {
        return ParsedQuery {
            query: query.to_string(),
            filter: None,
        };
    }

    // Try matching 2-token and 1-token temporal patterns
    let mut filter = TimeFilter::default();
    let mut matched_range: Option<(usize, usize)> = None; // byte range to remove

    // 2-token patterns: "半年前", "N年前", "N週間前", "N日前", "少し前"
    for i in 0..tokens.len().saturating_sub(1) {
        let t1 = &tokens[i];
        let t2 = &tokens[i + 1];
        let combined = format!("{}{}", t1.surface, t2.surface);

        if let Some(f) = match_two_token(t1, t2, today) {
            filter = f;
            matched_range = Some((t1.byte_start, t2.byte_end));
            break;
        }
        // Also try combined surface as single lookup
        if let Some(f) = match_temporal_word(&combined, today) {
            filter = f;
            matched_range = Some((t1.byte_start, t2.byte_end));
            break;
        }
    }

    // 1-token patterns if no 2-token match
    if matched_range.is_none() {
        for t in &tokens {
            if let Some(f) = match_temporal_word(&t.surface, today) {
                filter = f;
                matched_range = Some((t.byte_start, t.byte_end));
                break;
            }
        }
    }

    // "N月の/に" pattern: number token followed by "月"
    if matched_range.is_none() {
        for i in 0..tokens.len().saturating_sub(1) {
            let t1 = &tokens[i];
            let t2 = &tokens[i + 1];
            if t2.surface == "月" {
                if let Ok(month) = t1.surface.parse::<u32>() {
                    if (1..=12).contains(&month) {
                        let year = if month <= today.month() {
                            today.year()
                        } else {
                            today.year() - 1
                        };
                        let start =
                            NaiveDate::from_ymd_opt(year, month, 1).expect("valid month 1-12");
                        let end = start + Months::new(1);
                        filter.after = Some(start.format("%Y-%m-%d").to_string());
                        filter.before = Some(end.format("%Y-%m-%d").to_string());
                        // Include trailing particle (の/に) if present
                        let byte_end = if i + 2 < tokens.len()
                            && (tokens[i + 2].surface == "の" || tokens[i + 2].surface == "に")
                        {
                            tokens[i + 2].byte_end
                        } else {
                            t2.byte_end
                        };
                        matched_range = Some((t1.byte_start, byte_end));
                        break;
                    }
                }
            }
        }
    }

    let (cleaned_query, has_match) = match matched_range {
        Some((start, end)) => {
            let q = format!("{}{}", &query[..start], &query[end..]);
            (q.trim().to_string(), true)
        }
        None => (query.trim().to_string(), false),
    };

    ParsedQuery {
        query: cleaned_query,
        filter: if has_match { Some(filter) } else { None },
    }
}

/// Match a single temporal word.
fn match_temporal_word(word: &str, today: NaiveDate) -> Option<TimeFilter> {
    match word {
        "先月" => {
            let (after, before) = month_range(today, -1);
            Some(TimeFilter {
                after: Some(after),
                before: Some(before),
            })
        }
        "去年" | "昨年" => {
            let year = today.year() - 1;
            Some(TimeFilter {
                after: Some(format!("{year}-01-01")),
                before: Some(format!("{}-01-01", year + 1)),
            })
        }
        "一昨年" | "おととし" => {
            let year = today.year() - 2;
            Some(TimeFilter {
                after: Some(format!("{year}-01-01")),
                before: Some(format!("{}-01-01", year + 1)),
            })
        }
        "今月" => Some(TimeFilter {
            after: Some(format!("{}-{:02}-01", today.year(), today.month())),
            before: None,
        }),
        "今年" => Some(TimeFilter {
            after: Some(format!("{}-01-01", today.year())),
            before: None,
        }),
        "最近" | "少し前" => {
            let after_date = today - chrono::Duration::days(config::RECENT_DAYS);
            Some(TimeFilter {
                after: Some(after_date.format("%Y-%m-%d").to_string()),
                before: None,
            })
        }
        "先週" => {
            let after_date = today - chrono::Duration::days(7);
            Some(TimeFilter {
                after: Some(after_date.format("%Y-%m-%d").to_string()),
                before: None,
            })
        }
        "年末" => {
            let year = if today.month() <= 2 {
                today.year() - 1
            } else {
                today.year()
            };
            Some(TimeFilter {
                after: Some(format!("{year}-11-01")),
                before: Some(format!("{}-01-01", year + 1)),
            })
        }
        "年始" | "年初" => {
            let year = if today.month() >= 3 {
                today.year()
            } else {
                today.year() - 1
            };
            Some(TimeFilter {
                after: Some(format!("{year}-01-01")),
                before: Some(format!("{year}-03-01")),
            })
        }
        "半年前" => {
            let after_date = today - chrono::Duration::days(180);
            Some(TimeFilter {
                after: Some(after_date.format("%Y-%m-%d").to_string()),
                before: None,
            })
        }
        _ => None,
    }
}

/// Match two consecutive tokens as a temporal pattern.
fn match_two_token(
    t1: &tokenizer::Token,
    t2: &tokenizer::Token,
    today: NaiveDate,
) -> Option<TimeFilter> {
    // "N年前", "N週間前", "N日前"
    if t2.surface == "前" {
        if let Some(days) = parse_relative_duration(&t1.surface, &t2.surface) {
            let after_date = today - chrono::Duration::days(days);
            return Some(TimeFilter {
                after: Some(after_date.format("%Y-%m-%d").to_string()),
                before: None,
            });
        }
    }
    None
}

/// Parse "N年", "N週間", "N日" + "前" into days.
fn parse_relative_duration(num_part: &str, suffix: &str) -> Option<i64> {
    // Try extracting number from patterns like "3年", "2週間", "5日"
    let _ = suffix; // "前" is already checked by caller

    if let Some(n) = num_part
        .strip_suffix("年")
        .and_then(|s| s.parse::<i64>().ok())
    {
        return Some(n * 365);
    }
    if let Some(n) = num_part
        .strip_suffix("週間")
        .and_then(|s| s.parse::<i64>().ok())
    {
        return Some(n * 7);
    }
    if let Some(n) = num_part
        .strip_suffix("週")
        .and_then(|s| s.parse::<i64>().ok())
    {
        return Some(n * 7);
    }
    if let Some(n) = num_part
        .strip_suffix("日")
        .and_then(|s| s.parse::<i64>().ok())
    {
        return Some(n);
    }
    if let Some(n) = num_part
        .strip_suffix("ヶ月")
        .and_then(|s| s.parse::<i64>().ok())
    {
        return Some(n * 30);
    }
    if let Some(n) = num_part
        .strip_suffix("か月")
        .and_then(|s| s.parse::<i64>().ok())
    {
        return Some(n * 30);
    }
    None
}

/// Compute the (after, before) date range for a month offset from today.
fn month_range(today: NaiveDate, offset_months: i32) -> (String, String) {
    let first_of_month = NaiveDate::from_ymd_opt(today.year(), today.month(), 1).unwrap();
    let start = if offset_months < 0 {
        first_of_month - Months::new((-offset_months) as u32)
    } else {
        first_of_month + Months::new(offset_months as u32)
    };
    let end = start + Months::new(1);
    (
        start.format("%Y-%m-%d").to_string(),
        end.format("%Y-%m-%d").to_string(),
    )
}

/// Merge CLI-provided time filters with query-extracted filters.
/// CLI arguments take precedence.
pub fn merge_filters(
    cli_after: Option<&str>,
    cli_before: Option<&str>,
    cli_recent: Option<&str>,
    cli_year: Option<i32>,
    query_filter: Option<TimeFilter>,
) -> anyhow::Result<Option<TimeFilter>> {
    let today = Utc::now().date_naive();

    if cli_after.is_some() || cli_before.is_some() || cli_recent.is_some() || cli_year.is_some() {
        let mut filter = TimeFilter::default();

        if let Some(year) = cli_year {
            filter.after = Some(format!("{year}-01-01"));
            filter.before = Some(format!("{}-01-01", year + 1));
        }

        if let Some(recent) = cli_recent {
            let days = parse_duration(recent).ok_or_else(|| {
                anyhow::anyhow!("Invalid duration: '{recent}'. Use e.g. 30d, 2w, 3m")
            })?;
            let after_date = today - chrono::Duration::days(days);
            filter.after = Some(after_date.format("%Y-%m-%d").to_string());
        }

        if let Some(after) = cli_after {
            filter.after = Some(normalize_date(after)?);
        }
        if let Some(before) = cli_before {
            filter.before = Some(normalize_date(before)?);
        }

        if filter.after.is_some() || filter.before.is_some() {
            return Ok(Some(filter));
        }
    }

    Ok(query_filter)
}

/// Parse a duration string like "30d", "7d", "2w", "3m".
fn parse_duration(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    let (num_str, unit) = s.split_at(s.len() - 1);
    let num: i64 = num_str.parse().ok()?;
    match unit {
        "d" => Some(num),
        "w" => Some(num * 7),
        "m" => Some(num * 30),
        _ => s.parse::<i64>().ok(),
    }
}

/// Normalize a date input to YYYY-MM-DD format.
/// Accepts "YYYY-MM-DD", "YYYY-MM", "YYYY".
fn normalize_date(s: &str) -> anyhow::Result<String> {
    let normalized = match s.len() {
        4 => format!("{s}-01-01"),
        7 => format!("{s}-01"),
        _ => s.to_string(),
    };
    NaiveDate::parse_from_str(&normalized, "%Y-%m-%d")
        .map_err(|_| anyhow::anyhow!("Invalid date: '{s}'. Use YYYY, YYYY-MM, or YYYY-MM-DD."))?;
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    // ─── parse_temporal tests (existing + new) ────────────────

    #[test]
    fn test_no_temporal() {
        let result = parse_temporal_with_date("射撃 ルール", date(2026, 3, 24));
        assert_eq!(result.query, "射撃 ルール");
        assert!(result.filter.is_none());
    }

    #[test]
    fn test_last_month() {
        let result = parse_temporal_with_date("先月の調査", date(2026, 3, 24));
        assert!(result.filter.is_some());
        let f = result.filter.unwrap();
        assert_eq!(f.after.as_deref(), Some("2026-02-01"));
        assert_eq!(f.before.as_deref(), Some("2026-03-01"));
    }

    #[test]
    fn test_last_month_january() {
        let result = parse_temporal_with_date("先月のメモ", date(2026, 1, 15));
        let f = result.filter.unwrap();
        assert_eq!(f.after.as_deref(), Some("2025-12-01"));
        assert_eq!(f.before.as_deref(), Some("2026-01-01"));
    }

    #[test]
    fn test_last_year() {
        let result = parse_temporal_with_date("去年調べた射撃", date(2026, 3, 24));
        let f = result.filter.unwrap();
        assert_eq!(f.after.as_deref(), Some("2025-01-01"));
        assert_eq!(f.before.as_deref(), Some("2026-01-01"));
    }

    #[test]
    fn test_last_year_alt() {
        let result = parse_temporal_with_date("昨年のメモ", date(2026, 3, 24));
        let f = result.filter.unwrap();
        assert_eq!(f.after.as_deref(), Some("2025-01-01"));
    }

    #[test]
    fn test_this_month() {
        let result = parse_temporal_with_date("今月のメモ", date(2026, 3, 24));
        let f = result.filter.unwrap();
        assert_eq!(f.after.as_deref(), Some("2026-03-01"));
        assert!(f.before.is_none());
    }

    #[test]
    fn test_this_year() {
        let result = parse_temporal_with_date("今年の調査", date(2026, 3, 24));
        let f = result.filter.unwrap();
        assert_eq!(f.after.as_deref(), Some("2026-01-01"));
        assert!(f.before.is_none());
    }

    #[test]
    fn test_recent() {
        let result = parse_temporal_with_date("最近のメモ", date(2026, 3, 24));
        let f = result.filter.unwrap();
        assert_eq!(f.after.as_deref(), Some("2026-02-22"));
        assert!(f.before.is_none());
    }

    #[test]
    fn test_specific_month_past() {
        let result = parse_temporal_with_date("3月に書いた射撃", date(2026, 5, 1));
        let f = result.filter.unwrap();
        assert_eq!(f.after.as_deref(), Some("2026-03-01"));
        assert_eq!(f.before.as_deref(), Some("2026-04-01"));
    }

    #[test]
    fn test_specific_month_future() {
        let result = parse_temporal_with_date("3月に書いたメモ", date(2026, 1, 15));
        let f = result.filter.unwrap();
        assert_eq!(f.after.as_deref(), Some("2025-03-01"));
        assert_eq!(f.before.as_deref(), Some("2025-04-01"));
    }

    #[test]
    fn test_specific_month_december() {
        let result = parse_temporal_with_date("12月のメモ", date(2026, 3, 24));
        let f = result.filter.unwrap();
        assert_eq!(f.after.as_deref(), Some("2025-12-01"));
        assert_eq!(f.before.as_deref(), Some("2026-01-01"));
    }

    #[test]
    fn test_empty_query_after_temporal() {
        let result = parse_temporal_with_date("先月の", date(2026, 3, 24));
        assert!(result.filter.is_some());
    }

    // ─── NEW temporal expressions ─────────────────────────────

    #[test]
    fn test_last_week() {
        let result = parse_temporal_with_date("先週のメモ", date(2026, 3, 24));
        let f = result.filter.unwrap();
        assert_eq!(f.after.as_deref(), Some("2026-03-17"));
        assert!(f.before.is_none());
    }

    #[test]
    fn test_year_end() {
        let result = parse_temporal_with_date("年末のメモ", date(2026, 3, 24));
        let f = result.filter.unwrap();
        assert_eq!(f.after.as_deref(), Some("2026-11-01"));
        assert_eq!(f.before.as_deref(), Some("2027-01-01"));
    }

    #[test]
    fn test_year_end_in_january() {
        // In January, "年末" refers to last year's year-end
        let result = parse_temporal_with_date("年末のメモ", date(2026, 1, 15));
        let f = result.filter.unwrap();
        assert_eq!(f.after.as_deref(), Some("2025-11-01"));
        assert_eq!(f.before.as_deref(), Some("2026-01-01"));
    }

    #[test]
    fn test_year_start() {
        let result = parse_temporal_with_date("年始のメモ", date(2026, 5, 1));
        let f = result.filter.unwrap();
        assert_eq!(f.after.as_deref(), Some("2026-01-01"));
        assert_eq!(f.before.as_deref(), Some("2026-03-01"));
    }

    #[test]
    fn test_year_before_last() {
        let result = parse_temporal_with_date("一昨年のメモ", date(2026, 3, 24));
        let f = result.filter.unwrap();
        assert_eq!(f.after.as_deref(), Some("2024-01-01"));
        assert_eq!(f.before.as_deref(), Some("2025-01-01"));
    }

    #[test]
    fn test_half_year_ago() {
        let result = parse_temporal_with_date("半年前の調査", date(2026, 3, 24));
        let f = result.filter.unwrap();
        assert_eq!(f.after.as_deref(), Some("2025-09-25"));
        assert!(f.before.is_none());
    }

    #[test]
    fn test_sukoshi_mae() {
        let result = parse_temporal_with_date("少し前のメモ", date(2026, 3, 24));
        let f = result.filter.unwrap();
        assert_eq!(f.after.as_deref(), Some("2026-02-22"));
    }

    // ─── merge_filters tests ──────────────────────────────────

    #[test]
    fn test_merge_cli_after_takes_precedence() {
        let query_filter = Some(TimeFilter {
            after: Some("2025-01-01".to_string()),
            before: None,
        });
        let result = merge_filters(Some("2026-01-01"), None, None, None, query_filter).unwrap();
        let f = result.unwrap();
        assert_eq!(f.after.as_deref(), Some("2026-01-01"));
    }

    #[test]
    fn test_merge_cli_year() {
        let result = merge_filters(None, None, None, Some(2025), None).unwrap();
        let f = result.unwrap();
        assert_eq!(f.after.as_deref(), Some("2025-01-01"));
        assert_eq!(f.before.as_deref(), Some("2026-01-01"));
    }

    #[test]
    fn test_merge_cli_recent() {
        let result = merge_filters(None, None, Some("7d"), None, None).unwrap();
        let f = result.unwrap();
        assert!(f.after.is_some());
        assert!(f.before.is_none());
    }

    #[test]
    fn test_merge_no_filters() {
        let result = merge_filters(None, None, None, None, None).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_merge_query_filter_used_when_no_cli() {
        let query_filter = Some(TimeFilter {
            after: Some("2025-06-01".to_string()),
            before: Some("2025-07-01".to_string()),
        });
        let result = merge_filters(None, None, None, None, query_filter).unwrap();
        let f = result.unwrap();
        assert_eq!(f.after.as_deref(), Some("2025-06-01"));
        assert_eq!(f.before.as_deref(), Some("2025-07-01"));
    }

    #[test]
    fn test_merge_cli_after_and_before() {
        let result =
            merge_filters(Some("2025-01-01"), Some("2025-06-01"), None, None, None).unwrap();
        let f = result.unwrap();
        assert_eq!(f.after.as_deref(), Some("2025-01-01"));
        assert_eq!(f.before.as_deref(), Some("2025-06-01"));
    }

    #[test]
    fn test_merge_invalid_recent_returns_error() {
        let result = merge_filters(None, None, Some("xyz"), None, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_merge_invalid_after_returns_error() {
        let result = merge_filters(Some("garbage"), None, None, None, None);
        assert!(result.is_err());
    }

    // ─── edge cases ──────────────────────────────────────────

    #[test]
    fn test_invalid_month_not_stripped() {
        let result = parse_temporal_with_date("13月のメモ", date(2026, 3, 24));
        assert!(result.filter.is_none());
    }

    // ─── helper tests ────────────────────────────────────────

    #[test]
    fn test_parse_duration_days() {
        assert_eq!(parse_duration("30d"), Some(30));
        assert_eq!(parse_duration("7d"), Some(7));
    }

    #[test]
    fn test_parse_duration_weeks() {
        assert_eq!(parse_duration("2w"), Some(14));
    }

    #[test]
    fn test_parse_duration_months() {
        assert_eq!(parse_duration("3m"), Some(90));
    }

    #[test]
    fn test_parse_duration_invalid() {
        assert_eq!(parse_duration("xyz"), None);
        assert_eq!(parse_duration(""), None);
    }

    #[test]
    fn test_normalize_date_year() {
        assert_eq!(normalize_date("2025").unwrap(), "2025-01-01");
    }

    #[test]
    fn test_normalize_date_year_month() {
        assert_eq!(normalize_date("2025-03").unwrap(), "2025-03-01");
    }

    #[test]
    fn test_normalize_date_full() {
        assert_eq!(normalize_date("2025-03-15").unwrap(), "2025-03-15");
    }

    #[test]
    fn test_normalize_date_invalid() {
        assert!(normalize_date("abcd").is_err());
        assert!(normalize_date("not-a-date").is_err());
    }

    #[test]
    fn test_month_range_normal() {
        let (after, before) = month_range(date(2026, 3, 24), -1);
        assert_eq!(after, "2026-02-01");
        assert_eq!(before, "2026-03-01");
    }

    #[test]
    fn test_month_range_year_boundary() {
        let (after, before) = month_range(date(2026, 1, 15), -1);
        assert_eq!(after, "2025-12-01");
        assert_eq!(before, "2026-01-01");
    }
}
