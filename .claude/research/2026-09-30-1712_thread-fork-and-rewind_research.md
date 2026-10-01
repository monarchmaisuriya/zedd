# Fork for agent threads in zedd

- Date: 2026-09-30 17:12 (rewritten for fork only on 2026-09-30; rewind dropped by M)
- Status: handed to plan
- Mode: explore options
- Repos: zedd   Folders: crates/acp_thread, crates/agent, crates/agent_servers, crates/agent_ui
- Question / problem: how should zedd let M branch an agent thread from a past message into a new thread, like Claude Code's "Fork conversation from here"?

File location note: `zedd/docs/` is Zed's mdbook site (`docs/book.toml`), so this file lives under `zedd/.claude/research/`.

## Options: add a "fork" capability to agent connections, implement it for the native agent by copying the saved thread up to a chosen message, and expose it on each user message

- **Bottom line:** zedd has no fork, but the native agent already has every building block: persisted, stable user-message ids; a thread snapshot (`Thread::to_db`); a "save under a new session id, then open" path; and a persisted draft prompt. Option A adds a fork capability next to `truncate` on `AgentConnection`, implements it for the native agent, and adds "Fork from here" to each user message plus a whole-thread "Fork Thread" action.
- **Where we are:** framing done; code paths read directly; 9 primary sources carried over from the first pass. Next step is the plan (drafted alongside this brief).
- **Need from M:** pick an option, and say which agent this is for (native Zed agent, or an external ACP agent).

### Glossary

- **Native agent:** Zed's built-in agent (`crates/agent`). It runs inside Zed and saves threads in Zed's SQLite database (`threads.db`).
- **External agent:** a separate program Zed drives over ACP (Agent Client Protocol, JSON-RPC over stdio). It owns its own conversation history.
- **Session id:** the id of one thread (`acp::SessionId`). A fork gets a new one.
- **Client user message id:** a UUID Zed gives each user message (`ClientUserMessageId`); it is saved with the thread and is the cut point for a fork.
- **Capability:** an optional method on `AgentConnection` that returns `Some(...)` only for agents that support a feature (for example `truncate`). The UI shows a button only when the capability is present.

### The problem, framed

- **What:** on any past user message, create a new thread that contains the conversation before that message, with that message's text waiting in the editor; and a way to copy a whole thread.
- **Why:** try a different direction from a known-good point without losing or altering the original thread.
- **Who:** M, using zedd's Agent Panel.
- **Whom:** M.
- **When:** no deadline; used often.
- **Where:** `acp_thread` (connection capability), `agent` (native implementation), `agent_ui` (buttons and opening the new thread).
- **Which:** reuse the native thread snapshot and save paths; ACP's unstable `session/fork` for external agents only if M needs it.
- **How:** the UI asks the connection's fork capability for a new session, then opens it in the panel.
- **How much:** option A is about 5 files in 3 crates, no database schema change.

Refocused problem: "fork like Claude Code" means "copy a native thread's history, cut before a chosen user message, into a new thread that opens ready to edit that message". Files are not copied; Claude Code does not copy them either.

### What Claude Code does (the target)

