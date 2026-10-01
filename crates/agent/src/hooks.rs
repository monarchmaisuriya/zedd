//! Runs the user's agent hooks: shell commands that receive an agent event as JSON on stdin.

use std::path::{Path, PathBuf};

use agent_settings::AgentHook;
use futures::{AsyncWriteExt as _, FutureExt as _};
use gpui::BackgroundExecutor;
use util::command::{Stdio, new_command};

/// The most tool output, in characters, that a post-tool hook receives.
pub(crate) const HOOK_TOOL_OUTPUT_CHARS: usize = 64 * 1024;

/// The result of running every hook for one event.
#[derive(Debug, PartialEq)]
pub(crate) enum HooksOutcome {
    /// Every hook succeeded; their combined standard output, which may be empty.
    Passed(String),
    /// A hook failed; why, in words shown to the agent or the user.
    Failed(String),
}

/// The hooks among `hooks` that apply to `tool_name`.
pub(crate) fn hooks_for_tool(hooks: &[AgentHook], tool_name: &str) -> Vec<AgentHook> {
    hooks
        .iter()
        .filter(|hook| hook.applies_to(tool_name))
        .cloned()
        .collect()
}

/// Runs `hooks` one after another with `event` on stdin, stopping at the first that fails.
pub(crate) async fn run_hooks(
    hooks: Vec<AgentHook>,
    event: serde_json::Value,
    cwd: Option<PathBuf>,
    executor: BackgroundExecutor,
) -> HooksOutcome {
    let mut stdout = String::new();
    for hook in &hooks {
        match run_hook(hook, &event, cwd.as_deref(), &executor).await {
            Ok(output) => stdout.push_str(&output),
            Err(reason) => {
                return HooksOutcome::Failed(format!("hook `{}` {reason}", hook.command));
            }
        }
    }
    HooksOutcome::Passed(stdout)
}

async fn run_hook(
    hook: &AgentHook,
    event: &serde_json::Value,
    cwd: Option<&Path>,
    executor: &BackgroundExecutor,
) -> Result<String, String> {
    let (shell, command_flag) = if cfg!(windows) {
        ("cmd", "/C")
    } else {
        ("/bin/sh", "-c")
    };
    let mut command = new_command(shell);
    command
        .arg(command_flag)
        .arg(&hook.command)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    // A remote project's root does not exist on this machine; such hooks run in Zed's directory.
    if let Some(cwd) = cwd.filter(|cwd| cwd.is_dir()) {
        command.current_dir(cwd);
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("could not start: {error}"))?;
    let payload = event.to_string();
    let run = async move {
        if let Some(stdin) = child.stdin.as_mut()
            && let Err(error) = stdin.write_all(payload.as_bytes()).await
        {
            // A hook may exit without reading its input; its exit status decides the outcome.
            log::debug!("agent hook did not read its input: {error}");
        }
        child.output().await
    };
    let output = futures::select_biased! {
        output = run.fuse() => output.map_err(|error| format!("could not run: {error}"))?,
        _ = executor.timer(hook.timeout).fuse() => {
            return Err(format!("timed out after {} s", hook.timeout.as_secs()));
        }
    };
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let message = if stderr.trim().is_empty() {
        stdout.trim()
    } else {
        stderr.trim()
    };
    Err(match output.status.code() {
        Some(code) => format!("failed with exit code {code}: {message}"),
        None => format!("was stopped by a signal: {message}"),
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use gpui::TestAppContext;
    use std::time::Duration;

    fn hook(command: &str) -> AgentHook {
        AgentHook {
            matcher: None,
            command: command.to_string(),
            timeout: Duration::from_secs(10),
        }
    }

    #[gpui::test]
    async fn test_hooks_receive_the_event_and_pass_their_output(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let outcome = run_hooks(
            vec![hook("cat"), hook("echo second")],
            serde_json::json!({ "hook_event_name": "pre_tool_use" }),
            None,
            cx.executor(),
        )
        .await;
        assert_eq!(
            outcome,
            HooksOutcome::Passed("{\"hook_event_name\":\"pre_tool_use\"}second\n".to_string())
        );
    }

    #[gpui::test]
    async fn test_first_failing_hook_stops_the_rest(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let outcome = run_hooks(
            vec![hook("echo nope >&2; exit 2"), hook("echo never")],
            serde_json::json!({}),
            None,
            cx.executor(),
        )
        .await;
        assert_eq!(
            outcome,
            HooksOutcome::Failed(
                "hook `echo nope >&2; exit 2` failed with exit code 2: nope".to_string()
            )
        );
    }
}
