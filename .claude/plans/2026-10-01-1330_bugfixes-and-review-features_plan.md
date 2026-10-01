# Fix 8 upstream Zed bugs and add 6 review/supervision features

- Date: 2026-10-01 13:30
- Status: parts 1-13 done; part 14 waits on the upstream adapter PR (M approved that order)
- Repos: zedd   Branch: zedd-bugfixes-and-review-features (tree = main e7e3079c)
- Folders: crates/acp_thread, crates/agent_servers, crates/agent_ui, crates/agent, crates/util, crates/fs, crates/zed, crates/sidebar, crates/editor, crates/git_ui, crates/feature_flags, crates/settings_content, crates/project, crates/settings_ui, assets
- Subagents: six read-only design agents (done); implementation sequential
- Research: .claude/research/2026-10-01-1300_feature-gaps-and-zed-issues_research.md

## Context and problem

M asked to plan and implement all eight Zed bugs from the research and the top six features, using the recommended options for every decision.

## Goals / Non-goals

Goals: each bug fixed at its owning layer with a regression test; each feature working for the native agent and ACP agents where the data exists.
Non-goals: Windows-specific paths (safe save, process termination stay as today on Windows); Linux PR_SET_PDEATHSIG; upstream adapter changes (proposed to M separately); project-level agent hooks.

## Decisions taken on M's behalf ("go with your recommendations")

- #64410: base snapshot refreshed after format-on-save; overlapping concurrent change rejects the write.
- #62828: push "closed" signal from the connection; store evicts with identity guard.
- #64538: unknown terminal renders a placeholder; rest of the update applies.
- #60435: `AgentConnection::persists_draft_prompt`; one reader in ConversationView.
- #59323: SIGTERM, 2 s grace, SIGKILL; accept the quit-path limit.
- #63177: unix rename path with in-place fallbacks; Windows unchanged.
- #55726: restore last session, then open the file; `--wait` connections excluded.
- #63202: per-row cached views with explicit heights; no fps stopgap.
- Diff comments: `diff-review` flag enabled for everyone; also fix the dead "Send Review to Agent" button.
- Plan document: view-only (link + open file); editing needs an adapter fix (ask M).
- Background tasks: zedd side built (parser, model, strip, Stop); AIR mode not declared; adapter needs a non-AIR opt-in (ask M). Finished tasks stay listed.
- Verify gate: project key `agent_verification`, worktree trust required, runs only after turns that used tools, one fix attempt.
- Hooks: native agent, user settings only; pre_tool_use failure denies; hooks never skip Zed's permission prompt.

## What is true today

Per item, verified by the design agents with file:line; see "Design notes" per part. Highlights: `write_text_file` never refreshes `shared_buffers` [verified: acp_thread.rs:6135-6211]; `request_connection` returns any cached entry [verified: agent_connection_store.rs:148]; unknown terminal id is an error in `prepare`/`prepare_v2` [verified: acp_thread.rs:2883, 2908]; draft store gated on `is_draft_thread` [verified: conversation_view.rs:1919-1938]; `Child::kill` is SIGKILL only [verified: util/process.rs:115-121]; `Fs::save` truncates first [verified: fs.rs:1019-1035]; open-request launch skips restore [verified: main.rs:926-951]; sidebar spinner re-renders the whole list [verified: window.rs:2148-2159, list.rs:1072]; editor review comments exist but the agent diff never enables them and `SendReviewToAgent` has no handler [verified: git.rs, actions.rs:929]; ReviewBranchDiff flow exists [verified: agent_panel.rs:526-567]; ExitPlanMode raw_input carries `planFilePath` [verified: adapter renderer.ts:250-262]; agent settings are not read from project settings [verified: settings_store.rs:1111-1140].

## Design

Order: bugs (small to large), then features (small to large). Each part: write-scope rescan, change, tests (one at a time), mutation check where practical, suite + clippy + rustfmt check.

## Risks and mitigations

| Risk | Mitigation |
| --- | --- |
| Large surface in one branch | Parts are independent; each verified before the next |
| Safe save changes file identity | Rename only when identity can be preserved; otherwise in-place |
| Sidebar row heights drift | One height function in ui; test asserts row render counts |
| Verify / hooks run commands | Trusted worktrees only for project commands; hooks from user settings only |
| Background tasks dormant | Stated in report; upstream PR proposed |

## Parts

