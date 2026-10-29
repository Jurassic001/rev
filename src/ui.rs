//! Rendering. `draw` also records where the RMP link landed so main can turn it
//! into an OSC 8 hyperlink and so mouse clicks can hit it.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Cell, Clear, List, ListItem, ListState, Paragraph, Row, Table, Wrap,
};

use crate::anex::Grades;
use crate::app::{App, Link, Mode};
use crate::model::{self, ProfRow, RmpState};
use crate::names::Strength;

const SPINNER: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
pub const LINK_STYLE: Style = Style::new()
    .fg(Color::LightBlue)
    .add_modifier(Modifier::UNDERLINED);

fn gpa_color(g: f64) -> Color {
    if g >= 3.3 {
        Color::Green
    } else if g >= 2.7 {
        Color::Yellow
    } else {
        Color::Red
    }
}

fn quality_color(q: f64) -> Color {
    if q >= 4.0 {
        Color::Green
    } else if q >= 3.0 {
        Color::Yellow
    } else {
        Color::Red
    }
}

fn difficulty_color(d: f64) -> Color {
    if d <= 2.5 {
        Color::Green
    } else if d <= 3.5 {
        Color::Yellow
    } else {
        Color::Red
    }
}

fn thousands(n: u32) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn dim(s: impl Into<String>) -> Span<'static> {
    Span::styled(s.into(), Style::new().fg(Color::DarkGray))
}

pub fn draw(f: &mut Frame, app: &mut App) {
    app.link = None;
    let banner = banner_lines(app);
    let [header, banner_area, table, detail, footer] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(banner.len() as u16),
        Constraint::Min(6),
        Constraint::Length(13),
        Constraint::Length(1),
    ])
    .areas(f.area());

    draw_header(f, app, header);
    f.render_widget(Paragraph::new(banner), banner_area);
    draw_table(f, app, table);
    draw_detail(f, app, detail);
    draw_footer(f, app, footer);

    match app.mode {
        Mode::PickTerm => draw_picker(f, app),
        Mode::Help => draw_help(f),
        _ => {}
    }
}

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::bordered().title(" rev · Texas A&M course lookup ".bold());
    let inner = block.inner(area);
    f.render_widget(block, area);

    let editing = app.mode == Mode::EditCourse;
    let course = if editing {
        Span::styled(
            format!("{:<12}", app.input),
            Style::new().fg(Color::Black).bg(Color::White),
        )
    } else {
        let c = app
            .lookup
            .as_ref()
            .map(|l| format!("{} {}", l.subject, l.number))
            .unwrap_or_else(|| "—".into());
        Span::styled(c, Style::new().bold())
    };
    let term = app
        .term
        .as_ref()
        .map_or("loading…".to_string(), |t| t.desc.clone());
    let honors = if app.include_honors {
        Span::styled("shown", Style::new().fg(Color::Magenta))
    } else {
        dim("hidden")
    };
    let line = Line::from(vec![
        dim(" Course "),
        course,
        dim("   Term "),
        Span::styled(term, Style::new().bold()),
        dim(if app.terms_live || app.terms.is_empty() {
            ""
        } else {
            " (offline list)"
        }),
        dim("   Honors "),
        honors,
        dim("   Sort "),
        Span::raw(app.sort.label()),
    ]);
    f.render_widget(Paragraph::new(line), inner);
    if editing {
        let x = inner.x + " Course ".len() as u16 + app.cursor as u16;
        f.set_cursor_position((x.min(inner.right().saturating_sub(1)), inner.y));
    }
}

