# Fork an agent thread from a past message

- Date: 2026-09-30 17:32
- Status: revision 3 done, release built 2026-09-30 21:29; walkthrough pending M
- Decisions (M, 2026-09-30, "all recommended, go"; UI placement A: fork button in the click-to-edit message toolbar): 1A native only, 2A cut before message N, 3 yes "Fork Thread", 4A "(fork)" title suffix only, 5A subagent owner check; fork actions disabled while generating.
- Repos: zedd
- Folders: crates/acp_thread, crates/agent, crates/agent_ui
- Files: crates/acp_thread/src/connection.rs, crates/acp_thread/src/acp_thread.rs, crates/agent/src/thread.rs, crates/agent/src/agent.rs, crates/agent_ui/src/conversation_view/thread_view.rs, crates/agent_ui/src/agent_panel.rs, crates/agent_ui/src/agent_ui.rs (tests in the same crates)
- Subagents: none (parts depend on each other in order)
- Research: `.claude/research/2026-09-30-1712_thread-fork-and-rewind_research.md` (option A)

## Context and problem

M wants Claude-Code-style fork in zedd's Agent Panel: branch a thread from a past message into a new thread, leaving the original untouched. Rewind is explicitly out of scope. zedd has no fork today [verified: research brief], but the native agent already has persisted user-message ids, a live thread snapshot (`Thread::to_db`), a save-under-new-id path, and a persisted draft prompt.

## Goals / Non-goals

**Goals**
- On any user message in a native-agent thread, "Fork from here" creates a new thread containing every message before it, opens it, and puts that message's content in the editor, ready to edit and send.
- A whole-thread "Fork Thread" action copies the full conversation into a new thread (pending Decision 3).
- The new thread keeps the original's model, profile, and thinking settings, is titled "<original title> (fork)", and appears in the sidebar like any thread.
- The original thread is not modified.
- A forked thread cannot continue a subagent that belongs to another thread.

**Non-goals**
- Rewind or code restore (M dropped it).
- Copying or isolating files; the fork shares the project working tree, as in Claude Code.
- External ACP agents (pending Decision 1). The capability is designed so they can be added later.
- A stored "forked from" link in the sidebar (needs a sidebar schema change; Decision 4).

## What is true today

```
ThreadView (user message N, has ClientUserMessageId)
   |                                                     <-- no fork action
AcpThread  -> connection: Rc<dyn AgentConnection>
                 |- truncate()  -> Option<AgentSessionTruncate>   pattern to copy
                 '- fork()                                         <-- missing
NativeAgentConnection
   |- sessions[id].thread : Entity<Thread>
   |- Thread::to_db() -> DbThread (live snapshot)
   '- ThreadsDatabase::save_thread(id, db_thread, folder_paths)
AgentPanel::open_thread(session_id, ...)  -> sidebar row created from the opened conversation
```

