//! The owner's channels in plain words (owner, 2026-11-25: "currently its exposing a lot of alphanumeric
//! codes"). Every post to Discord, alerts and progress alike, is passed through `plain` as it is queued
//! (`notice::alert`, `alert_once`, `progress`): job, lease, revision and hand-over numbers, build names,
//! decision codes, the profile's id and table names are taken out, ISO dates are written "Jul 18". The
//! log (`vyreon-core log`) and the detail commands keep everything. The posts are composed in plain words
//! in the first place (`progress`, `watch::alert_for`); this is the net under them.

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// "2024-07-18" as "Jul 18"; None for anything else.
pub fn day(iso: &str) -> Option<String> {
    let b = iso.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' || !iso.chars().enumerate().all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit()) {
        return None;
    }
    let m: usize = iso[5..7].parse().ok()?;
    let d: u32 = iso[8..10].parse().ok()?;
    (1..=12).contains(&m).then(|| format!("{} {d}", MONTHS[m - 1]))
}

/// A build's name (`<commit>+<inventory>`) or a bare commit (7 to 40 hex digits holding a letter).
fn is_build(t: &str) -> bool {
    let hex = |s: &str| s.len() >= 7 && s.len() <= 40 && s.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()) && s.chars().any(|c| c.is_ascii_alphabetic());
    match t.split_once('+') {
        Some((a, b)) => hex(a) && b.chars().all(|c| c.is_ascii_hexdigit()) && !b.is_empty(),
        None => hex(t),
    }
}

/// A decision code: C-077, D-465, P-30.
fn is_code(t: &str) -> bool {
    let mut parts = t.splitn(2, '-');
    matches!((parts.next(), parts.next()), (Some(p), Some(n)) if ["C", "D", "P", "E"].contains(&p) && !n.is_empty() && n.len() <= 4 && n.chars().all(|c| c.is_ascii_digit()))
}

/// The internal words a number follows that names a record, not a quantity.
const NUMBERED: [&str; 8] = ["job", "jobs", "lease", "leases", "revision", "revisions", "hand-over", "report"];

