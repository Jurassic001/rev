//! Terms and sections, independent of where the roster comes from.

use std::time::{SystemTime, UNIX_EPOCH};

const CAMPUSES: &[&str] = &["College Station", "Galveston", "Qatar", "McAllen"];

#[derive(Clone, Debug)]
pub struct Term {
    pub code: String,
    pub desc: String,
}

impl Term {
    /// "Fall 2026 - College Station" -> College Station. Terms that aren't tied
    /// to one campus ("Full Yr Professional 2026-2027") give `None`.
    pub fn campus(&self) -> Option<&'static str> {
        let (_, tail) = self.desc.rsplit_once(" - ")?;
        CAMPUSES
            .iter()
            .find(|c| c.eq_ignore_ascii_case(tail.trim()))
            .copied()
    }
}

#[derive(Clone, Debug)]
pub struct Instructor {
    pub name: String,
    pub primary: bool,
}

#[derive(Clone, Debug)]
pub struct Section {
    pub section: String,
    pub crn: String,
    pub title: String,
    pub subject_desc: String,
    pub campus: String,
    /// "online" / "hybrid"; empty for ordinary in-person sections.
    pub delivery: String,
    pub meetings: String,
    pub open: Option<bool>,
    pub honors: bool,
    pub instructors: Vec<Instructor>,
}

impl Section {
    /// Sections with no campus listed are kept rather than guessed at.
    pub fn on_campus(&self, campus: Option<&str>) -> bool {
        match campus {
            Some(c) => self.campus.is_empty() || self.campus.eq_ignore_ascii_case(c),
            None => true,
        }
    }
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

/// College Station terms around today, for when the term list can't be had.
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

    fn term(code: &str, desc: &str) -> Term {
        Term {
            code: code.into(),
            desc: desc.into(),
        }
    }

    #[test]
    fn resolves_terms() {
        let terms = vec![
            term("202632", "Fall 2026 - Galveston"),
            term("202631", "Fall 2026 - College Station"),
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

    #[test]
    fn campuses() {
        assert_eq!(
            term("202711", "Spring 2027 - College Station").campus(),
            Some("College Station")
        );
        assert_eq!(
            term("202712", "Spring 2027 - Galveston").campus(),
            Some("Galveston")
        );
        assert_eq!(
            term("202715", "Spring 2027 - Half Year Term").campus(),
            None
        );
        assert_eq!(
            term("202641", "Full Yr Professional 2026-2027").campus(),
            None
        );
    }
}
