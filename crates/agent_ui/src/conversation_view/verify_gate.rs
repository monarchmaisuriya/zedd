//! Checks the agent's work with the project's verification command after its turns.

use std::{path::PathBuf, time::Duration};

use gpui::{App, Entity, Task};
use project::{Project, project_settings::ProjectSettings, trusted_worktrees::TrustedWorktrees};
use settings::{Settings as _, SettingsLocation};
use util::rel_path::RelPath;

/// The most output, from its end, that a fix request quotes.
const FIX_PROMPT_OUTPUT_CHARS: usize = 4000;

/// A thread's verification state: whether it is on, whether a check is running, and whether the
/// agent was already asked to fix a failure since the user's last message.
pub(crate) struct VerifyGate {
    pub(crate) enabled: bool,
    fix_requested: bool,
    pub(crate) running: Option<Task<()>>,
}

pub(crate) enum VerifyOutcome {
    Passed,
    Failed { summary: String, output: String },
}

/// What a check runs, read from the settings of the project's first worktree.
pub(crate) struct VerifyCommand {
    pub(crate) command: String,
    pub(crate) timeout: Duration,
    pub(crate) cwd: PathBuf,
}

impl VerifyGate {
    pub(crate) fn new(project: Option<&Entity<Project>>, cx: &App) -> Self {
        Self {
            enabled: project
                .and_then(|project| project_verification_settings(project, cx))
                .is_some_and(|settings| settings.enabled_by_default),
            fix_requested: false,
            running: None,
        }
    }

    /// A message from the user starts a new round, in which the agent may be asked to fix once.
    pub(crate) fn user_sent_message(&mut self) {
        self.fix_requested = false;
    }

    /// The prompt asking the agent to fix a failed check, at most once per user message.
    pub(crate) fn fix_prompt(&mut self, command: &str, outcome: &VerifyOutcome) -> Option<String> {
        let VerifyOutcome::Failed { summary, output } = outcome else {
            return None;
        };
        if std::mem::replace(&mut self.fix_requested, true) {
            return None;
        }
        let skip = output
            .chars()
            .count()
            .saturating_sub(FIX_PROMPT_OUTPUT_CHARS);
        let tail: String = output.chars().skip(skip).collect();
        Some(format!(
            "The verification command `{command}` {summary}. Fix the cause. The command runs \
             again when you finish.\n\nEnd of its output:\n```\n{tail}\n```"
        ))
    }
}

/// The command to check the project with, when one is configured and the project is trusted to
/// run it.
pub(crate) fn verify_command(project: &Entity<Project>, cx: &App) -> Option<VerifyCommand> {
    let settings = project_verification_settings(project, cx)?;
    let command = settings.command.clone()?;
    let worktree_store = project.read(cx).worktree_store();
    if TrustedWorktrees::has_restricted_worktrees(&worktree_store, cx) {
        return None;
    }
    let cwd = project
        .read(cx)
        .visible_worktrees(cx)
        .next()?
        .read(cx)
        .abs_path()
        .to_path_buf();
    Some(VerifyCommand {
        command,
        timeout: settings.timeout,
        cwd,
    })
}

/// The configured command, whether or not the project is trusted to run it.
pub(crate) fn configured_command(project: &Entity<Project>, cx: &App) -> Option<String> {
    project_verification_settings(project, cx)?.command.clone()
}

fn project_verification_settings<'a>(
    project: &Entity<Project>,
    cx: &'a App,
) -> Option<&'a project::project_settings::AgentVerificationSettings> {
    let worktree = project.read(cx).visible_worktrees(cx).next()?;
    let location = SettingsLocation {
        worktree_id: worktree.read(cx).id(),
        path: RelPath::empty(),
    };
    Some(&ProjectSettings::get(Some(location), cx).agent_verification)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gate() -> VerifyGate {
        VerifyGate {
            enabled: true,
            fix_requested: false,
            running: None,
        }
    }

    fn failure() -> VerifyOutcome {
        VerifyOutcome::Failed {
            summary: "failed with exit code 1".to_string(),
            output: "test result: FAILED".to_string(),
        }
    }

    #[test]
    fn test_fix_is_requested_once_per_user_message() {
        let mut gate = gate();

        let prompt = gate
            .fix_prompt("cargo test", &failure())
            .expect("the first failure asks for a fix");
        assert!(prompt.contains("`cargo test` failed with exit code 1"));
        assert!(prompt.contains("test result: FAILED"));
        assert!(
            gate.fix_prompt("cargo test", &failure()).is_none(),
            "a second failure gives up instead of looping"
        );

        gate.user_sent_message();
        assert!(gate.fix_prompt("cargo test", &failure()).is_some());
    }

    #[test]
    fn test_passing_check_asks_for_nothing() {
        let mut gate = gate();
        assert!(
            gate.fix_prompt("cargo test", &VerifyOutcome::Passed)
                .is_none()
        );
        assert!(
            gate.fix_prompt("cargo test", &failure()).is_some(),
            "a pass does not use up the fix attempt"
        );
    }

    #[test]
    fn test_fix_prompt_quotes_only_the_end_of_long_output() {
        let mut gate = gate();
        let output = format!("{}END", "x".repeat(FIX_PROMPT_OUTPUT_CHARS * 2));
        let prompt = gate
            .fix_prompt(
                "make check",
                &VerifyOutcome::Failed {
                    summary: "timed out".to_string(),
                    output,
                },
            )
            .unwrap();
        assert!(prompt.contains("END"));
        assert!(prompt.len() < FIX_PROMPT_OUTPUT_CHARS + 500);
    }
}
