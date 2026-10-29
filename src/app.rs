//! UI state and input handling.

use std::collections::HashMap;
use std::sync::mpsc::Sender;

use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::{Position, Rect};
use ratatui::widgets::TableState;

use crate::anex::Record;
use crate::howdy::{self, Section, Term};
use crate::model::{self, ProfRow, RmpState, Sort};
use crate::rmp::Found;
use crate::worker::{Job, Msg};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    EditCourse,
    PickTerm,
    Help,
}

pub struct Lookup {
    pub term: Term,
    pub subject: String,
    pub number: String,
    pub sections: Option<Result<Vec<Section>, String>>,
    pub anex: Option<Result<Vec<Record>, String>>,
    pub rmp: HashMap<String, RmpState>,
    pub done: bool,
}

impl Lookup {
    pub fn sections(&self) -> &[Section] {
        match &self.sections {
            Some(Ok(s)) => s,
            _ => &[],
        }
    }

    pub fn records(&self) -> &[Record] {
        match &self.anex {
            Some(Ok(r)) => r,
            _ => &[],
        }
    }
}

/// The RMP link as drawn on screen, for mouse clicks and the OSC 8 overlay.
pub struct Link {
    pub area: Rect,
    pub url: String,
    pub text: String,
}

pub struct App {
    jobs: Sender<Job>,
    pub mode: Mode,
    pub input: String,
    /// Cursor position in `input`, in chars.
    pub cursor: usize,
    pub terms: Vec<Term>,
    pub terms_live: bool,
    pub term: Option<Term>,
    term_query: Option<String>,
    pub picker_filter: String,
    pub picker_sel: usize,
    pub include_honors: bool,
    pub sort: Sort,
    generation: u64,
    pub lookup: Option<Lookup>,
    /// Course requested before the term list arrived.
    pending: Option<(String, String)>,
    pub rows: Vec<ProfRow>,
    pub table: TableState,
    selected_key: Option<String>,
    pub status: String,
    pub notice: Option<String>,
    pub tick: usize,
    pub quit: bool,
    pub link: Option<Link>,
    pub table_body: Rect,
}

/// "csce 121", "CSCE121", "CSCE-121" -> ("CSCE", "121")
pub fn parse_course(s: &str) -> Option<(String, String)> {
    let s: String = s.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    let split = s.find(|c: char| c.is_ascii_digit())?;
    let (subject, number) = s.split_at(split);
    let ok = (2..=5).contains(&subject.len())
        && subject.chars().all(|c| c.is_ascii_alphabetic())
        && !number.is_empty();
    ok.then(|| (subject.to_uppercase(), number.to_uppercase()))
}

impl App {
    pub fn new(
        jobs: Sender<Job>,
        course: Option<String>,
        term_query: Option<String>,
        include_honors: bool,
    ) -> App {
        let pending = course.as_deref().and_then(parse_course);
        let mut app = App {
            jobs,
            mode: if pending.is_some() {
                Mode::Normal
            } else {
                Mode::EditCourse
            },
            input: course.clone().unwrap_or_default(),
            cursor: 0,
            terms: Vec::new(),
            terms_live: false,
            term: None,
            term_query,
            picker_filter: String::new(),
            picker_sel: 0,
            include_honors,
            sort: Sort::Gpa,
            generation: 0,
            lookup: None,
            pending,
            rows: Vec::new(),
            table: TableState::default(),
            selected_key: None,
            status: "Loading Howdy's term list…".into(),
            notice: None,
            tick: 0,
            quit: false,
            link: None,
            table_body: Rect::default(),
        };
        if course.is_some() && app.pending.is_none() {
            app.notice = Some("Enter a course like CSCE 121".into());
            app.mode = Mode::EditCourse;
        }
        app.cursor = app.input.chars().count();
        let _ = app.jobs.send(Job::Terms);
        app
    }

    pub fn busy(&self) -> bool {
        self.term.is_none() || self.lookup.as_ref().is_some_and(|l| !l.done)
    }