fn banner_lines(app: &App) -> Vec<Line<'static>> {
    let Some(l) = &app.lookup else {
        return Vec::new();
    };
    let mut lines = Vec::new();
    let warn = Style::new().fg(Color::Yellow);
    let course = format!("{} {}", l.subject, l.number);

    match &l.sections {
        Some(Ok(s)) if !s.is_empty() => {
            let honors = s.iter().filter(|s| s.honors).count();
            let title = s
                .iter()
                .map(|s| s.title.as_str())
                .find(|t| !t.is_empty())
                .unwrap_or("");
            let mut spans = vec![
                Span::styled(format!(" {course} "), Style::new().bold()),
                Span::raw(format!("{title} · {} sections", s.len())),
            ];
            if honors > 0 && !app.include_honors {
                spans.push(dim(format!(" ({honors} honors hidden, h to show)")));
            }
            lines.push(Line::from(spans));
        }
        Some(Ok(_)) => {
            lines.push(Line::styled(
                format!(" ⚠ Howdy lists no {course} sections for {}.", l.term.desc),
                warn,
            ));
            lines.push(Line::styled(
                "   Showing recent instructors from anex instead.",
                warn,
            ));
        }
        Some(Err(e)) => {
            lines.push(Line::styled(format!(" ⚠ {e}."), warn));
            lines.push(Line::styled(
                format!(
                    "   Showing recent instructors from anex instead; this is NOT the {} roster.",
                    l.term.desc
                ),
                warn,
            ));
        }
        None => {}
    }
    match &l.anex {
        Some(Ok(r)) if r.is_empty() => lines.push(Line::styled(
            format!(" ⚠ anex has no grade history for {course}."),
            warn,
        )),
        Some(Ok(r)) => {
            let all = model::course_grades(r, app.include_honors);
            let newest = r
                .iter()
                .max_by_key(|r| r.term_key())
                .map(|r| r.term_label())
                .unwrap_or_default();
            if let Some(gpa) = all.gpa() {
                lines.push(Line::from(vec![
                    dim(" Course average (anex): GPA "),
                    Span::styled(format!("{gpa:.2}"), Style::new().fg(gpa_color(gpa)).bold()),
                    dim(format!(
                        " over {} students · data through {newest}",
                        thousands(all.students())
                    )),
                ]));
            }
        }
        Some(Err(e)) => lines.push(Line::styled(
            format!(" ⚠ {e}; grade distributions unavailable."),
            warn,
        )),
        None => {}
    }
    lines
}

fn rmp_cells(row: &ProfRow, state: Option<&RmpState>) -> [Cell<'static>; 4] {
    let right = |s: String, style: Style| Cell::from(Line::styled(s, style).right_aligned());
    if row.person.is_none() {
        return std::array::from_fn(|_| Cell::from(""));
    }
    match state {
        Some(RmpState::Found(f)) => {
            let t = &f.teacher;
            if t.num_ratings == 0 {
                return [
                    right("—".into(), Style::new()),
                    right("—".into(), Style::new()),
                    right("0".into(), Style::new()),
                    right("".into(), Style::new()),
                ];
            }
            let unsure = if f.strength == Strength::Initial {
                "~"
            } else {
                ""
            };
            [
                right(
                    format!("{unsure}{:.1}", t.quality),
                    Style::new().fg(quality_color(t.quality)),
                ),
                right(
                    format!("{:.1}", t.difficulty),
                    Style::new().fg(difficulty_color(t.difficulty)),
                ),
                right(t.num_ratings.to_string(), Style::new()),
                right(
                    t.would_take_again
                        .map(|p| format!("{p:.0}%"))
                        .unwrap_or_default(),
                    Style::new(),
                ),
            ]
        }
        Some(RmpState::Pending) => {
            std::array::from_fn(|_| right("…".into(), Style::new().fg(Color::DarkGray)))
        }
        Some(RmpState::Failed(_)) => std::array::from_fn(|i| {
            right(
                if i == 0 { "err" } else { "" }.into(),
                Style::new().fg(Color::Red),
            )
        }),
        Some(RmpState::Missing) | None => std::array::from_fn(|i| {
            right(
                if i == 0 { "—" } else { "" }.into(),
                Style::new().fg(Color::DarkGray),
            )
        }),
    }
}