- Capability pattern: `AgentConnection::truncate` returns `Option<Rc<dyn AgentSessionTruncate>>`, default `None` [verified: `crates/acp_thread/src/connection.rs:218-223`, `:278-280`].
- Cut logic to mirror: `Thread::truncate` finds the `Message::User` with the given id and drains from it, removing matching `request_token_usage` [verified: `crates/agent/src/thread.rs:2420-2446`].
- Snapshot: `Thread::to_db` copies messages, model, profile, thinking, draft prompt, sandbox temp dir, and sandbox grants [verified: `thread.rs:1922-1956`].
- Save path: `run_save_worker` calls `database.save_thread(id, db_thread, folder_paths)` then `thread_store.reload` [verified: `crates/agent/src/agent.rs:1835-1866`]; `thread_save_payload` builds folder paths [verified: `agent.rs:1873`].
- Open path: the clipboard debug action saves under `acp::SessionId::new(uuid)` and calls `open_thread(session_id, None, Some(title), window, cx)` [verified: `crates/agent_ui/src/agent_panel.rs:3843-3857`].
- Draft prompt: `register_session` copies the thread's draft prompt into the `AcpThread` [verified: `agent.rs:802-820`], and `ThreadView` loads `thread.draft_prompt()` into the editor [verified: `crates/agent_ui/src/conversation_view/thread_view.rs:852`].
- Sidebar: `ThreadMetadataStore` upserts rows from opened conversation views [verified: `crates/agent_ui/src/thread_metadata_store.rs:1195-1232`, `:1335-1355`].
- Sandbox temp dir: created lazily per thread and stored on it [verified: `thread.rs:1520-1539`]. Copying it would make two threads share one temp directory.
- Subagent resume has no owner check: `resume_subagent_thread` looks up any live session by id and prompts it [verified: `agent.rs:3303-3334`]. A fork keeps the original's spawn-agent tool calls, which name the original's children, and the tool invites follow-ups by `session_id` [verified: `crates/agent/src/tools/spawn_agent_tool.rs:27-54`, `:184-188`]. `SubagentContext.parent_thread_id` records the owner [verified: `thread.rs:141-147`].
- UI placement: user-message actions render in `ThreadView`, gated on capabilities and `!is_subagent` [verified: `thread_view.rs:6131-6183`].
- Upstream pattern for "open a derived thread" is an action handled by the panel (`NewNativeAgentThreadFromSummary`) [verified: `crates/agent_ui/src/agent_ui.rs:412-416`, `agent_panel.rs:386-394`].

## Decisions for M (before Part 1)

1. **Which agents?** A: native Zed agent only; B: also external agents via ACP `session/fork` (unstable Draft; whole-thread only).
   RECOMMENDATION: A because it is the only path that supports forking at a message. Completeness: A 8/10, B 8/10 native + 4/10 external.
2. **Where does "Fork from here" cut?** A: before message N, with N's content in the editor (edit and resend on the new branch); B: after N's reply (the fork continues from that answer).
   RECOMMENDATION: A because it matches how edit-and-resend already cuts (`Thread::truncate`), and B is covered by forking from the next message or by "Fork Thread". Completeness: A 9/10, B 7/10.
3. **Add a whole-thread "Fork Thread" action?** Same call with no cut, in the thread's options menu and the command palette (`agent: fork thread`).
   RECOMMENDATION: yes, because it is Claude Code's `/branch` and costs one menu entry. Completeness: with 9/10, without 7/10.
4. **Link back to the original?** A: title suffix "(fork)" only; B: store `forked_from` in the sidebar metadata and show it.
   RECOMMENDATION: A because B changes the sidebar database schema for a label. Completeness: A 7/10, B 9/10.
5. **Subagent follow-ups across threads.** A: make subagent resume require that the child's `parent_thread_id` equals the calling thread (a fork then starts new subagents); B: allow sharing.
   RECOMMENDATION: A because sharing lets one thread silently change another's subagent state, and the owner check belongs where resume happens, not in the fork code. Completeness: A 9/10, B 5/10.

Also: forking while the thread is generating. Recommendation: disable the fork actions while `AcpThread::status()` is `Generating` [verified: `crates/acp_thread/src/acp_thread.rs:3754-3756`], because a half-written reply would be copied. Not a real tradeoff; stated here so M can object.

## Design

New capability in `acp_thread`:

```rust
// connection.rs
fn fork(&self, _session_id: &acp_v1::SessionId, _cx: &App) -> Option<Rc<dyn AgentSessionFork>> { None }

pub trait AgentSessionFork {
    /// Creates a new session with this session's history before `before_message`
    /// (all of it when `None`). The original session is not modified.
    fn run(&self, before_message: Option<ClientUserMessageId>, cx: &mut App) -> Task<Result<ForkedSession>>;
}
```

`ForkedSession` carries the new `SessionId` and title. `AcpThread::supports_fork(cx)` mirrors `supports_truncate`.

