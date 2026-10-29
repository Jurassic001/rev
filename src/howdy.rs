//! TAMU's public class search (howdy.tamu.edu "Class Search"). The page loads
//! every section of a term from one JSON endpoint and filters client-side, so we
//! do the same and cache the term on disk.
//!
//! Field names follow the Banner `SWV_CLASS_SEARCH_*` view the page uses. The
//! parser looks fields up loosely so a renamed or missing column degrades one
//! value instead of failing the whole lookup.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use reqwest::blocking::Client;
use serde_json::{Value, json};

use crate::net;

const BASE: &str = "https://howdy.tamu.edu/api";

#[derive(Clone, Debug)]
pub struct Term {
    pub code: String,
    pub desc: String,
}

#[derive(Clone, Debug)]
pub struct Instructor {
    pub name: String,
    pub primary: bool,
}

#[derive(Clone, Debug)]
pub struct Section {
    pub subject: String,
    pub course: String,
    pub section: String,
    pub crn: String,
    pub title: String,
    pub subject_desc: String,
    pub schedule: String,
    pub meetings: String,
    pub open: Option<bool>,
    pub honors: bool,
    pub instructors: Vec<Instructor>,
}

/// First non-empty field among `keys`, stringified.
fn field(v: &Value, keys: &[&str]) -> String {
    for k in keys {
        match v.get(*k) {
            Some(Value::String(s)) if !s.trim().is_empty() => return s.trim().to_string(),
            Some(Value::Number(n)) => return n.to_string(),
            _ => {}
        }
    }
    String::new()
}

/// Some columns hold JSON encoded as a string; accept either form.
fn nested(v: &Value, key: &str) -> Vec<Value> {
    match v.get(key) {
        Some(Value::String(s)) => serde_json::from_str::<Vec<Value>>(s).unwrap_or_default(),
        Some(Value::Array(a)) => a.clone(),
        _ => Vec::new(),
    }
}

pub fn fetch_terms(client: &Client, refresh: bool) -> Result<Vec<Term>> {
    let text = net::cached(
        "howdy-terms.json",
        Duration::from_secs(24 * 3600),
        refresh,
        || net::body(client.get(format!("{BASE}/all-terms")).send()?, "Howdy"),
    )?;
    let raw: Vec<Value> = serde_json::from_str(&text).context("Howdy terms list was not JSON")?;
    let mut terms: Vec<Term> = raw
        .iter()
        .map(|v| Term {
            code: field(v, &["STVTERM_CODE", "termCode", "code"]),
            desc: field(v, &["STVTERM_DESC", "termDesc", "description"]),
        })
        .filter(|t| !t.code.is_empty())
        .collect();
    terms.sort_by(|a, b| b.code.cmp(&a.code));
    Ok(terms)
}

/// Every section offered in `term`.
pub fn fetch_term(client: &Client, term: &str, refresh: bool) -> Result<Vec<Section>> {
    let key = format!("howdy-sections-{term}.json");
    let text = net::cached(&key, Duration::from_secs(6 * 3600), refresh, || {
        let resp = client
            .post(format!("{BASE}/course-sections"))
            .json(&json!({ "startRow": 0, "endRow": 0, "termCode": term, "publicSearch": "Y" }))
            .send()?;
        net::body(resp, "Howdy")
    })?;
    let raw: Vec<Value> = serde_json::from_str(&text).context("Howdy section list was not JSON")?;
    Ok(raw.iter().map(parse_section).collect())
}

fn parse_section(v: &Value) -> Section {
    let section = field(v, &["SWV_CLASS_SEARCH_SECTION"]);
    let title = field(v, &["SWV_CLASS_SEARCH_TITLE"]);
    let attrs = field(
        v,
        &["SWV_CLASS_SEARCH_ATTRIBUTES", "SWV_CLASS_SEARCH_ATTRIBUTE"],
    );
    let honors = crate::anex::is_honors_section(&section)
        || title.to_uppercase().contains("HONORS")
        || attrs.to_uppercase().contains("HONOR");
    let open = match field(v, &["STUSEAT_OPEN"]).as_str() {
        "Y" => Some(true),
        "N" => Some(false),
        _ => None,
    };
    Section {
        subject: field(v, &["SWV_CLASS_SEARCH_SUBJECT"]),
        course: field(v, &["SWV_CLASS_SEARCH_COURSE"]),
        crn: field(v, &["SWV_CLASS_SEARCH_CRN"]),
        subject_desc: field(v, &["SWV_CLASS_SEARCH_SUBJECT_DESC"]),
        schedule: field(v, &["SWV_CLASS_SEARCH_SCHD"]),
        meetings: meetings(v),
        instructors: instructors(v),
        section,
        title,
        open,
        honors,
    }
}

