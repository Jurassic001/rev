//! Rate My Professors. The public search page embeds its results as a Relay
//! store (`window.__RELAY_STORE__ = {...}`), which we read instead of the
//! rendered HTML.

use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::names::{self, Person, Strength};
use crate::net;

/// RMP's id for Texas A&M University at College Station (base64 of "School-1003").
const TAMU_ID: &str = "U2Nob29sLTEwMDM=";
const TAMU_LEGACY_ID: u32 = 1003;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Teacher {
    pub legacy_id: u64,
    pub first: String,
    pub last: String,
    pub department: String,
    pub quality: f64,
    pub difficulty: f64,
    pub num_ratings: u32,
    pub would_take_again: Option<f64>,
}

impl Teacher {
    pub fn url(&self) -> String {
        format!(
            "https://www.ratemyprofessors.com/professor/{}",
            self.legacy_id
        )
    }
}

#[derive(Clone, Debug)]
pub struct Found {
    pub teacher: Teacher,
    pub strength: Strength,
}

fn search(client: &Client, query: &str, refresh: bool) -> Result<Vec<Teacher>> {
    let key = format!("rmp-{}.json", query.to_lowercase());
    let text = net::cached(&key, Duration::from_secs(3 * 24 * 3600), refresh, || {
        let resp = client
            .get(format!(
                "https://www.ratemyprofessors.com/search/professors/{TAMU_LEGACY_ID}"
            ))
            .query(&[("q", query)])
            .send()?;
        let teachers = teachers_from_page(&net::body(resp, "Rate My Professors")?)?;
        Ok(serde_json::to_string(&teachers)?)
    })?;
    Ok(serde_json::from_str(&text)?)
}

fn teachers_from_page(html: &str) -> Result<Vec<Teacher>> {
    let start = html
        .find("window.__RELAY_STORE__")
        .context("Rate My Professors page layout changed (no relay store)")?;
    let rest = &html[start..];
    let json = rest[rest.find('=').context("malformed relay store")? + 1..].trim_start();
    // Reads one JSON value and ignores the script that follows it.
    let store = Value::deserialize(&mut serde_json::Deserializer::from_str(json))?;
    let Value::Object(nodes) = store else {
        return Ok(Vec::new());
    };
    Ok(nodes
        .values()
        .filter(|n| n["__typename"] == "Teacher" && n["school"]["__ref"] == TAMU_ID)
        .filter_map(|n| {
            Some(Teacher {
                legacy_id: n["legacyId"].as_u64()?,
                first: n["firstName"].as_str()?.trim().to_string(),
                last: n["lastName"].as_str()?.trim().to_string(),
                department: n["department"].as_str().unwrap_or("").to_string(),
                quality: n["avgRating"].as_f64().unwrap_or(0.0),
                difficulty: n["avgDifficulty"].as_f64().unwrap_or(0.0),
                num_ratings: n["numRatings"].as_u64().unwrap_or(0) as u32,
                // RMP reports -1 when nobody answered the question
                would_take_again: n["wouldTakeAgainPercent"].as_f64().filter(|p| *p >= 0.0),
            })
        })
        .collect())
}

/// Words two department names share, ignoring filler.
fn dept_overlap(a: &str, b: &str) -> usize {
    const FILLER: &[&str] = &["AND", "OF", "THE", "FOR", "IN"];
    let a = names::tokens(a);
    names::tokens(b)
        .iter()
        .filter(|w| !FILLER.contains(&w.as_str()) && a.contains(w))
        .count()
}

/// Best TAMU match for `person`, trying "First Last" first and then the last
/// name alone (RMP's search is picky about middle names and nicknames).
pub fn lookup(
    client: &Client,
    person: &Person,
    dept_hint: &str,
    refresh: bool,
) -> Result<Option<Found>> {
    let mut best: Option<(Strength, usize, u32, Teacher)> = None;
    for query in person.search_queries() {
        for t in search(client, &query, refresh)? {
            let Some(strength) = names::rmp_match(person, &t.first, &t.last) else {
                continue;
            };
            let score = (
                strength,
                dept_overlap(dept_hint, &t.department),
                t.num_ratings,
            );
            if best.as_ref().is_none_or(|b| score > (b.0, b.1, b.2)) {
                best = Some((score.0, score.1, score.2, t));
            }
        }
        if best.as_ref().is_some_and(|b| b.0 == Strength::Exact) {
            break;
        }
    }
    Ok(best.map(|(strength, _, _, teacher)| Found { teacher, strength }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_relay_store() {
        let html = r#"<script>window.__RELAY_STORE__ = {"VGVhY2hlci0yMzAwMTAw":{"__typename":"Teacher","legacyId":2300100,"avgRating":2.6,"numRatings":103,"wouldTakeAgainPercent":48.0769,"avgDifficulty":2.7,"department":"Computer Science","school":{"__ref":"U2Nob29sLTEwMDM="},"firstName":"Shawn","lastName":"Lupoli"},"other":{"__typename":"Teacher","legacyId":5,"firstName":"A","lastName":"B","school":{"__ref":"elsewhere"},"wouldTakeAgainPercent":-1}};
        window.__APP_INITIAL_STATE__ = {};</script>"#;
        let t = teachers_from_page(html).unwrap();
        assert_eq!(t.len(), 1);
        assert_eq!(
            t[0].url(),
            "https://www.ratemyprofessors.com/professor/2300100"
        );
        assert_eq!(t[0].num_ratings, 103);
        assert!(dept_overlap("Computer Science & Engineering", &t[0].department) == 2);
    }
}