### Part 1: #64410 ACP write duplicates tokens
- [x] Conflict check + post-write snapshot in `write_text_file` (done when: two new tests pass, each fails without its half)
### Part 2: #64538 unknown terminal placeholder
- [x] `PreparedToolCallContent::UnavailableTerminal`; rewrite the tests that used a missing terminal as a failure trigger; placeholder tests
### Part 3: #62828 evict dead connection
- [x] `AgentConnection::closed`; AcpConnection signal; store eviction; test
### Part 4: #59323 graceful agent termination
- [x] `Child::terminate_gracefully`; AcpConnection drop; unix tests
### Part 5: #60435 unsent ACP prompt survives restart
- [x] `persists_draft_prompt`; `store_owns_draft`; one reader; restore test
### Part 6: #63177 safe save
- [x] `replace_file_contents` for save/write (unix); fs integration tests
### Part 7: #55726 restore session on file-open launch
- [x] `restore_last_session` split; open-request branch; zed test
### Part 8: #63202 sidebar per-row cached views
- [x] `SidebarRow` cached views; row heights; render-count test (M chose option A)
### Part 9: Branch review
- [x] Bug/security-only prompt builder; `git::ReviewBranch` command + git panel item; tests
### Part 10: Diff comments to the agent
- [x] Enable `diff-review`; agent diff comments + send; fix `SendReviewToAgent`; keybindings; tests
### Part 11: Plan document (view-only)
- [x] `ToolCall::plan_file_path`; open on request; "Plan: file ›" row; tests
### Part 12: Verify gate
- [x] `agent_verification` project setting; `VerifyGate`; terminal run; one fix prompt; toggle; tests
### Part 13: Native agent hooks
- [x] `agent.hooks` settings; runner; pre/post tool, prompt submit, stop; tests
### Part 14: Background tasks (zedd side)
- [ ] Untyped `async_task_*` parser; `BackgroundTask` model; activity-bar strip; Stop via `_session/async_task/stop`; tests (deferred: dead code until the adapter has a non-AIR opt-in, see Open questions)

## Standardized review

