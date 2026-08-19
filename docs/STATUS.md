# Implementation status

This file is the durable execution ledger for the current milestone. Product and architecture authority remains in `docs/PLAN.md`.

## Ledger update rule

Every commit must include the corresponding update to this file: task status, decisive verification evidence, and next action. Roadmap or acceptance-criteria changes must also update `docs/PLAN.md`. Because a commit cannot record its own hash before it exists, the next task begins by reconciling the preceding row with the actual commit and push result.

## Authority envelope

- Mode: implementation
- Authorized repository: this AgentMeter checkout
- Allowed mutations: edit and verify repository files; commit each completed step and push it to GitHub `github/main`
- External mutations: GitHub pushes are authorized; no deploy, release, repository rename, Amp-origin push, or production data operation
- Data boundary: synthetic fixtures only; do not copy real prompts, responses, code, account identifiers, secrets, or home paths into the repository

## Operational reality

- Base revision: `3898451` (`Render source health in the Sources view`)
- Branch: `main`, tracking GitHub `origin/main` at `topit/agentmeter`; M1, M2, and M3-01 through M3-03 have been pushed
- The earlier Amp-hosted remote is no longer configured in this checkout; pushes go to `origin`
- Toolchain: Rust 1.96.0; repository checks are defined in `AGENTS.md`
- Database runtime selected for M1: bundled SQLite through `rusqlite`; no external database service

## M1 task ledger

| Task | Owner | Base | Depends on | Status | Required verification | Result / next action |
|---|---|---|---|---|---|---|
| M1-01 Schema and storage | coordinator | `01c9532` | — | completed | storage tests; workspace check/test/clippy | schema v1, atomic ingestion, diagnostics, checkpoints, UTC projection and JSON report implemented |
| M1-02 Reference collectors | coordinator | `01c9532` | core ingestion contract | completed | append/rewrite/truncation/parser-version tests | JSONL and mutable snapshot reference adapters implemented |
| M1-03 Fixture policy | documentation worker | `01c9532` | accepted privacy rules | integrated | coordinator review; `git diff --check` | `docs/FIXTURES.md` accepted |
| M1-04 Integration | coordinator | `01c9532` | M1-01, M1-02, M1-03 | integrated | full repository checks and reference pipeline test | all Tier 1 checks passed; ready for local milestone commit |

## M2 task ledger

| Task | Owner | Base | Depends on | Status | Required verification | Result / next action |
|---|---|---|---|---|---|---|
| M2-01 Amp Stream JSON | coordinator | `dbbd1af` | M1 ledger | pushed | fixture tests; collector-to-storage pipeline; workspace checks | committed and pushed as `5ac3e1e` |
| M2-02 Amp local history | coordinator | `5ac3e1e` | undocumented schema research | pushed | reconciliation fixtures; independent parser cross-check; workspace checks | committed and pushed as `fcc9481` |
| M2-03a Codex JSONL core | coordinator | `fcc9481` | official protocol research | pushed | cumulative/incremental/archive fixtures; storage pipeline; workspace checks | committed and pushed as `cabf805` |
| M2-03b Codex lineage | coordinator | `cabf805` | official history lineage | pushed | paginated/legacy fork, replay, revert and archive fixtures | committed and pushed as `5b54c3e` |
| M2-04 Pi | coordinator | `5b54c3e` | upstream format research | pushed | fixture and cross-check suite | committed and pushed as `75ec3c7` |
| M2-05 Source health | coordinator | `27372d3` | collector diagnostics and checkpoints | pushed | portable health-state tests; storage integration; workspace checks | committed and pushed as `12f5539` |
| M2-06 Cost facts | coordinator | `12f5539` | canonical event-cost ingestion contract | pushed | provider-cost atomicity; Pi pipeline; workspace checks | committed and pushed as `8080c26` |
| M2-07 Reconciliation report | coordinator | `8080c26` | complete collector facts | pushed | deterministic reconciliation/cross-check fixtures; workspace checks | committed and pushed as `e5d77ee` |
| M2-08 Variant assessment | coordinator | `e5d77ee` | upstream Codex/Pi contracts | pushed | official-source evidence; variant fixtures where supported; workspace checks | committed and pushed as `eb161d5` |
| M3-01 GPUI window shell | coordinator | `eb161d5` | tested GPUI revision | pushed; macOS validation blocked | macOS compile; locale/theme shell tests; representative UI verification | committed and pushed as `eddbcbf`; native build is blocked by the runner's missing Metal Toolchain |
| M3-02 Overview snapshot | coordinator | `eddbcbf` | portable shell and aggregate queries | pushed | snapshot-generation tests; async stale-result tests; workspace checks | committed and pushed as `a6d6002` |
| M3-03 Overview presentation | coordinator | `a6d6002` | immutable overview snapshot | pushed; native visual validation pending | off-render-path loading tests; localized state tests; representative UI verification | committed and pushed as `ec266ee`; runtime visual matrix remains blocked by the Metal Toolchain |
| M3-04 Sources presentation | coordinator | `ec266ee` | source-health snapshots | pushed | localized remediation-state tests; stale-result tests; representative UI verification | committed and pushed as `3898451`; native visual validation remains blocked by the missing Metal Toolchain |
| M3-05 Settings presentation | coordinator | `3898451` | preferences persistence contract | pushed | preference round-trip tests; persistence and stale-save tests; workspace checks | committed and pushed as `b28fea0` |
| M3-06 CI native validation | coordinator | `b28fea0` | public-repository CI budget | pushed | green macOS runner runs; reviewed screenshot matrix artifacts | committed and pushed as `9952d9a` and `e597ab0`; evidence recorded in `9c2eb9d` whose CI run is green; M3 exited |
| M4-01 Kimi/Kimi Code adapter | coordinator | `9c2eb9d` | upstream wire.jsonl format research | completed | fixture and cross-check suite; storage pipeline; workspace checks | Kimi wire adapter implemented for both legacy and current layouts; the next task reconciles this row with the actual commit and push |