fn draw_table(f: &mut Frame, app: &mut App, area: Rect) {
    let right = |s: String| Cell::from(Line::from(s).right_aligned());
    let header = Row::new([
        Cell::from("Professor"),
        Cell::from("Sections"),
        right("GPA".into()),
        right("A%".into()),
        right("B%".into()),
        right("C%".into()),
        right("D/F%".into()),
        right("Q%".into()),
        right("Students".into()),
        right("Quality".into()),
        right("Diff".into()),
        right("#RMP".into()),
        right("Again".into()),
    ])
    .style(Style::new().bold().fg(Color::Cyan));

    let rows: Vec<Row> = app
        .rows
        .iter()
        .map(|r| {
            let g = &r.grades;
            let mut cells = vec![Cell::from(r.name.clone()), Cell::from(section_list(r))];
            match g.gpa() {
                Some(gpa) => {
                    let p = g.percents();
                    cells.push(Cell::from(
                        Line::styled(format!("{gpa:.2}"), Style::new().fg(gpa_color(gpa)))
                            .right_aligned(),
                    ));
                    for v in [p[0], p[1], p[2], p[3] + p[4], p[5]] {
                        cells.push(right(format!("{v:.0}")));
                    }
                    cells.push(right(thousands(g.students())));
                }
                None => {
                    cells.push(Cell::from(
                        Line::styled("no data", Style::new().fg(Color::DarkGray)).right_aligned(),
                    ));
                    cells.extend((0..6).map(|_| Cell::from("")));
                }
            }
            cells.extend(rmp_cells(r, app.rmp_state(r)));
            Row::new(cells)
        })
        .collect();

    let widths = [
        Constraint::Fill(1),
        Constraint::Length(16),
        Constraint::Length(7),
        Constraint::Length(4),
        Constraint::Length(4),
        Constraint::Length(4),
        Constraint::Length(5),
        Constraint::Length(4),
        Constraint::Length(8),
        Constraint::Length(7),
        Constraint::Length(5),
        Constraint::Length(5),
        Constraint::Length(5),
    ];
    let title = if app.lookup.as_ref().is_some_and(|l| l.sections().is_empty()) {
        " Recent instructors (anex history) "
    } else {
        " Professors this term "
    };
    let block = Block::bordered().title(title);
    let inner = block.inner(area);
    app.table_body = Rect {
        y: inner.y + 1,
        height: inner.height.saturating_sub(1),
        ..inner
    };

    if app.rows.is_empty() {
        let msg = match &app.lookup {
            None if app.mode == Mode::EditCourse => {
                "Type a course (e.g. CSCE 121) and press Enter."
            }
            None => "Press / to enter a course.",
            Some(l) if l.sections.is_none() => "Loading…",
            Some(l) if !l.sections().is_empty() => {
                "Every section this term is honors. Press h to include honors sections."
            }
            Some(_) => "No professors found for this course.",
        };
        f.render_widget(Paragraph::new(dim(format!(" {msg}"))).block(block), area);
        return;
    }
    let table = Table::new(rows, widths)
        .header(header)
        .block(block)
        .column_spacing(1)
        .row_highlight_style(Style::new().bg(Color::Rgb(40, 50, 70)).bold())
        .highlight_symbol("▶ ");
    f.render_stateful_widget(table, area, &mut app.table);
}

