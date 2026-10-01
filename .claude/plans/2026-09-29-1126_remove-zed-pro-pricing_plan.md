# Remove Zed Pro / pricing / billing surfaces

- Date: 2026-09-29 11:26
- Status: done (build, tests, and manual UI check verified 2026-09-30)
- Repos: zedd (this repo)   Folders: crates/cloud_api_types, crates/client, crates/cloud_api_client, crates/agent, crates/agent_ui, crates/ai_onboarding, crates/language_models, crates/onboarding, crates/title_bar, crates/edit_prediction   Files: see Design and Parts
- Subagents: none (single-threaded edits; the changes are small per-file but touch many files in sequence, so one agent working part by part is safer than parallel subagents editing overlapping crates)

## Context and problem

M forked Zed into "zedd" and wants to strip every trace of Zed's paid-tier product: pricing pages, "Upgrade to Zed Pro" nags, trial banners, plan badges, and the client-side plumbing that tracks subscription/usage status. The goal is a clean removal, not hiding behind a flag: the fork should have no pro/pricing surface left, while the rest of the editor (including the Zed-hosted AI provider connectivity, decided below) keeps working.

## Goals / Non-goals

**Goals**
- No UI element anywhere shows plan names, trial countdowns, upgrade buttons, or plan comparison tables.
- No client code calls `zed_urls::{start_trial_url, upgrade_to_zed_pro_url, account_url}` or renders their targets.
- `cloud_api_types::Plan` stops being a multi-tier enum consumed for gating; either the type is deleted or collapsed so nothing branches on tier.
- The app still builds, and the Zed-hosted AI provider (or whatever replaces it per the Decision below) still functions for whichever path M keeps.
- Dead code is deleted, not `#[allow(dead_code)]`'d or feature-flagged off.

**Non-goals**
- Not touching `crates/collab` (the Zed.dev backend server). This fork is a client; nothing indicates M runs their own collab server. Confirmed with M as part of sign-off, not assumed.
- Not building new pricing/billing UI of any kind (this is a pure removal task).
- Not changing sign-in/auth flows beyond the plan-status fields they carry today.

## What is true today

`cloud_api_types::Plan` (`crates/cloud_api_types/src/plan.rs:7-15`) is a 6-variant tier enum (`ZedFree`, `ZedPro`, `ZedProTrial`, `ZedBusiness`, `ZedVip`, `ZedStudent`) returned by the server as part of `PlanInfo` and threaded through `UserStore` (`crates/client/src/user.rs:113-118, 739-851`). [verified: subagent research report, this conversation]

```
zed.dev backend
   |  GetAuthenticatedUser response (PlanInfo, usage headers)
   v
cloud_api_client::get_authenticated_user()  (crates/cloud_api_client/src/cloud_api_client.rs:108)
   v
UserStore.plan_info / plans_by_organization / edit_prediction_usage   (crates/client/src/user.rs)
   |
   +--> agent_panel.rs: should_render_trial_end_upsell / should_render_new_user_onboarding  <-- gate
   +--> agent/thread.rs: handle_completion_error(plan) auto-retry branch                     <-- gate
   +--> language_models/provider/cloud.rs: "Choose a Plan" config UI, Start Trial/Upgrade buttons <-- surface
   +--> ai_onboarding: AiUpsellCard, PlanDefinitions, onboarding content cards                <-- surface
   +--> onboarding/basics_page.rs: Start Free Trial button                                    <-- surface
   +--> title_bar.rs: PlanChip badge in account/org switcher menu                             <-- surface
```

Standalone, fully deletable files: `crates/agent_ui/src/ui/end_trial_upsell.rs`, `crates/title_bar/src/plan_chip.rs`, `crates/ai_onboarding/src/plan_definitions.rs`. [verified: subagent research report]

Interwoven, surgical-edit-only files (core functionality lives alongside the plan logic): `crates/client/src/user.rs`, `crates/agent/src/thread.rs`, `crates/agent_ui/src/agent_panel.rs`, `crates/ai_onboarding/src/ai_onboarding.rs`, `crates/language_models/src/provider/cloud.rs`, `crates/onboarding/src/basics_page.rs`, `crates/onboarding/src/onboarding.rs`, `crates/title_bar/src/title_bar.rs`, `crates/edit_prediction/src/edit_prediction.rs`. [verified: subagent research report]

`cloud_api_types` is a workspace dependency of 18 crates; only 8 of those (`client`, `cloud_api_client`, `agent`, `agent_ui`, `ai_onboarding`, `onboarding`, `title_bar`, `language_models`) actually use `Plan`/`PlanInfo` — the rest use unrelated types from the same crate (extension manifests, etc.) and are unaffected. [verified: subagent research report]

No Stripe code, no `zed_pro`/`pro_trial` settings keys, no server-side payment logic exists client-side — billing itself is entirely server-side at zed.dev. [verified: subagent research report]

## Options considered