## Verification contract

- Tier 0 while iterating: affected crate tests plus workspace type checking when contracts change.
- Tier 1 candidate: formatting, workspace check, all tests, Clippy with warnings denied, migration tests, and collector-to-storage pipeline test.
- Tier 2 is not required: this milestone does not package, deploy, or touch a production database.
- Broad-run budget: one candidate run, plus one rerun only if candidate-affecting fixes are required.

## Accepted M1 decisions

- A checkpoint lookup returns `NeverIngested`, `Current`, or `Invalidated`; parser-version invalidation must drive a full source-owned `Rebuild`/`Replace`, never an Append from byte zero.
- Duplicate event IDs are idempotent only when normalized facts and semantic provenance match. Physical record offsets may differ for a repeated native event.
- Mutable snapshots always replace the events owned by that source in one transaction.
- Canonical events and checkpoints commit together; failed ingestion preserves both and records a visible failed run/source diagnostic.
- The first rebuildable projection is explicitly UTC (`daily_usage_utc`). User-timezone aggregation remains a later query/UI design task rather than being mislabeled as local time.
- Reconciliation reports contain counts, mode and warnings but no event or message content.

## Review record

- Primary independent review found one blocking parser-upgrade sequence, one semantic-provenance issue, and missing reference-format cases.
- The candidate now has an explicit rebuild contract, semantic provenance equality, an end-to-end parser-upgrade replacement test, and empty/malformed/schema-drift coverage for both reference formats.
- Focused re-review found all prior findings resolved, no introduced regression, and no remaining blocker.
- The original broad candidate evidence became stale after review fixes; one final broad rerun is authorized by the verification contract.

## M1 verification evidence

- `cargo fmt --all --check`: passed.
- `cargo check --workspace --all-targets`: passed.
- `cargo test --workspace`: passed (27 tests: 24 unit tests and 3 collector-to-storage integration tests).
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `git diff --check`: passed.
- Privacy-pattern scan found only the intentional synthetic `/fixture/home/...` paths permitted by `docs/FIXTURES.md`; no real home paths, credential signatures, or raw logs were found.

## M2-01 verification evidence

- `cargo fmt --all --check`: passed.
- `cargo check --workspace --all-targets`: passed.
- `cargo test --workspace`: passed (33 tests: 29 unit tests and 4 integration tests).
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- Amp-specific collector tests cover documented flat usage, observed iteration usage, top-level filtering, append, rewrite, malformed, zero-token, and incomplete-tail behavior.
- The Amp collector-to-storage integration test verifies resumed append and canonical daily aggregation.
- `git diff --check` and privacy-pattern scan passed; the sole path match is an intentional `/fixture/home/...` synthetic path.

## M2-02 verification evidence

- `cargo fmt --all --check`: passed.
- `cargo check --workspace --all-targets`: passed.
- `cargo test --workspace`: passed (40 tests: 35 unit tests and 5 integration tests).
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- Amp local-history tests cover discovery, full/partial ledger reconciliation, message-ID precedence, disagreement diagnostics, malformed/schema-drift input, invalid timestamps, negative/zero counters, and portable custom roots.
- Collector-to-storage integration verifies source-owned replacement removes stale events and rebuilds canonical daily usage.
- Reconciliation outcomes were compared with the current Tokens and Tokscale Amp parsers; AgentMeter deliberately avoids their inferred Anthropic provider and synthetic per-message timestamp.
- `git diff --check` and privacy-pattern scan passed; both path matches are intentional `/fixture/home/...` synthetic paths and fixtures contain no content-bearing message/tool fields.