/// `[{"NAME":"Shawn Lupoli (P)", ...}]` -> instructors, primary first.
fn instructors(v: &Value) -> Vec<Instructor> {
    let mut out: Vec<Instructor> = nested(v, "SWV_CLASS_SEARCH_INSTRCTR_JSON")
        .iter()
        .filter_map(|i| {
            let raw = field(i, &["NAME", "name"]);
            let (name, primary) = match raw.rfind('(') {
                Some(p) if raw.ends_with(')') => {
                    (raw[..p].trim().to_string(), raw[p..].contains('P'))
                }
                _ => (raw.clone(), false),
            };
            let upper = name.to_uppercase();
            let placeholder = upper.is_empty()
                || upper.contains("NOT ASSIGNED")
                || upper == "TBA"
                || upper == "STAFF";
            (!placeholder).then_some(Instructor { name, primary })
        })
        .collect();
    out.sort_by_key(|i| !i.primary);
    out
}

/// "MWF 09:10-10:00" for each regular meeting (final-exam slots skipped).
fn meetings(v: &Value) -> String {
    const DAYS: [(&str, &str); 7] = [
        ("SSRMEET_MON_DAY", "M"),
        ("SSRMEET_TUE_DAY", "T"),
        ("SSRMEET_WED_DAY", "W"),
        ("SSRMEET_THU_DAY", "R"),
        ("SSRMEET_FRI_DAY", "F"),
        ("SSRMEET_SAT_DAY", "S"),
        ("SSRMEET_SUN_DAY", "U"),
    ];
    let hhmm = |s: String| match s.len() {
        4 => format!("{}:{}", &s[..2], &s[2..]),
        _ => s,
    };
    nested(v, "SWV_CLASS_SEARCH_JSON_CLOB")
        .iter()
        .filter(|m| {
            !field(m, &["SSRMEET_MTYP_CODE"])
                .to_uppercase()
                .starts_with("EXAM")
        })
        .filter_map(|m| {
            let days: String = DAYS
                .iter()
                .filter(|(k, _)| !field(m, &[k]).is_empty())
                .map(|(_, d)| *d)
                .collect();
            let begin = hhmm(field(m, &["SSRMEET_BEGIN_TIME"]));
            let end = hhmm(field(m, &["SSRMEET_END_TIME"]));
            match (days.is_empty(), begin.is_empty()) {
                (true, true) => None,
                (false, true) => Some(days),
                _ => Some(format!("{days} {begin}-{end}").trim().to_string()),
            }
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// Banner term code: year, season digit (1 spring, 2 summer, 3 fall), campus
/// digit (1 College Station).
pub fn term_code(year: i32, season: u8) -> String {
    format!("{year}{season}1")
}

fn season_name(season: u8) -> &'static str {
    match season {
        1 => "Spring",
        2 => "Summer",
        _ => "Fall",
    }
}

/// College Station terms around today, for when Howdy's term list can't be had.
pub fn guess_terms() -> Vec<Term> {
    let (year, _) = today();
    let mut terms = Vec::new();
    for y in (year - 2..=year + 1).rev() {
        for s in (1..=3).rev() {
            terms.push(Term {
                code: term_code(y, s),
                desc: format!("{} {y} - College Station", season_name(s)),
            });
        }
    }
    terms
}

/// The term students are most likely planning for right now.
pub fn likely_term_code() -> String {
    let (year, month) = today();
    match month {
        1..=3 => term_code(year, 1),
        4..=9 => term_code(year, 3),
        _ => term_code(year + 1, 1),
    }
}

/// Resolve user input like "fall 2026", "Spring 2027 Galveston", or "202631".
pub fn resolve_term(query: &str, terms: &[Term]) -> Option<Term> {
    let q = query.trim().to_lowercase();
    if let Some(t) = terms.iter().find(|t| t.code == q) {
        return Some(t.clone());
    }
    let words: Vec<&str> = q.split_whitespace().collect();
    let hits: Vec<&Term> = terms
        .iter()
        .filter(|t| {
            let d = t.desc.to_lowercase();
            words.iter().all(|w| d.contains(w))
        })
        .collect();
    if let Some(t) = hits
        .iter()
        .find(|t| t.desc.to_lowercase().contains("college station"))
        .or(hits.first())
    {
        return Some((*t).clone());
    }
    // Not in the list (or no list at all): build it from "<season> <year>".
    let season = words.iter().find_map(|w| match *w {
        "spring" => Some(1),
        "summer" => Some(2),
        "fall" => Some(3),
        _ => None,
    })?;
    let year: i32 = words.iter().find_map(|w| w.parse().ok())?;
    Some(Term {
        code: term_code(year, season),
        desc: format!("{} {year} - College Station", season_name(season)),
    })
}

/// (year, month) in UTC, without pulling in a date crate.
fn today() -> (i32, u32) {
    let days = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() / 86400) as i64;
    // Howard Hinnant's civil_from_days
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = (yoe + era * 400 + i64::from(month <= 2)) as i32;
    (year, month)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_section() {
        let v = json!({
            "SWV_CLASS_SEARCH_SUBJECT": "CSCE",
            "SWV_CLASS_SEARCH_COURSE": "121",
            "SWV_CLASS_SEARCH_SECTION": "501",
            "SWV_CLASS_SEARCH_CRN": 12345,
            "SWV_CLASS_SEARCH_TITLE": "INTRO PGM DESIGN CONCEPT",
            "SWV_CLASS_SEARCH_INSTRCTR_JSON": "[{\"NAME\":\"Jane Doe\"},{\"NAME\":\"Shawn Lupoli (P)\",\"MORE\":1}]",
            "SWV_CLASS_SEARCH_JSON_CLOB": "[{\"SSRMEET_MON_DAY\":\"M\",\"SSRMEET_WED_DAY\":\"W\",\"SSRMEET_FRI_DAY\":\"F\",\"SSRMEET_BEGIN_TIME\":\"0910\",\"SSRMEET_END_TIME\":\"1000\",\"SSRMEET_MTYP_CODE\":\"Lecture\"},{\"SSRMEET_TUE_DAY\":\"T\",\"SSRMEET_BEGIN_TIME\":\"1030\",\"SSRMEET_END_TIME\":\"1230\",\"SSRMEET_MTYP_CODE\":\"Examination\"}]",
            "STUSEAT_OPEN": "Y"
        });
        let s = parse_section(&v);
        assert_eq!(s.crn, "12345");
        assert_eq!(s.meetings, "MWF 09:10-10:00");
        assert_eq!(s.instructors[0].name, "Shawn Lupoli");
        assert!(s.instructors[0].primary);
        assert_eq!(s.instructors[1].name, "Jane Doe");
        assert!(!s.honors);
        assert_eq!(s.open, Some(true));

        let h = parse_section(
            &json!({ "SWV_CLASS_SEARCH_SECTION": "201", "SWV_CLASS_SEARCH_INSTRCTR_JSON": "[{\"NAME\":\"Not Assigned\"}]" }),
        );
        assert!(h.honors);
        assert!(h.instructors.is_empty());
    }

    #[test]
    fn resolves_terms() {
        let terms = vec![
            Term {
                code: "202632".into(),
                desc: "Fall 2026 - Galveston".into(),
            },
            Term {
                code: "202631".into(),
                desc: "Fall 2026 - College Station".into(),
            },
        ];
        assert_eq!(resolve_term("fall 2026", &terms).unwrap().code, "202631");
        assert_eq!(
            resolve_term("Fall 2026 galveston", &terms).unwrap().code,
            "202632"
        );
        assert_eq!(resolve_term("spring 2027", &terms).unwrap().code, "202711");
        assert_eq!(resolve_term("202632", &terms).unwrap().code, "202632");
        assert!(resolve_term("whenever", &terms).is_none());
    }
}
