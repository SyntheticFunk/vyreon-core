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
    let digits = |s: &str| s.len() >= 7 && s.len() <= 40 && s.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase());
    match t.split_once('+') {
        // C-100: "05837898+3d485555ec3d" (a commit of digits alone) is a build too
        Some((a, b)) => !b.is_empty() && b.chars().all(|c| c.is_ascii_hexdigit()) && (hex(a) || (digits(a) && b.chars().any(|c| c.is_ascii_alphabetic()))),
        None => hex(t),
    }
}

/// A decision code: C-077, D-465, P-30; C-100: or several as one word, "D-515..D-522", "C-069/C-070".
fn is_code(t: &str) -> bool {
    let one = |t: &str| {
        let mut parts = t.splitn(2, '-');
        matches!((parts.next(), parts.next()), (Some(p), Some(n)) if ["C", "D", "P", "E"].contains(&p) && !n.is_empty() && n.len() <= 4 && n.chars().all(|c| c.is_ascii_digit()))
    };
    !t.is_empty() && t.split("..").flat_map(|p| p.split('/')).all(one)
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
        // "build 1ae946f7+…", "image 3d7567c8": the name goes. C-100 (the posts of 2026-11-26/27: "Daily runs
        // stopped: was withdrawn"): the word stays as "the build" (the sentence keeps its subject or object),
        // except in a parenthesis of its own, "(build 3d7567c8+…)", which goes whole.
        if (lower == "build" || lower == "image" || lower == "builds") && i + 1 < words.len() {
            let (_, c, t) = split_punct(words[i + 1]);
            let (name, possessive) = match c.strip_suffix("'s") {
                Some(n) => (n, "'s"),
                None => (c, ""),
            };
            if is_build(name) {
                let alone = lead.ends_with('(') && t.starts_with(')');
                // "(job 21823, build 3d7567c8): …": the parenthesis the job's number left open closes on the build
                let closes = t.starts_with(')') && out.last().is_some_and(|w| w == "(," || w == "(");
                if alone {
                    let kept = format!("{}{}", &lead[..lead.len() - 1], &t[1..]);
                    if !kept.is_empty() {
                        out.push(kept);
                    }
                } else if closes {
                    out.pop();
                    if t.len() > 1 {
                        out.push(t[1..].to_owned());
                    }
                } else {
                    // "the" where the sentence has no article of its own: at a line's or a clause's start, or
                    // after a preposition ("Engine build X withdrawn" keeps "Engine build withdrawn")
                    let prev = out.last().map(String::as_str).unwrap_or("");
                    let prev_word = prev.trim_matches(|ch: char| !ch.is_alphanumeric()).to_ascii_lowercase();
                    let bare = prev.is_empty()
                        || prev.ends_with([':', ',', ';', '(', '.'])
                        || ["to", "from", "of", "by", "for", "on", "under", "with", "and", "or", "than", "as", "in", "until", "while", "when", "because"].contains(&prev_word.as_str());
                    let noun = if lower == "image" { "image" } else { "build" };
                    let article = match (bare, core.starts_with(|ch: char| ch.is_ascii_uppercase())) {
                        (false, _) => "",
                        (true, true) => "The ",
                        (true, false) => "the ",
                    };
                    out.push(format!("{lead}{article}{noun}{possessive}{t}"));
                }
                i += 2;
                continue;
            }
        }
        // "job 21823", "jobs 4 5 6", "Hand-over 31:", "lease 9": the word and its numbers go
        // (C-100: not when the word closes a clause of its own, "12 jobs: 3 failed": that number is a quantity)
        if NUMBERED.contains(&lower.as_str()) && trail.is_empty() {
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
                if lower == "hand-over" {
                    // C-100: the event keeps its name ("hand-over 25 to 2dde1132" read "to")
                    out.push(format!("{lead}{core}{last_trail}"));
                } else if !lead.is_empty() || !last_trail.is_empty() {
                    out.push(format!("{lead}{last_trail}"));
                }
                i = j;
                continue;
            }
        }
        // C-100: a bare build name the sentence needs: after a preposition ("hand-over to 2a8f8c4f" read
        // "hand-over to") or either side of an arrow ("fc36eefb → 2dde1132")
        let before = out.last().map(|w| w.trim_matches(|ch: char| !ch.is_alphanumeric() && ch != '→').to_ascii_lowercase()).unwrap_or_default();
        let after_arrow = words.get(i + 1).is_some_and(|n| *n == "→");
        let needed: Option<&str> = if !is_build(core) {
            None
        } else if after_arrow {
            Some("the old build")
        } else if before == "→" {
            Some("the new build")
        } else if before == "to" {
            Some("another build")
        } else if ["from", "of", "by", "for", "on", "under", "with"].contains(&before.as_str()) {
            Some("the build")
        } else {
            None
        };
        let replaced: Option<String> = if let Some(words) = needed {
            Some(words.to_owned())
        } else if is_build(core) || is_code(core) {
            Some(String::new())
        } else if core.strip_suffix("'s").is_some_and(is_build) {
            // C-100: "a51cb572's canary block finishes its day"
            Some("that build's".into())
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
    // C-100: a removed code's own punctuation goes with it ("release <build>: D-568: the …" read "release:: the …")
    let fixes = [
        ("(,)", ""), ("()", ""), ("[]", ""), (" ,", ","), (" .", "."), (" :", ":"), (" ;", ";"), ("( ", "("), (" )", ")"), ("(, ", "("), (", )", ")"),
        ("(:", "("), ("(;", "("), ("(])", ""), ("([])", ""), ("::", ":"), (": :", ":"), (":;", ":"), (";:", ":"), (",,", ","), (", ,", ","), (":,", ":"), (",:", ":"), (";;", ";"), ("; ;", ";"), (":.", "."), (",.", "."), (";.", "."),
        ("  ", " "),
    ];
    loop {
        let before = s.clone();
        for (from, to) in fixes {
            s = s.replace(from, to);
        }
        s = colon_before_note(&s);
        if s == before {
            break;
        }
    }
    // a line that a removed code left starting with punctuation: "D-568: the fix" read ": the fix"
    let s = s.trim().trim_start_matches([':', ';', ',']).trim_start();
    format!("{indent}{s}")
}