## M2-03a verification evidence

- `cargo fmt --all --check`: passed.
- `cargo check --workspace --all-targets`: passed.
- `cargo test --workspace`: passed (47 tests: 41 unit tests and 6 integration tests).
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- Codex tests cover active/archive precedence, exclusive cache/reasoning normalization, `last_token_usage`, cumulative-only delta, equal snapshot suppression, last-only baseline synthesis, resumed parser state, rewrite recovery, malformed complete lines, null info, and incomplete tails.
- Collector-to-storage integration verifies a cumulative checkpoint resumes into exact canonical daily totals.
- A final state-machine review found and fixed last-only baseline loss before submission; its regression test proves the following cumulative snapshot does not count that usage twice.
- Official OpenAI Codex source defines the contract; Waku, Tokens/Tokscale, and ccusage were used only to cross-check normalization and cumulative behavior.
- `git diff --check` and privacy scanning passed; the sole path match is an intentional `/fixture/home/...` synthetic path and fixtures contain no transcript-bearing fields.

## M2-03b verification evidence

- `cargo fmt --all --check`, `cargo check --workspace --all-targets`, and `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo test --workspace`: passed (54 tests: 47 unit tests and 7 collector-to-storage integration tests).
- Official Codex source at commit `0acf302db5ffedea4b8ef0112f4cbcddd65cff57` defines filename-derived rollout identity, `history_base` pointer semantics, exclusive ordinal/byte cutoffs, archive lookup, and cycle handling.
- Paginated tests cover an archived parent, inherited cumulative baseline, child-only delta emission, missing lineage, cycle diagnostics, and branch-scoped identity when revert rollouts reuse an ordinal.
- Legacy tests require an exact parent usage prefix bounded at fork time, including checkpoint resume through an incomplete replay tail. Missing or divergent lineage remains visible and lowers retained usage confidence rather than relying on timestamp-density heuristics.
- Collector-to-storage integration verifies that parent usage plus a paginated child's advancing cumulative total aggregates exactly once.
- Parser version 2 invalidates only Codex source checkpoints so the corrected rollout-scoped identities and lineage baselines rebuild transactionally.
- `git diff --check` passed. Privacy scanning found only policy examples and intentional `/fixture/home/...` synthetic paths; no secrets, real user home paths, or transcript-bearing fields were added.

## M2-04 verification evidence

- `cargo fmt --all --check`, `cargo check --workspace --all-targets`, and `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo test --workspace`: passed (61 tests: 53 unit tests and 8 collector-to-storage integration tests).
- Official Pi source at commit `2509b5c037d366979f2febfce4174b88aeaadc6a` defines session roots, v1-v3 headers, append/rewrite behavior, assistant usage, native entry IDs, copied forks, compaction, branch summaries, and token subset semantics.
- Pi tests cover recursive discovery, stable source identity, assistant and summary usage, response-model precedence, reasoning normalization, fork deduplication, v1 offset identity, missing parents, schema drift, malformed/incomplete records, append checkpoints, and rewrite replacement.
- Collector-to-storage integration verifies copied fork usage is counted once while child and branch-summary work reaches canonical daily projections.
- Waku, Tokens, Tokscale, and ccusage were cross-checks only. AgentMeter additionally counts official summary usage, uses native lineage IDs, and does not convert componentless totals into guessed output.
- `git diff --check` passed. Privacy scanning found only policy examples and intentional `/fixture/home/...` synthetic paths; no source logs, content-bearing payloads, secrets, or real user paths were added.

## M2-05 verification evidence

- `cargo fmt --all --check`, `cargo check --workspace --all-targets`, and `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo test --workspace`: passed (63 tests: 54 unit tests and 9 storage/pipeline integration tests).
- `agentmeter-core` now owns immutable `SourceHealthSnapshot` domain types and structured state, permission, and remediation enums; paths are explicitly local-only facts that require redaction before export.
- One storage query derives installation/source identity, parser version, last scan/success/event, latest records changed, warnings, errors, and deterministic content generations from existing canonical tables without a schema migration.
- Explicit pre-batch collection failures are persisted with typed collection, permission, or unsupported-schema classification. Successful ingestion clears stale errors and proves granted permission without re-enabling a user-disabled installation.
- Integration coverage exercises missing setup, never-scanned sources, healthy and partial runs, warning- and failure-based unsupported schemas, generic errors, denied/recovered permission, disabled-state preservation across rediscovery, and stable/changing generations.
- Desktop localization maps every health state and remediation to complete English and Simplified Chinese messages; presentation code does not parse diagnostic prose.
- `git diff --check` passed. Privacy scanning found only policy examples and intentional `/fixture/home/...` synthetic paths; no real paths, source logs, content-bearing payloads, or secrets were added.

