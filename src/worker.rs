//! Background thread that does all network work so the UI never blocks.
//! Results are tagged with the lookup generation; the UI drops stale ones.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;

use reqwest::blocking::Client;

use crate::anex::{self, Record};
use crate::model::{self, Roster};
use crate::rmp::{self, Found};
use crate::sb;
use crate::schedule::{Section, Term};

/// Parallel RMP requests; enough to be quick without being rude.
const RMP_THREADS: usize = 4;

/// Where a lookup's professor list comes from.
#[derive(Clone, Debug)]
pub enum Source {
    /// AggieSB+'s copy of the term's class schedule.
    Sb(Term),
    /// Names the user typed in from Schedule Builder (`--noSB`).
    Manual(Vec<String>),
}

impl Source {
    pub fn manual_names(&self) -> Option<&[String]> {
        match self {
            Source::Manual(names) => Some(names),
            Source::Sb(_) => None,
        }
    }
}

pub enum Job {
    Terms,
    Lookup {
        generation: u64,
        source: Source,
        subject: String,
        number: String,
        refresh: bool,
    },
}

pub enum Msg {
    Terms(Result<Vec<Term>, String>),
    Status {
        generation: u64,
        text: String,
    },
    Course {
        generation: u64,
        /// `None` when the roster was typed in.
        sections: Option<Result<Vec<Section>, String>>,
        /// Sections on other campuses, dropped from `sections`.
        other_campus: usize,
        anex: Result<Vec<Record>, String>,
    },
    Rmp {
        generation: u64,
        key: String,
        result: Result<Option<Found>, String>,
    },
    Done {
        generation: u64,
    },
}

pub fn spawn() -> (Sender<Job>, Receiver<Msg>) {
    let (job_tx, job_rx) = channel();
    let (msg_tx, msg_rx) = channel();
    thread::spawn(move || run(job_rx, msg_tx));
    (job_tx, msg_rx)
}

fn run(jobs: Receiver<Job>, out: Sender<Msg>) {
    let client = match crate::net::client() {
        Ok(c) => c,
        Err(e) => {
            let _ = out.send(Msg::Terms(Err(e.to_string())));
            return;
        }
    };
    while let Ok(first) = jobs.recv() {
        // Collapse a burst of requests into the newest lookup.
        let mut want_terms = false;
        let mut lookup = None;
        for job in std::iter::once(first).chain(jobs.try_iter()) {
            match job {
                Job::Terms => want_terms = true,
                l @ Job::Lookup { .. } => lookup = Some(l),
            }
        }
        if want_terms {
            let terms = sb::fetch_terms(&client, false).map_err(|e| format!("{e:#}"));
            if out.send(Msg::Terms(terms)).is_err() {
                return;
            }
        }
        if let Some(Job::Lookup {
            generation,
            source,
            subject,
            number,
            refresh,
        }) = lookup
            && !do_lookup(
                &client, &out, generation, &source, &subject, &number, refresh,
            )
        {
            return;
        }
    }
}

