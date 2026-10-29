//! Joins the three sources into one row per professor.

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};

use crate::anex::{Grades, Record};
use crate::names::{self, Person};
use crate::rmp::Found;
use crate::schedule::Section;

/// How many of anex's most recent terms count as "currently teaching" when
/// we have to fall back to anex for the roster.
const RECENT_TERMS: usize = 4;

#[derive(Clone, Debug)]
pub enum RmpState {
    Pending,
    Found(Found),
    Missing,
    Failed(String),
}

impl RmpState {
    pub fn rated(&self) -> Option<&Found> {
        match self {
            RmpState::Found(f) if f.teacher.num_ratings > 0 => Some(f),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sort {
    Gpa,
    Quality,
    Difficulty,
    Ratings,
    Name,
}

impl Sort {
    pub fn next(self) -> Sort {
        match self {
            Sort::Gpa => Sort::Quality,
            Sort::Quality => Sort::Difficulty,
            Sort::Difficulty => Sort::Ratings,
            Sort::Ratings => Sort::Name,
            Sort::Name => Sort::Gpa,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Sort::Gpa => "GPA ↓",
            Sort::Quality => "quality ↓",
            Sort::Difficulty => "difficulty ↑",
            Sort::Ratings => "# ratings ↓",
            Sort::Name => "name",
        }
    }
}

pub struct TermGrades {
    pub label: String,
    pub grades: Grades,
    pub sections: usize,
}

pub struct ProfRow {
    pub name: String,
    /// `None` for sections that have no instructor assigned yet.
    pub person: Option<Person>,
    pub sections: Vec<Section>,
    pub grades: Grades,
    /// Newest term first.
    pub history: Vec<TermGrades>,
}

impl ProfRow {
    pub fn key(&self) -> String {
        self.person
            .as_ref()
            .map_or_else(|| "TBA".to_string(), Person::key)
    }
}

fn aggregate<'a>(records: impl Iterator<Item = &'a Record>) -> (Grades, Vec<TermGrades>) {
    let mut total = Grades::default();
    let mut by_term: BTreeMap<(u16, u8), TermGrades> = BTreeMap::new();
    for r in records {
        total.add(&r.grades);
        let t = by_term.entry(r.term_key()).or_insert_with(|| TermGrades {
            label: r.term_label(),
            grades: Grades::default(),
            sections: 0,
        });
        t.grades.add(&r.grades);
        t.sections += 1;
    }
    (total, by_term.into_values().rev().collect())
}

/// Who's teaching, as far as we know.
#[derive(Clone, Copy)]
pub enum Roster<'a> {
    /// This term's sections, already limited to the term's campus.
    Sections(&'a [Section]),
    /// Names the user typed in from Schedule Builder (`--noSB`).
    Names(&'a [String]),
    /// Nothing to go on: whoever anex shows teaching it recently.
    AnexRecent,
}

impl<'a> Roster<'a> {
    pub fn pick(manual: Option<&'a [String]>, sections: &'a [Section]) -> Roster<'a> {
        match manual {
            Some(names) => Roster::Names(names),
            None if !sections.is_empty() => Roster::Sections(sections),
            None => Roster::AnexRecent,
        }
    }
}

/// Distinct instructors across `sections`, in first-seen order.
fn instructors<'a>(sections: impl Iterator<Item = &'a Section>) -> Vec<(String, Person)> {
    let mut out: Vec<(String, Person)> = Vec::new();
    for s in sections {
        for i in &s.instructors {
            if let Some(p) = Person::from_full(&i.name)
                && !out.iter().any(|(_, q)| q.key() == p.key())
            {
                out.push((i.name.clone(), p));
            }
        }
    }
    out
}

/// Typed-in names, deduplicated; entries without a first and last name are dropped.
fn typed_names(names: &[String]) -> Vec<(String, Person)> {
    let mut out: Vec<(String, Person)> = Vec::new();
    for n in names {
        if let Some(p) = Person::from_full(n)
            && !out.iter().any(|(_, q)| q.key() == p.key())
        {
            out.push((names::natural_order(n), p));
        }
    }
    out
}

/// anex spellings of everyone who taught the course in its last few terms.
fn recent_anex_profs<'a>(records: impl Iterator<Item = &'a Record> + Clone) -> Vec<String> {
    let mut terms: Vec<(u16, u8)> = records.clone().map(Record::term_key).collect();
    terms.sort_unstable();
    terms.dedup();
    let cutoff = terms
        .iter()
        .rev()
        .nth(RECENT_TERMS - 1)
        .or(terms.first())
        .copied()
        .unwrap_or_default();
    let mut profs: Vec<String> = records
        .filter(|r| r.term_key() >= cutoff)
        .map(|r| r.prof.clone())
        .collect();
    profs.sort();
    profs.dedup();
    profs
}