## M2-06 verification evidence

- Exact non-negative JSON decimals are parsed without `f64` into integer nano-USD; tests cover fractional, exponent, trailing-zero, negative, over-precise, and overflowing values.
- The core ingestion contract distinguishes provider-reported, API-equivalent estimate, subscription credit, and unpriced facts. USD amounts are prohibited for subscription credits and unpriced facts at persistence time.
- Usage, provenance, costs, and checkpoints share one SQLite transaction. Storage tests cover replay idempotence, changed-cost identity conflicts, invalid-cost rollback, and stale-cost deletion during source replacement.
- Pi parser version 2 retains authoritative source totals or exactly summed component costs for assistant, compaction, and branch-summary usage. Malformed costs warn without dropping valid token facts, and copied fork costs follow the same native-lineage deduplication as usage.
- `cargo fmt --all --check`, `cargo check --workspace --all-targets`, and `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo test --workspace`: passed (69 tests: 60 unit tests and 9 storage/pipeline integration tests), including the Pi adapter fixture and collector-to-storage cost path.
- `git diff --check` passed; the final privacy-pattern scan found only policy examples and intentional synthetic fixture paths.

## M2-07 verification evidence

- Due-source selection uses the latest successful full `Replace`, so frequent incremental appends cannot postpone periodic reconciliation. Only enabled, permission-granted sources are eligible, and an exact interval boundary is due.
- The Amp Stream pipeline proves a due source is passed through `IngestStart::Rebuild`, produces a source-owned `Replace`, preserves canonical totals, and advances the reconciliation watermark.
- Versioned deterministic JSON reports canonical buckets, event/confidence counts, source-total coverage, and explicit match/mismatch/partial/unavailable states without paths or content-bearing identifiers and diagnostics.
- Aggregate fixture/reference expectations are constrained to reviewed source UI/CLI, Waku, Tokens, Tokscale, ccusage, or fixture categories. Amp, Codex, and Pi storage pipelines all cross-check their expected adapter totals through the report API.
- `cargo fmt --all --check`, `cargo check --workspace --all-targets`, and `cargo clippy --workspace --all-targets -- -D warnings`: passed after one candidate rerun fixed a test-only macro import.
- `cargo test --workspace`: passed (71 tests: 62 unit tests and 9 storage/pipeline integration tests).
- `git diff --check` passed; the final privacy-pattern scan found only policy examples and intentional synthetic fixture paths.

## M2-08 verification evidence

- Current official Codex rollout code defines `.jsonl.zst` as a complete zstd-compressed JSONL stream in the same active/archive roots and prefers a plain sibling. AgentMeter mirrors that identity/precedence, decodes through the existing parser, always replaces compressed sources, and fails corrupt frames visibly.
- Persistent `codex exec` uses the normal rollout with source `exec`; no separate headless parser is needed. Ephemeral exec/App Server sessions have no rollout and are documented as unavailable. Non-CLI rollout sources are skipped with a warning instead of being mislabeled `codex-cli`.
- Codex parser version 3 rebuilds affected checkpoints for source classification. Synthetic tests cover active/archive/plain/compressed precedence, exact decompression, repeated replacement, corrupt zstd, accepted exec, and deferred `vscode` source behavior.
- Current Pi source and history confirm project-nested defaults and retained flat/custom directories share one JSONL contract. Recursive discovery already covered both; a focused fixture now proves simultaneous flat v1 and nested v3 discovery.
- `zstd` 0.13.3 is used directly for official compressed rollouts. Its Rust wrapper is MIT, the safe/sys layers are MIT or Apache-2.0, the minimum Rust version is below the workspace toolchain, and its bundled native implementation supports the macOS-first/Windows-later target boundary.
- `cargo fmt --all --check`, `cargo check --workspace --all-targets`, and `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo test --workspace`: passed (75 tests: 66 unit tests and 9 storage/pipeline integration tests).
- `git diff --check` passed; the final privacy-pattern scan found only policy examples and intentional synthetic fixture paths.

## M3-01 verification evidence

