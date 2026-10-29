//! AggieSB+ (aggieschedulebuilderplus.vercel.app), a student-run TAMU planner
//! with a public, documented API that mirrors the class schedule:
//! https://api-aggiesbp.nitheeshk.com/docs
//!
//! It is unofficial. `--noSB` skips it and lets the user type in the
//! professors they see in Schedule Builder instead.

use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::StatusCode;
use reqwest::blocking::Client;
use serde::Deserialize;

use crate::anex::is_honors_section;
use crate::net;
use crate::schedule::{Instructor, Section, Term};

const BASE: &str = "https://api-aggiesbp.nitheeshk.com";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawTerm {
    term_code: String,
    term_desc: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawSection {
    crn: String,
    dept_desc: Option<String>,
    section_number: String,
    course_title: Option<String>,
    campus: Option<String>,
    instruction_type: Option<String>,
    is_open: Option<bool>,
    attributes_text: Option<String>,
    #[serde(default)]
    instructors: Vec<RawInstructor>,
    #[serde(default)]
    meetings: Vec<RawMeeting>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawInstructor {
    name: String,
    #[serde(default)]
    is_primary: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawMeeting {
    #[serde(default)]
    days_of_week: Vec<String>,
    begin_time: Option<String>,
    end_time: Option<String>,
    meeting_type: Option<String>,
}

/// Current and upcoming terms, newest code first.
pub fn fetch_terms(client: &Client, refresh: bool) -> Result<Vec<Term>> {
    let text = net::cached(
        "sb-terms.json",
        Duration::from_secs(24 * 3600),
        refresh,
        || net::body(client.get(format!("{BASE}/terms")).send()?, "AggieSB+"),
    )?;
    let raw: Vec<RawTerm> =
        serde_json::from_str(&text).context("AggieSB+ terms were not in the expected format")?;
    let mut terms: Vec<Term> = raw
        .into_iter()
        .map(|t| Term {
            code: t.term_code,
            desc: t.term_desc,
        })
        .collect();
    terms.sort_by(|a, b| b.code.cmp(&a.code));
    Ok(terms)
}

/// Every section of one course in a term. A course that isn't offered comes
/// back empty rather than as an error.
pub fn fetch_course(
    client: &Client,
    term: &str,
    subject: &str,
    number: &str,
    refresh: bool,
) -> Result<Vec<Section>> {
    let code = format!("{subject}{number}");
    let key = format!("sb-{term}-{code}.json");
    let text = net::cached(&key, Duration::from_secs(6 * 3600), refresh, || {
        let resp = client
            .get(format!("{BASE}/sections/{term}/course/{code}"))
            .send()?;
        if resp.status() == StatusCode::NOT_FOUND {
            return Ok("[]".into());
        }
        net::body(resp, "AggieSB+")
    })?;
    let raw: Vec<RawSection> =
        serde_json::from_str(&text).context("AggieSB+ sections were not in the expected format")?;
    Ok(raw.into_iter().map(section).collect())
}

fn section(r: RawSection) -> Section {
    let attrs = r.attributes_text.unwrap_or_default();
    let mode = r.instruction_type.unwrap_or_default().to_lowercase();
    let mut instructors: Vec<Instructor> = r
        .instructors
        .into_iter()
        .filter(|i| !i.name.trim().is_empty())
        .map(|i| Instructor {
            name: i.name.trim().to_string(),
            primary: i.is_primary,
        })
        .collect();
    instructors.sort_by_key(|i| !i.primary);
    Section {
        honors: is_honors_section(&r.section_number) || attrs.to_lowercase().contains("honors"),
        crn: r.crn,
        title: r.course_title.unwrap_or_default(),
        // "CSCE - Computer Science & Engineering" -> "Computer Science & Engineering"
        subject_desc: r
            .dept_desc
            .map(|d| {
                d.split_once(" - ")
                    .map_or(d.clone(), |(_, rest)| rest.to_string())
            })
            .unwrap_or_default(),
        campus: r.campus.unwrap_or_default(),
        delivery: if mode.contains("web") {
            "online".into()
        } else if mode.contains("hybrid") || mode.contains("blended") {
            "hybrid".into()
        } else {
            String::new()
        },
        meetings: r
            .meetings
            .iter()
            .filter_map(meeting)
            .collect::<Vec<_>>()
            .join("; "),
        open: r.is_open,
        section: r.section_number,
        instructors,
    }
}

/// "09:10 AM" -> ("9:10", "a")
fn clock(t: &str) -> Option<(String, &'static str)> {
    let (hm, ampm) = t.trim().split_once(' ')?;
    let suffix = if ampm.eq_ignore_ascii_case("PM") {
        "p"
    } else {
        "a"
    };
    Some((hm.trim_start_matches('0').to_string(), suffix))
}

/// "MWF 9:10-10:00a", "TR 12:45-1:35p lab"; final-exam slots are skipped.
fn meeting(m: &RawMeeting) -> Option<String> {
    let kind = m.meeting_type.as_deref().unwrap_or("").to_lowercase();
    if kind.starts_with("exam") {
        return None;
    }
    let days = m.days_of_week.concat();
    let begin = m.begin_time.as_deref().and_then(clock);
    let end = m.end_time.as_deref().and_then(clock);
    let time = match (begin, end) {
        (Some((b, bs)), Some((e, es))) if bs == es => format!("{b}-{e}{es}"),
        (Some((b, bs)), Some((e, es))) => format!("{b}{bs}-{e}{es}"),
        _ => String::new(),
    };
    let mut out = format!("{days} {time}").trim().to_string();
    if out.is_empty() {
        return None;
    }
    if kind.starts_with("lab") {
        out.push_str(" lab");
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sections() {
        let json = r#"[{"id":"202711_1","termCode":"202711","crn":"51234","dept":"CSCE","deptDesc":"CSCE - Computer Science & Engineering",
            "courseNumber":"221","sectionNumber":"500","courseTitle":"DATA STRUC & ALGORITHMS","campus":"College Station",
            "instructionType":"Traditional Face-to-Face (F2F)","isOpen":false,"attributesText":"College Station",
            "instructors":[{"name":"Second Person","isPrimary":false},{"name":"Teresa Leyk","isPrimary":true,"hasCv":true}],
            "meetings":[{"daysOfWeek":["M","W","F"],"beginTime":"08:00 AM","endTime":"08:50 AM","meetingType":"Lecture"},
                        {"daysOfWeek":["T","R"],"beginTime":"11:30 AM","endTime":"12:20 PM","meetingType":"Laboratory"},
                        {"daysOfWeek":["F"],"beginTime":"10:30 AM","endTime":"12:30 PM","meetingType":"Examination"}]},
            {"crn":"51299","dept":"CSCE","courseNumber":"221","sectionNumber":"700","campus":"College Station",
             "instructionType":"Web Based","attributesText":"College Station| Honors","instructors":[],"meetings":[]}]"#;
        let raw: Vec<RawSection> = serde_json::from_str(json).unwrap();
        let s: Vec<Section> = raw.into_iter().map(section).collect();
        assert_eq!(s[0].meetings, "MWF 8:00-8:50a; TR 11:30a-12:20p lab");
        assert_eq!(s[0].instructors[0].name, "Teresa Leyk");
        assert_eq!(s[0].subject_desc, "Computer Science & Engineering");
        assert_eq!(s[0].open, Some(false));
        assert!(!s[0].honors);
        assert!(s[1].honors, "attribute marks honors even outside 200-299");
        assert_eq!(s[1].delivery, "online");
        assert!(s[1].on_campus(Some("College Station")));
    }
}