fn section_list(r: &ProfRow) -> String {
    if r.sections.is_empty() {
        return r
            .history
            .first()
            .map(|t| format!("last {}", t.label))
            .unwrap_or_default();
    }
    r.sections
        .iter()
        .map(|s| s.section.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

fn draw_detail(f: &mut Frame, app: &mut App, area: Rect) {
    let [grades, sections, rmp] = Layout::horizontal([
        Constraint::Length(38),
        Constraint::Fill(1),
        Constraint::Length(44),
    ])
    .areas(area);

    let Some(row) = app.selected_row() else {
        for (a, t) in [
            (grades, " Grades (anex) "),
            (sections, " Sections "),
            (rmp, " Rate My Professors "),
        ] {
            f.render_widget(Block::bordered().title(t), a);
        }
        return;
    };
    f.render_widget(grade_panel(row, grades.width.saturating_sub(2)), grades);
    f.render_widget(section_panel(row), sections);
    let state = app.rmp_state(row).cloned();
    let link = draw_rmp(f, state.as_ref(), rmp);
    if app.mode != Mode::PickTerm && app.mode != Mode::Help {
        app.link = link;
    }
}

fn grade_panel(row: &ProfRow, width: u16) -> Paragraph<'static> {
    let block = Block::bordered().title(" Grades for this course (anex) ");
    let g: &Grades = &row.grades;
    let Some(gpa) = g.gpa() else {
        let msg = if row.person.is_none() {
            "Instructor not assigned yet."
        } else {
            "No anex records for this professor in this course (new to it, or anex's data ends before they taught it)."
        };
        return Paragraph::new(dim(format!(" {msg}")))
            .wrap(Wrap { trim: false })
            .block(block);
    };
    let mut lines = vec![Line::from(vec![
        Span::raw(" GPA "),
        Span::styled(format!("{gpa:.2}"), Style::new().fg(gpa_color(gpa)).bold()),
        dim(format!(
            "  {} students, {} terms",
            thousands(g.students()),
            row.history.len()
        )),
    ])];
    let colors = [
        Color::Green,
        Color::LightGreen,
        Color::Yellow,
        Color::LightRed,
        Color::Red,
        Color::Magenta,
    ];
    let bar_w = width.saturating_sub(10) as f64;
    for ((label, pct), color) in ["A", "B", "C", "D", "F", "Q"]
        .iter()
        .zip(g.percents())
        .zip(colors)
    {
        let filled = (pct / 100.0 * bar_w).round() as usize;
        lines.push(Line::from(vec![
            Span::raw(format!(" {label} ")),
            Span::styled("█".repeat(filled), Style::new().fg(color)),
            Span::styled(
                "░".repeat(bar_w as usize - filled.min(bar_w as usize)),
                Style::new().fg(Color::DarkGray),
            ),
            Span::raw(format!("{pct:5.1}%")),
        ]));
    }
    lines.push(dim(" Recent terms:").into());
    for t in row.history.iter().take(3) {
        let gpa = t.grades.gpa().unwrap_or(0.0);
        lines.push(Line::from(vec![
            Span::raw(format!("  {:<13}", t.label)),
            Span::styled(format!("{gpa:.2}"), Style::new().fg(gpa_color(gpa))),
            dim(format!("  {} students", thousands(t.grades.students()))),
        ]));
    }
    Paragraph::new(lines).block(block)
}

fn section_panel(row: &ProfRow) -> Paragraph<'static> {
    if row.sections.is_empty() {
        let block = Block::bordered().title(" Terms taught (anex) ");
        let lines: Vec<Line> = row
            .history
            .iter()
            .map(|t| {
                Line::from(vec![
                    Span::raw(format!(" {:<12}", t.label)),
                    dim(format!(
                        "{} section{}",
                        t.sections,
                        if t.sections == 1 { "" } else { "s" }
                    )),
                ])
            })
            .collect();
        return Paragraph::new(lines).block(block);
    }
    let block = Block::bordered().title(" Sections this term (Howdy) ");
    let mut lines: Vec<Line> = vec![dim(" Sec  CRN    Meets").into()];
    lines.extend(row.sections.iter().map(|s| {
        let mut spans = vec![
            Span::styled(format!(" {:<4}", s.section), Style::new().bold()),
            dim(format!(" {:<6}", s.crn)),
            Span::raw(format!(
                " {}",
                if s.meetings.is_empty() {
                    "TBA".to_string()
                } else {
                    s.meetings.clone()
                }
            )),
        ];
        // Lectures are the norm; only call out labs, online sections, etc.
        if !s.schedule.is_empty() && !s.schedule.eq_ignore_ascii_case("lecture") {
            spans.push(dim(format!("  {}", s.schedule)));
        }
        if s.honors {
            spans.push(Span::styled("  honors", Style::new().fg(Color::Magenta)));
        }
        match s.open {
            Some(true) => spans.push(Span::styled("  open", Style::new().fg(Color::Green))),
            Some(false) => spans.push(Span::styled("  full", Style::new().fg(Color::Red))),
            None => {}
        }
        Line::from(spans)
    }));
    Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .block(block)
}

