# Keep Claude Code's injected turns out of replayed threads

- Date: 2026-10-01 11:15
- Status: done in zedd; upstream PR open
- Repos: zedd (branch zedd-claude-code-transcript), agentclientprotocol/claude-agent-acp (fork under monarchmaisuriya)
- Folders: zedd crates/agent_servers; adapter src/
- Files: zedd crates/agent_servers/src/acp.rs; adapter src/acp-agent.ts (+ its test)
- Subagents: none

## Context and problem

Reloaded Claude Code threads show subagent hand-backs ("Another Claude session sent a message: <agent-message …>"), task notifications (`<task-notification>…`) and skill text as huge raw user-message boxes. M did not type them.

## What is true today

- Claude Code stores these turns with `isMeta: true` (hand-back also `origin.kind: "peer"`); M's real prompt is `isMeta: false, origin.kind: "human"` [verified: session file df6b5157…jsonl].
- Adapter v0.84.0 live path skips plain-text user messages [verified: dist/acp-agent.js:4645-4651]; replay forwards every user message, stripping only marker tags [verified: :5518-5522, :775], and sends no marker [verified: :7695].
- zedd forwards `user_message_chunk` into a user message [verified: agent_servers/src/acp.rs `handle_session_notification`]; it knows the agent is the Claude adapter from `agentInfo.name` [verified: acp.rs:857, CLAUDE_AGENT_ACP_NAME].

## Decision (option C)

- Owning-layer fix upstream: on replay, skip user messages with `isMeta: true`, as the live path does. Issue + PR on agentclientprotocol/claude-agent-acp.
- Labeled stopgap in zedd: for the Claude adapter only, drop `user_message_chunk` text that starts with one of the three Claude Code envelopes. Teardown condition: remove when the Zed registry ships an adapter version containing the upstream fix.

## Risks and mitigations

| Risk | Mitigation |
| --- | --- |
| A real prompt starting with an envelope is hidden | Only for the Claude adapter; envelopes are harness framing, not natural prose |
| Compaction summaries are meta and must stay | Upstream skip runs after the compaction handling; verified in the adapter tests |
| Upstream declines | Stopgap stays; revisit |

## Parts

### Part 1: zedd stopgap
- [x] `ClientContext.drops_injected_user_turns`, set from `agentInfo.name`; `is_claude_injected_user_turn`; filter in `handle_session_notification` (done when: tests for dropped envelope, kept real prompt, other agents untouched)

### Part 2: upstream fix
- [x] Issue describing replay vs live mismatch with evidence (done when: issue URL)
- [x] PR: skip `isMeta` user messages on replay, with a test (done when: adapter tests pass and PR URL)

### Part 3: verify
- [x] zedd `agent_servers` suite, clippy, rustfmt check, debug build

## Standardized review

- **Bottom line:** ready; the stopgap is a labeled scaffold with a teardown condition, the real fix is upstream.
- Goal fidelity: pass. Current-system accuracy: pass (cited). Architecture: pass with note: the zedd part is a client-side workaround at the agent-dialect layer (acp.rs already holds Claude-adapter-specific handling), not the owning layer; named as a scaffold. Failure and recovery: pass. Verification: pass. Decision readiness: pass (M chose C and approved the public issue/PR).

## Changes made

- Evidence: replaying the session through adapter 0.84.0 (`session/load`, no model call) emitted the hand-back and the task notification as bare `user_message_chunk`; the SDK reports them with `origin.kind` `peer` and `task-notification` (the user's prompt is `human`); skill text is not replayed, so the stopgap covers only the two envelopes seen.
- Part 1: `ClientContext.drops_injected_user_turns` (set when `agentInfo.name` is the Claude adapter) and `is_injected_claude_user_turn` in `agent_servers/src/acp.rs`, labeled with the teardown condition and the issue link. Test `test_injected_claude_user_turns_are_recognized_by_framing`; `agent_servers` 44 passed; clippy and rustfmt clean.
- Part 2: issue https://github.com/agentclientprotocol/claude-agent-acp/issues/1203; PR https://github.com/agentclientprotocol/claude-agent-acp/pull/1204 (`fix: skip harness-injected turns on session replay`): replay skips text-only user turns with an autonomous `origin.kind`; new test fails without the change; adapter check, build and 2090 tests pass.
- Deviation: the upstream fix keys on `origin.kind` (covers both turns) instead of `isMeta` (only the hand-back has it).

## Open questions

None.