- `/branch` (or `--fork-session`) creates a new session id with the conversation up to that point; the original stays unchanged and still appears in the session list [verified, primary: [Claude Code sessions](https://code.claude.com/docs/en/sessions)].
- A fork does not copy files; it shares the working directory, and the docs point to git worktrees for isolation [verified, primary: same page].
- "Allow for this session" permission grants carry over with `/branch` but not with `--fork-session` [verified, primary: same page].
- The VS Code extension offers "Fork conversation from here" on each message [verified, primary: [Claude Code in VS Code](https://code.claude.com/docs/en/vs-code)]. Whether that fork includes the hovered message itself is not stated [unverified: try it in VS Code].

### What zedd has today

```
User message N (id = ClientUserMessageId, persisted)
   |
   +-- edit + resend   (rewind in place)             exists
   +-- Restore Checkpoint                             exists
   +-- Fork from here                                 <-- missing

Native thread in memory (Thread)
   |  Thread::to_db()  -> DbThread { messages, model, profile, draft_prompt, ... }
   |  cut messages before N                           <-- missing
   v
ThreadStore::save_thread(new SessionId, DbThread)     exists (clipboard debug path)
   v
AgentPanel::open_thread(new SessionId)                exists
```

- **No fork anywhere.** No fork, branch, or duplicate symbol exists in the agent crates, and Zed never calls ACP's `session/fork` [verified: grep of `crates/` by the codebase map; `grep -n fork crates/agent_servers/src/acp.rs` finds nothing].
- **Capabilities follow one pattern.** `AgentConnection` has optional methods such as `truncate` and `set_title` that return `None` by default, each with a small `AgentSession...` trait [verified: `crates/acp_thread/src/connection.rs:218-231`, `:278-280`].
- **User messages have persisted ids.** `Thread::truncate` finds the cut point with `Message::User(UserMessage { id, .. }) if id == &client_user_message_id` and drops everything from there, including matching `request_token_usage` entries [verified: `crates/agent/src/thread.rs:2420-2446`].
- **A full in-memory snapshot exists.** `Thread::to_db` builds a `DbThread` with messages, model, profile, thinking settings, draft prompt, sandbox temp dir, and sandbox grants [verified: `crates/agent/src/thread.rs:1922-1956`].
- **The save-and-open path exists.** The debug action `load_thread_from_clipboard` makes a new `SessionId` from a UUID, calls `thread_store.save_thread(session_id, db_thread, ...)`, then `this.open_thread(session_id, ...)` [verified: `crates/agent_ui/src/agent_panel.rs:3843-3857`].
- **That debug path resets most settings.** `SharedThread::to_db_thread` keeps only title (prefixed "🔗"), messages, updated_at, and model; profile, thinking, and sandbox fields are reset [verified: `crates/agent/src/db.rs:130-186`]. A fork should keep model, profile, and thinking settings, so it should start from `Thread::to_db`, not `SharedThread`.
- **A draft prompt is persisted and restored.** `DbThread.draft_prompt` exists [verified: `db.rs`, `DbThread`], and `register_session` copies the thread's draft prompt into the `AcpThread` [verified: `crates/agent/src/agent.rs:802-820`]. So a fork can put message N's text into the editor by setting `draft_prompt` [inferred: that the message editor reads `AcpThread::draft_prompt` on open; check in `conversation_view.rs`].
- **Sidebar rows come from opened conversations.** `ThreadMetadataStore` subscribes to conversation views and upserts a row on their events [verified: `crates/agent_ui/src/thread_metadata_store.rs:1195-1232`, `:1335-1355`]. So opening the fork in the panel should create its sidebar row with no extra code [inferred; check by running].
- **Sidebar rows have no "forked from" field.** `ThreadMetadata` holds thread_id, session_id, agent_id, title, title_override, timestamps, worktree_paths, remote_connection, archived [verified: `thread_metadata_store.rs:1341-1353`]. A visible link back to the original thread would need a schema change.
- **Subagent sessions would be shared.** A subagent child thread records its parent's session id (`SubagentContext { parent_thread_id, depth }`) [verified: `crates/agent/src/thread.rs:141-147`], and the spawn-agent tool lets the model send follow-ups to an existing child by `session_id` [verified: `crates/agent/src/tools/spawn_agent_tool.rs:27-53`]. A forked thread keeps the tool calls that name the original's children, so the fork's agent could send a follow-up to a child the original thread also uses [inferred].
- **External agents cannot fork at a message.** The ACP crate in use (2.2.0, `unstable` feature on) defines `session/fork` with `ForkSessionRequest { session_id, cwd, additional_directories, mcp_servers, meta }` and no message id [inferred from the codebase map: schema 1.9.1 `src/v1/agent.rs:1119-1156`]. The spec lists session fork as a Draft RFD [verified, primary: [ACP session fork RFD](https://agentclientprotocol.com/rfds/session-fork)]. Fork-at-message (PR #629) was closed in favour of an open "session cursor" RFD (PR #2114) [verified, primary: [PR #629](https://github.com/agentclientprotocol/agent-client-protocol/pull/629), [PR #2114](https://github.com/agentclientprotocol/agent-client-protocol/pull/2114)].
- **Upstream demand.** Zed issue #54954 asks for "Fork conversation", a default title of "<title> (fork)", and a visible link to the source; closed without a maintainer plan [verified, tertiary: [zed#54954](https://github.com/zed-industries/zed/issues/54954)].

### What must be true

Hard constraints:
- The cut point must be a user message, because only user messages carry ids [verified: `thread.rs:2420-2446`].
- The fork must be taken from the live in-memory thread, not the database, because saves are queued and can lag [verified: `enqueue_save` / `run_save_worker`, `agent.rs:1810-1860`].
- The original thread must not change [verified: Claude Code's documented behavior; M's goal].
- External-agent history is owned by the agent, so Zed cannot cut it [verified: ACP sources above].

Conventions:
- New optional features are `AgentConnection` capabilities returning `Option<Rc<dyn AgentSession...>>` [verified: `connection.rs`].
- UI actions on a user message sit in the message block in `thread_view.rs`, gated on capabilities and `!is_subagent` [verified: `crates/agent_ui/src/conversation_view/thread_view.rs:6131-6183`].

Open checks:
- Whether forking while the thread is still generating should be blocked or should snapshot what exists [unverified: decide in plan; Claude Code's behavior not documented].
- Whether a compaction marker (`CompactionMarker::Manual { marker_id }`, `thread.rs:4845`) before the cut point replays correctly in the fork [unverified: fork across a `/compact` in a test].
- Whether copying `sandboxed_terminal_temp_dir` would make two threads share one temp directory [unverified: read its users in `crates/agent`].

### Options

| Aspect | A: Native fork capability (recommended) | B: A plus external agents via `session/fork` | C: Text-only fork for any agent |
| --- | --- | --- | --- |
| How it works | New `AgentSessionFork` capability on `AgentConnection`. Native implementation: `Thread::to_db`, cut `messages` before message N, keep matching token usage, clear summary, reset sandbox temp dir, set `draft_prompt` to message N, title "<title> (fork)", save under a new `SessionId`, open in the panel. "Fork Thread" (whole thread) uses the same call with no cut. | Everything in A, plus `AcpConnection` implements the capability with `session/fork` when the agent advertises it. Only whole-thread fork is possible. | New thread whose first prompt contains the old conversation as text, like "New From Summary" but verbatim. |
| Fits our code | Follows the `truncate` capability pattern; reuses `to_db`, `save_thread`, `open_thread`. | Uses an unstable crate feature already enabled in `Cargo.toml`. | Reuses `NewNativeAgentThreadFromSummary` plumbing. |
| Cost and blast radius | About 5 files in 3 crates; no schema change; additive UI. | Small on top of A, but only agents that implement the Draft method benefit. | Small. |
| When it fails | Subagent children shared between fork and original; forking mid-generation; external-agent threads show no fork button. | Spec changes (Draft RFD); per-message fork impossible. | The agent sees text, not its real tool history; context fills fast. Not a real fork. |
| Completeness | 8/10 for native threads | 8/10 native, 4/10 external | 3/10 |

### Flow of the recommended option

```
User clicks "Fork from here" on message N  (or "Fork Thread" in the thread menu)
   v
ThreadView -> connection.fork(session_id, Some(N) | None)        <-- new capability
   v
NativeAgentConnection
   |- thread.to_db()                     (live snapshot)
   |- cut messages before N; token usage for kept ids only       <-- new
   |- draft_prompt = N's content; title = "<title> (fork)"       <-- new
   |- save_thread(new SessionId)
   v
AgentPanel opens new SessionId  -> sidebar row created on open (existing)
Original thread: untouched
```

### Recommendation

RECOMMENDATION: A because every step reuses code that already exists for native threads, it needs no schema change, and the capability slot lets B be added later without touching the UI.

What would change it: if an external agent is M's main agent, A does nothing visible for it, and B still cannot fork at a message.

### Sources

- Claude Code docs, Sessions (fork): https://code.claude.com/docs/en/sessions
- Claude Code docs, VS Code extension: https://code.claude.com/docs/en/vs-code
- ACP RFD, Session fork: https://agentclientprotocol.com/rfds/session-fork
- ACP RFD updates: https://agentclientprotocol.com/rfds/updates
- ACP v2 required session methods: https://agentclientprotocol.com/rfds/v2/required-session-methods
- ACP PR #629, forking at a specific message (closed): https://github.com/agentclientprotocol/agent-client-protocol/pull/629
- ACP PR #2114, session cursor RFD (open): https://github.com/agentclientprotocol/agent-client-protocol/pull/2114
- Zed docs, Agent Panel: https://zed.dev/docs/ai/agent-panel
- Zed issue #54954, fork conversation into a new thread: https://github.com/zed-industries/zed/issues/54954
- zedd source (read-only): `crates/acp_thread/src/connection.rs`, `crates/agent/src/thread.rs`, `crates/agent/src/db.rs`, `crates/agent/src/agent.rs`, `crates/agent/src/tools/spawn_agent_tool.rs`, `crates/agent_servers/src/acp.rs`, `crates/agent_ui/src/agent_panel.rs`, `crates/agent_ui/src/thread_metadata_store.rs`, `crates/agent_ui/src/conversation_view/thread_view.rs`

Method: direct reads of the files above, `grep` for fork, draft_prompt, save, and subagent symbols; web sources read with a summarizing fetcher on 2026-09-30 (GitHub details paraphrased).

## Standardized review

## Review: research: fork for agent threads in zedd

### At a glance

- **Bottom line:** present as is
- **Where we are:** reviewed the full rewritten brief and re-read its code claims; complete; waiting on M's pick and one answer.
- **Need from M:** 1 answer (which agent; Finding 1).
- **Why:** option A's premises are verified in source; the remaining risks are named open checks that belong in the plan.
- **Intended outcome:** let M choose how zedd forks agent threads before implementation.
- **Findings:** 0 blocking, 2 must-verify, 1 note.

### Decisions needed

1. **Finding 1** — Say whether fork is for the native agent, external agents, or both. Recommendation: native (A).

### Findings

### 1. MUST-VERIFY — The recommendation does nothing if M's main agent is external

- **Why it matters:** A only adds fork to native threads; threads with external agents would show no fork button.
- **Recommendation:** M answers before implementation; if external, the plan adds B and accepts whole-thread fork only.
- **Need from M:** which agent.

#### What is wrong

- **Criterion:** Question and decision fit
- **Where:** "Recommendation"
- **Problem:** the target agent is unknown and decides which option applies.
- **Fix:** Research brief — record M's answer.

#### Evidence

- [verified] `AcpConnection` has no fork, truncate, or client-id capability outside `test_support` (`crates/agent_servers/src/acp.rs:1967`).

### 2. MUST-VERIFY — Forked threads can send follow-ups to the original thread's subagents

- **Why it matters:** the fork's agent may continue a child session the original thread still uses, so the two threads interfere.
- **Recommendation:** the plan picks a rule (keep sharing, or have the fork refuse follow-ups to children it did not create) and tests it.
- **Need from M:** Nothing at research stage; a plan decision.

#### What is wrong

- **Criterion:** Codebase and system fit
- **Where:** "What zedd has today", subagent bullet
- **Problem:** the effect is inferred, not reproduced.
- **Fix:** Plan — add a test that forks a thread with a spawn-agent call and checks follow-up behavior.

#### Evidence

- [verified] `SubagentContext { parent_thread_id, depth }` (`thread.rs:141-147`).
- [verified] Follow-ups by `session_id` are allowed (`spawn_agent_tool.rs:27-53`).
- [inference] Copied tool calls keep the original children's ids.

### 3. NOTE — Claude Code's exact per-message fork point is unverified

- **Why it matters:** "before message N, with N in the editor" is chosen for fit with Zed's truncate semantics, not copied from Claude Code.
- **Recommendation:** keep the choice; confirm with M in the plan's decisions.
- **Need from M:** Nothing.

#### What is wrong

- **Criterion:** Uncertainty and limits
- **Where:** "What Claude Code does"
- **Problem:** the target behavior is stated as unverified, correctly, but the recommendation leans on it.
- **Fix:** Plan — make the fork point an explicit decision.

#### Evidence

- [unverified] The VS Code docs do not state whether the hovered message is included.

### Criterion coverage

- **Question and decision fit — violation.** Target agent unknown. Fix: Finding 1.
- **Method — pass.** Direct code reads plus primary docs and specs; method recorded. Fix: None.
- **Source quality — pass.** Claude Code and ACP claims cite official docs and specs; the Zed issue is marked tertiary. Fix: None.
- **Coverage and alternatives — pass.** Native, protocol, and text-only approaches compared; status quo (no fork) shown in the diagram. Fix: None.
- **Claim traceability — pass.** Load-bearing claims carry file:line read in this pass; one schema location is marked as from the map. Fix: None.
- **Evidence handling — pass.** Inferences (draft prompt fill, sidebar row, subagent sharing) are tagged with checks. Fix: None.
- **Analysis quality — pass.** Same rows for all options. Fix: None.
- **Codebase and system fit — violation.** Subagent sharing unresolved. Fix: Finding 2.
- **Uncertainty and limits — violation.** Fork point basis. Fix: Finding 3.
- **Recommendation proportionality — pass.** Names the condition that would change it. Fix: None.
- **Reproducibility — pass.** Files and greps listed; web read date stated. Fix: None.

### What is working

- The save-under-new-id path is quoted from source (`agent_panel.rs:3843-3857`).
- The brief rejects `SharedThread::to_db_thread` for a concrete reason: it resets profile and thinking settings (`db.rs:130-186`).
- The live-snapshot constraint is grounded in the queued save worker (`agent.rs:1810-1860`).

### Scope and limits

- **Reviewed:** the full brief and every file:line it cites except the ACP schema location.
- **Not reviewed:** `sandboxed_terminal_temp_dir` users; message editor draft loading; ACP PR #2114 contents.
- **Checks not run:** no prototype, no build.
- **Confidence:** high on the native building blocks; medium on subagent and draft-prompt behavior until tested.

### In plain terms

Option A is well supported. M should say which agent this is for; the plan must settle what happens to subagents and exactly where the fork cuts.