/// C-100: "release: (owner): the date gate retired" (a code went from between the colon and its note) reads
/// "release (owner): the date gate retired": a colon straight before a parenthesis that is itself followed by a
/// colon is dropped.
fn colon_before_note(s: &str) -> String {
    let mut from = 0;
    while let Some(found) = s[from..].find(": (") {
        let at = from + found;
        let rest = &s[at + 3..];
        if let Some(close) = rest.find(')')
            && rest[close + 1..].starts_with(':')
            && !rest[..close].contains('(')
        {
            return format!("{}{}", &s[..at], &s[at + 1..]);
        }
        from = at + 3;
    }
    s.to_owned()
}

/// C-091 (owner, 2026-11-26: "no commands, secret names, file paths, job ids or decision codes in posts"): the
/// alerts channel's text: `plain`, and before it every command in backticks taken out, every parenthesis that
/// names a file, a setting or a secret, and any such word left. The commands stay behind `vyreon-core problems`.
pub fn for_alert(text: &str) -> String {
    // the backtick spans, and a "(`...`)" or ": `...`" around one
    let mut out = String::new();
    let mut parts = text.split('`');
    if let Some(first) = parts.next() {
        out.push_str(first);
    }
    for (i, part) in parts.enumerate() {
        if i % 2 == 1 {
            out.push_str(part);
        }
    }
    let out = remove_parens(&out);
    let words: Vec<String> = out
        .lines()
        .map(|line| line.split(' ').filter(|w| !internal(w)).collect::<Vec<_>>().join(" "))
        .collect();
    plain(&words.join("\n")).lines().map(str::trim_end).collect::<Vec<_>>().join("\n")
}