/// `text` with its internal codes taken out (see the module), line by line.
pub fn plain(text: &str) -> String {
    // a command in backticks is the owner's to run as it is: left whole
    text.lines()
        .map(|line| {
            line.split('`')
                .enumerate()
                .map(|(i, part)| if i % 2 == 1 { part.to_owned() } else { plain_part(part) })
                .collect::<Vec<_>>()
                .join("`")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// A stretch outside backticks, its leading and trailing spaces kept (they join it to a command).
fn plain_part(part: &str) -> String {
    let lead = part.len() - part.trim_start_matches(' ').len();
    let trail = part.len() - part.trim_end_matches(' ').len();
    if part.trim().is_empty() {
        return part.to_owned();
    }
    let body = plain_line(part.trim_matches(' '));
    format!("{}{body}{}", " ".repeat(lead), if trail > 0 && part.len() > lead { " ".repeat(trail) } else { String::new() })
}

fn plain_line(line: &str) -> String {
    // the leading indentation is kept
    let indent: String = line.chars().take_while(|c| *c == ' ').collect();
    let words: Vec<&str> = line.trim_start_matches(' ').split(' ').collect();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < words.len() {
        let w = words[i];
        let (lead, core, trail) = split_punct(w);
        let lower = core.to_ascii_lowercase();
        // "build 1ae946f7+…", "image 3d7567c8": the word and the name go
        if (lower == "build" || lower == "image" || lower == "builds") && i + 1 < words.len() {
            let (_, c, t) = split_punct(words[i + 1]);
            if is_build(c) {
                let kept = format!("{lead}{t}");
                if !(kept.is_empty() || kept == "()" || kept == "[]") {
                    out.push(kept);
                }
                i += 2;
                continue;
            }
        }
        // "job 21823", "jobs 4 5 6", "Hand-over 31:", "lease 9": the word and its numbers go
        if NUMBERED.contains(&lower.as_str()) {
            let mut j = i + 1;
            let mut last_trail = trail.to_owned();
            while j < words.len() {
                let (_, c, t) = split_punct(words[j]);
                if !c.is_empty() && c.chars().all(|x| x.is_ascii_digit()) {
                    last_trail = t.to_owned();
                    j += 1;
                } else {
                    break;
                }
            }
            if j > i + 1 {
                if !lead.is_empty() || !last_trail.is_empty() {
                    out.push(format!("{lead}{last_trail}"));
                }
                i = j;
                continue;
            }
        }
        let replaced: Option<String> = if is_build(core) || is_code(core) {
            Some(String::new())
        } else if core == "FREE_REPORT_64" {
            Some("the 64".into())
        } else if core.starts_with("core.") || core.starts_with("vyreon.") {
            Some(String::new())
        } else {
            day(core)
        };
        match replaced {
            Some(r) if r.is_empty() => {
                // a lone bracket pair left by it goes too; other punctuation stays
                let kept = format!("{lead}{trail}");
                if !(kept == "()" || kept == "[]" || kept.is_empty()) {
                    out.push(kept);
                }
            }
            Some(r) => out.push(format!("{lead}{r}{trail}")),
            None => out.push(w.to_owned()),
        }
        i += 1;
    }
    let mut s = out.into_iter().filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ");
    let fixes = [("()", ""), ("[]", ""), (" ,", ","), (" .", "."), (" :", ":"), (" ;", ";"), ("( ", "("), (" )", ")"), ("(, ", "("), (", )", ")"), ("  ", " ")];
    loop {
        let before = s.clone();
        for (from, to) in fixes {
            s = s.replace(from, to);
        }
        if s == before {
            break;
        }
    }
    format!("{indent}{}", s.trim())
}

/// (leading punctuation, the word, trailing punctuation).
fn split_punct(w: &str) -> (&str, &str, &str) {
    let lead_end = w.find(|c: char| !matches!(c, '(' | '[' | '"' | '\'' | '`')).unwrap_or(w.len());
    let rest = &w[lead_end..];
    let core_end = rest.rfind(|c: char| !matches!(c, ')' | ']' | ',' | '.' | ':' | ';' | '"' | '\'' | '`' | '!' | '?')).map_or(0, |i| i + rest[i..].chars().next().map_or(1, char::len_utf8));
    (&w[..lead_end], &rest[..core_end], &rest[core_end..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_ids_and_iso_dates_go() {
        assert_eq!(day("2024-07-18").as_deref(), Some("Jul 18"));
        assert_eq!(day("2024-7-18"), None);
        assert_eq!(
            plain("Day 2024-09-05 failed for the last time and is set aside (job 21823): stop: refused (C-075)."),
            "Day Sep 5 failed for the last time and is set aside: stop: refused."
        );
        assert_eq!(plain("Build 1ae946f7+715623e13454 withdrawn: handed over to build 3d7567c8+aa11bb22cc33"), "withdrawn: handed over to");
        assert_eq!(plain("Day 2025-12-10 refused by the engine's input plan (build 3d7567c8+aa11bb22cc33): close_quotes"), "Day Dec 10 refused by the engine's input plan: close_quotes");
        assert_eq!(plain("jobs 4 5 6 cancelled for FREE_REPORT_64 in core.job"), "cancelled for the 64 in");
        assert_eq!(plain("   Queue resumed"), "   Queue resumed");
        // a command in backticks stays whole
        assert_eq!(plain("To do: read the plan: `vyreon-core recompute FREE_REPORT_64 3d7567c8+aa --plan`"), "To do: read the plan: `vyreon-core recompute FREE_REPORT_64 3d7567c8+aa --plan`");
        // quantities stay: a number after an ordinary word, a percentage, a time
        assert_eq!(plain("18 of 597 days done (3%) · 9 computing at 19:45"), "18 of 597 days done (3%) · 9 computing at 19:45");
        assert_eq!(plain("7 withheld by engine bugs (IBIT, ARKK)"), "7 withheld by engine bugs (IBIT, ARKK)");
    }
}