    pub fn on_msg(&mut self, msg: Msg) {
        match msg {
            Msg::Terms(result) => self.on_terms(result),
            Msg::Status { generation, text } if generation == self.generation => self.status = text,
            Msg::Course {
                generation,
                sections,
                anex,
            } if generation == self.generation => {
                let Some(l) = &mut self.lookup else { return };
                let people = model::people(
                    sections.as_deref().unwrap_or_default(),
                    anex.as_deref().unwrap_or_default(),
                );
                for p in people {
                    l.rmp.insert(p.key(), RmpState::Pending);
                }
                l.sections = Some(sections);
                l.anex = Some(anex);
                self.rebuild_rows();
            }
            Msg::Rmp {
                generation,
                key,
                result,
            } if generation == self.generation => {
                let Some(l) = &mut self.lookup else { return };
                let state = match result {
                    Ok(Some(found)) => RmpState::Found(found),
                    Ok(None) => RmpState::Missing,
                    Err(e) => RmpState::Failed(e),
                };
                l.rmp.insert(key, state);
                self.rebuild_rows();
            }
            Msg::Done { generation } if generation == self.generation => {
                if let Some(l) = &mut self.lookup {
                    l.done = true;
                    let found = l
                        .rmp
                        .values()
                        .filter(|s| matches!(s, RmpState::Found(_)))
                        .count();
                    self.status = format!(
                        "{} professors · {found}/{} found on RMP",
                        self.rows.len(),
                        l.rmp.len()
                    );
                }
            }
            _ => {} // stale generation
        }
    }

    fn on_terms(&mut self, result: Result<Vec<Term>, String>) {
        match result {
            Ok(t) if !t.is_empty() => {
                self.terms = t;
                self.terms_live = true;
            }
            other => {
                self.terms = howdy::guess_terms();
                let why = other.err().unwrap_or_else(|| "it was empty".into());
                self.notice = Some(format!(
                    "Howdy term list unavailable ({why}); offering standard terms"
                ));
            }
        }
        let wanted = self.term_query.take();
        let resolved = wanted
            .as_deref()
            .and_then(|q| howdy::resolve_term(q, &self.terms));
        if let (Some(q), None) = (&wanted, &resolved) {
            self.notice = Some(format!("No term matches \"{q}\"; press t to choose one"));
        }
        self.term = resolved.or_else(|| default_term(&self.terms));
        self.status.clear();
        if let Some((s, n)) = self.pending.take() {
            self.start_lookup(s, n, false);
        }
    }

    fn start_lookup(&mut self, subject: String, number: String, refresh: bool) {
        let Some(term) = self.term.clone() else {
            self.pending = Some((subject, number));
            return;
        };
        self.generation += 1;
        self.lookup = Some(Lookup {
            term: term.clone(),
            subject: subject.clone(),
            number: number.clone(),
            sections: None,
            anex: None,
            rmp: HashMap::new(),
            done: false,
        });
        self.rows.clear();
        self.selected_key = None;
        self.table = TableState::default();
        self.status = format!("Looking up {subject} {number}…");
        let _ = self.jobs.send(Job::Lookup {
            generation: self.generation,
            term,
            subject,
            number,
            refresh,
        });
    }

    fn rebuild_rows(&mut self) {
        let Some(l) = &self.lookup else { return };
        let mut rows = model::build_rows(l.sections(), l.records(), self.include_honors);
        model::sort_rows(&mut rows, self.sort, &l.rmp);
        let idx = self
            .selected_key
            .as_ref()
            .and_then(|k| rows.iter().position(|r| &r.key() == k))
            .or((!rows.is_empty()).then_some(0));
        self.rows = rows;
        self.select(idx);
    }

    fn select(&mut self, idx: Option<usize>) {
        let idx = idx
            .map(|i| i.min(self.rows.len().saturating_sub(1)))
            .filter(|_| !self.rows.is_empty());
        self.table.select(idx);
        self.selected_key = idx.map(|i| self.rows[i].key());
    }

    fn move_selection(&mut self, delta: isize) {
        let cur = self.table.selected().unwrap_or(0) as isize;
        self.select(Some((cur + delta).max(0) as usize));
    }

    pub fn selected_row(&self) -> Option<&ProfRow> {
        self.rows.get(self.table.selected()?)
    }