Native implementation (`agent` crate):
- `Thread::to_fork_db(before_message, cx) -> Task<Result<DbThread>>`: calls `to_db`, then cuts messages before the given user message (error if not found), keeps `request_token_usage` only for kept ids, recomputes nothing else, clears `detailed_summary`, `ui_scroll_position`, and `sandboxed_terminal_temp_dir`, sets `draft_prompt` to message N's content (cleared for a whole-thread fork, because the original owns its unsent draft), sets title to "<title> (fork)". Keeps model, profile, speed, thinking, and sandbox grants (grants match Claude Code's `/branch`, which carries "allow for this session").
- `NativeAgentConnection::fork` returns `NativeAgentSessionFork`, which builds the fork DB thread from the live session, saves it with a new `SessionId` through the same database and folder-path helpers the save worker uses, reloads the thread store, and returns the new id.
- Subagent owner check (Decision 5A) in `resume_subagent_thread`: reject when the child's `parent_thread_id` is not this thread's id, with an error telling the model to start a new subagent.

UI (`agent_ui`):
- `ThreadView`: "Fork from here" on each user message when `supports_fork && client_id.is_some() && !is_subagent && !generating`; placed with the existing message actions.
- Thread options menu and command palette: "Fork Thread" (Decision 3).
- Both run the capability, then ask `AgentPanel` to open the returned session with the same `open_thread` call the clipboard path uses. The exact hand-off (panel action vs direct panel call) is chosen in Part 3 after reading how `ThreadView` reaches the panel today.

No database schema change: the fork is an ordinary `DbThread` row.

## Risks and mitigations

| Risk | Likelihood | Impact | Mitigation | How noticed |
| --- | --- | --- | --- | --- |
| Cut drops a compaction marker's partner, so the fork replays wrongly | Low | Fork shows or sends a broken history | Test: fork before and after a manual `/compact` and replay | Part 2 test |
| Two threads share a sandbox temp dir | Medium if copied | Terminal files collide | Clear `sandboxed_terminal_temp_dir` in the fork; test | Part 2 test |
| Subagent owner check breaks existing resume flows | Low | Legit follow-ups fail | Existing subagent tests must pass; add owner-mismatch test | Part 1 tests |
| Fork opens but has no sidebar row | Low | Fork is hard to find later | Manual check in Part 3; sidebar rows come from opened conversations | Part 3 manual run |
| Save races with the original's queued save | Low | None to the original; fork is a separate id | Fork writes only its new id | Part 2 test |

## Acceptance criteria

- Given a native thread with user messages M1..M3, when "Fork from here" runs on M2, then a new thread opens with M1 and its reply only, M2's content is in the editor, and the original thread still has M1..M3.
- Given the same thread, when "Fork Thread" runs, then a new thread opens with M1..M3 and an empty editor (Decision 3).
- Given a forked thread, when it is reopened after restarting Zed, then its history, title "<title> (fork)", model, and profile are intact.
- Given a forked thread that contains a spawn-agent call from the original, when the model sends a follow-up to that subagent's `session_id`, then it gets an error and the original's subagent receives nothing (Decision 5A).
- Given an external-agent thread or a subagent thread, then no fork action is shown.
- Given a thread that is generating, then the fork actions are disabled.

## Rollout and rollback

Local fork, no flags. Each part leaves the workspace compiling and tested; commits are M's. Rollback is reverting the part's changes; forked threads already saved remain ordinary threads and need no cleanup. Abort condition: if the cut or replay behaves differently from `Thread::truncate` in tests, stop and re-scope Part 2.

## Parts