fn draw_rmp(f: &mut Frame, state: Option<&RmpState>, area: Rect) -> Option<Link> {
    let block = Block::bordered().title(" Rate My Professors ");
    let inner = block.inner(area);
    let found = match state {
        Some(RmpState::Found(found)) => found,
        other => {
            let msg = match other {
                Some(RmpState::Pending) => "Searching…".to_string(),
                Some(RmpState::Failed(e)) => format!("Lookup failed: {e}"),
                None => String::new(),
                _ => "No matching TAMU professor on Rate My Professors.".to_string(),
            };
            f.render_widget(
                Paragraph::new(dim(format!(" {msg}")))
                    .wrap(Wrap { trim: false })
                    .block(block),
                area,
            );
            return None;
        }
    };
    let t = &found.teacher;
    let stat = |label: &str, value: String, color: Color| {
        Line::from(vec![
            dim(format!(" {label:<11}")),
            Span::styled(value, Style::new().fg(color).bold()),
        ])
    };
    let mut lines = vec![
        Line::from(vec![
            Span::styled(format!(" {} {}", t.first, t.last), Style::new().bold()),
            dim(format!(" · {}", t.department)),
        ]),
        Line::raw(""),
    ];
    if t.num_ratings > 0 {
        lines.push(stat(
            "Quality",
            format!("{:.1} / 5", t.quality),
            quality_color(t.quality),
        ));
        lines.push(stat(
            "Difficulty",
            format!("{:.1} / 5", t.difficulty),
            difficulty_color(t.difficulty),
        ));
        lines.push(stat("Ratings", t.num_ratings.to_string(), Color::White));
        if let Some(p) = t.would_take_again {
            lines.push(stat("Take again", format!("{p:.0}%"), Color::White));
        }
    } else {
        lines.push(dim(" Listed, but nobody has rated them yet.").into());
    }
    if found.strength == Strength::Initial {
        lines.push(Line::styled(
            " Matched on first initial only; double-check.",
            Style::new().fg(Color::Yellow),
        ));
    }
    let link_y = inner.bottom().saturating_sub(2);
    f.render_widget(Paragraph::new(lines).block(block), area);

    // The link itself: drawn here as plain styled text, upgraded to a real
    // OSC 8 hyperlink after the frame is flushed.
    let url = t.url();
    let text: String = url
        .trim_start_matches("https://www.")
        .chars()
        .take(inner.width.saturating_sub(1) as usize)
        .collect();
    let link_area = Rect {
        x: inner.x + 1,
        y: link_y,
        width: text.chars().count() as u16,
        height: 1,
    };
    f.render_widget(
        Paragraph::new(Span::styled(text.clone(), LINK_STYLE)),
        link_area,
    );
    f.render_widget(
        Paragraph::new(dim(" click it or press o to open")),
        Rect {
            y: link_y + 1,
            height: 1,
            ..inner
        },
    );
    Some(Link {
        area: link_area,
        url,
        text,
    })
}

fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    let keys: &[(&str, &str)] = match app.mode {
        Mode::EditCourse => &[("Enter", "search"), ("Tab", "term"), ("Esc", "cancel")],
        Mode::PickTerm => &[
            ("type", "filter"),
            ("↑↓", "move"),
            ("Enter", "choose"),
            ("Esc", "cancel"),
        ],
        _ => &[
            ("/", "course"),
            ("t", "term"),
            ("h", "honors"),
            ("s", "sort"),
            ("o", "open RMP"),
            ("r", "refresh"),
            ("?", "help"),
            ("q", "quit"),
        ],
    };
    let mut spans: Vec<Span> = Vec::new();
    for (k, v) in keys {
        spans.push(Span::styled(
            format!(" {k}"),
            Style::new().fg(Color::Cyan).bold(),
        ));
        spans.push(dim(format!(" {v} ")));
    }
    let keys = Line::from(spans);
    let [left, right] =
        Layout::horizontal([Constraint::Length(keys.width() as u16), Constraint::Fill(1)])
            .areas(area);
    f.render_widget(Paragraph::new(keys), left);

    let status = match &app.notice {
        Some(n) => Span::styled(format!("{n} "), Style::new().fg(Color::Yellow)),
        None if app.busy() => Span::raw(format!(
            "{} {} ",
            SPINNER[app.tick % SPINNER.len()],
            app.status
        )),
        None => dim(format!("{} ", app.status)),
    };
    f.render_widget(Paragraph::new(Line::from(status).right_aligned()), right);
}

