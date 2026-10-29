# rev

A terminal app for picking a professor at Texas A&M. Give it a course and a
term and it shows who is teaching each section, how each professor has graded
that course in the past (from [anex](https://anex.us)), and how students rate
them on [Rate My Professors](https://www.ratemyprofessors.com), with a
clickable link to each professor's RMP page.

```
┌ rev · Texas A&M course lookup ───────────────────────────────────────────────────────────────────────────────────────┐
│ Course CSCE 221   Term Spring 2027 - College Station   Honors hidden   Sort GPA ↓                                    │
└──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘
 CSCE 221 DATA STRUCTURES & ALGORITHMS · 12 sections (1 honors hidden, h to show) · 1 other-campus section hidden
 Course average (anex): GPA 3.24 over 8,357 students · data through Summer 2026
┌ Professors this term (AggieSB+) ─────────────────────────────────────────────────────────────────────────────────────┐
│  Professor                      Sections             GPA   A%   B%   C%  D/F%   Q% Students Quality  Diff  #RMP Again│
│▶ Roger A. Pearce                506-508             3.33   44   39   10     1    6       93     4.0   3.0     2  100%│
│  Teresa Leyk                    500-505             3.20   39   38   14     3    6    5,204     3.1   3.5   156   61%│
│  Amit Merchant                  509-511          no data                                        4.9   2.5    58   97%│
│                                                                                                                      │
└──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘
┌ Grades for this course (anex) ─────┐┌ Sections this term (AggieSB+) ─────┐┌ Rate My Professors ──────────────────────┐
│ GPA 3.33  93 students, 1 term      ││ Sec  CRN    Meets                  ││ Roger Pearce · Computer Science          │
│ A ███████████░░░░░░░░░░░░░░░ 44.1% ││ 506  12352  MWF 9:10-10:00a  open  ││                                          │
│ B ██████████░░░░░░░░░░░░░░░░ 38.7% ││             TR 8:25-9:15a lab      ││ Quality    4.0 / 5                       │
│ C ███░░░░░░░░░░░░░░░░░░░░░░░  9.7% ││ 507  16242  MWF 9:10-10:00a  open  ││ Difficulty 3.0 / 5                       │
│ D ░░░░░░░░░░░░░░░░░░░░░░░░░░  0.0% ││             TR 9:35-10:25a lab     ││ Ratings    2                             │
│ F ░░░░░░░░░░░░░░░░░░░░░░░░░░  1.1% ││ 508  22284  MWF 9:10-10:00a  open  ││ Take again 100%                          │
│ Q ██░░░░░░░░░░░░░░░░░░░░░░░░  6.5% ││             TR 11:35a-12:25p lab   ││                                          │
│ Recent terms:                      ││                                    ││                                          │
│  Spring 2026  3.33  93 students    ││                                    ││                                          │
│                                    ││                                    ││ ratemyprofessors.com/professor/2802744   │
│                                    ││                                    ││ click it or press o to open              │
└────────────────────────────────────┘└────────────────────────────────────┘└──────────────────────────────────────────┘
 / course  t term  h honors  s sort  o open RMP  r refresh  ? help  q quit                3 professors · 3 found on RMP
```

## Usage

```sh
rev                                   # start empty and type a course in
rev CSCE 221                          # the upcoming College Station term
rev "MATH 151" --term "fall 2026"
rev ECEN 214 --term 202631 --honors
rev CSCE 221 --noSB --prof "Teresa Leyk" --prof "Pearce, Roger"
```

| Option | Meaning |
| --- | --- |
| `COURSE` | Subject and number: `CSCE 221`, `csce221`, `MATH-151` |
| `-t`, `--term TERM` | A term name (`"spring 2027"`, `"fall 2026 galveston"`) or a Banner code (`202711`). Defaults to the term students are most likely planning for: January–March picks that spring, April–September that fall, and October–December the next spring. |
| `--honors` | Include honors sections, which are hidden by default. Sections 200–299 and sections tagged Honors count as honors. |
| `--noSB` | Don't use AggieSB+ for the roster; type in the professors yourself (see below). |
| `-p`, `--prof NAME` | With `--noSB`: a professor teaching the course. Repeat it or separate names with `;`. |
| `-h`, `--help` | Show help. |

Sections on other campuses than the term's are always hidden, so a College
Station term never shows a Galveston section of the same course. The banner
says how many were dropped.

### Keys

| Key | Action |
| --- | --- |
| `/` `c` `e` | Enter a course |
| `t` | Choose the term from a filterable list (type to filter) |
| `p` | With `--noSB`: edit the list of professors |
| `h` | Show or hide honors sections |
| `s` | Cycle the sort: GPA, quality, difficulty, number of ratings, name |
| `o` `Enter` | Open the selected professor's RMP page |
| click | Select a row, or open the RMP link |
| `r` | Refresh, ignoring cached data |
| `↑` `↓` `j` `k` `PgUp` `PgDn` `g` `G` | Move |
| `?` | Help |
| `q` `Ctrl-C` | Quit |

### Typing in the roster (`--noSB`)

AggieSB+ is a student-run project, not an official TAMU service. If it's
down, out of date, or you'd rather not depend on it, start with `--noSB`.
rev then never contacts AggieSB+. After you enter a course it opens a
"Professors from Schedule Builder" box: type each professor you see listed
for the course in Schedule Builder and press Enter. `Teresa Leyk` and
`Leyk, Teresa` both work, and several names can go on one line separated by
`;`. Use `↑` `↓` and `Del` to remove a name, and Esc (or Enter on a blank
line) when you're done. rev then looks up those professors on anex and RMP.
Press `p` any time to change the list.

Names need a first and a last name; anex records only a last name and first
initial, so a last name alone would be ambiguous.

### Reading the table

- **Sections**: this term's sections for that professor. When the roster
  didn't come from AggieSB+, it shows the last term they taught the course
  instead.
- **GPA, A% … Q%, Students**: every term anex has for that professor in this
  course, with honors sections left out unless honors are shown. Q% is the
  share of students who Q-dropped. `no data` means anex has no records for
  them in this course, usually because they're new to it.
- **Quality, Diff, #RMP, Again**: the professor's Rate My Professors overall
  quality, difficulty, number of ratings, and would-take-again percentage.
  `—` means no matching TAMU professor was found, and `err` means the lookup
  failed (select the row to see why). A `~` in front of the
  quality score means the match used only the first initial, so double-check
  it.
- **TBA (not assigned)**: sections with no instructor listed yet.

If AggieSB+ is unreachable or lists no sections for the course, rev says so
in the banner and falls back to the professors who taught it in anex's most
recent four terms. That is history, not this term's roster.

## Where the data comes from

| Source | Used for | Notes |
| --- | --- | --- |
| [AggieSB+ API](https://api-aggiesbp.nitheeshk.com/docs) | Terms, sections, instructors, meeting times, open/full | Unofficial mirror of the class schedule. Howdy's own class search sits behind a Cloudflare bot check, so rev doesn't use it. Skipped entirely with `--noSB`. |
| [anex.us](https://anex.us/grades/) | Past grade distributions per section | Built from the registrar's grade reports, so it trails the current term. The banner says how far the data goes. |
| [Rate My Professors](https://www.ratemyprofessors.com) | Quality, difficulty, ratings, would take again | Searches Texas A&M (College Station) only. |

Professors are matched across the three sources by name. anex writes names as
`LEYK T`, so the anex side matches on last name and first initial, and
common nicknames (Bill/William, Mike/Michael, …) are understood on the RMP
side.

## Caching

Responses are cached on disk so reopening a course is instant and the sites
aren't asked for the same data over and over:

| Data | Kept for |
| --- | --- |
| AggieSB+ term list | 1 day |
| AggieSB+ sections for a course | 6 hours |
| anex grades for a course | 7 days |
| RMP search results | 3 days |

The cache lives in `%LOCALAPPDATA%\rev` on Windows, `~/.cache/rev` on Linux,
and `~/Library/Caches/rev` on macOS (`just cache-dir` prints it). Press `r`
to refetch the current course, or `just clear-cache` to drop everything.

## Clickable links

The RMP URL in the bottom-right pane is written as an
[OSC 8 hyperlink](https://gist.github.com/egmontkobayashi/d48f2a9c57d1e4016a3be19ef8a12b5f),
so terminals that support it (Windows Terminal, WezTerm, kitty, iTerm2, GNOME
Terminal, …) treat it as a real link. A plain click on it opens it in your
browser through rev itself, which works in any terminal. Ctrl+click is left
to the terminal's own link handling, and `o` or Enter opens it from the
keyboard.

rev captures the mouse for clicks and scrolling, so to select text with the
mouse, hold Shift while dragging (in most terminals).

## Limitations

- anex only knows a last name and first initial, so two professors with the
  same last name and initial who taught the same course are counted as one.
- RMP matching is by name, so it can pick the wrong person, and a professor
  with no RMP page shows as not found. A `~` marks matches made from a first
  initial alone.
- RMP lookups are for the College Station school, so Galveston and Qatar
  terms may find few or no ratings.
- AggieSB+ is unofficial and could change or disappear; `--noSB` keeps rev
  usable if it does.
- rev isn't affiliated with Texas A&M, anex, Rate My Professors, or AggieSB+.

## Development

| Recipe | Does |
| --- | --- |
| `just run ARGS…` | Run the app (release build) |
| `just build` | Build `target/release/rev` |
| `just install` | Install `rev` into `~/.cargo/bin` |
| `just test [FILTER]` | Offline unit tests, including rendered-screen tests |
| `just live [FILTER]` | Tests that query AggieSB+, anex, and RMP for real |
| `just lint` | Clippy with warnings as errors |
| `just fmt` | Format the code |
| `just check` | Format check, lints, and unit tests: run before committing |
| `just clear-cache` | Delete cached responses |
| `just cache-dir` | Print the cache location |

Code layout:

| File | Role |
| --- | --- |
| `src/main.rs` | Argument parsing, terminal setup, event loop, OSC 8 link overlay |
| `src/app.rs` | App state and key/mouse handling |
| `src/ui.rs` | Rendering (ratatui) |
| `src/worker.rs` | Background thread that does all network work |
| `src/sb.rs` | AggieSB+ client |
| `src/anex.rs` | anex client and grade math |
| `src/rmp.rs` | Rate My Professors search and matching |
| `src/model.rs` | Joins the roster, anex, and RMP into table rows |
| `src/names.rs` | Name parsing and fuzzy matching |
| `src/schedule.rs` | Terms, sections, and campus filtering |
| `src/net.rs` | HTTP client and disk cache |
