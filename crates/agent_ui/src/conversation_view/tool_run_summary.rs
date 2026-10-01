//! Wording for runs of tool calls folded into one line of the agent transcript,
//! e.g. "Searched code, read 3 files" or "Edited main.rs".

/// What a tool call did, as far as its wording is concerned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ToolVerb {
    Read,
    Edit,
    Delete,
    Move,
    Search,
    Execute,
    Fetch,
    Think,
    SwitchMode,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ToolSummaryItem {
    pub verb: ToolVerb,
    /// The file name for file tools, the command for commands, otherwise the tool's title.
    pub subject: Option<String>,
    pub running: bool,
}

const MAX_SUBJECT_CHARS: usize = 80;

/// The first non-empty line of `text`, cut to a length that fits on one line.
pub(crate) fn short_subject(text: &str) -> Option<String> {
    let line = text.lines().map(str::trim).find(|line| !line.is_empty())?;
    if line.chars().count() <= MAX_SUBJECT_CHARS {
        return Some(line.to_string());
    }
    let cut = line.chars().take(MAX_SUBJECT_CHARS - 1).collect::<String>();
    Some(format!("{}…", cut.trim_end()))
}

/// One line for a whole run. Finished tools are counted per kind in the order they
/// first appear; tools still running follow in the present tense.
pub(crate) fn run_summary(items: &[ToolSummaryItem]) -> String {
    let mut groups: Vec<Vec<&ToolSummaryItem>> = Vec::new();
    for item in items.iter().filter(|item| !item.running) {
        match groups
            .iter_mut()
            .find(|group| group.first().is_some_and(|first| same_group(first, item)))
        {
            Some(group) => group.push(item),
            None => groups.push(vec![item]),
        }
    }
    let phrases = groups
        .iter()
        .filter_map(|group| past_phrase(group))
        .chain(items.iter().filter(|item| item.running).map(present_phrase))
        .collect::<Vec<_>>();
    capitalize(&phrases.join(", "))
}

/// The line for one tool inside an opened run.
pub(crate) fn member_line(item: &ToolSummaryItem) -> String {
    let line = match item.verb {
        ToolVerb::Read | ToolVerb::Edit | ToolVerb::Delete | ToolVerb::Move => {
            if item.running {
                present_phrase(item)
            } else {
                past_phrase(&[item]).unwrap_or_default()
            }
        }
        ToolVerb::Execute => {
            let command = item.subject.as_deref().unwrap_or("a command");
            if item.running {
                format!("running {command}")
            } else {
                format!("ran {command}")
            }
        }
        _ => match &item.subject {
            Some(subject) => subject.clone(),
            None if item.running => present_phrase(item),
            None => past_phrase(&[item]).unwrap_or_default(),
        },
    };
    capitalize(&line)
}

/// Tools of other kinds are told apart by name, so "used X, used Y" stays readable.
fn same_group(first: &ToolSummaryItem, item: &ToolSummaryItem) -> bool {
    first.verb == item.verb && (item.verb != ToolVerb::Other || first.subject == item.subject)
}

fn past_phrase(group: &[&ToolSummaryItem]) -> Option<String> {
    let first = group.first()?;
    let count = group.len();
    let subject = if count == 1 {
        first.subject.as_deref()
    } else {
        None
    };
    Some(match first.verb {
        ToolVerb::Read => file_phrase("read", subject, count),
        ToolVerb::Edit => file_phrase("edited", subject, count),
        ToolVerb::Delete => file_phrase("deleted", subject, count),
        ToolVerb::Move => file_phrase("moved", subject, count),
        ToolVerb::Search if count == 1 => "searched code".to_string(),
        ToolVerb::Search => format!("ran {count} searches"),
        ToolVerb::Execute if count == 1 => "ran a command".to_string(),
        ToolVerb::Execute => format!("ran {count} commands"),
        ToolVerb::Fetch if count == 1 => "fetched a page".to_string(),
        ToolVerb::Fetch => format!("fetched {count} pages"),
        ToolVerb::Think if count == 1 => "thought".to_string(),
        ToolVerb::Think => format!("thought {count} times"),
        ToolVerb::SwitchMode => "switched mode".to_string(),
        ToolVerb::Other => match (first.subject.as_deref(), count) {
            (Some(name), 1) => format!("used {name}"),
            (Some(name), count) => format!("used {name} {count} times"),
            (None, 1) => "used a tool".to_string(),
            (None, count) => format!("used {count} tools"),
        },
    })
}