/// Returns false once the UI has gone away.
fn do_lookup(
    client: &Client,
    out: &Sender<Msg>,
    generation: u64,
    source: &Source,
    subject: &str,
    number: &str,
    refresh: bool,
) -> bool {
    let status = |text: String| out.send(Msg::Status { generation, text }).is_ok();
    let loading = match source {
        Source::Sb(term) => format!(
            "Loading {subject} {number} for {} from AggieSB+…",
            term.desc
        ),
        Source::Manual(_) => format!("Loading anex grades for {subject} {number}…"),
    };
    if !status(loading) {
        return false;
    }

    let (sections, anex) = thread::scope(|s| {
        let anex = s.spawn(|| anex::fetch(client, subject, number, refresh));
        let sections = match source {
            Source::Sb(term) => Some(sb::fetch_course(
                client, &term.code, subject, number, refresh,
            )),
            Source::Manual(_) => None,
        };
        (sections, anex.join().expect("anex thread panicked"))
    });

    // Other campuses' sections are never shown, so drop them before anything
    // (RMP lookups included) sees them.
    let campus = match source {
        Source::Sb(term) => term.campus(),
        Source::Manual(_) => None,
    };
    let mut other_campus = 0;
    let sections = sections.map(|result| {
        result
            .map(|all| {
                let (here, away): (Vec<Section>, Vec<Section>) =
                    all.into_iter().partition(|s| s.on_campus(campus));
                other_campus = away.len();
                here
            })
            .map_err(|e| format!("{e:#}"))
    });
    let anex = anex.map_err(|e| format!("{e:#}"));

    let on_campus: &[Section] = match &sections {
        Some(Ok(s)) => s,
        _ => &[],
    };
    let roster = Roster::pick(source.manual_names(), on_campus);
    let people = model::people(roster, anex.as_deref().unwrap_or_default());
    let dept_hint = on_campus
        .first()
        .map(|s| s.subject_desc.clone())
        .unwrap_or_default();
    if out
        .send(Msg::Course {
            generation,
            sections,
            other_campus,
            anex,
        })
        .is_err()
    {
        return false;
    }

    if !people.is_empty()
        && !status(format!(
            "Looking up {} professors on Rate My Professors…",
            people.len()
        ))
    {
        return false;
    }
    let next = AtomicUsize::new(0);
    thread::scope(|s| {
        for _ in 0..RMP_THREADS.min(people.len()) {
            s.spawn(|| {
                while let Some(p) = people.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let result =
                        rmp::lookup(client, p, &dept_hint, refresh).map_err(|e| format!("{e:#}"));
                    if out
                        .send(Msg::Rmp {
                            generation,
                            key: p.key(),
                            result,
                        })
                        .is_err()
                    {
                        return;
                    }
                }
            });
        }
    });
    out.send(Msg::Done { generation }).is_ok()
}

#[cfg(test)]
mod live {
    //! Hits the real sites: `cargo test -- --ignored --nocapture`
    use super::*;
    use crate::names::Person;

    #[test]
    #[ignore]
    fn anex_and_rmp() {
        let client = crate::net::client().unwrap();
        let records = anex::fetch(&client, "CSCE", "121", true).unwrap();
        let rows = model::build_rows(Roster::AnexRecent, &records, false);
        for r in &rows {
            let p = r.person.as_ref().unwrap();
            let found = rmp::lookup(&client, p, "Computer Science & Engineering", true).unwrap();
            println!(
                "{:<16} gpa {:.2} n={:<5} -> {:?}",
                r.name,
                r.grades.gpa().unwrap_or(0.0),
                r.grades.students(),
                found.map(|f| (
                    f.teacher.first,
                    f.teacher.last,
                    f.teacher.num_ratings,
                    f.strength
                ))
            );
        }
        let full = rmp::lookup(
            &client,
            &Person::from_full("Shawn Lupoli").unwrap(),
            "",
            true,
        )
        .unwrap()
        .unwrap();
        assert_eq!(full.teacher.legacy_id, 2300100);
    }

    #[test]
    #[ignore]
    fn aggie_sb() {
        let client = crate::net::client().unwrap();
        let terms = sb::fetch_terms(&client, true).unwrap();
        println!(
            "terms: {:?}",
            terms.iter().map(|t| &t.desc).collect::<Vec<_>>()
        );
        let term = terms
            .iter()
            .find(|t| t.code == "202711")
            .expect("Spring 2027 listed");
        let sections = sb::fetch_course(&client, &term.code, "CSCE", "221", true).unwrap();
        for s in &sections {
            println!(
                "{} {:<16} on_campus={} honors={} {:?} | {}",
                s.section,
                s.campus,
                s.on_campus(term.campus()),
                s.honors,
                s.instructors.iter().map(|i| &i.name).collect::<Vec<_>>(),
                s.meetings
            );
        }
        assert!(
            sections.iter().any(|s| !s.on_campus(term.campus())),
            "expected a Galveston section"
        );
        assert!(
            sb::fetch_course(&client, &term.code, "CSCE", "121", true)
                .unwrap()
                .is_empty()
        );
    }
}