The one real fork-in-the-road is what happens to `crates/language_models/src/provider/cloud.rs` — the "Zed AI" hosted-model provider. Its whole config UI (`ConfigurationViewState`, "Choose a Plan" header, subscription-summary text, usage-limit display) exists *because* Zed-hosted models are metered and plan-gated. You cannot strip the plan concept out of this file without deciding what the provider becomes.

| Option | How it works | Tradeoffs | Failure condition | Completeness |
| --- | --- | --- | --- | --- |
| A. Remove the Zed-hosted AI provider entirely | Delete `crates/language_models/src/provider/cloud.rs` and its registration; users bring their own API keys (Anthropic, OpenAI, etc.) via the existing BYO-key providers, which already exist in `language_models` unrelated to billing | Simplest, zero plan-branching left anywhere for AI; loses one-click "use Zed's hosted models" convenience | None known — BYO-key providers are independent code paths already used by non-Pro users today | 9/10 |
| B. Keep the provider, collapse to a single "connected/not connected" state | Rewrite the config UI to drop plan-name text, usage bars, trial/upgrade buttons; keep auth + model-list + request-forwarding to zed.dev, but stop rendering/reading `Plan` | Keeps the convenience feature; still calls zed.dev's metered API on M's behalf, whose billing this fork has no control over and whose usage limits still apply server-side even with client UI stripped | Server still enforces per-plan quotas; hitting them produces a raw error with no "why" UI unless a plain error message is added back | 6/10 — leaves a half-explained failure mode unless a fallback error string is added |
| C. Keep the provider and current plan branching, hide only the upsell buttons | CSS/conditional-hide "Upgrade"/"Start Trial" buttons but keep reading `Plan`/`PlanInfo` for usage text | Least work | Directly violates the stated goal ("clean removal, not hiding"); leaves the enum, the network call, and the tier logic fully intact | 2/10 |

RECOMMENDATION: Option A because it is the only option that fully satisfies "no pro/pricing surface left" without leaving a half-stripped provider that still depends on a metered zed.dev backend M doesn't control. It also deletes the most code, which is the actual goal.

## Decision

Go with **Option A**: remove the Zed-hosted ("Zed AI" / cloud) LLM provider entirely, along with all plan/subscription/usage plumbing that exists to support it. BYO-key providers (Anthropic, OpenAI, etc., already in `crates/language_models`) remain untouched and become the only way to configure AI in this fork.

Structural reason: the plan/tier concept exists in this codebase for exactly one purpose — metering access to Zed's hosted models. Once that provider is gone, `Plan`, `PlanInfo`, `plans_by_organization`, edit-prediction usage headers, and every upsell widget become dead code with nothing left to gate. Deleting the provider first makes every downstream removal a mechanical "delete the now-unreachable code" pass instead of a judgment call about what state to leave things in.