    pub fn rmp_state(&self, row: &ProfRow) -> Option<&RmpState> {
        self.lookup.as_ref()?.rmp.get(&row.key())
    }

    fn selected_found(&self) -> Option<&Found> {
        match self.rmp_state(self.selected_row()?)? {
            RmpState::Found(f) => Some(f),
            _ => None,
        }
    }

    fn open_selected(&mut self) {
        let Some(f) = self.selected_found() else {
            self.notice = Some("No Rate My Professors page for this professor".into());
            return;
        };
        let url = f.teacher.url();
        self.open_url(&url);
    }

    fn open_url(&mut self, url: &str) {
        self.notice = Some(match open::that_detached(url) {
            Ok(()) => format!("Opened {url}"),
            Err(e) => format!("Couldn't open a browser ({e}); the link is {url}"),
        });
    }

    /// Indices into `terms` that match the picker's filter.
    pub fn picker_matches(&self) -> Vec<usize> {
        let words: Vec<String> = self
            .picker_filter
            .to_lowercase()
            .split_whitespace()
            .map(String::from)
            .collect();
        self.terms
            .iter()
            .enumerate()
            .filter(|(_, t)| {
                let d = format!("{} {}", t.desc.to_lowercase(), t.code);
                words.iter().all(|w| d.contains(w.as_str()))
            })
            .map(|(i, _)| i)
            .collect()
    }

    fn current_course(&self) -> Option<(String, String)> {
        self.lookup
            .as_ref()
            .map(|l| (l.subject.clone(), l.number.clone()))
            .or_else(|| self.pending.clone())
    }