fn popup(f: &mut Frame, width: u16, height: u16, title: &str) -> Rect {
    let area = f.area();
    let w = width.min(area.width.saturating_sub(4));
    let h = height.min(area.height.saturating_sub(2));
    let rect = Rect {
        x: area.x + (area.width - w) / 2,
        y: area.y + (area.height - h) / 2,
        width: w,
        height: h,
    };
    f.render_widget(Clear, rect);
    let block = Block::bordered()
        .title(title.to_string())
        .border_style(Style::new().fg(Color::Cyan));
    let inner = block.inner(rect);
    f.render_widget(block, rect);
    inner
}

fn draw_picker(f: &mut Frame, app: &App) {
    let inner = popup(f, 64, 24, " Choose a term ");
    let [filter, list] =
        Layout::vertical([Constraint::Length(2), Constraint::Fill(1)]).areas(inner);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            dim(" Filter: "),
            Span::raw(app.picker_filter.clone()),
            Span::raw("▏"),
        ])),
        filter,
    );
    let matches = app.picker_matches();
    let items: Vec<ListItem> = matches
        .iter()
        .map(|&i| {
            let t = &app.terms[i];
            ListItem::new(Line::from(vec![
                Span::raw(format!(" {}", t.desc)),
                dim(format!("  {}", t.code)),
            ]))
        })
        .collect();
    let mut state =
        ListState::default().with_selected(matches.iter().position(|&i| i == app.picker_sel));
    let list_widget = List::new(items)
        .highlight_style(Style::new().bg(Color::Rgb(40, 50, 70)).bold())
        .highlight_symbol("▶");
    f.render_stateful_widget(list_widget, list, &mut state);
}