- GPUI 0.2.2 and `gpui_platform` are pinned to exact Zed revision `c24358d96cdb4ce14ecbc088462295353b0103f0`, the last upstream main revision before its Rust 1.97 toolchain bump; both crates declare Apache-2.0 and the workspace remains on Rust 1.96.0.
- GPUI dependencies are scoped to macOS. Portable `ShellState` owns the selected route, locale, theme preference, system appearance, and semantic palette; GPUI owns only window/view rendering.
- The first window provides localized Overview, Sessions, Sources, Models, Pricing, and Settings navigation, semantic light/dark colors, system appearance observation, pointer navigation, tab stops, button roles, accessibility labels, and visible keyboard focus.
- Portable tests cover default and changed routes, complete English/Chinese navigation, system and explicit theme resolution, and preserving route selection across locale/theme updates.
- `cargo fmt --all --check`, `cargo check --workspace --all-targets`, and `cargo clippy --workspace --all-targets -- -D warnings` passed on the Linux orb after resolving the exact dependency graph.
- `cargo test --workspace` passed (77 tests: 68 unit tests and 9 storage/pipeline integration tests).
- A Linux-to-`aarch64-apple-darwin` check advanced through GPUI dependencies with Clang but stopped in upstream `media` binding generation because the orb has no macOS SDK; this is an environment limitation, not a successful macOS compile.
- Native validation used a clean detached checkout of `eddbcbf` on an Apple Silicon runner with macOS 26.5.2, Xcode/CLT 26.6, and repository-pinned Rust 1.96.0. Formatting passed and the 69 non-desktop tests passed in an isolated synthetic-data environment.
- The macOS workspace check, tests, Clippy, desktop build, launch, and visual checks all stop before AgentMeter compilation because `gpui_macos` cannot invoke the missing Metal Toolchain. Xcode's component downloader also fails because its installed frameworks do not match the current macOS release; repairing that installation requires a system/toolchain update and was not authorized.
- Native compilation, launch, interaction, accessibility inspection, and the required representative light/dark English/Chinese visual matrix remain pending and must not be inferred from source inspection or portable tests.

## M3-02 verification evidence

- `agentmeter-core` owns an immutable `OverviewSnapshot` containing mutually exclusive token buckets, event/session/active-day/model counts, separately labeled provider-reported and API-equivalent cost totals, data-quality counts, and the complete source-health snapshot.
- Storage builds the snapshot directly from the canonical event ledger and cost facts. Missing cost kinds remain `None` rather than becoming a valid zero, UTC active days are explicit, provider/model identity remains separate, and unpriced events stay visible.
- A deterministic content generation covers every exposed headline fact plus the nested source-health generation. Repeated unchanged queries return the same snapshot; changed canonical data changes the generation.
- Portable desktop state issues single-use request generations and rejects out-of-order completions before they can replace the current immutable snapshot. GPUI and storage remain unaware of each other's lifecycle.
- `cargo fmt --all --check`, `cargo check --workspace --all-targets`, and `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo test --workspace`: passed (79 tests: 70 unit tests and 9 storage/pipeline integration tests).
- `git diff --check` passed; the privacy scan found only policy examples and intentional synthetic fixture paths.

## M3-03 verification evidence

- The new portable `agentmeter-app` crate owns local database directory creation, SQLite opening, migration, and immutable Overview queries. `apps/desktop` depends on this service but does not own filesystem or database work.
- GPUI starts the synchronous SQLite service on its background executor, then applies the result on the entity context through the existing single-use request generation. No database or filesystem call runs during rendering.
- Presentation state distinguishes loading, genuine empty, populated, partial-data, data-directory error, and database error. Setup, unsupported-schema, and collection failures take precedence over empty totals so collection failure never appears as a valid zero.
- Overview renders a collection-health strip and token, session, active-day, model, provider-reported cost, and API-equivalent estimate cards. Missing cost kinds remain localized “not available” rather than `$0`; all new copy and errors have English and Simplified Chinese entries.
- Count and USD formatting tests cover both locales; service tests use temporary synthetic databases and verify both successful initialization and privacy-safe directory-error classification.
- On the Apple Silicon runner, a temporary `runtime_shaders` feature bypassed only the unavailable Metal CLI. The fixed GPUI revision exposed one Rust 2024 opaque-return lifetime overcapture; a precise capture fixed it, after which the final six-crate workspace candidate passed all-target type checking, all 84 tests, and Clippy with warnings denied.
- The Linux orb also passed `cargo fmt --all --check`, `cargo check --workspace --all-targets`, all 84 tests, and `cargo clippy --workspace --all-targets -- -D warnings` with the production dependency features.
- `git diff --check` passed; the privacy scan found only policy examples and intentional synthetic fixture paths.
- The committed dependency remains the production `font-kit` configuration, not `runtime_shaders`. Native launch, interaction, and the light/dark English/Chinese visual matrix still require the runner's Metal Toolchain and remain pending.