/// Everyone the worker should look up on RMP. Honors sections are included so
/// toggling them on later needs no new requests.
pub fn people(roster: Roster, records: &[Record]) -> Vec<Person> {
    match roster {
        Roster::Sections(sections) => instructors(sections.iter())
            .into_iter()
            .map(|(_, p)| p)
            .collect(),
        Roster::Names(names) => typed_names(names).into_iter().map(|(_, p)| p).collect(),
        Roster::AnexRecent => recent_anex_profs(records.iter())
            .iter()
            .filter_map(|p| Person::from_anex(p))
            .collect(),
    }
}

/// anex records for one course, ready to be matched against full names.
struct AnexIndex<'a> {
    records: Vec<&'a Record>,
    spellings: Vec<(&'a str, Person)>,
}

impl<'a> AnexIndex<'a> {
    fn new(records: &'a [Record], include_honors: bool) -> AnexIndex<'a> {
        let records: Vec<&Record> = records
            .iter()
            .filter(|r| include_honors || !r.honors())
            .collect();
        let mut names: Vec<&str> = records.iter().map(|r| r.prof.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        let spellings = names
            .into_iter()
            .filter_map(|n| Some((n, Person::from_anex(n)?)))
            .collect();
        AnexIndex { records, spellings }
    }

    fn grades_for(&self, person: &Person) -> (Grades, Vec<TermGrades>) {
        let Person::Full(full) = person else {
            return Default::default();
        };
        let matches: Vec<_> = self
            .spellings
            .iter()
            .filter_map(|(n, p)| Some((names::anex_match(full, p)?, *n)))
            .collect();
        // Only keep the most confident spelling(s), so "MOORE J" doesn't
        // pull in a middle-initial match when an exact one exists.
        let best = matches.iter().map(|(s, _)| *s).max();
        let spellings: Vec<&str> = matches
            .iter()
            .filter(|(s, _)| Some(*s) == best)
            .map(|(_, n)| *n)
            .collect();
        aggregate(
            self.records
                .iter()
                .copied()
                .filter(|r| spellings.contains(&r.prof.as_str())),
        )
    }
}

/// One row per professor on the roster.
pub fn build_rows(roster: Roster, records: &[Record], include_honors: bool) -> Vec<ProfRow> {
    let anex = AnexIndex::new(records, include_honors);
    let sections = match roster {
        Roster::Sections(sections) => sections,
        Roster::Names(names) => {
            return typed_names(names)
                .into_iter()
                .map(|(name, person)| {
                    let (grades, history) = anex.grades_for(&person);
                    ProfRow {
                        name,
                        person: Some(person),
                        sections: Vec::new(),
                        grades,
                        history,
                    }
                })
                .collect();
        }
        Roster::AnexRecent => {
            return recent_anex_profs(anex.records.iter().copied())
                .into_iter()
                .filter_map(|prof| {
                    let person = Person::from_anex(&prof)?;
                    let (grades, history) =
                        aggregate(anex.records.iter().copied().filter(|r| r.prof == prof));
                    Some(ProfRow {
                        name: person.display(),
                        person: Some(person),
                        sections: Vec::new(),
                        grades,
                        history,
                    })
                })
                .collect();
        }
    };

    let sections: Vec<&Section> = sections
        .iter()
        .filter(|s| include_honors || !s.honors)
        .collect();
    let mut rows: Vec<ProfRow> = instructors(sections.iter().copied())
        .into_iter()
        .map(|(name, person)| {
            let (grades, history) = anex.grades_for(&person);
            let mine = sections
                .iter()
                .filter(|s| {
                    s.instructors.iter().any(|i| {
                        Person::from_full(&i.name).is_some_and(|p| p.key() == person.key())
                    })
                })
                .map(|s| (*s).clone())
                .collect();
            ProfRow {
                name,
                person: Some(person),
                sections: mine,
                grades,
                history,
            }
        })
        .collect();

    let unassigned: Vec<Section> = sections
        .iter()
        .filter(|s| s.instructors.is_empty())
        .map(|s| (*s).clone())
        .collect();
    if !unassigned.is_empty() {
        rows.push(ProfRow {
            name: "TBA (not assigned)".into(),
            person: None,
            sections: unassigned,
            grades: Grades::default(),
            history: Vec::new(),
        });
    }
    rows
}

/// Grades across every professor, for the "course average" line.
pub fn course_grades(records: &[Record], include_honors: bool) -> Grades {
    aggregate(records.iter().filter(|r| include_honors || !r.honors())).0
}

fn cmp_opt(a: Option<f64>, b: Option<f64>, descending: bool) -> Ordering {
    match (a, b) {
        (Some(x), Some(y)) if descending => y.total_cmp(&x),
        (Some(x), Some(y)) => x.total_cmp(&y),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

pub fn sort_rows(rows: &mut [ProfRow], sort: Sort, rmp: &HashMap<String, RmpState>) {
    let rated = |r: &ProfRow| {
        rmp.get(&r.key())
            .and_then(RmpState::rated)
            .map(|f| f.teacher.clone())
    };
    rows.sort_by(|a, b| {
        let (ra, rb) = (rated(a), rated(b));
        let primary = match sort {
            Sort::Gpa => cmp_opt(a.grades.gpa(), b.grades.gpa(), true),
            Sort::Quality => cmp_opt(ra.map(|t| t.quality), rb.map(|t| t.quality), true),
            Sort::Difficulty => cmp_opt(ra.map(|t| t.difficulty), rb.map(|t| t.difficulty), false),
            Sort::Ratings => cmp_opt(
                ra.map(|t| t.num_ratings as f64),
                rb.map(|t| t.num_ratings as f64),
                true,
            ),
            Sort::Name => Ordering::Equal,
        };
        a.person
            .is_none()
            .cmp(&b.person.is_none())
            .then(primary)
            .then_with(|| a.name.cmp(&b.name))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schedule::Instructor;

    fn record(prof: &str, section: &str, year: u16, sem: &str, a: u32, f: u32) -> Record {
        Record {
            prof: prof.into(),
            section: section.into(),
            year,
            semester: sem.into(),
            grades: Grades {
                a,
                f,
                ..Default::default()
            },
        }
    }

    fn section(num: &str, who: &[&str]) -> Section {
        Section {
            section: num.into(),
            crn: "1".into(),
            title: String::new(),
            subject_desc: String::new(),
            campus: "College Station".into(),
            delivery: String::new(),
            meetings: String::new(),
            open: None,
            honors: crate::anex::is_honors_section(num),
            instructors: who
                .iter()
                .map(|n| Instructor {
                    name: n.to_string(),
                    primary: true,
                })
                .collect(),
        }
    }

    #[test]
    fn joins_sections_and_anex() {
        let records = vec![
            record("LUPOLI S", "501", 2023, "FALL", 10, 0),
            record("LUPOLI S", "200", 2023, "FALL", 10, 0),
            record("LUPOLI S", "502", 2024, "SPRING", 5, 5),
            record("SMITH J", "503", 2024, "SPRING", 1, 1),
        ];
        let sections = vec![
            section("501", &["Shawn Lupoli"]),
            section("201", &["Honors Only"]),
            section("502", &[]),
        ];
        let rows = build_rows(Roster::Sections(&sections), &records, false);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].name, "Shawn Lupoli");
        assert_eq!(rows[0].grades.a, 15); // honors section 200 excluded
        assert_eq!(rows[0].history[0].label, "Spring 2024");
        assert!(rows[1].person.is_none());

        let with_honors = build_rows(Roster::Sections(&sections), &records, true);
        assert_eq!(with_honors.len(), 3);
        assert_eq!(with_honors[0].grades.a, 25);
    }

    #[test]
    fn falls_back_to_recent_anex_profs() {
        let records = vec![
            record("OLD P", "501", 2015, "FALL", 1, 0),
            record("A A", "501", 2023, "SPRING", 1, 0),
            record("B B", "501", 2023, "FALL", 1, 0),
            record("C C", "501", 2024, "SPRING", 1, 0),
            record("D D", "501", 2024, "SUMMER", 1, 0),
        ];
        let rows = build_rows(Roster::AnexRecent, &records, false);
        let names: Vec<_> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["A. A", "B. B", "C. C", "D. D"]);
        assert_eq!(people(Roster::AnexRecent, &records).len(), 4);
    }

    #[test]
    fn typed_in_names() {
        let records = vec![
            record("LEYK T", "501", 2024, "SPRING", 10, 2),
            record("LEYK T", "200", 2024, "SPRING", 9, 0),
            record("PEARCE R", "502", 2023, "FALL", 3, 3),
        ];
        let names: Vec<String> = ["Leyk, Teresa", "Roger A. Pearce", "Teresa Leyk", "Merchant"]
            .map(String::from)
            .to_vec();
        let roster = Roster::pick(Some(&names), &[]);
        let rows = build_rows(roster, &records, false);
        let shown: Vec<_> = rows.iter().map(|r| r.name.as_str()).collect();
        // duplicate Leyk collapsed, last-name-only entry dropped
        assert_eq!(shown, ["Teresa Leyk", "Roger A. Pearce"]);
        assert_eq!(rows[0].grades.a, 10);
        assert_eq!(rows[1].grades.f, 3);
        assert_eq!(people(roster, &records).len(), 2);
    }
}