- **Bottom line:** executable; two items depend on upstream adapter changes that need M's OK (plan editing, background-task events) and are scoped as view-only / dormant.
- Goal fidelity: pass. Current-system accuracy: pass (design agents' file:line evidence; #64410 re-verified in code and tests). Scope: pass with note: 14 parts is large; parts are independent. Architecture: pass (each fix at its owning layer, named per part). Sequencing: pass. Completeness: pass (settings, keymaps, tests per part). Failure and recovery: pass (fallback paths for save; trust gating; fail-closed hooks). Verification: pass (test per part). Feasibility: must-verify: GPUI cached rows (part 8) and real processes in GPUI tests (part 12) need a spike. Decision readiness: pass (decisions recorded above; upstream PRs deferred to M).

## Changes made

- Part 1: `write_text_file` rejects a write whose hunks overlap a change made since the agent's last read ("changed since it was last read") and stores the post-write (post-format) snapshot as the new base. Tests `test_consecutive_writes_without_read_do_not_duplicate`, `test_write_rejects_change_overlapping_since_read` (each fails without its half); `test_edits_concurrently_to_user` still passes.
- Part 2: an unknown terminal id prepares to `UnavailableTerminal`, rendered as "Terminal output is no longer available."; the rest of the update applies; `prepare_v2` is infallible. Deviation: 7 tests (not 3) used a missing terminal only to force a failure; no other real input can fail conversion (v1 `Role` and `EmbeddedResourceResource` are closed), so they now assert the new contract and their status/permission/sleep-prevention properties, renamed to describe the behavior. `acp_thread` 220; clippy and rustfmt clean.

- Part 3: `AgentConnection::closed()` resolves when the ACP transport closes or the agent process exits (`AcpConnection` fills it from `io_task` and `wait_task`); `AgentConnectionStore` evicts the cached entry (identity-guarded) so the next thread starts a fresh process. Test `test_acp_server_exit_evicts_cached_connection` (fails with eviction disabled). Deviation: `test_acp_server_exit_transitions_conversation_to_load_error_without_panic` now holds the connection, because the fake's in-process agent lives only as long as the wrapper the store held; in production the exited process cannot receive the close anyway. The fake's exit now fires once, like a real process. agent_ui 505, agent_servers 46, acp_thread 220; clippy and rustfmt clean.
- Part 4: `util::process::Child::terminate_gracefully(grace)` sends SIGTERM to the process group, then SIGKILLs the group when the process exits or the grace ends (Windows: kills immediately, as before). `AcpConnection::drop` uses it with a 2 s grace on the background executor. Tests `test_terminate_gracefully_lets_process_handle_sigterm` (fails when the first signal is SIGKILL) and `test_terminate_gracefully_kills_process_ignoring_sigterm`. Known limit (accepted in Decisions): if zedd quits within the grace period, the SIGKILL escalation may not run, so an agent that ignores SIGTERM can outlive zedd. util 139, agent_servers 46; clippy and rustfmt clean.
- Part 5: `AgentConnection::persists_draft_prompt()` (true only for the native agent, which saves it in its thread database) and `draft_prompt_store::store_owns_draft(thread)` decide who keeps an unsent prompt; the writer, the promotion delete and the single load-time reader in `ConversationView` all use it, and the two panel reads were removed. Test `test_unsent_prompt_survives_reload_for_agent_without_draft_storage` (fails with ownership limited to drafts, and fails without the load-time read). Deviation: the test lives in conversation_view, not agent_panel, because `Agent::Stub.server()` builds a fresh stub that cannot load sessions (`agent_ui.rs:495`) and ignores `set_stub_agent_connection`, so a panel reload test cannot reach a loaded thread. agent_ui 506, agent 761, sidebar 156, acp_thread 220; clippy and rustfmt clean.
- Part 6: `RealFs::save` and `RealFs::write` go through `replace_file_contents`: on unix the new bytes go to a temporary file beside the original (mode, owner and on macOS ACLs/xattrs copied), are fsynced, then renamed over it; symlinks keep their link. In-place fallback for new files, non-regular files, hard-linked files, directories that refuse new files, and owner/metadata copy failures; a read-only file still refuses. Windows writes in place as before. Tests: replace keeps mode and never touches the old inode (fails when always in place), hard link stays shared (fails without the link guard), symlink kept, read-only refused, missing file created. Known limit: on Linux, extended attributes and SELinux labels are not copied to the replacement. fs and worktree suites pass; clippy and rustfmt clean.
- Part 7: `restore_last_session` (the restore loop, extracted from `restore_or_create_workspace`) runs before a launch open request is handled (Finder/URL opens in `main.rs`), and `handle_cli_connection(is_launch)` runs it first on a CLI launch that opens something. Test `test_e2e_cli_launch_with_paths_restores_last_session_first` (fails with the restore disabled). Deviation: `--wait` is not excluded, because on macOS a CLI cold start already restores the session (the path arrives after startup, `main.rs` comment on #61346), so excluding it only on Linux would split the platforms. Found and fixed a pre-existing flake: zed's test `init_test` shared one process-wide app database across parallel tests, so restore tests (`test_multi_workspace_session_restore` on the original code too) failed about half the time; each test now gets its own database (6 of 6 full runs green; the suite takes ~8.5 s instead of ~3 s). zed 93; clippy and rustfmt clean.
- Part 9: `build_branch_review_prompt` asks for bugs and security issues only (file, line, failure, fix; no style). New `git::ReviewBranch` command (command palette and git panel overflow "Review Branch with Agent") diffs the current branch against the default branch and sends it without opening the diff view; it shares `request_branch_review` with the Review Diff button, which now refuses an empty diff with "No changes against {base} to review". Tests: prompt contract; empty diff is not sent and notifies (fails without the guard). git_ui 163, agent_ui 507; clippy and rustfmt clean.
- Part 10: `diff-review` is on for everyone. The editor now handles `editor::SendReviewToAgent` (it had no handler): it takes its stored comments (`take_all_review_comments` moved out of a test-only block), attaches each one's file, rows and code, and dispatches `agent::SendReviewComments`. The agent diff pane sends them to its own thread (toast if that thread is no longer open); elsewhere the panel sends them to the active thread (queued if busy, via `ThreadView::send_or_queue_content`) or a new one. Agent diff editors show the review button. New `editor::AddReviewComment` opens the comment box for the selected lines. Keys: `cmd-alt-g c` / `cmd-alt-g enter` (macOS), `alt-g c` / `alt-g enter` (Linux, Windows). Tests: editor sends comments with code and clears them; prompt attaches code per comment; comments reach the active thread (fails when routing skips it). editor 1162, agent_ui 509, git_ui 163; clippy and rustfmt clean. Deviation: the plan named the action `agent::SendAgentDiffComments` and a toolbar "Send N Comments" for the agent diff; the existing "Send Review to Agent" flow is reused instead (the editor's count/send machinery), so there is no separate toolbar button.
- Part 11: `ToolCall::plan_file_path()` returns the absolute `planFilePath` of a SwitchMode tool call (Claude's ExitPlanMode). When such a call asks for permission, the conversation opens the plan in the editor without taking focus, once per tool call, and the permission card shows a "Plan: <file>" link. Tests: path only for absolute SwitchMode requests; plan opens while focus stays in the dock (fails when opened with focus). acp_thread 221, agent_ui 510; clippy and rustfmt clean. The plan stays view-only: edits the user makes are not re-read by the adapter (needs the upstream change in Open questions).
- Part 12: project setting `agent_verification { command, timeout_seconds, enabled_by_default }` (settings content, `ProjectSettings`, default.json, VS Code import). After a root-thread turn that ends normally, used tools, and has no queued user message, `ThreadView::verify_turn` runs the command in the first worktree's root (trusted projects only) through the thread's terminal, shows it as a "Verify: `cmd`" tool call, and on failure (exit code, signal, or timeout) sends one fix request quoting the end of the output; a second failure stops, and the next user message re-arms it (`verify_gate.rs`). Composer toggle (check icon) appears when a command is configured; disabled with a "Trust this project" tooltip when the project is restricted. Tests: gate asks once per user message, passes don't use the attempt, long output trimmed; real-shell test (`echo checking; exit 3`) shows a failed Verify call and one fix request (5 of 5 stable; fails without the turn-end hook). agent_ui 514, settings suites pass; clippy and rustfmt clean. Deviation: no settings UI item (the setting is documented in default.json and completes from the JSON schema).
- Part 13: user setting `agent.hooks { pre_tool_use, post_tool_use, user_prompt_submit, stop }`, each a list of `{ matcher?, command, timeout_seconds? }` (agent settings are never read from project settings, so hooks are user-only). Runner `crates/agent/src/hooks.rs` runs `sh -c` with the event JSON on stdin, in the project root when it exists on this machine (a remote project's root does not, which would otherwise have blocked every tool). Native agent integration: a failing pre-tool hook blocks the call with its reason (a matching pre-tool hook waits for the tool's full input, so that tool does not stream); a failing post-tool hook appends "Hook feedback" to the result; a failing prompt hook blocks the prompt and removes it from the thread, a passing one's output is added as context; a failing stop hook sends its output back as a visible user message for one more turn. Invalid matchers run the hook for every tool. Hooks never approve; Zed's permission prompt still applies. Tests: runner passes stdin and stops at the first failure; pre-tool block (fails when hooks are skipped); prompt block; stop feedback once (both fail when their hook is skipped). agent 766, settings suites pass; clippy and rustfmt clean.
- Part 8 (option A, M's choice): `ThreadItem` now has a fixed height, `height()`: title row + padding + border + one small-label line when it shows its second line; the line height is computed the way labels compute it (window line height times the small text size, rounded), so measured rows did not change (47 px with a second line at the tests' 14 px rem, before and after; the earlier "under 1 px" estimate assumed `h_4`, which would have shrunk rows by 3 px). Each sidebar row is a cached `SidebarRow` view laid out at `row_height`, which builds the row the same way it renders (`thread_item` / `terminal_item` builders split out of `render_thread` / `render_terminal`); rows observe the sidebar so hover and selection still redraw them. Tests: every row's declared height equals its uncached layout (fails without the group separator); a running thread's row redraws each animation frame while its neighbor does not (fails with uncached rows). sidebar 158, ui, agent_ui pass; clippy and rustfmt clean.

## Open questions

- Part 14 (background tasks): the adapter (0.84.0) sends `async_task_spawned` / `async_task_progress` / `async_task_state_update` only when the client advertises `asyncTasks` inside `_meta.jetbrains.air`, and any `_meta.jetbrains.air` block makes it treat the client as AIR for everything (`air-extension.js:61-63` `isAirClient`), which changes its tool-call output. So the zedd side would be code nothing can exercise. Recommendation: first the upstream adapter PR for a non-AIR opt-in, then build the zedd side against it.

- Resolved (M chose A): Part 8 (sidebar CPU): GPUI marks every ancestor of an animating view dirty (`window.rs:2148-2159`), so the sidebar view re-renders each frame regardless; the saving comes only from per-row cached views, which a cached view lays out from a fixed style without rendering (`view.rs:430-435`), so each row needs an exact height up front. Header rows have one (`Tab::content_height`); thread rows do not (the metadata line takes its label's line height, `thread_item.rs:486-500`). Options: (A) give `ThreadItem` a deterministic height (fixed metadata line height; may shift rows by under 1 px); (B) rows measure and store their own height (no visual change; one-frame lag when a row gains or loses its metadata line). Recommendation: A.

- Upstream adapter PRs (M approved, opened): agentclientprotocol/claude-agent-acp#1206 lets a non-AIR client opt into async task updates with `_meta["async-tasks"]: true` (part 14 builds on it once merged); agentclientprotocol/claude-agent-acp#1207 approves the plan file as edited before approval (makes part 11's plan document editable).