## M3-04 verification evidence

- The new `SourcesService` in `agentmeter-app` loads immutable `SourceHealthSnapshot`s through the shared local-database lifecycle (`LocalDataErrorKind` now classifies data-directory versus database failures for both overview and sources services; no raw paths or storage errors reach presentation).
- The Sources route distinguishes healthy, partial, setup-required, unsupported-schema, error, and disabled sources from the typed `SourceHealthState` only; status color comes from new `success`/`warning`/`danger`/`info` semantic palette tokens and the localized status label always renders beside it.
- Each card shows adapter identity, native-or-root path (local-only fact), source kind, parser version, permission, last scan/success/event as explicit UTC timestamps, records changed, warning count with verbatim source-native warnings, verbatim error text, and the typed remediation. No action buttons are rendered because no application-service collection command exists yet; remediation is guidance text, not an inert control.
- Portable state rejects out-of-order snapshot and error completions via single-use request generations. A snapshot containing setup-required or errored sources classifies as populated, never as "no sources configured".
- Every new message key (sources states, field labels, permission states, error copy) has English and Simplified Chinese entries in the same change; UTC timestamp formatting is tested against independently computed reference instants, including a negative instant and a leap day.
- Tests: 95 total (86 unit tests and 9 collector-to-storage integration tests). Sources state, card localization, service success/failure, and both-locale label coverage are covered portably; GPUI layout was type-checked but not launched.
- `cargo fmt --all --check`, `cargo check --workspace --all-targets`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, and `git diff --check` passed on the user's Apple Silicon Mac after temporarily adding `runtime_shaders` to `gpui_platform` locally; the bypass was reverted before committing and the committed dependency remains production `font-kit`. Native launch and the light/dark English/Chinese visual matrix remain blocked by the missing Metal Toolchain.

## M3-05 verification evidence

- `agentmeter-core` adds `LanguagePreference`, `AppearancePreference`, and `AppPreferences` with system-following defaults. Storage persists one JSON document under the existing v1 `preferences` key-value table — the table was already reserved in schema v1, so no schema migration was required and `SCHEMA_VERSION` stays 1. Unknown or malformed stored values surface as `InvalidPreference` errors instead of silently becoming defaults; storage tests cover defaults, cross-connection round-trips, and both corruption shapes.
- `agentmeter-app` adds `PreferencesService` load/save sharing the local-database lifecycle and `LocalDataErrorKind` classification; service tests cover round-trips across fresh connections and data-directory failure for both operations.
- Desktop `SettingsState` applies selections optimistically (immediate repaint, no restart), rejects out-of-order load and save completions through single-use request generations, clears the save error on each new attempt, and records only fresh failures. Saves persist the complete preference snapshot, so rapid consecutive selections stay last-write-wins. Portable tests cover stale load/error rejection, stale save rejection, optimistic selection, pre-load selection, and language/appearance resolution.
- The Settings route renders language (System / English / 简体中文, language names shown natively in both UI locales by convention) and appearance (System / Light / Dark) options as real buttons that invoke the `PreferencesService` save command on the background executor — no inert controls. Load failures classify as localized data-directory/database errors and save failures render a localized danger banner. Persisted preferences apply at startup and repaint existing windows live.
- Every new key ships with English and Simplified Chinese text in the same change.
- Tests: 106 total (97 unit tests and 9 collector-to-storage integration tests).
- `cargo fmt --all --check`, `cargo check --workspace --all-targets`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, and `git diff --check` passed on the user's Apple Silicon Mac with the temporary local-only `runtime_shaders` bypass, which was reverted before committing; the committed dependency remains production `font-kit`. Native launch and the visual matrix remain blocked by the missing Metal Toolchain.

## M3-06 verification evidence