fn draw_help(f: &mut Frame) {
    let inner = popup(f, 72, 26, " Help ");
    let key = |k: &str, v: &str| {
        Line::from(vec![
            Span::styled(format!(" {k:<14}"), Style::new().fg(Color::Cyan).bold()),
            Span::raw(v.to_string()),
        ])
    };
    let lines = vec![
        key("/  c  e", "enter a course (e.g. CSCE 121)"),
        key("t", "choose the term (fall, spring, summer; any campus)"),
        key("h", "show / hide honors sections (hidden by default)"),
        key("s", "cycle sort: GPA, quality, difficulty, # ratings, name"),
        key("o  Enter", "open the selected professor's RMP page"),
        key("click", "select a row, or open the RMP link"),
        key("r", "refresh, ignoring cached data"),
        key("↑↓ j k PgUp/Dn", "move"),
        key("q  Ctrl-C", "quit"),
        Line::raw(""),
        dim(" Sources").into(),
        Line::raw("  Howdy public class search: who teaches each section this term"),
        Line::raw("  anex.us: past grade distributions for this course, per professor"),
        Line::raw("  Rate My Professors: quality, difficulty, would-take-again"),
        Line::raw(""),
        dim(" GPA and percentages cover every term anex has for that professor").into(),
        dim(" in this course. Q% is the share of students who Q-dropped.").into(),
        dim(" ~ before a quality score means the RMP match used only a first").into(),
        dim(" initial. Results are cached for a few hours; press r to refetch.").into(),
        Line::raw(""),
        dim(" Press any key to close.").into(),
    ];
    f.render_widget(Paragraph::new(lines), inner);
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::channel;

    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use crate::anex::{Grades, Record};
    use crate::app::App;
    use crate::howdy::{Instructor, Section, Term};
    use crate::names::Strength;
    use crate::rmp::{Found, Teacher};
    use crate::worker::Msg;

    fn section(num: &str, who: &str) -> Section {
        Section {
            subject: "CSCE".into(),
            course: "121".into(),
            section: num.into(),
            crn: format!("4{num}1"),
            title: "INTRO PGM DESIGN CONCEPT".into(),
            subject_desc: "Computer Science & Engineering".into(),
            schedule: "Lecture".into(),
            meetings: "MWF 09:10-10:00".into(),
            open: Some(num != "502"),
            honors: crate::anex::is_honors_section(num),
            instructors: (!who.is_empty())
                .then(|| Instructor {
                    name: who.into(),
                    primary: true,
                })
                .into_iter()
                .collect(),
        }
    }

    fn record(prof: &str, section: &str, year: u16, sem: &str, g: [u32; 6]) -> Record {
        let [a, b, c, d, f, q] = g;
        Record {
            prof: prof.into(),
            section: section.into(),
            year,
            semester: sem.into(),
            grades: Grades { a, b, c, d, f, q },
        }
    }

    fn render(app: &mut App) -> String {
        let mut term = Terminal::new(TestBackend::new(120, 34)).unwrap();
        term.draw(|f| super::draw(f, app)).unwrap();
        let buf = term.backend().buffer();
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn renders_lookup() {
        let (tx, _rx) = channel();
        let mut app = App::new(tx, Some("CSCE 121".into()), None, false);
        app.on_msg(Msg::Terms(Ok(vec![Term {
            code: crate::howdy::likely_term_code(),
            desc: "Spring 2027 - College Station".into(),
        }])));
        let sections = vec![
            section("501", "Shawn Lupoli"),
            section("502", "Shawn Lupoli"),
            section("503", "Qin Huang"),
            section("200", "Honors Person"),
            section("504", ""),
        ];
        let anex = vec![
            record("LUPOLI S", "501", 2024, "SPRING", [40, 30, 15, 5, 5, 5]),
            record("HUANG Q", "502", 2023, "FALL", [60, 20, 10, 3, 2, 5]),
        ];
        app.on_msg(Msg::Course {
            generation: 1,
            sections: Ok(sections),
            anex: Ok(anex),
        });
        let teacher = Teacher {
            legacy_id: 2300100,
            first: "Shawn".into(),
            last: "Lupoli".into(),
            department: "Computer Science".into(),
            quality: 2.6,
            difficulty: 2.7,
            num_ratings: 103,
            would_take_again: Some(48.0),
        };
        app.on_msg(Msg::Rmp {
            generation: 1,
            key: "SHAWN LUPOLI".into(),
            result: Ok(Some(Found {
                teacher,
                strength: Strength::Exact,
            })),
        });
        app.on_msg(Msg::Rmp {
            generation: 1,
            key: "QIN HUANG".into(),
            result: Ok(None),
        });
        // GPA sort puts Huang first; select Lupoli to show his RMP pane
        app.on_key(ratatui::crossterm::event::KeyEvent::from(
            ratatui::crossterm::event::KeyCode::Down,
        ));
        let screen = render(&mut app);
        println!("{screen}");
        assert!(screen.contains("Shawn Lupoli"));
        assert!(screen.contains("ratemyprofessors.com/professor/2300100"));
        assert!(screen.contains("1 honors hidden"));
        assert!(!screen.contains("Honors Person"));
        assert!(screen.contains("TBA (not assigned)"));
        let link = app.link.as_ref().expect("link recorded");
        assert_eq!(
            link.url,
            "https://www.ratemyprofessors.com/professor/2300100"
        );
    }

    #[test]
    fn renders_fallback_and_picker() {
        let (tx, _rx) = channel();
        let mut app = App::new(tx, Some("CSCE 121".into()), Some("fall 2026".into()), false);
        app.on_msg(Msg::Terms(Err("Howdy answered with a Cloudflare browser challenge instead of data (HTTP 403 Forbidden)".into())));
        app.on_msg(Msg::Course {
            generation: 1,
            sections: Err("Howdy answered with a Cloudflare browser challenge instead of data (HTTP 403 Forbidden)".into()),
            anex: Ok(vec![record("MOORE J", "501", 2024, "SPRING", [10, 10, 10, 1, 1, 2])]),
        });
        println!("{}", render(&mut app));
        app.on_key(ratatui::crossterm::event::KeyEvent::from(
            ratatui::crossterm::event::KeyCode::Char('t'),
        ));
        let screen = render(&mut app);
        println!("{screen}");
        assert!(screen.contains("Fall 2026 - College Station"));
        assert!(
            app.link.is_none(),
            "no link overlay while a popup covers the screen"
        );
    }
}