/// A word naming a file, a path, a setting or a secret.
fn internal(word: &str) -> bool {
    let (_, w, _) = split_punct(word);
    w.contains("README")
        || w.ends_with(".md")
        || w.ends_with(".yaml")
        || w.ends_with(".sh")
        || w.starts_with("deploy/")
        || w.starts_with("vyreon-core-")
        || w == "kubectl"
        || (w.starts_with("VYREON_") && w.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'))
}

/// `text` without the parentheses whose words name a file, a setting or a secret (`internal`).
fn remove_parens(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(open) = rest.find('(') {
        // its matching close, nested ones inside
        let mut depth = 0;
        let mut close = None;
        for (i, ch) in rest[open..].char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        close = Some(open + i);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(close) = close else { break };
        let inner = &rest[open + 1..close];
        out.push_str(&rest[..open]);
        if inner.split(' ').any(internal) || inner.trim().is_empty() {
            // the space before it goes with it
            while out.ends_with(' ') {
                out.pop();
            }
        } else {
            out.push_str(&rest[open..=close]);
        }
        rest = &rest[close + 1..];
    }
    out.push_str(rest);
    out
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

    /// C-091: the alerts carry no command, file, setting or secret name (the posts of 2026-11-26 named the
    /// secret and the README).
    #[test]
    fn an_alert_names_no_command_file_or_secret() {
        assert_eq!(
            for_alert("🔴 Needs you: the spend is not known (no DigitalOcean token: the secret vyreon-core-do-billing is not made (deploy/k8s/README.md, Spend)), so the ceiling is not watched. To do: `vyreon-core spend`"),
            "🔴 Needs you: the spend is not known, so the ceiling is not watched. To do:"
        );
        assert_eq!(for_alert("The watch cannot read the WAL: the core login lacks pg_monitor (deploy/k8s/README.md, Backups B0)"), "The watch cannot read the WAL: the core login lacks pg_monitor");
        assert_eq!(for_alert("Backups unverified: never read (VYREON_CORE_PG_CLUSTER, its Role: README B0)"), "Backups unverified: never read");
        assert_eq!(for_alert("⚠️ Problem: Day 2024-09-05 set aside (job 21823): stop: refused (C-075)."), "⚠️ Problem: Day Sep 5 set aside: stop: refused.");
        assert_eq!(for_alert("18 of 597 days (3%) at 19:45"), "18 of 597 days (3%) at 19:45");
    }

    /// C-100: the posts of 2026-11-26 and 11-27 as core composed them (`core.problem.summary`, verbatim), which
    /// read "release: (owner): the date gate…", "release:: the evening audit's…" and "Daily runs stopped: was
    /// withdrawn": a removed code takes its own punctuation with it, and a removed build's name leaves "the
    /// build" where the sentence needs its subject or object.
    #[test]
    fn a_removed_code_or_build_leaves_a_readable_sentence() {
        assert_eq!(
            for_alert("Work queue paused, nothing is handed out: release c2397caa+3933cc2a6d95: D-567 (owner): the date gate on replay quotes retired; a51cb572's canary block finishes its day"),
            "Work queue paused, nothing is handed out: release (owner): the date gate on replay quotes retired; that build's canary block finishes its day"
        );
        assert_eq!(
            for_alert("Work queue paused, nothing is handed out: release 760a8760+93486ee1fd46: D-568: the evening audit's real engine findings fixed (operations only); the two test days' pods finish their reference checks"),
            "Work queue paused, nothing is handed out: release: the evening audit's real engine findings fixed (operations only); the two test days' pods finish their reference checks"
        );
        assert_eq!(
            for_alert("Daily runs stopped: build a51cb572+acffd2a117e3 was withdrawn, so no new session is queued"),
            "Daily runs stopped: the build was withdrawn, so no new session is queued"
        );
        // others of the same week, from the problems and the log
        assert_eq!(
            for_alert("Work queue paused, nothing is handed out: release a51cb572+acffd2a117e3: stage 1 + the evening's operations batch (D-515..D-522, D-560..D-566); canary next"),
            "Work queue paused, nothing is handed out: release: stage 1 + the evening's operations batch; canary next"
        );
        assert_eq!(for_alert("Work queue paused, nothing is handed out: hand-over to fc36eefb (D-459)"), "Work queue paused, nothing is handed out: hand-over to another build");
        assert_eq!(for_alert("Work queue paused, nothing is handed out: hand-over 25 to 2dde1132"), "Work queue paused, nothing is handed out: hand-over to another build");
        assert_eq!(for_alert("Work queue paused, nothing is handed out: core deploy (C-069/C-070)"), "Work queue paused, nothing is handed out: core deploy");
        assert_eq!(plain("Daily run: session 2026-11-25 queued for build 714e47a3+5b2beb37d0c5 (jobs [30061, 30062, 30063])."), "Daily run: session Nov 25 queued for the build.");
        assert_eq!(
            plain("Engine build 05837898+3d485555ec3d withdrawn: replaced by the phase-1 image 914ffed7+7d4e45c05f63 (C-062: withdraw a build when its image is replaced)."),
            "Engine build withdrawn: replaced by the phase-1 image (withdraw a build when its image is replaced)."
        );
        assert_eq!(plain("Hand-over 26: fc36eefb → 2dde1132, 42 blocks moved"), "Hand-over: the old build → the new build, 42 blocks moved");
        assert_eq!(for_alert("Day 2025-12-10 refused by the engine's input plan (job 30012, build 3d7567c8): close_quotes"), "Day Dec 10 refused by the engine's input plan: close_quotes");
        // a parenthesis of its own still goes whole, a quantity stays
        assert_eq!(plain("Day 2025-12-10 refused by the engine's input plan (build 3d7567c8+aa11bb22cc33): close_quotes"), "Day Dec 10 refused by the engine's input plan: close_quotes");
        assert_eq!(plain("8 builds ran 12 jobs: 3 failed"), "8 builds ran 12 jobs: 3 failed");
    }

    #[test]
    fn codes_ids_and_iso_dates_go() {
        assert_eq!(day("2024-07-18").as_deref(), Some("Jul 18"));
        assert_eq!(day("2024-7-18"), None);
        assert_eq!(
            plain("Day 2024-09-05 failed for the last time and is set aside (job 21823): stop: refused (C-075)."),
            "Day Sep 5 failed for the last time and is set aside: stop: refused."
        );
        // C-100: a build's name goes, the sentence keeps its subject and object
        assert_eq!(plain("Build 1ae946f7+715623e13454 withdrawn: handed over to build 3d7567c8+aa11bb22cc33"), "The build withdrawn: handed over to the build");
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