- The repository is public, so GitHub-hosted macOS runners are free of minute quota. `.github/workflows/ci.yml` runs the four repository gates on Linux (fast portable feedback) and, on `macos-latest`, verifies the Metal Toolchain (`xcrun metal --version`), re-runs all four gates with production features (no `runtime_shaders` bypass), builds the desktop app, launches it, and captures a 12-screenshot matrix — Overview/Sources/Settings × English/Simplified Chinese × light/dark — through `LANG`, the `AppleInterfaceStyle` default, and `screencapture`, uploading the images as workflow artifacts.
- `Route::from_name` is the only production-code change: an automation hook that maps the `AGENTMETER_INITIAL_ROUTE` environment variable to a startup route so headless validation can capture every view. Unknown values keep the Overview default; the parser is covered by portable tests.
- Local gates for the change passed on the user's Mac with the usual temporary `runtime_shaders` bypass, reverted before committing.
- First hosted run `32140316545` (`9952d9a`) was green in 8m37s on the macOS job: Metal Toolchain verified, all four gates and 107 tests passed with production features, the app built and launched, and all 12 matrix screenshots were captured. Review found two defects: the system `AppleInterfaceStyle` default did not switch the app to dark, and a macOS 26 screen-capture TCC prompt overlaid most captures.
- Second hosted run `32141838068` (`e597ab0`) fixed both: the matrix now seeds the app's own `app_preferences` row per capture (exercising startup preference loading) and pre-grants screen capture in the system TCC database. Reviewed screenshots confirm dark palettes `#151617`/`#1f2022`, light palettes `#f7f7f8`/`#ffffff`, complete Chinese navigation and copy with no tofu or clipping, selected preference options matching the seeded values, and no permission dialogs. Six screenshots covering all three routes and all four locale/theme combinations were individually reviewed; the remaining six share the same seeding and rendering path.
- **M3 exit criteria are met**: representative UI states verified in all four locale/theme combinations from production-feature builds on hardware with a working Metal Toolchain. An interactive keyboard-focus and accessibility pass on a local Mac remains recommended before M5 packaging.
- Tests: 107 total (98 unit tests and 9 collector-to-storage integration tests).

## M4-01 verification evidence

- Upstream research: two Kimi products write `wire.jsonl` journals. The frozen Python `kimi-cli` uses `KIMI_SHARE_DIR`/`~/.kimi` with `{"timestamp", "message": {type, payload}}` records and reports usage on `StatusUpdate.token_usage` (with a provider `message_id`). The current TypeScript `kimi-code` uses `KIMI_CODE_HOME`/`~/.kimi-code` with flat `{"type", ..., "time"}` records and reports usage on `usage.record` (`usage.{inputOther,output,inputCacheRead,inputCacheCreation}`, `usageScope`). Facts were read from MoonshotAI/kimi-cli and MoonshotAI/kimi-code source; ccusage and tokscale were cross-checks only.
- Both layouts report one delta per completed LLM request, so records sum directly without a cumulative-to-delta state machine. `step.end` copies of per-step usage, `goal.update` budgets, and `token_counting.*` estimates are ignored; zero-usage lines are skipped with visible warnings.
- Deliberate divergence from ccusage/tokscale: session-scoped `usage.record`s (compaction, title generation) are real per-request deltas and are counted, with the scope retained in provenance notes; the community parsers skip them and undercount.
- Canonical buckets: `input_other`→input, `output`→output (reasoning included; `reasoning: 0` with a note), `input_cache_read`→cache read, `input_cache_creation`→cache write. Legacy files carry no model and stay unattributed; flat records strip the `kimi-code/` prefix and fall back to the checkpointed last concrete `llm.request` model for symbolic placeholders like `__kimi_env_model__`.
- Identity: legacy `message_id` is the native id; within one batch repeats keep the larger total with warnings and equal repeats are skipped; fallback fingerprints are versioned and scoped to source lineage. Flat records have no upstream ids and use a versioned per-source fingerprint over (time, raw model, buckets, scope). Checkpoints commit at newline boundaries, carry parser state across appends, and detect protocol-migration rewrites through prefix fingerprints (transactional Replace).
- Known v1 limitation recorded for follow-up: legacy `/fork` and the new CLI's forks copy wire prefixes into new session directories without lineage markers; cross-source duplicates are not yet deduplicated (ccusage also double counts them). Cross-batch identity conflicts surface as visible storage errors rather than silent double counting.
- `cargo fmt --all --check`, `cargo check --workspace --all-targets`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, and `git diff --check` passed on the user's Mac with the temporary local-only `runtime_shaders` bypass, reverted before committing.
- Tests: 120 total (110 unit tests and 10 collector-to-storage integration tests), including the Kimi adapter fixture suite (discovery for both layouts and subagents, delta parsing, duplicate/step-end handling, symbolic-model fallback, timestamp fallback, zero/malformed/truncated cases, append resume, migration rewrite, parser-state resume) and the collector-to-storage pipeline asserting exact canonical daily totals and a fixture cross-check.

## Development handoff

Handoff point: the M4-01 Kimi adapter commit on GitHub `topit/agentmeter`, branch `main`, written directly after `9c2eb9d`. The next task begins by reconciling this commit's push result and the CI run, then starts M4-02 (Factory Droid adapter).

### Product and implementation state