fn file_phrase(verb: &str, subject: Option<&str>, count: usize) -> String {
    match (subject, count) {
        (Some(file), _) => format!("{verb} {file}"),
        (None, 1) => format!("{verb} a file"),
        (None, count) => format!("{verb} {count} files"),
    }
}

fn present_phrase(item: &ToolSummaryItem) -> String {
    let subject = item.subject.as_deref();
    match item.verb {
        ToolVerb::Read => format!("reading {}", subject.unwrap_or("a file")),
        ToolVerb::Edit => format!("editing {}", subject.unwrap_or("a file")),
        ToolVerb::Delete => format!("deleting {}", subject.unwrap_or("a file")),
        ToolVerb::Move => format!("moving {}", subject.unwrap_or("a file")),
        ToolVerb::Search => "searching code".to_string(),
        ToolVerb::Execute => format!("running {}", subject.unwrap_or("a command")),
        ToolVerb::Fetch => format!("fetching {}", subject.unwrap_or("a page")),
        ToolVerb::Think => "thinking".to_string(),
        ToolVerb::SwitchMode => "switching mode".to_string(),
        ToolVerb::Other => format!("using {}", subject.unwrap_or("a tool")),
    }
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(verb: ToolVerb, subject: Option<&str>) -> ToolSummaryItem {
        ToolSummaryItem {
            verb,
            subject: subject.map(str::to_string),
            running: false,
        }
    }

    fn running(verb: ToolVerb, subject: Option<&str>) -> ToolSummaryItem {
        ToolSummaryItem {
            running: true,
            ..item(verb, subject)
        }
    }

    #[test]
    fn counts_files_of_one_kind() {
        let items = (0..9)
            .map(|_| item(ToolVerb::Read, Some("a.rs")))
            .collect::<Vec<_>>();
        assert_eq!(run_summary(&items), "Read 9 files");
    }

    #[test]
    fn names_the_file_of_a_single_edit() {
        assert_eq!(
            run_summary(&[item(ToolVerb::Edit, Some("globals.css"))]),
            "Edited globals.css"
        );
        assert_eq!(run_summary(&[item(ToolVerb::Edit, None)]), "Edited a file");
    }

    #[test]
    fn joins_kinds_in_first_seen_order() {
        let items = [
            item(ToolVerb::Search, Some("grep foo")),
            item(ToolVerb::Read, Some("a.rs")),
            item(ToolVerb::Search, Some("grep bar")),
            item(ToolVerb::Execute, Some("cargo test")),
        ];
        assert_eq!(
            run_summary(&items),
            "Ran 2 searches, read a.rs, ran a command"
        );
    }

    #[test]
    fn running_tools_follow_in_the_present_tense() {
        let items = [
            item(ToolVerb::Read, Some("a.rs")),
            item(ToolVerb::Read, Some("b.rs")),
            running(ToolVerb::Execute, Some("cargo test")),
        ];
        assert_eq!(run_summary(&items), "Read 2 files, running cargo test");
        assert_eq!(
            run_summary(&[running(ToolVerb::Read, Some("package.json"))]),
            "Reading package.json"
        );
    }

    #[test]
    fn other_tools_are_told_apart_by_name() {
        let items = [
            item(ToolVerb::Other, Some("ToolSearch")),
            item(ToolVerb::Other, Some("preview start")),
            item(ToolVerb::Other, Some("ToolSearch")),
            item(ToolVerb::Other, None),
        ];
        assert_eq!(
            run_summary(&items),
            "Used ToolSearch 2 times, used preview start, used a tool"
        );
    }

    #[test]
    fn member_lines_describe_one_tool() {
        assert_eq!(
            member_line(&item(ToolVerb::Read, Some("a.rs"))),
            "Read a.rs"
        );
        assert_eq!(
            member_line(&item(ToolVerb::Execute, Some("cargo test"))),
            "Ran cargo test"
        );
        assert_eq!(
            member_line(&running(ToolVerb::Execute, Some("cargo test"))),
            "Running cargo test"
        );
        assert_eq!(
            member_line(&item(ToolVerb::Search, Some("grep foo in src"))),
            "Grep foo in src"
        );
        assert_eq!(member_line(&item(ToolVerb::Fetch, None)), "Fetched a page");
    }

    #[test]
    fn short_subject_keeps_one_line() {
        assert_eq!(
            short_subject("\n  cargo test\nmore"),
            Some("cargo test".into())
        );
        assert_eq!(short_subject("   \n"), None);
        let long = "x".repeat(200);
        let short = short_subject(&long).unwrap_or_default();
        assert_eq!(short.chars().count(), MAX_SUBJECT_CHARS);
        assert!(short.ends_with('…'));
    }
}