### Part 1: Capability contract and subagent owner check
- [x] Add `AgentConnection::fork`, `AgentSessionFork`, `ForkedSession`, and `AcpThread::supports_fork` in `crates/acp_thread` (done when: `cargo check -p acp_thread` passes and the default returns `None`).
- [x] Add the owner check to `resume_subagent_thread` (done when: a new test resuming another thread's subagent fails with the new error, and existing subagent tests pass).

### Part 2: Native fork
- [x] `Thread::to_fork_db` with cut, token-usage filter, cleared fields, draft prompt, title (done when: unit tests for cut at first, middle, missing id, and whole thread pass).
- [x] `NativeAgentConnection::fork` saving under a new session id (done when: a test forks, loads the new id from the database, and sees the expected messages while the original is unchanged).
- [x] Compaction and sandbox temp dir tests (done when: fork across a manual `/compact` replays, and the fork's temp dir is `None`).

### Part 3: UI
- [x] "Fork from here" on user messages with the gating above (done when: an `agent_ui` test shows the action for native messages and not for external or subagent threads).
- [ ] "Fork Thread" in the thread options menu and command palette (Decision 3) (done when: the action opens a new thread in the panel).
- [ ] Open the fork in the panel (done when: manual run shows the new thread, its sidebar row, and the draft in the editor).

### Part 4: Verification
- [x] `cargo check`, `./script/clippy` on touched crates, `cargo test -p acp_thread -p agent -p agent_ui` (done when: all pass).
- [ ] `cargo build -p zed` and a manual walk through every acceptance criterion (done when: M confirms).

## Standardized review

## Review: plan: fork an agent thread from a past message

### At a glance

- **Bottom line:** present as is
- **Where we are:** reviewing this plan against the research brief and the cited source; complete; waiting on M's 5 decisions and sign-off.
- **Need from M:** 5 decisions (listed in the plan) and sign-off.
- **Why:** every current-system claim was re-read in source; the open choices are surfaced before Part 1; one note concerns a design detail left to Part 3.
- **Intended outcome:** a build pass can add fork to native threads without inventing requirements.
- **Findings:** 0 blocking, 0 must-verify, 2 notes.

### Findings

### 1. NOTE — The ThreadView-to-panel hand-off is deferred to Part 3

- **Why it matters:** the UI parts cannot be estimated precisely until the mechanism is chosen.
- **Recommendation:** keep it deferred; the existing `NewNativeAgentThreadFromSummary` action is the default pattern.
- **Need from M:** Nothing.

#### What is wrong

- **Criterion:** Feasibility and precision
- **Where:** Design, UI bullet 3
- **Problem:** one design detail is named as an open read rather than decided.
- **Fix:** Plan — record the chosen hand-off in Part 3 before coding it.

#### Evidence

- [verified] The action pattern exists (`agent_ui.rs:412-416`, `agent_panel.rs:386-394`).
- [unverified] Whether `ClientUserMessageId` can sit in an action (actions need `JsonSchema`; `ClientUserMessageId` derives `Serialize`/`Deserialize` only, `connection.rs:17-18`).

### 2. NOTE — The subagent owner check changes behavior outside fork

- **Why it matters:** any thread that today resumes a subagent it did not create will start failing.
- **Recommendation:** keep it (Decision 5A); it closes a pre-existing gap, and Part 1 runs the existing subagent tests.
- **Need from M:** Decision 5.

#### What is wrong

- **Criterion:** Scope and cohesion
- **Where:** Part 1, task 2
- **Problem:** the fix is required by fork but its effect is wider than fork.
- **Fix:** Plan — already scoped as its own task with its own test; M decides via Decision 5.

#### Evidence

- [verified] `resume_subagent_thread` has no owner check (`agent.rs:3303-3334`).

### Criterion coverage

- **Goal fidelity — pass.** Goals and non-goals match M's "fork only" request; rewind excluded. Fix: None.
- **Current-system accuracy — pass.** All cited locations re-read in this session. Fix: None.
- **Scope and cohesion — violation.** Owner check reaches beyond fork. Fix: Finding 2.
- **Architecture and dependency direction — pass.** Contract in `acp_thread`, implementation in `agent`, UI in `agent_ui`, matching `truncate`; owner check at the resume site. Fix: None.
- **Sequencing and dependencies — pass.** Contract, then implementation, then UI; decisions before Part 1. Fix: None.
- **Completeness — pass.** Code, tests, persistence, sidebar, gating, and generating state covered; no schema change. Fix: None.
- **Failure and recovery — pass.** Missing message id errors; generating state disabled; rollback leaves ordinary threads. Fix: None.
- **Verification — pass.** Every task has a test or manual done condition; acceptance criteria are Given/When/Then. Fix: None.
- **Feasibility and precision — violation.** Hand-off deferred. Fix: Finding 1.
- **Decision readiness — pass.** Five decisions listed with recommendations before Part 1. Fix: None.

### What is working

- The cut mirrors the existing `Thread::truncate` logic (`thread.rs:2420-2446`), so forks and edit-and-resend agree on message boundaries.
- The sandbox temp dir risk is grounded in lazy per-thread creation (`thread.rs:1520-1539`).
- The subagent fix is placed at the resume site that owns the rule (`agent.rs:3303`).

### Scope and limits

- **Reviewed:** the full plan, the research brief, and every cited source location.
- **Not reviewed:** the thread options menu code; the exact tests existing in `crates/agent/src/tests`.
- **Checks not run:** no build or prototype.
- **Confidence:** high on design and sequencing; medium on Part 3 effort.

### In plain terms

The plan is ready once M answers the five decisions. Two small items are noted: the UI hand-off is picked during Part 3, and the subagent fix affects more than fork, which is why it is Decision 5.

## Revision 2 (2026-09-30 20:49): per-message fork icons and external agents

M, 2026-09-30: "in the dropdown I want based on the message like screenshot I shared and go with B". Interpreted as: fork icons attached to each message (user messages and agent replies), like the Claude app screenshot; keep "Fork Thread" in the menu; add external agents that support ACP `session/fork` (whole thread only). This overrides Decision 1 (now native + external) and the Part 3 placement (click-to-edit toolbar).

### What is true (verified 2026-09-30)
- zedd's ACP crate enables `session/fork` through the `unstable` feature (agent-client-protocol 2.2.0 `unstable` includes `unstable_session_fork`).
- `ForkSessionRequest` carries `session_id`, `cwd`, `additional_directories`, `mcp_servers`; no message point, so external fork is whole-thread only (schema 1.9.1 `src/v1/agent.rs:1127-1157`). `ForkSessionResponse` returns the new `session_id`, modes, config options; it does not replay history (`:1211`).
- Agents advertise it with `agent_capabilities.session_capabilities.fork` (`:4081`).
- Opening an existing external session by id uses `load_session` (with history) or falls back to `resume_session` (no history), else errors (`crates/agent_ui/src/conversation_view.rs` near `:1154`). `AgentPanel::open_thread` hard-codes the native agent; the private `external_thread_by_session(agent, ...)` accepts any agent (`agent_panel.rs:4566`); `Agent::from(AgentId)` exists (`agent_ui.rs:437`).

### Design
- Contract (`acp_thread`): `AgentSessionFork::run(point: ForkPoint, cx)` with `ForkPoint::WholeThread` and `ForkPoint::BeforeMessage { message, draft_message }`; `AgentSessionFork::supports_fork_at_message()`; `AcpThread::supports_fork_at_message(cx)`.
- Native (`agent`): `Thread::to_fork_db(&ForkPoint, cx)`; `draft_message: false` gives an empty editor. Supports every point.
- External (`agent_servers`): `AcpConnection::fork` returns a fork handle only when the agent advertises `session.fork` and can load or resume sessions. `run(WholeThread)` sends `session/fork` for the session's directories and MCP servers and returns the new id with title "<title> (fork)"; `run(BeforeMessage)` fails with a clear error. `supports_fork_at_message()` is false.
- UI (`agent_ui`): the fork opens with the thread's own agent through a new `AgentPanel::open_forked_thread(agent, ...)`. User messages get a fork icon in a small row under the message (shown on hover), gated on `supports_fork_at_message`; it forks `BeforeMessage { message, draft_message: true }`. Agent turns get a fork icon in the existing per-turn controls row; it forks `BeforeMessage { next user message, draft_message: false }`, or `WholeThread` for the last turn (so external agents get it on the last turn only). The icon in the click-to-edit toolbar is removed. Generating and subagent gates stay.

### Parts (revision 2)
- [x] Contract: `ForkPoint`, `supports_fork_at_message`; native `to_fork_db` takes a `ForkPoint` (done when: agent fork tests updated plus a new "after reply, empty editor" test pass).
- [x] External: `AcpConnection::fork` with capability gating and whole-thread-only run (done when: `agent_servers` tests cover advertised, not advertised, and message-point rejected).
- [x] UI: per-message icons, per-turn icon, `open_forked_thread` with the thread's agent (done when: `agent_ui` tests cover message-level vs whole-thread-only agents and the request each icon sends).
- [ ] Verify: suites, clippy, rustfmt, debug and release builds (done when: all pass), then M's walkthrough on a native and an external thread.

### Standardized review (revision 2)
- Goal fidelity: pass; matches M's screenshot request and option B. Assumption to confirm in the walkthrough: user-message icons on hover (not always shown).
- Current-system accuracy: pass; every claim above cites code or the crate source.
- Architecture: pass; `ForkPoint` is decided by the view, executed by each connection; external capability lives in `AcpConnection`, like `close`/`resume`.
- Failure and recovery: pass; external message-level fork fails loudly; agents without load or resume never show fork; ACP errors surface in the thread banner.
- Verification: pass; each part has tests; the external path cannot be tested end to end without an agent that implements `session/fork` (none was available during the research), so it is covered by the in-repo fake ACP agent.
- Risk (note): `session/fork` is an unstable ACP method; if the spec changes, only `AcpConnection::fork` changes.

## Revision 3 (2026-09-30 21:15): per-message fork for Claude Code; OpenCode whole thread

M, 2026-09-30: "B, build release add for opencode too".

### What is true (verified 2026-09-30)
- Claude Code ACP adapter `@agentclientprotocol/claude-agent-acp` 0.84.0 (installed at `~/Library/Application Support/Zed/external_agents/registry/npx/claude-acp`) advertises `fork`, `resume`, `loadSession`; handles `session/fork`; cuts at a point given in the non-standard `_meta.jetbrains.air.fork = { version: 1, messageId }`, where `messageId` is an agent reply id and the fork keeps everything up to and including that reply (`dist/fork-session.js:6-24, 107-141`). The extension is not advertised to non-AIR clients (`dist/acp-agent.js:1291-1300`); `agentInfo.name` is `@agentclientprotocol/claude-agent-acp`.
- OpenCode 1.18.33 (Zed registry copy) advertises `fork`, `resume`, `loadSession`; its ACP `forkSession` passes only `{directory, sessionID}` to its internal fork, so it forks the whole session only (embedded `ACP.forkSession`). Per-message fork for OpenCode needs a change in OpenCode, not zedd.
- ACP threads have no `ClientUserMessageId` on user messages (`AcpConnection` does not implement `client_user_message_ids`), so a fork point keyed by that id cannot address ACP messages. Reply chunks carry the agent's message id in `MessageIdentity` (`acp_thread.rs:310-315`). `agentInfo.name` is read at connect (`acp.rs:853-858`). Initial editor content is applied when an existing session is opened (`agent_panel.rs:4591-4602` to `thread_view.rs:827`).

### Design
- Contract: `ForkPoint::BeforeUserMessage { entry_ix, draft_message }` addresses the user message by its position in the `AcpThread` transcript, which every agent shares. `AgentSessionFork::granularity()` returns `WholeThread`, `AfterReplies` (can cut only right after an agent reply), or `AnyUserMessage`. `ForkedSession.draft_prompt` carries a draft for agents whose fork cannot store one.
- Native: maps the entry to its `ClientUserMessageId` and cuts as before (`Thread::to_fork_db` keeps a native `ForkCut`). Granularity `AnyUserMessage`.
- ACP: granularity `AfterReplies` only when `agentInfo.name` is the Claude adapter (the extension is not advertised; an agent that ignored it would silently fork the whole thread), else `WholeThread`. For a point, finds the last agent reply before the user message and sends its message id in `_meta.jetbrains.air.fork`; returns the user message as `draft_prompt` when asked.
- UI: user-message icon shown for `AnyUserMessage`, and for `AfterReplies` when a reply precedes the message; reply icon uses the next user message's entry; the fork opens with `draft_prompt` as initial editor content.

### Parts (revision 3)
- [x] Contract, native mapping, stub (done when: agent and acp_thread tests pass with entry-based points).
- [x] ACP Claude adapter point via the AIR extension, name-gated (done when: agent_servers test shows the meta sent for the Claude adapter, not for another agent, and the draft returned).
- [x] UI gating by granularity and draft on open (done when: agent_ui tests cover all three granularities).
- [ ] Verify: suites, clippy, rustfmt, debug and release builds.

### Standardized review (revision 3)
- Goal fidelity: pass for Claude Code; OpenCode per-message is out of zedd's reach and is reported, not faked.
- Architecture: pass; the view speaks in transcript positions, each connection maps them to its own ids.
- Failure behavior: pass; the adapter rejects an unknown reply id with an error shown in the banner; no silent whole-thread fork because the extension is sent only to the named adapter.
- Risk (must-verify): the AIR extension is private and may change with adapter updates; pinned by name and `version: 1`, and removable if ACP standardizes fork points.

## Changes made

- Part 1: Added the fork capability (`AgentConnection::fork` defaulting to `None`, `AgentSessionFork`, `ForkedSession`, `AcpThread::supports_fork`) in `crates/acp_thread/src/connection.rs` and `acp_thread.rs`. `resume_subagent_thread` in `crates/agent/src/agent.rs` now rejects a subagent whose `parent_thread_id` is not the calling thread. New test `test_resume_subagent_rejects_thread_that_did_not_spawn_it` passes, and fails with the check disabled. Existing tests pass: `agent` subagent filter 34/34, `acp_thread` 218/218.
- Lint fix (M approved 2026-09-30): removed 5 redundant `user_store.clone()` calls left by the pricing removal (`crates/agent/src/tests/mod.rs:4410`, `:5200`, `crates/agent/src/tools/evals/{edit_file.rs:253,terminal_tool.rs:167,write_file.rs:101}`). `./script/clippy -p acp_thread -p agent` now exits 0.
- Part 2: `Thread::to_fork_db` in `crates/agent/src/thread.rs`; `NativeAgentConnection::fork` and `NativeAgentSessionFork` in `crates/agent/src/agent.rs`, with `NativeAgent::session_folder_paths` extracted from `thread_save_payload` so fork and save share one folder-path rule. Deviations from the plan text: the fork's `updated_at` is set to now (the database uses it as `created_at` for a new row, `db.rs:538-541`); a whole-thread fork of an empty thread fails; `fork()` returns `None` for subagent sessions (capability-level gate, not only UI); an untitled original gives the fork an empty title (the UI default applies) because the default title constant lives in `agent_ui`. Compaction test checks the cut keeps or drops the `/compact` marker and summary together (message-prefix equality), not a full reload-and-send. New tests (7) pass; full suites: `agent` 760 passed / 11 ignored (pre-existing), `acp_thread` 218 passed; clippy exit 0; rustfmt check clean on touched files; `cargo check -p agent_ui` clean.
- Flaky test fix (M approved 2026-09-30): `thread_metadata_store` tests shared one process-wide test database, so parallel tests raced on the remote-connection backfill flag (`test_migrate_thread_remote_connections_backfills_from_workspace_db` failed 2/20 parallel, 0/20 serial). `init_test` in `crates/agent_ui/src/thread_metadata_store.rs` now installs `db::AppDatabase::test_new()`, matching `test_support.rs`. After: 0/40 parallel failures.
- Part 3: `ForkThread` action (`agent: fork thread`) in `crates/agent_ui/src/agent_ui.rs`, handled by `AgentPanel::fork_active_thread` and shown as "Fork Thread" under "Current Thread" in the panel menu. `ThreadView::can_fork`/`fork` in `conversation_view/thread_view.rs` (gated on fork support, not a subagent, not generating; errors go to the thread error banner; the fork opens in the panel with the original's work dirs). "Fork from here" (`GitBranch` icon) added to the click-to-edit message toolbar. `StubAgentConnection::with_supports_fork` records fork requests for tests. New tests (3) pass; the subagent gate test fails with the gate disabled. One more pricing-leftover redundant clone fixed (`crates/agent_ui/src/inline_assistant.rs:1867`). Suites: `agent_ui` 474, `agent` 760 (11 pre-existing ignored), `acp_thread` 218; `./script/clippy -p acp_thread -p agent -p agent_ui` exit 0. rustfmt check clean on fork lines; one pricing-diff line (`agent_panel.rs:6240`) wrapped to rustfmt's layout with M's OK; rustfmt check now clean on every touched file.
- Review follow-up (M approved 2026-09-30): `ThreadView::fork` now shows "The thread was forked and saved to your thread history, but the agent panel is not available to open it." in the thread error banner instead of returning silently when no agent panel exists. `test_fork_from_message_requests_a_fork_before_that_message` now asserts that error (its workspace has no panel). `agent_ui` 474 passed; `./script/clippy -p agent_ui` exit 0; rustfmt clean.
- Revision 2 (M, 2026-09-30, "go with B"): `ForkPoint` (`WholeThread`, `BeforeMessage { message, draft_message }`) and `AgentSessionFork::supports_fork_at_message` in `crates/acp_thread/src/connection.rs`; native `Thread::to_fork_db(&ForkPoint)`; external fork `AcpConnection::fork` / `AcpSessionFork` in `crates/agent_servers/src/acp.rs` (offered only when the agent advertises `session.fork` and can load or resume; whole thread only; message points rejected); `AgentPanel::open_forked_thread(agent, ...)` opens the fork with the thread's own agent. UI: fork icon under each user message on hover (fork-at-message agents only), fork icon in each agent turn's controls row (before the next message with an empty editor, or whole thread on the last turn); removed the icon from the click-to-edit toolbar. Tests added: native after-reply, external gating/whole-thread-only/`session/fork` request, UI per-reply points for both agent kinds. Suites: `acp_thread` 218, `agent` 761 (11 pre-existing ignored), `agent_servers` 42, `agent_ui` 476; clippy exit 0; rustfmt clean (layouts applied by hand); debug build passes.
- Revision 3 (M, 2026-09-30, "B, build release add for opencode too"): `ForkPoint::BeforeUserMessage { entry_ix, draft_message }`, `ForkGranularity` (`WholeThread`, `AfterReplies`, `AnyUserMessage`), `ForkedSession.draft_prompt` in `crates/acp_thread/src/connection.rs`; `AcpThread::fork_granularity`. Native keeps its own `ForkCut` in `crates/agent/src/thread.rs` and maps transcript positions to message ids in `NativeAgentSessionFork::run`. `crates/agent_servers/src/acp.rs`: `reads_air_fork_points` set when `agentInfo.name` is `@agentclientprotocol/claude-agent-acp`; `AcpSessionFork::air_fork_point` sends the last reply id before the message as `_meta.jetbrains.air.fork` and returns the message as a draft. UI gates the user-message icon by granularity (after-replies agents need a preceding reply) and opens forks with the draft as initial editor content (`AgentPanel::open_forked_thread`). OpenCode: whole-thread fork only (its ACP fork ignores message points). Tests: `claude_adapter_forks_after_the_reply_before_a_message`, `test_agents_that_cut_after_replies_skip_the_first_message`, updated native and UI tests. Suites: `acp_thread` 218, `agent` 761 (11 pre-existing ignored), `agent_servers` 43, `agent_ui` 477; clippy exit 0; rustfmt clean (layouts by hand).