- M1 and M2 are complete. The canonical SQLite ledger, reference ingestion contracts, Amp/Codex/Pi collectors, source-health model, provider-reported costs, reconciliation reports, Codex compressed rollouts, and Pi legacy/current discovery are implemented with synthetic fixture coverage.
- M3 is complete and exited. GPUI is pinned to Zed revision `c24358d96cdb4ce14ecbc088462295353b0103f0`; the shell, Overview, Sources, and Settings are implemented and verified on hosted Apple Silicon runners with the full locale/theme visual matrix (see the M3-06 evidence). CI reruns the Linux gates, the production-feature macOS gates, and the matrix on every push to `main`.
- M4-01 (Kimi/Kimi Code) is implemented: both `wire.jsonl` layouts, delta semantics, duplicate handling, and the storage pipeline are covered by synthetic fixtures. The next bounded task is M4-02, the Factory Droid adapter (session settings snapshots, latest-per-session semantics per `docs/PLAN.md` section 7).

### Ownership map for the next task

- `crates/agentmeter-core/src/lib.rs`: portable domain contracts, including `SourceHealth`, `SourceHealthSnapshot`, typed states, permissions, remediation, and `AppPreferences`.
- `crates/agentmeter-storage/src/lib.rs`: `Database::source_health_snapshot()` and `Database::preferences()`/`set_preferences()`; preferences live in the v1 key-value `preferences` table as one JSON document under the `app_preferences` key.
- `crates/agentmeter-app/src/lib.rs`: application-service boundary for the local database lifecycle (`LocalDataErrorKind`/`LocalDataServiceError`) with `OverviewService`, `SourcesService`, and `PreferencesService`. All filesystem and database work belongs here, never in GPUI or desktop presentation state.
- `apps/desktop/src/overview.rs`, `sources.rs`, and `settings.rs`: reference patterns for single-use request generations, stale completion rejection, optimistic selection, and loading/empty/partial/error classification.
- `apps/desktop/src/i18n.rs`: typed exhaustive English and Simplified Chinese catalog, including permission, preference-option, and UTC timestamp helpers.
- `apps/desktop/src/gpui_app.rs`: the shell with Overview, Sources, and Settings rendering. Keep filesystem/database work on `background_executor`; apply results through the entity context. Views must use `ThemePalette` semantic tokens, not literal colors.

### M3 status: exited via CI evidence

M3 acceptance is complete: production-feature builds on hosted Apple Silicon runners (working Metal Toolchain) plus the reviewed 12-shot visual matrix from run `32141838068`. Every push to `main` re-runs the matrix automatically. The local Mac's missing Metal Toolchain no longer blocks any milestone work; repair it only when local interactive development (launching, keyboard-focus, and accessibility inspection) is desired, and re-verify interactively before M5 packaging.

The current milestone is M4; see the ledger above and the ownership map below.

### Required verification and macOS limitation

Run before each completion commit:

```sh
cargo fmt --all --check
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
git diff --check
```

The M3-04 and M3-05 candidates passed all gates on the user's Apple Silicon Mac after temporarily adding `runtime_shaders` to `gpui_platform` locally; the bypass was reverted before each commit and must never be committed as a substitute for production shader compilation.

Native launch and visual acceptance remain blocked on the user's Mac: the Metal Toolchain is absent (`xcrun metal --version` still fails). Repair likely requires updating macOS and restarting, then running `xcodebuild -runFirstLaunch`, `xcodebuild -downloadComponent MetalToolchain`, and `xcrun metal --version`. Treat system update/restart as user-approved work only when explicitly authorized.

### Workflow constraints

- Reconcile the preceding ledger row with its actual commit at the start of every task; M3-06 was reconciled to `9c2eb9d`, and M4-01 still needs reconciliation with its commit and CI run.
- Update this file before every commit. Update `docs/PLAN.md` in the same commit whenever scope, roadmap order, acceptance criteria, or the immediate next step changes.
- Commit each completed step and push only to GitHub `main`. In this checkout the GitHub remote is named `origin` (`git@github.com:topit/agentmeter.git`); port-22 SSH hangs on this network, so push through `ssh://git@ssh.github.com:443/topit/agentmeter.git` until the proxy forwards port 22.
- Use synthetic fixtures only. Never read or commit real agent histories, prompts, responses, tool payloads, account IDs, secrets, or real home paths.
- Do not call a client supported until every acceptance criterion in `docs/PLAN.md` is met.

## Budget ledger

- Budget source: user settings, amount not provided
- Budget total: UNKNOWN
- Consumed: UNKNOWN
- Integration/verification reserve: 20% policy applies, exact amount UNKNOWN
- Threshold state: UNKNOWN
- Cost decision: serialize schema and contract work; use one bounded documentation worker only