    pub fn on_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }
        self.notice = None;
        match self.mode {
            Mode::Normal => self.normal_key(key),
            Mode::EditCourse => self.edit_key(key),
            Mode::PickTerm => self.picker_key(key),
            Mode::Help => self.mode = Mode::Normal,
        }
    }

    fn normal_key(&mut self, key: KeyEvent) {
        let page = self.table_body.height.max(1) as isize;
        match key.code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('/' | 'c' | 'e') => {
                self.mode = Mode::EditCourse;
                self.input = self
                    .current_course()
                    .map(|(s, n)| format!("{s} {n}"))
                    .unwrap_or_default();
                self.cursor = self.input.chars().count();
            }
            KeyCode::Char('t') => {
                if self.terms.is_empty() {
                    self.notice = Some("Still loading the term list…".into());
                    return;
                }
                self.mode = Mode::PickTerm;
                self.picker_filter.clear();
                let current = self.term.as_ref().map(|t| t.code.clone());
                self.picker_sel = self
                    .terms
                    .iter()
                    .position(|t| Some(&t.code) == current.as_ref())
                    .unwrap_or(0);
            }
            KeyCode::Char('h') => {
                self.include_honors = !self.include_honors;
                self.rebuild_rows();
            }
            KeyCode::Char('s') => {
                self.sort = self.sort.next();
                self.rebuild_rows();
            }
            KeyCode::Char('o') | KeyCode::Enter => self.open_selected(),
            KeyCode::Char('r') => match self.current_course() {
                Some((s, n)) => self.start_lookup(s, n, true),
                None => self.notice = Some("Nothing to refresh yet".into()),
            },
            KeyCode::Char('?') => self.mode = Mode::Help,
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::PageUp => self.move_selection(-page),
            KeyCode::PageDown => self.move_selection(page),
            KeyCode::Home | KeyCode::Char('g') => self.select(Some(0)),
            KeyCode::End | KeyCode::Char('G') => self.select(Some(usize::MAX)),
            _ => {}
        }
    }

    fn edit_key(&mut self, key: KeyEvent) {
        let byte = |s: &str, i: usize| s.char_indices().nth(i).map_or(s.len(), |(b, _)| b);
        match key.code {
            KeyCode::Enter => match parse_course(&self.input) {
                Some((s, n)) => {
                    self.mode = Mode::Normal;
                    self.start_lookup(s, n, false);
                }
                None => self.notice = Some("Enter a course like CSCE 121".into()),
            },
            KeyCode::Esc => self.mode = Mode::Normal,
            KeyCode::Tab => self.normal_key(KeyEvent::from(KeyCode::Char('t'))),
            KeyCode::Backspace if self.cursor > 0 => {
                self.cursor -= 1;
                let b = byte(&self.input, self.cursor);
                self.input.remove(b);
            }
            KeyCode::Delete if self.cursor < self.input.chars().count() => {
                let b = byte(&self.input, self.cursor);
                self.input.remove(b);
            }
            KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right => self.cursor = (self.cursor + 1).min(self.input.chars().count()),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.input.chars().count(),
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                let b = byte(&self.input, self.cursor);
                self.input.insert(b, c);
                self.cursor += 1;
            }
            _ => {}
        }
    }

    fn picker_key(&mut self, key: KeyEvent) {
        let matches = self.picker_matches();
        let pos = matches
            .iter()
            .position(|&i| i == self.picker_sel)
            .unwrap_or(0);
        let pick = |p: isize| {
            let last = matches.len().checked_sub(1)?;
            matches.get(p.clamp(0, last as isize) as usize).copied()
        };
        match key.code {
            KeyCode::Esc => self.mode = Mode::Normal,
            KeyCode::Enter => {
                if let Some(i) = pick(pos as isize) {
                    self.term = Some(self.terms[i].clone());
                    self.mode = Mode::Normal;
                    if let Some((s, n)) = self.current_course() {
                        self.start_lookup(s, n, false);
                    } else {
                        self.mode = Mode::EditCourse;
                    }
                }
            }
            KeyCode::Up => self.picker_sel = pick(pos as isize - 1).unwrap_or(0),
            KeyCode::Down => self.picker_sel = pick(pos as isize + 1).unwrap_or(0),
            KeyCode::PageUp => self.picker_sel = pick(pos as isize - 10).unwrap_or(0),
            KeyCode::PageDown => self.picker_sel = pick(pos as isize + 10).unwrap_or(0),
            KeyCode::Backspace => {
                self.picker_filter.pop();
                self.picker_sel = self.picker_matches().first().copied().unwrap_or(0);
            }
            KeyCode::Char(c) => {
                self.picker_filter.push(c);
                self.picker_sel = self.picker_matches().first().copied().unwrap_or(0);
            }
            _ => {}
        }
    }

    pub fn on_mouse(&mut self, ev: MouseEvent) {
        if self.mode != Mode::Normal {
            return;
        }
        let at = Position::new(ev.column, ev.row);
        match ev.kind {
            MouseEventKind::ScrollDown => self.move_selection(1),
            MouseEventKind::ScrollUp => self.move_selection(-1),
            MouseEventKind::Down(MouseButton::Left) => {
                self.notice = None;
                // Ctrl+click is the terminal's own hyperlink gesture; let it handle that.
                if let Some(link) = &self.link
                    && link.area.contains(at)
                    && !ev.modifiers.contains(KeyModifiers::CONTROL)
                {
                    let url = link.url.clone();
                    self.open_url(&url);
                } else if self.table_body.contains(at) {
                    let row = self.table.offset() + (at.y - self.table_body.y) as usize;
                    if row < self.rows.len() {
                        self.select(Some(row));
                    }
                }
            }
            _ => {}
        }
    }
}

/// The upcoming College Station term if Howdy has it, else the newest one before it.
fn default_term(terms: &[Term]) -> Option<Term> {
    let likely = howdy::likely_term_code();
    terms
        .iter()
        .find(|t| t.code == likely)
        .or_else(|| {
            terms
                .iter()
                .find(|t| t.code <= likely && t.desc.contains("College Station"))
        })
        .or(terms.first())
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::parse_course;

    #[test]
    fn courses() {
        assert_eq!(
            parse_course("csce 121"),
            Some(("CSCE".into(), "121".into()))
        );
        assert_eq!(
            parse_course("MATH-151"),
            Some(("MATH".into(), "151".into()))
        );
        assert_eq!(parse_course("ENGR102"), Some(("ENGR".into(), "102".into())));
        assert_eq!(parse_course("121"), None);
        assert_eq!(parse_course("CSCE"), None);
    }
}
