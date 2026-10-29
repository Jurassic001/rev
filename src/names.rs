//! Name normalization and the fuzzy matching that ties together the three
//! sources, which all spell instructors differently:
//!   Howdy: "Shawn Lupoli"   anex: "LUPOLI S"   RMP: first "Shawn", last "Lupoli"

const SUFFIXES: &[&str] = &["JR", "SR", "II", "III", "IV"];

/// Uppercase letter-only tokens. Hyphens split tokens so "Smith-Jones" and
/// "SMITH JONES" compare equal; apostrophes and periods are dropped.
pub fn tokens(name: &str) -> Vec<String> {
    name.split(|c: char| c.is_whitespace() || c == '-' || c == ',')
        .map(|t| {
            t.chars()
                .filter(|c| c.is_alphabetic())
                .flat_map(char::to_uppercase)
                .collect::<String>()
        })
        .filter(|t| !t.is_empty())
        .collect()
}

fn strip_suffixes(mut t: Vec<String>) -> Vec<String> {
    while t.len() > 2 && t.last().is_some_and(|l| SUFFIXES.contains(&l.as_str())) {
        t.pop();
    }
    t
}

/// "SHAWN LUPOLI" -> "Shawn Lupoli"
pub fn title_case(name: &str) -> String {
    name.split(' ')
        .map(|w| {
            let mut out = String::new();
            let mut start = true;
            for c in w.chars() {
                if start {
                    out.extend(c.to_uppercase());
                } else {
                    out.extend(c.to_lowercase());
                }
                start = c == '-' || c == '\'';
            }
            out
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// "Leyk, Teresa" -> "Teresa Leyk"; "Robert Lightfoot, Jr." stays as is.
pub fn natural_order(name: &str) -> String {
    let name = name.trim();
    match name.split_once(',') {
        Some((last, first)) if !tokens(first).iter().all(|t| SUFFIXES.contains(&t.as_str())) => {
            format!("{} {}", first.trim(), last.trim())
        }
        _ => name.to_string(),
    }
}

/// An instructor as one source knows them.
#[derive(Clone, Debug)]
pub enum Person {
    /// Full name from Howdy, e.g. ["SHAWN", "LUPOLI"].
    Full(Vec<String>),
    /// anex "LUPOLI S": last-name tokens plus a first initial.
    Initial { last: Vec<String>, initial: char },
}

impl Person {
    pub fn from_full(name: &str) -> Option<Person> {
        let t = strip_suffixes(tokens(&natural_order(name)));
        (t.len() >= 2).then_some(Person::Full(t))
    }

    pub fn from_anex(prof: &str) -> Option<Person> {
        let mut t = tokens(prof);
        let initial = t.pop()?.chars().next()?;
        (!t.is_empty()).then_some(Person::Initial { last: t, initial })
    }

    /// Stable identity used to key RMP results.
    pub fn key(&self) -> String {
        match self {
            Person::Full(t) => t.join(" "),
            Person::Initial { last, initial } => format!("{} {initial}.", last.join(" ")),
        }
    }

    pub fn display(&self) -> String {
        match self {
            Person::Full(t) => title_case(&t.join(" ")),
            Person::Initial { last, initial } => {
                format!("{initial}. {}", title_case(&last.join(" ")))
            }
        }
    }

    /// What to type into RMP's search box.
    pub fn search_queries(&self) -> Vec<String> {
        match self {
            Person::Full(t) => {
                let first_last = format!("{} {}", t[0], t[t.len() - 1]);
                vec![title_case(&first_last), title_case(&t[t.len() - 1])]
            }
            Person::Initial { last, .. } => vec![title_case(&last.join(" "))],
        }
    }
}

/// How sure we are that two spellings are the same person.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Strength {
    Initial = 1,
    Nickname = 2,
    Exact = 3,
}

/// Does an anex record ("LUPOLI S") belong to this Howdy instructor?
pub fn anex_match(full: &[String], anex: &Person) -> Option<Strength> {
    let Person::Initial { last, initial } = anex else {
        return None;
    };
    if full.len() <= last.len() || !full.ends_with(last) {
        return None;
    }
    let given = &full[..full.len() - last.len()];
    if given[0].starts_with(*initial) {
        Some(Strength::Exact)
    } else if given.iter().any(|g| g.starts_with(*initial)) {
        // goes by a middle name in one source
        Some(Strength::Initial)
    } else {
        None
    }
}

const NICKNAMES: &[(&str, &str)] = &[
    ("ROBERT", "BOB"),
    ("ROBERT", "ROB"),
    ("ROBERT", "BOBBY"),
    ("WILLIAM", "BILL"),
    ("WILLIAM", "WILL"),
    ("WILLIAM", "BILLY"),
    ("JAMES", "JIM"),
    ("JAMES", "JIMMY"),
    ("MICHAEL", "MIKE"),
    ("RICHARD", "RICK"),
    ("RICHARD", "DICK"),
    ("ANTHONY", "TONY"),
    ("KATHERINE", "KATE"),
    ("KATHERINE", "KATIE"),
    ("CATHERINE", "CATHY"),
    ("ELIZABETH", "LIZ"),
    ("ELIZABETH", "BETH"),
    ("MARGARET", "PEGGY"),
    ("MARGARET", "MAGGIE"),
    ("THOMAS", "TOM"),
    ("EDWARD", "ED"),
    ("EDWARD", "TED"),
    ("JOHN", "JACK"),
    ("CHARLES", "CHUCK"),
    ("CHARLES", "CHARLIE"),
    ("JOSEPH", "JOE"),
    ("SUSAN", "SUE"),
    ("PATRICIA", "PATTY"),
    ("DEBORAH", "DEBBIE"),
    ("ALEXANDER", "ALEX"),
    ("ALEXANDRA", "ALEX"),
    ("SAMUEL", "SAM"),
    ("SAMANTHA", "SAM"),
    ("GREGORY", "GREG"),
    ("JEFFREY", "JEFF"),
    ("JENNIFER", "JEN"),
    ("JENNIFER", "JENNY"),
    ("LAWRENCE", "LARRY"),
    ("HENRY", "HANK"),
    ("FREDERICK", "FRED"),
    ("DOUGLAS", "DOUG"),
    ("PETER", "PETE"),
];

fn first_name_strength(a: &str, b: &str) -> Option<Strength> {
    if a == b {
        return Some(Strength::Exact);
    }
    if a.len() == 1 || b.len() == 1 {
        // one side is only an initial ("J. Smith")
        return (a.chars().next() == b.chars().next()).then_some(Strength::Initial);
    }
    let common = a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count();
    let short = a.len().min(b.len());
    if (common == short && short >= 3) || common >= 4 {
        return Some(Strength::Nickname); // Tim / Timothy, Kathy / Katherine
    }
    NICKNAMES
        .iter()
        .any(|&(f, n)| (f == a && n == b) || (f == b && n == a))
        .then_some(Strength::Nickname)
}

/// Score an RMP teacher against an instructor. `None` means "not this person".
pub fn rmp_match(person: &Person, rmp_first: &str, rmp_last: &str) -> Option<Strength> {
    let rf = tokens(rmp_first);
    let rl = tokens(rmp_last);
    let (Some(rf0), Some(rl_last)) = (rf.first(), rl.last()) else {
        return None;
    };
    match person {
        Person::Full(t) => {
            let given = if t.len() > rl.len() && t.ends_with(&rl) {
                &t[..t.len() - rl.len()]
            } else if t.last() == Some(rl_last) {
                &t[..t.len() - 1]
            } else {
                return None;
            };
            let first = first_name_strength(&given[0], rf0);
            // RMP lists some people under their middle name
            let middle = given[1..]
                .iter()
                .any(|g| g == rf0)
                .then_some(Strength::Nickname);
            first.max(middle)
        }
        Person::Initial { last, initial } => {
            let last_ok = rl == *last || rl.last() == last.last();
            (last_ok && rf0.starts_with(*initial)).then_some(Strength::Initial)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anex_names() {
        let Person::Full(howdy) =
            Person::from_full("Shawn Lupoli (P)".trim_end_matches(" (P)")).unwrap()
        else {
            panic!()
        };
        assert_eq!(
            anex_match(&howdy, &Person::from_anex("LUPOLI S").unwrap()),
            Some(Strength::Exact)
        );
        assert_eq!(
            anex_match(&howdy, &Person::from_anex("LUPOLI J").unwrap()),
            None
        );
        assert_eq!(
            anex_match(&howdy, &Person::from_anex("POLI S").unwrap()),
            None
        );

        let Person::Full(compound) = Person::from_full("Maria De La Cruz").unwrap() else {
            panic!()
        };
        assert_eq!(
            anex_match(&compound, &Person::from_anex("DE LA CRUZ M").unwrap()),
            Some(Strength::Exact)
        );
        assert_eq!(
            anex_match(&compound, &Person::from_anex("CRUZ M").unwrap()),
            Some(Strength::Exact)
        );

        let Person::Full(middle) = Person::from_full("J. Michael Moore").unwrap() else {
            panic!()
        };
        assert_eq!(
            anex_match(&middle, &Person::from_anex("MOORE M").unwrap()),
            Some(Strength::Initial)
        );
    }

    #[test]
    fn rmp_names() {
        let p = Person::from_full("Robert Lightfoot Jr.").unwrap();
        assert_eq!(rmp_match(&p, "Bob", "Lightfoot"), Some(Strength::Nickname));
        assert_eq!(rmp_match(&p, "Robert", "Lightfoot"), Some(Strength::Exact));
        assert_eq!(rmp_match(&p, "Rachel", "Lightfoot"), None);
        assert_eq!(rmp_match(&p, "R.", "Lightfoot"), Some(Strength::Initial));
        assert_eq!(rmp_match(&p, "Robert", "Light"), None);
        assert_eq!(
            rmp_match(&Person::from_full("Katherine Lee").unwrap(), "Kathy", "Lee"),
            Some(Strength::Nickname)
        );

        let p = Person::from_full("Mary Smith-Jones").unwrap();
        assert_eq!(rmp_match(&p, "Mary", "Smith Jones"), Some(Strength::Exact));
        assert_eq!(rmp_match(&p, "Mary", "Jones"), Some(Strength::Exact));

        let p = Person::from_anex("LUPOLI S").unwrap();
        assert_eq!(rmp_match(&p, "Shawn", "Lupoli"), Some(Strength::Initial));
        assert_eq!(p.display(), "S. Lupoli");
    }

    #[test]
    fn name_order() {
        assert_eq!(natural_order(" Leyk, Teresa "), "Teresa Leyk");
        assert_eq!(
            natural_order("Robert Lightfoot, Jr."),
            "Robert Lightfoot, Jr."
        );
        assert_eq!(
            Person::from_full("Lightfoot, Robert").unwrap().key(),
            "ROBERT LIGHTFOOT"
        );
        assert_eq!(
            Person::from_full("Robert Lightfoot, Jr.").unwrap().key(),
            "ROBERT LIGHTFOOT"
        );
        assert!(Person::from_full("Leyk").is_none());
    }

    #[test]
    fn titles() {
        assert_eq!(title_case("O'BRIEN SMITH-JONES"), "O'Brien Smith-Jones");
    }
}
