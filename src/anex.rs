//! Historical grade distributions from anex.us (compiled from the registrar's
//! grade reports).

use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::blocking::Client;
use serde::Deserialize;

use crate::net;

#[derive(Clone, Copy, Debug, Default)]
pub struct Grades {
    pub a: u32,
    pub b: u32,
    pub c: u32,
    pub d: u32,
    pub f: u32,
    pub q: u32,
}

impl Grades {
    pub fn add(&mut self, o: &Grades) {
        self.a += o.a;
        self.b += o.b;
        self.c += o.c;
        self.d += o.d;
        self.f += o.f;
        self.q += o.q;
    }

    pub fn graded(&self) -> u32 {
        self.a + self.b + self.c + self.d + self.f
    }

    /// Students who finished with a letter grade or Q-dropped.
    pub fn students(&self) -> u32 {
        self.graded() + self.q
    }

    pub fn gpa(&self) -> Option<f64> {
        let n = self.graded();
        (n > 0).then(|| (4 * self.a + 3 * self.b + 2 * self.c + self.d) as f64 / n as f64)
    }

    /// Percentages of A, B, C, D, F, Q out of all students (so they sum to 100).
    pub fn percents(&self) -> [f64; 6] {
        let n = self.students().max(1) as f64;
        [self.a, self.b, self.c, self.d, self.f, self.q].map(|x| 100.0 * x as f64 / n)
    }
}

#[derive(Clone, Debug)]
pub struct Record {
    pub prof: String,
    pub section: String,
    pub year: u16,
    pub semester: String,
    pub grades: Grades,
}

impl Record {
    /// Sort key: newer terms compare greater.
    pub fn term_key(&self) -> (u16, u8) {
        let s = match self.semester.as_str() {
            "SPRING" => 1,
            "SUMMER" => 2,
            "FALL" => 3,
            _ => 0,
        };
        (self.year, s)
    }

    pub fn term_label(&self) -> String {
        format!("{} {}", crate::names::title_case(&self.semester), self.year)
    }

    pub fn honors(&self) -> bool {
        is_honors_section(&self.section)
    }
}

/// TAMU numbers honors sections 200-299.
pub fn is_honors_section(section: &str) -> bool {
    section
        .trim()
        .parse::<u32>()
        .is_ok_and(|n| (200..300).contains(&n))
}

#[derive(Deserialize)]
struct Resp {
    classes: Vec<Raw>,
}

#[derive(Deserialize)]
#[allow(non_snake_case)]
struct Raw {
    section: String,
    A: String,
    B: String,
    C: String,
    D: String,
    F: String,
    Q: String,
    prof: String,
    year: String,
    semester: String,
}

fn n(s: &str) -> u32 {
    s.trim().parse().unwrap_or(0)
}

pub fn fetch(client: &Client, dept: &str, number: &str, refresh: bool) -> Result<Vec<Record>> {
    let key = format!("anex-{dept}-{number}.json");
    let text = net::cached(&key, Duration::from_secs(7 * 24 * 3600), refresh, || {
        let resp = client
            .post("https://anex.us/grades/getData/")
            .form(&[("dept", dept), ("number", number)])
            .send()?;
        net::body(resp, "anex")
    })?;
    let resp: Resp = serde_json::from_str(&text).context("anex returned unexpected data")?;
    Ok(resp
        .classes
        .into_iter()
        .map(|r| Record {
            grades: Grades {
                a: n(&r.A),
                b: n(&r.B),
                c: n(&r.C),
                d: n(&r.D),
                f: n(&r.F),
                q: n(&r.Q),
            },
            prof: r.prof.trim().to_string(),
            section: r.section,
            year: r.year.trim().parse().unwrap_or(0),
            semester: r.semester.trim().to_uppercase(),
        })
        .collect())
}