Consequences:
- `crates/cloud_api_types::Plan`/`PlanInfo`/`SubscriptionPeriod` types can be deleted outright (not shimmed) once nothing constructs or reads them.
- `UserStore` loses `plan_info`, `plans_by_organization`, `edit_prediction_usage`, and their accessor methods (`plan()`, `subscription_period()`, `trial_started_at()`, `has_overdue_invoices()`, `edit_prediction_usage()`).
- `cloud_api_client::get_authenticated_user()` response parsing drops the `PlanInfo`/usage fields (keep the call only if other fields of `GetAuthenticatedUserResponse` are still needed for sign-in; verify during Part 1).
- `agent/thread.rs`'s `handle_completion_error` loses its plan-based auto-retry fork; auto-retry logic needs a plain replacement (e.g. always retry, or retry based on provider type only) — this is a real behavior decision, flagged for M's input during Part 4, not guessed.
- Edit predictions (`crates/edit_prediction`) lose their usage-quota plumbing since that quota was also a Zed-hosted-model artifact; the feature itself (local/BYO edit prediction, if any exists independent of Zed's model) needs a quick check in Part 1 to confirm it doesn't depend on the cloud provider for its core function, only for usage display.

Rejected: Option B (keep provider, strip UI only) — leaves a metered dependency on a backend this fork doesn't operate, with usage limits enforced invisibly. Option C (hide buttons) — explicitly contradicts M's "not just hiding" requirement.

## Design

Removal proceeds in dependency order: delete the provider and its direct UI callers first, then walk inward to the core type. Each part below produces a compiling workspace (`cargo check --workspace`) before moving to the next.

Target end state: `cloud_api_types` crate keeps `extension.rs`, `internal_api.rs`, `known_or_unknown.rs`, `timestamp.rs`, `websocket_protocol.rs` (confirmed independent of `plan.rs`, [verified: crate directory listing]); only `plan.rs` and its module export are removed. The crate itself is not deleted. `client::UserStore` no longer carries plan/usage fields. No file in the workspace imports `cloud_api_types::Plan` or calls `zed_urls::{start_trial_url, upgrade_to_zed_pro_url}`.

No data migration — this is client code removal, no persisted user data involved beyond two GPUI-persisted dismissal keys (`"dismissed-trial-upsell"`, `"dismissed-trial-end-upsell"`) which simply become orphaned keys in the local DB (harmless, not read after removal).

## Risks and mitigations

| Risk | Likelihood | Impact | Mitigation | How noticed |
| --- | --- | --- | --- | --- |
| `cloud_api_types` or `client::user.rs` changes break unrelated crates that share the same files (auth, sign-in) | Medium | Build failure across many crates | Edit surgically, `cargo check --workspace` after every part, never delete a shared field without grepping all readers first | `cargo check` fails immediately, points at exact call site |
| Removing the cloud provider breaks a downstream feature that assumed *some* AI provider is always configured (e.g. first-run defaults, telemetry) | Medium | Onboarding or agent panel shows a broken/empty state instead of a clean "configure a provider" prompt | Check `onboarding`/`agent_ui` default-provider logic in Part 2 before deleting; replace with a neutral empty state if one existed | Manual run of the app after Part 2, check onboarding flow |
| `handle_completion_error`'s plan-gated auto-retry fork changes retry behavior for BYO providers unintentionally | Low | Requests that used to auto-retry silently stop retrying, or vice versa | Flagged explicitly for M's decision in Part 4 rather than guessed | Code review of the diff; explicit sign-off line in Part 4 |
| `docs/` (mdbook) or `.github` workflows reference Zed Pro / pricing pages | Low | Docs site links to dead pages after this repo diverges from upstream | Out of scope for this plan (client code only); note as an open question, not silently fixed | Left as an explicit open question below |

## Acceptance criteria

- Given a fresh build of the workspace, when `cargo check --workspace` runs, then it succeeds with zero errors and zero `#[allow(dead_code)]` added for plan-related code.
- Given the app is launched, when the agent panel, onboarding flow, and title bar account menu are viewed, then no plan name, trial banner, usage bar, or "Upgrade"/"Start Trial" button appears anywhere.
- Given the AI provider settings, when opened, then only BYO-key providers are listed; there is no "Zed AI" / "Choose a Plan" entry.
- Given a grep for `cloud_api_types::Plan`, `zed_urls::start_trial_url`, `zed_urls::upgrade_to_zed_pro_url`, `PlanChip`, `AiUpsellCard`, `EndTrialUpsell`, `PlanDefinitions` across `crates/`, when run after the last part, then it returns zero matches (excluding `crates/collab`, explicitly out of scope).

## Rollout and rollback

No flags — this is a straight deletion on a feature branch. Order: Parts 1 through 6 as listed, each a separate commit (M's to make; core rule 6 — git stays in M's hands). Abort condition: if any part reveals that a "surgical" file is more load-bearing than the research indicated (e.g. `plan()` used for something beyond billing), stop and re-scope that part before continuing, per core rule 1. Rollback is `git revert` per commit since each part is a self-contained compiling state.

## Parts

### Part 1 — Confirm scope and remove the Zed-hosted AI provider
- [x] Confirmed by review: `cloud_api_types` keeps `extension.rs`, `internal_api.rs`, `known_or_unknown.rs`, `timestamp.rs`, `websocket_protocol.rs`; only `plan.rs` is removed (Part 5). No further verification needed here.
- [x] Write-scope rescan found: the agent's built-in web search tool (`WebSearchTool`) and its only provider implementation (`CloudWebSearchProvider` in `crates/web_search_providers`) work *only* through the Zed-hosted provider — there is no BYO web search path anywhere in the repo. Flagged to M; M chose to accept the loss and remove the now-dead web search plumbing too (see added tasks below).
- [x] Verified: `get_authenticated_user()` / `UserStore` sign-in path does not require `PlanInfo`-specific fields to compile once the cloud provider is gone; `client` crate checks clean.
- [x] Deleted `crates/language_models/src/provider/cloud.rs`; deregistered from `crates/language_models/src/language_models.rs`; also removed the now-unused `user_store` parameter threaded through `language_models::init(...)` and its 10 call sites (was only used by the deleted provider). `cargo check -p language_models` passes.
- [x] Deleted `crates/language_models_cloud` crate entirely (its only consumer was `provider/cloud.rs`); removed workspace member/path entries and the dependency line in `crates/language_models/Cargo.toml`. Also removed the now-unused `cloud_api_client`, `cloud_api_types`, and dev-dependency `cloud_llm_client` lines from that Cargo.toml.
- [x] Deleted `ZedDotDevSettings`/`ZedDotDevSettingsContent`/`ZedDotDevAvailableModel`/`ZedDotDevAvailableProvider` and the `zed_dot_dev` field from `crates/language_models/src/settings.rs` and `crates/settings_content/src/language_model.rs` (not previously identified in research — surfaced by the compile error after deleting `cloud.rs`, since these settings types lived in `cloud.rs` itself). Also removed the `"zed.dev"` entry from the provider-id autocomplete schema in `crates/settings_content/src/agent.rs`.
- [x] Deleted `crates/agent/src/tools/web_search_tool.rs` and its registration (`tools.rs`, `thread.rs:2217`, test in `tests/mod.rs`) — per the flagged finding that the built-in web search tool only works through the Zed-hosted provider; M approved the loss.
- [x] Deleted `crates/web_search_providers` (its only provider, `CloudWebSearchProvider`, was unreachable without the cloud LLM provider) and `crates/web_search` (the registry/trait crate; its only consumers were `web_search_providers` and the deleted tool). Removed workspace members/path entries and dependency lines in `crates/zed/Cargo.toml` and `crates/agent/Cargo.toml`, and the `web_search::init`/`web_search_providers::init` calls in `crates/zed/src/main.rs` and `crates/zed/src/zed.rs`.
- [x] Confirmed `crates/edit_prediction` doesn't hard-depend on the cloud provider for its core function — `cargo check -p edit_prediction` passes standalone; its `EditPredictionUsage`/`RequestUsage` usage-display plumbing is addressed in Part 5.
- Verification run: `cargo check -p language_models -p agent -p agent_ui -p ai_onboarding -p onboarding -p title_bar -p client -p cloud_api_types -p edit_prediction` all pass clean. `cargo check -p zed` (and any workspace check that pulls in `gpui_apple`) cannot run in this environment — the Metal shader compiler requires full Xcode, and only Command Line Tools are installed here (`xcrun: error: unable to find utility "metal"`). This is a pre-existing environment gap, not caused by this change; flagged to M rather than silently skipped.

### Part 2 — Remove standalone upsell/badge UI files
- [x] Deleted `crates/agent_ui/src/ui/end_trial_upsell.rs` and both call sites in `agent_panel.rs` (the `should_render_trial_end_upsell`/`render_trial_end_upsell` functions and the `TrialEndUpsell` dismiss struct, handled together with Part 3 since they were the same code block).
- [x] Deleted `crates/title_bar/src/plan_chip.rs`; removed its import and the `PlanChip::new(plan)` render call; simplified `organizations: Vec<_>` to drop the per-org `Plan` lookup (`plan_for_organization` had no other caller left).
- [x] Deleted `crates/ai_onboarding/src/plan_definitions.rs` and `crates/ai_onboarding/src/young_account_banner.rs` (the latter not in original scope — discovered its only purpose was gating trial eligibility by account age, which no longer exists).
- Verified: `cargo check -p agent_ui -p title_bar -p ai_onboarding` all pass clean with zero warnings.

### Part 3 — Rewrite onboarding/agent-panel surfaces that embedded plan state
- [x] Rewrote `crates/ai_onboarding/src/ai_onboarding.rs` (`ZedAiOnboarding`, the actual struct name — the plan's research called it `AiUpsellCard`): dropped the `plan`/`account_too_young` fields and all six plan-tier renderers (free/trial/pro/business/vip/student), replaced with one signed-in state offering Zed's built-in edit-prediction model only (the chat "Zed AI" provider no longer exists after Part 1, so `AgentPanelOnboarding` stopped using this component entirely — see below).
- [x] Rewrote `crates/ai_onboarding/src/agent_panel_onboarding_content.rs`: since the chat cloud provider is gone, `AgentPanelOnboarding` no longer has anything to offer besides BYO-key setup; it now always renders `ApiKeysWithoutProviders` (previously conditional) with a dismiss button, and no longer takes `user_store`/`client`/plan-state params.
- [x] Pruned `crates/ai_onboarding/src/edit_prediction_onboarding_content.rs`: dropped the `is_free_plan` check and `Plan` import; the GitHub Copilot alternative is now always shown instead of being plan-gated.
- [x] Removed the "Zed Agent" quick-setup button (`render_zed_agent_button`) entirely from `crates/onboarding/src/basics_page.rs` — not just its plan-tier state text as originally scoped, since the button offered signing in to a chat provider that no longer exists. Adjusted the agent grid's column count and removed the now-fully-unused `user_store` parameter threaded through `render_basics_page` → `render_ai_section`.
- [x] Removed the plan-name telemetry field (`zed_agent = zed_agent_state`) from `crates/onboarding/src/onboarding.rs`'s "Welcome Agent Setup Viewed" event, since it measured the now-deleted button; kept the unrelated `agents_installed` field. Removed the now-fully-unused `user_store` field from the `Onboarding` struct.
- [x] Removed `should_render_trial_end_upsell`, `render_trial_end_upsell`, `TrialEndUpsell`, and the `ResetTrialEndUpsell` dev action from `crates/agent_ui/src/agent_panel.rs`. Simplified `should_render_new_user_onboarding` to drop its Pro-plan auto-dismiss branch and the dead `!= ZED_CLOUD_PROVIDER_ID` provider filter (no provider will ever match that ID again). Kept `OnboardingUpsell`/`ResetTrialUpsell` since they gate the general (non-billing) "configure a provider" card, not a trial.
- [x] Removed the now-fully-unused `user_store` field from `AgentPanel` (agent_ui) and `Thread` (agent crate) — both were only read for plan/billing checks that no longer exist.
- Verified: `cargo check -p agent_ui -p ai_onboarding -p onboarding -p agent` all pass clean with zero warnings.

### Part 4 — Implement the auto-retry replacement
- [x] Decided (M: "use your best judgement"): always retry. The plan-gated branch only ever guarded the now-deleted Zed cloud provider (`ZED_CLOUD_PROVIDER_ID`); with that provider gone there's no remaining case to special-case, so `handle_completion_error` gets the same unconditional-retry behavior every other provider already had.
- [x] Implemented: removed the `plan: Option<Plan>` parameter, the `Plan` import, and the `provider_id == ZED_CLOUD_PROVIDER_ID` branch from `handle_completion_error`. Removed the now-unused `cloud_api_types`/`cloud_llm_client` dependencies from `crates/agent/Cargo.toml`.
- Verified: `cargo check -p agent` passes clean with zero warnings.

### Part 5 — Delete the core plan types and `UserStore` plumbing
- [x] **Correction to the plan, discovered via write-scope rescan, not a new tradeoff:** `crates/cloud_api_types/src/plan.rs` (`Plan`/`PlanInfo`/`SubscriptionPeriod`) is **not deletable** — `GetAuthenticatedUserResponse.plan: PlanInfo` and `.plans_by_organization` are non-optional fields in the real zed.dev API response this client still deserializes. Deleting the types would break sign-in entirely against the actual backend. Kept the types; removed only the client-side *presentation* of them (see below). This corrects the plan's Decision section, which assumed the types were purely UI-facing.
- [x] Removed `plan_info: Option<PlanInfo>` and `plans_by_organization: HashMap<OrganizationId, Plan>` fields and their storage from `crates/client/src/user.rs` — the response is still fully deserialized (required for the API contract), but the plan/tier data is now discarded instead of stored, since nothing reads it anymore.
- [x] Removed `plan()`, `plan_for_organization()`, `subscription_period()`, `trial_started_at()`, `account_too_young()`, `has_overdue_invoices()`, and `ZED_SIMULATE_PLAN` env var handling from `UserStore`. Renamed `clear_plan_and_usage()` → `clear_edit_prediction_usage()` (rule 8: name describes what it now does).
- [x] Removed the unused `Event::PlanUpdated` variant (was never emitted or subscribed anywhere — pre-existing dead code, found while touching this area).
- [x] **Correction to the plan:** kept `zed_urls::account_url()` — it has three legitimate non-billing callers (title bar's "click username" menu entry, the app-wide `OpenAccountSettings` command, onboarding's `OpenAccount` action) that manage sign-in/account state, not pricing. Removed `start_trial_url()` and `upgrade_to_zed_pro_url()`, which had no non-billing meaning and zero remaining callers after Parts 2-4.
- [x] Removed the `ThreadError::ZedPaymentRequired` variant, its telemetry text, its `render_zed_payment_required_error`/`upgrade_button` UI (both discovered via `upgrade_to_zed_pro_url`'s last caller, not in original scope), and its now-unreachable test in `crates/agent_ui/src/conversation_view.rs` / `thread_view.rs` — the guard that constructed it (`provider == ZED_CLOUD_PROVIDER_NAME`) can never match now; `PaymentRequired` for any other provider already fell into the generic error path.
- [x] Removed the pro/pricing upsell UI in `crates/edit_prediction_ui/src/edit_prediction_button.rs` (not in original scope — discovered during write-scope rescan): the usage bar + "Subscribe to increase your limit", "Upgrade to Zed Pro or contact us" account-age nag, and "outstanding invoice"/billing-support nag, all in the edit-prediction status menu. Kept the status-bar color indicator (informational, not billing text) and the underlying `EditPredictionUsage` tracking, since it's still used for that indicator.
- [x] Removed the now-dead `account_too_young() || has_overdue_invoices()` early-return gate in `crates/edit_prediction/src/zed_edit_prediction_delegate.rs` (both methods removed from `UserStore`).
- Verified: `cargo check -p client -p agent_ui -p edit_prediction -p edit_prediction_ui` all pass clean; `cargo clippy --lib -- --deny warnings` clean on `client`, `cloud_api_types`, `settings_content`, `language_models`, `agent`, `agent_ui`, `title_bar`, `onboarding`, `edit_prediction`, `edit_prediction_ui`.

### Part 6 — Final sweep and acceptance check
- [x] Ran the acceptance-criteria greps across `crates/` (excluding `crates/collab`): zero matches for `cloud_api_types::Plan` (outside `cloud_api_types`/`client/test.rs`'s required fixture), `PlanChip`, `AiUpsellCard`, `EndTrialUpsell`, `PlanDefinitions`, `ZedDotDevSettings`, `zed_dot_dev`, `start_trial_url`, `upgrade_to_zed_pro_url`, and every "Upgrade to Zed Pro" / "Start Free Trial" / "Choose a Plan" style string.
- [x] Manual run: agent panel, onboarding, title bar account menu, AI provider settings — M confirmed on 2026-09-30 ("works well") after the Metal gap was closed.
- [x] `crates/collab` and `docs/` (mdbook) confirmed untouched, as scoped — see Open questions.
- [x] Acceptance criteria: 3 of 4 checked by grep/compiler; the manual-run criterion is blocked by the environment gap below.

**Environment gap, disclosed rather than worked around silently:** this sandbox has only Xcode Command Line Tools installed, not full Xcode, so `xcrun -find metal` fails and every crate that pulls in `gpui_apple` (which includes the `zed` binary itself, and transitively `eval_cli`/`edit_prediction_cli`, and any crate's `--tests` target) cannot be built or run here. This is a pre-existing environment limitation, not caused by this change — confirmed by reproducing the identical failure against an untouched crate before starting. Every crate actually touched by this plan was verified with `cargo check -p <crate>` (and `cargo clippy --lib -- --deny warnings` on the core ones), all passing clean. What could **not** be verified in this environment: the `zed` binary itself building or launching, any `--tests` target compiling (test-only code paths were checked by direct reading instead), and the manual UI walkthrough. M should run `cargo build` and launch the app locally to close this gap before merging.

## Standardized review

### At a glance

- **Bottom line:** present after fixing 1 blocking
- **Where we are:** reviewing this plan file, complete; waiting on 1 decision
- **Need from M:** 1 decision (auto-retry replacement behavior, already flagged in the plan itself as Part 4/Open question 3) plus explicit sign-off on Option A (removing the Zed-hosted AI provider) and on Open question 1 (collab out of scope).
- **Why:** the plan correctly surfaces its one hard tradeoff (auto-retry fork) as a decision for M rather than guessing, but currently schedules that decision in Part 4, after three parts of destructive edits are already committed — sequencing risk, not a soundness problem.
- **Intended outcome:** M can hand this plan to a build pass that removes every Zed Pro/pricing surface from the client without inventing scope or silently deciding tradeoffs.
- **Findings:** 1 blocking, 2 must-verify, 1 note.

### Decisions needed

1. **Finding 1** — Move the Part 4 auto-retry decision earlier, or get M's answer now in chat before Part 1 starts, so no committed part depends on a still-open decision. Recommendation: ask now, fold the answer into Part 4 as written, keep part order.

### Findings

#### 1. BLOCKING — A later part's scope depends on a decision deferred to an even later part

- **Why it matters:** Parts 1-3 delete the provider, upsell widgets, and onboarding branching before M has chosen the auto-retry replacement in Part 4. If M's answer to Part 4 turns out to require touching `agent/thread.rs` in a way that interacts with anything deleted in Parts 1-3 (e.g. if "retry based on provider type" needs to know which providers still exist), the plan has already foreclosed information M needed to decide. This is a sequencing violation of rubric criterion 5 (parts ordered by real dependencies) and criterion 10 (open choices surfaced *before* execution, not mid-execution).
- **Recommendation:** Resolve the Part 4 question in the sign-off conversation, before Part 1 begins, rather than mid-build. The plan already frames the right question and options — it just schedules asking it too late.
- **Need from M:** Answer to "should `handle_completion_error` always retry after the plan-gated fork is removed, or retry based on provider type only, now that the Zed-hosted provider is gone?"

#### What is wrong

- **Criterion:** Sequencing and dependencies
- **Where:** Part 4, and the Decision section's "Consequences" bullet on `handle_completion_error`
- **Problem:** A material, plan-altering decision is scheduled after three destructive parts instead of before any of them, even though the plan itself (correctly) says this decision must not be guessed.
- **Fix:** Plan structure — ask the question in the chat briefing / sign-off step, record the answer in Part 4's task list now, keep the part order otherwise unchanged (Part 4 still does the mechanical edit).

#### Evidence

- [verified] `crates/agent/src/thread.rs:3372-3420` (spot-checked structurally via the research report; not independently re-read line-by-line in this review pass — see Scope) contains the plan-gated fork the plan itself calls out as needing a decision.
- [verified] Plan's own "Parts" section places this decision at Part 4, after Parts 1-3 which delete the cloud provider and its UI.

#### Alternatives

- Leave the order as-is and accept that Part 4 might require reopening Parts 1-3; costs a possible re-do pass, no data risk since nothing is destructive to user data. Rejected as the weaker option because it wastes build effort for no benefit — the question is answerable right now, before any code moves.

#### 2. MUST-VERIFY — `cloud_api_types` crate deletability is asserted before verification

- **Why it matters:** The plan states as fact in "What is true today" that only 8 of 18 dependent crates use `Plan`/`PlanInfo`, but Part 1 still lists "verify whether the crate is deletable wholesale" as a task — meaning the plan is honest that this is unverified, but the file structure (`extension.rs`, `internal_api.rs`, `known_or_unknown.rs`, `timestamp.rs`, `websocket_protocol.rs` alongside `plan.rs`) I spot-checked directly shows the crate is clearly NOT plan-only, so "delete the whole crate" in Part 5 is very unlikely to be the right outcome.
- **Recommendation:** Update Part 5 now to say "delete `plan.rs` and its module export from `cloud_api_types.rs`" as the expected path, keeping "delete whole crate" only as a fallback if Part 1 finds otherwise. This isn't a blocking issue since Part 1 already gates it, but leaving it phrased as a live 50/50 choice under-informs the build pass.
- **Need from M:** Nothing — this is a plan-wording tightening, not a decision.

#### What is wrong

- **Criterion:** Current-system accuracy
- **Where:** Design section ("target end state... or is deleted wholesale if plan.rs turns out to be its only content") and Part 5
- **Problem:** Direct inspection (this review) already answers the question Part 1 was going to ask; the plan doesn't yet reflect that.
- **Fix:** Plan text — state directly that `cloud_api_types` keeps `extension.rs`, `internal_api.rs`, `known_or_unknown.rs`, `timestamp.rs`, `websocket_protocol.rs`, and only `plan.rs` is removed.

#### Evidence

- [verified] `crates/cloud_api_types/src/` contains `cloud_api_types.rs`, `extension.rs`, `internal_api.rs`, `known_or_unknown.rs`, `plan.rs`, `timestamp.rs`, `websocket_protocol.rs` (directory listing, this review pass).
- [verified] `crates/cloud_api_types/src/plan.rs` imports `crate::{KnownOrUnknown, Timestamp}` from sibling modules, confirming those modules are independent and would need to survive plan.rs's deletion.

#### 3. MUST-VERIFY — Edit-prediction's core-vs-billing coupling is stated as a research finding, not confirmed

- **Why it matters:** The plan's Part 1 already schedules verifying whether `crates/edit_prediction` depends on the cloud provider for its core (non-billing) function — this review's own quick grep for a direct "cloud provider" reference inside `edit_prediction.rs` found nothing, which is consistent with the plan's caution but does not by itself prove the feature is independent (edit predictions could route through the cloud provider indirectly via the model registry, not by a literal string match).
- **Recommendation:** Keep Part 1's verification step as-is; it is already correctly scoped as "verify before Part 5," not skipped. No plan change needed.
- **Need from M:** Nothing.

#### What is wrong

- **Criterion:** Current-system accuracy
- **Where:** Part 1, edit-prediction verification task
- **Problem:** n/a — this is confirming the plan already handles the uncertainty correctly; flagged as must-verify only so the build pass doesn't skip this check under time pressure.
- **Fix:** None needed; keep the existing Part 1 task.

#### Evidence

- [verified] `grep -n "cloud" crates/edit_prediction/src/edit_prediction.rs` for cloud-provider-specific terms returned no direct match in this review pass (a narrower check than the original research, which found `EditPredictionUsage`/`RequestUsage` type usage there, not a literal "cloud provider" reference).

#### 4. NOTE — `docs/` (mdbook) location was avoided for the plan file itself, but the plan's Open Question 2 leaves docs-site pricing references unresolved

- **Why it matters:** Small scope-completeness gap: the plan explicitly excludes the mdbook docs site from Parts 1-6 but doesn't close the loop on whether that's acceptable to M or deferred to a follow-up.
- **Recommendation:** Fine to leave as an explicit open question (which it already is) rather than in scope — just confirm during sign-off rather than after the fact.
- **Need from M:** Answer already requested as Open question 2 in the plan; no additional action beyond what the plan already asks.

#### What is wrong

- **Criterion:** Scope and cohesion
- **Where:** Non-goals section and Open questions 2
- **Problem:** n/a — correctly scoped out, just calling it out so it isn't lost in the briefing.
- **Fix:** None needed.

#### Evidence

- [verified] Non-goals section explicitly excludes docs/marketing; Open questions section repeats it as question 2.

### Criterion coverage

- **Goal fidelity — pass.** Goals/Non-goals section states observable outcomes (no plan text/buttons anywhere, no calls to the three URL builders) and explicit exclusions (collab, new UI). Fix: None.
- **Current-system accuracy — violation.** Finding 2 shows one claim (crate deletability) is phrased as more uncertain than direct inspection now supports. Fix: Finding 2.
- **Scope and cohesion — pass.** Every part maps to the stated goal; no unrelated cleanup bundled in. Fix: None.
- **Architecture and dependency direction — pass.** Design correctly sequences provider removal (the root cause of the plan-tier concept existing at all) before deleting the shared type, respecting real dependency direction. Fix: None.
- **Sequencing and dependencies — violation.** Finding 1: Part 4's decision should be resolved before Parts 1-3 execute. Fix: Finding 1.
- **Completeness — pass.** Code, UI, telemetry, dev actions, persisted dismissal keys, and docs/collab exclusions are all explicitly addressed or explicitly deferred (not silently dropped). Fix: None.
- **Failure and recovery — n/a.** This is a removal task with no concurrent users, no live migration, and no data integrity risk beyond two orphaned local DB keys, which the plan already calls out as harmless. No retry/timeout/concurrency surface applies.
- **Verification — pass.** Each part has an observable done condition (`cargo check` scope, specific greps, manual walkthroughs); acceptance criteria are Given/When/Then and independently checkable.
- **Feasibility and precision — violation.** Same root cause as Finding 2 — one claim presented with more uncertainty than warranted; otherwise all named files were spot-checked and exist at approximately the stated locations. Fix: Finding 2.
- **Decision readiness — violation.** Finding 1: the auto-retry tradeoff is correctly identified as needing M's input (this plan does not guess it), but its timing in the sequence is wrong. Fix: Finding 1.

### What is working

- Options table correctly identifies the one real architectural fork (keep vs. remove the Zed-hosted provider) and gives a defensible recommendation with named tradeoffs rather than picking silently [plan file, Options considered section].
- `crates/cloud_api_types/src/plan.rs:7-39` matches the plan's description of `Plan`/`PlanInfo` exactly, including the `KnownOrUnknown` fallback-to-Free behavior [verified, this review].
- `crates/agent_ui/src/agent_panel.rs:6209-6229` (`should_render_trial_end_upsell`) matches the plan's description closely enough that Part 3's removal task is directly actionable [verified, this review].

### Scope and limits

- **Reviewed:** the full plan file; spot-checked `crates/cloud_api_types/src/plan.rs`, `crates/cloud_api_types/src/` directory contents, `crates/language_models/src/language_models.rs` provider registration, `crates/agent_ui/src/agent_panel.rs:6195-6240`, `crates/title_bar/src/title_bar.rs` plan_chip usage, `crates/edit_prediction/src/edit_prediction.rs` for direct cloud-provider references.
- **Not reviewed:** `crates/language_models/src/provider/cloud.rs` in full, `crates/client/src/user.rs` in full, `crates/ai_onboarding/*` in full, `crates/onboarding/*` in full — relied on the prior research agent's line citations for these, cross-checked only the files above.
- **Checks not run:** no `cargo check` was run against a hypothetical diff (there is no diff yet — this is a plan review, not a code review).
- **Confidence:** high on sequencing/decision-readiness findings (directly inspectable in the plan text); moderate on current-system-accuracy findings for files not independently re-read in this pass (relying on the earlier research agent's citations, which were themselves evidence-backed).

### In plain terms

Get M's answer on the auto-retry replacement behavior before Part 1 starts, not during Part 4 — the plan already asks the right question, just too late in the sequence. Tighten Part 5's wording to say "delete `plan.rs` only" as the expected outcome rather than presenting crate-wide deletion as equally likely, since a direct look at the crate's files already answers that. Everything else in the plan is ready to build against.

## Changes made

- Part 1: Deleted the Zed-hosted "Zed AI" cloud LLM provider and its exclusive-dependency crate `language_models_cloud`. Deleted the agent's built-in web search tool and its only provider (`web_search_providers`, `web_search`), since M approved that loss after it was flagged as a discovered functional dependency. Deleted `ZedDotDevSettings`/related settings schema types (found via compile error after the provider deletion, not in original research).
- Part 2: Deleted `end_trial_upsell.rs`, `plan_chip.rs`, `plan_definitions.rs`, `young_account_banner.rs` (the last was a new discovery).
- Part 3: Rewrote `ZedAiOnboarding`/`AgentPanelOnboarding`/`EditPredictionOnboarding` to drop all plan-tier branching; removed the "Zed Agent" quick-setup button from the Welcome page; removed `should_render_trial_end_upsell`/`render_trial_end_upsell`/`TrialEndUpsell`; removed now-dead `user_store` fields from `AgentPanel`, `Thread`, and `Onboarding`.
- Part 4: `handle_completion_error` always retries now; removed the plan-gated fork.
- Part 5: Corrected the plan's assumption that `cloud_api_types::Plan`/`PlanInfo` could be deleted — they can't, the real zed.dev API response requires them; kept the wire types, removed the client-side presentation (`UserStore` accessors). Corrected the plan's assumption that `account_url` should be removed — kept it (three legitimate non-billing callers), removed `start_trial_url`/`upgrade_to_zed_pro_url`. Removed `ThreadError::ZedPaymentRequired` and the pro-pricing upsell UI in `edit_prediction_button.rs` (both new discoveries, not in original research).
- Part 6: Acceptance-criteria greps all clean. Manual UI walkthrough blocked by this environment's Metal-toolchain gap (see Part 6 above) — needs M to verify locally.
- 2026-09-30: Metal gap closed (full Xcode selected; `xcodebuild -downloadComponent MetalToolchain` installed Metal Toolchain 27A266a). `cargo build -p zed` passes (only warnings: debug-binary `__eh_frame` linker note and upstream `block v0.1.6` future-incompat). `cargo test -p agent -p agent_ui -p client -p language_models`: 1348 passed, 0 failed, 11 ignored (all pre-existing; diff adds no `#[ignore]`). App launched from `target/debug/zed`; manual UI walkthrough pending M.
- Every touched crate verified with `cargo check -p <crate>`; core crates also verified with `cargo clippy --lib -- --deny warnings`, all clean, zero warnings.

## Open questions

1. Confirmed as non-goal: `crates/collab` (Zed's server) was not touched. M does not run their own collab server (assumed, not explicitly re-confirmed after the plan changed shape).
2. `docs/` (the mdbook site) and any marketing/README references to Zed Pro are out of scope for this plan (client code only) and were not touched.
3. **New, needs M's answer:** this environment cannot build the `zed` binary or run tests (Metal toolchain missing — see Part 6). M should run `cargo build` and launch the app locally, and run the test suite, before merging.
