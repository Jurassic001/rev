//! Background thread that does all network work so the UI never blocks.
//! Results are tagged with the lookup generation; the UI drops stale ones.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;

use reqwest::blocking::Client;

use crate::anex::{self, Record};
use crate::howdy::{self, Section, Term};
use crate::model;
use crate::rmp::{self, Found};

/// Parallel RMP requests; enough to be quick without being rude.
const RMP_THREADS: usize = 4;

pub enum Job {
    Terms,
    Lookup {
        generation: u64,
        term: Term,
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
        sections: Result<Vec<Section>, String>,
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
    let mut terms_cache: HashMap<String, Arc<Vec<Section>>> = HashMap::new();
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
            let terms = howdy::fetch_terms(&client, false).map_err(|e| format!("{e:#}"));
            if out.send(Msg::Terms(terms)).is_err() {
                return;
            }
        }
        if let Some(Job::Lookup {
            generation,
            term,
            subject,
            number,
            refresh,
        }) = lookup
        {
            let ok = do_lookup(
                &client,
                &out,
                &mut terms_cache,
                generation,
                &term,
                &subject,
                &number,
                refresh,
            );
            if !ok {
                return;
            }
        }
    }
}

/// Returns false once the UI has gone away.
#[allow(clippy::too_many_arguments)]
fn do_lookup(
    client: &Client,
    out: &Sender<Msg>,
    terms_cache: &mut HashMap<String, Arc<Vec<Section>>>,
    generation: u64,
    term: &Term,
    subject: &str,
    number: &str,
    refresh: bool,
) -> bool {
    let status = |text: String| out.send(Msg::Status { generation, text }).is_ok();
    let cached = (!refresh)
        .then(|| terms_cache.get(&term.code).cloned())
        .flatten();
    if cached.is_none() && !status(format!("Loading the {} class list from Howdy…", term.desc)) {
        return false;
    }

    let (howdy, anex) = thread::scope(|s| {
        let anex = s.spawn(|| anex::fetch(client, subject, number, refresh));
        let howdy = match cached {
            Some(all) => Ok(all),
            None => howdy::fetch_term(client, &term.code, refresh).map(Arc::new),
        };
        (howdy, anex.join().expect("anex thread panicked"))
    });
    if let Ok(all) = &howdy {
        terms_cache.insert(term.code.clone(), all.clone());
    }
    let sections: Result<Vec<Section>, String> = howdy
        .map(|all| {
            all.iter()
                .filter(|s| s.subject.eq_ignore_ascii_case(subject) && s.course == number)
                .cloned()
                .collect()
        })
        .map_err(|e| format!("{e:#}"));
    let anex = anex.map_err(|e| format!("{e:#}"));

    let people = model::people(
        sections.as_deref().unwrap_or_default(),
        anex.as_deref().unwrap_or_default(),
    );
    let dept_hint = sections
        .as_ref()
        .ok()
        .and_then(|s| s.first())
        .map(|s| s.subject_desc.clone())
        .unwrap_or_default();
    if out
        .send(Msg::Course {
            generation,
            sections,
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
        let rows = model::build_rows(&[], &records, false);
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
                    f.teacher.quality,
                    f.teacher.difficulty,
                    f.teacher.num_ratings,
                    f.strength,
                    f.teacher.legacy_id
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
    fn howdy() {
        let client = crate::net::client().unwrap();
        match howdy::fetch_terms(&client, true) {
            Ok(t) => println!(
                "{} terms, newest: {:?}",
                t.len(),
                t.iter().take(5).collect::<Vec<_>>()
            ),
            Err(e) => println!("terms failed: {e:#}"),
        }
    }
}
