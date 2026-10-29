mod anex;
mod app;
mod howdy;
mod model;
mod names;
mod net;
mod rmp;
mod ui;
mod worker;

use std::io::{self, Write};
use std::sync::mpsc::Receiver;
use std::time::Duration;

use anyhow::Context;
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyEventKind};
use ratatui::crossterm::style::{Attribute, Color, Print, SetAttribute, SetForegroundColor};
use ratatui::crossterm::{cursor, execute, queue};

use crate::app::App;
use crate::worker::Msg;

const USAGE: &str = "\
rev - who teaches a Texas A&M course, how they grade, and how students rate them

USAGE:
    rev [COURSE] [--term TERM] [--honors]

EXAMPLES:
    rev CSCE 121
    rev \"MATH 151\" --term \"fall 2026\"
    rev ENGR102 -t 202711 --honors

OPTIONS:
    -t, --term TERM   term name (\"spring 2027\", \"fall 2026 galveston\") or Banner code;
                      defaults to the upcoming College Station term
    --honors          include honors sections (hidden by default; toggle with h)
    -h, --help        show this message";

fn main() -> anyhow::Result<()> {
    let mut course = Vec::new();
    let mut term = None;
    let mut honors = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            "-t" | "--term" => term = Some(args.next().context("--term needs a value")?),
            "--honors" => honors = true,
            _ if arg.starts_with("--term=") => term = Some(arg["--term=".len()..].to_string()),
            _ => course.push(arg),
        }
    }
    let course = (!course.is_empty()).then(|| course.join(" "));

    let (jobs, msgs) = worker::spawn();
    let mut app = App::new(jobs, course, term, honors);

    // ratatui's panic hook restores the screen; this one also releases the mouse.
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = execute!(io::stdout(), event::DisableMouseCapture);
        hook(info);
    }));
    let mut terminal = ratatui::init();
    execute!(io::stdout(), event::EnableMouseCapture)?;
    let result = run(&mut terminal, &mut app, &msgs);
    let _ = execute!(io::stdout(), event::DisableMouseCapture);
    ratatui::restore();
    result
}

fn run(terminal: &mut DefaultTerminal, app: &mut App, msgs: &Receiver<Msg>) -> anyhow::Result<()> {
    while !app.quit {
        for msg in msgs.try_iter() {
            app.on_msg(msg);
        }
        terminal.draw(|f| ui::draw(f, app))?;
        write_hyperlink(terminal, app)?;

        if event::poll(Duration::from_millis(100))? {
            loop {
                match event::read()? {
                    // Windows reports key releases too
                    Event::Key(key) if key.kind == KeyEventKind::Press => app.on_key(key),
                    Event::Mouse(mouse) => app.on_mouse(mouse),
                    _ => {}
                }
                if app.quit || !event::poll(Duration::ZERO)? {
                    break;
                }
            }
        }
        app.tick += 1;
    }
    Ok(())
}

/// ratatui has no hyperlink support, so after each frame we reprint the link
/// text in place wrapped in an OSC 8 escape. Terminals that understand it
/// (Windows Terminal, VS Code, iTerm2, ...) make it Ctrl/Cmd-clickable; the
/// rest ignore the escape and show the same text.
fn write_hyperlink(terminal: &mut DefaultTerminal, app: &App) -> io::Result<()> {
    let Some(link) = &app.link else { return Ok(()) };
    let w = terminal.backend_mut();
    queue!(
        w,
        cursor::SavePosition,
        cursor::MoveTo(link.area.x, link.area.y),
        SetForegroundColor(Color::Blue),
        SetAttribute(Attribute::Underlined),
        Print(format!(
            "\x1b]8;;{}\x1b\\{}\x1b]8;;\x1b\\",
            link.url, link.text
        )),
        SetAttribute(Attribute::Reset),
        cursor::RestorePosition,
    )?;
    w.flush()
}
