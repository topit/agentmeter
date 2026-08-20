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

- Base revision: `45af315` (`Add the Sessions usage view`)
- Branch: `main`, tracking GitHub `github/main` at `topit/agentmeter`; M1 through M4-07 have been pushed
- The Amp-hosted remote is configured as `origin` for fetches only; pushes go to `github`
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
| M4-01 Kimi/Kimi Code adapter | coordinator | `9c2eb9d` | upstream wire.jsonl format research | pushed | fixture and cross-check suite; storage pipeline; workspace checks | committed and pushed as `8cf9974`; CI run `32207108342` green on Linux and macOS |
| M4-02 Codex Desktop adapter | coordinator | `8cf9974` | shared Codex home / container storage research | pushed | fixture and cross-check suite; storage pipeline; workspace checks | committed and pushed as `ef26398`; CI run `32208756268` green |
| M4-03 DeepSeek Harness | coordinator | `ef26398` | local contract research | deferred to v1.1 | research verdict; fixtures or documented explicit integration | user confirmed the product is the `@deepseek-ai/dsh` npm CLI (`dsh`); deferred 2026-08-19 to finish v1 core scope first; format research archived at `docs/research/deepseek-harness.md` |
| M4-04 Pricing snapshots and estimates | coordinator | `addcd76` | reviewed rate dataset | pushed | exact-integer estimate tests; matching precedence tests; repricing storage tests | committed and pushed as `3bf7a68`; CI run `32226694932` green |
| M4-05 Seed rate dataset | coordinator | `3bf7a68` | official pricing pages | pushed | per-rate official sources; integer conversion tests; workspace checks | committed and pushed as `35d1901`; CI run `32228441956` green |
| M4-06 Activity view | coordinator | `35d1901` | canonical ledger and reversible estimates | pushed; layout follow-up completed | UTC aggregation tests; stale-result tests; macOS compile; workspace checks | committed as `eb72ad2`; narrow-window follow-up `2a60a1f` and CI run `32239856279` are green |
| M4-07 Sessions view | coordinator | `2a60a1f` | canonical session ledger and cost facts | pushed | source-scoped aggregation tests; stale-result tests; macOS compile; workspace checks | committed and pushed as `45af315`; CI run `32242486063` green; next is Models/Pricing |
| M4-08 Models/Pricing views | coordinator | `45af315` | model ledger, reviewed rates, and reversible estimates | pushed | model aggregation; pricing provenance; stale-result tests; macOS compile; workspace checks | committed and pushed as `d415dbb`; deferred verification completed 2026-08-19 — full workspace gates (142 tests), CI run `32243890892` green, and reviewed Models/Pricing visual matrices |
| M4-09 JSON/CSV export | coordinator | `d415dbb` | privacy-reviewed export payload contract | completed | payload privacy tests; exact-decimal tests; stale-result tests; workspace checks | versioned JSON and CSV exports of the canonical event ledger implemented behind explicit Settings actions; the next task reconciles this row with the actual commit and push |
| M5-01 Ingestion orchestration | coordinator | `ddcfb2e` | collector-to-storage pipeline contracts | completed | end-to-end ingestion tests; checkpoint resume tests; health diagnostics; workspace checks | IngestionService, startup collection, and the Sources rescan command implemented; the next task reconciles this row with the actual commit and push |
| M5-02 Performance validation | coordinator | `64cdd87` | section 16 targets | pushed | synthetic-corpus benchmark; projection incrementality tests; workspace checks | committed and pushed as `b099cee`; CI run `32325397259` green |
| M5-03 Cancellable cold rebuild | coordinator | `b099cee` | source-owned transaction boundaries | pushed | cancellation resume tests; desktop cancel-command tests; workspace checks | committed and pushed as `0d171cf`; CI run `32344024791` green |
| M5-04 Retention and packaging skeleton | coordinator | `0d171cf` | open decisions 1 and 4 | pushed | retention behavior test; release-package workflow validated; workspace checks | committed and pushed as `378c907`; CI run `32367331877` green; release run `32367374166` produced a verified arm64 DMG |
| M5-05 First-run progress experience | coordinator | `378c907` | real first-run validation | pushed | progress reporting tests; collecting-state tests; workspace checks | committed and pushed as `a61be6c` (+ evidence `0519819`); CI green; DMG re-cut by release run `32370110427`; single-instance fresh scan verified at ~35 s / 74,736 events |
| M5-06 Privacy review and README | coordinator | `a61be6c` | docs/PLAN.md section 13 rules | completed | privacy-pattern scan; README refresh; workspace checks | repository privacy scan clean and the README now describes the real product, privacy guarantees, and first-run expectations; the next task reconciles this row with the actual commit and push |

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

## M4-02 verification evidence

- Research (verified from openai/codex source at HEAD plus a real desktop installation): the Codex desktop app drives the shared `app-server` daemon and persists through the same `LocalThreadStore`/`RolloutRecorder` path under the same `CODEX_HOME` (`~/.codex`), producing identical rollout JSONL and `.jsonl.zst` files. No new adapter or discovery root is needed. Desktop sessions are distinguished by `session_meta.payload.originator == "Codex Desktop"` with `source == "vscode"` (the app-server default, shared with the IDE extension, so `source` alone cannot separate them).
- The adapter now accepts upstream's interactive session sources (`cli`, `exec`, `vscode`, `chatgpt`, `atlas`, mirroring upstream `INTERACTIVE_SESSION_SOURCES`); service sources such as `mcp` remain skipped with a visible warning. Behavior for `cli`/`exec` sessions is unchanged.
- Client attribution: `originator` `Codex Desktop` → `codex-desktop`; `codex_vscode` or a bare `vscode` source → `codex-vscode`; everything else (including third-party harness originators such as `waku`) keeps `codex-cli`, with the raw source and originator recorded in provenance notes whenever attribution differs from the CLI.
- Parser version 4 invalidates Codex checkpoints so existing installations transactionally rebuild and pick up previously skipped desktop and IDE sessions.
- A sanitized Factory Droid research note (v1.1 backlog) is archived at `docs/research/factory-droid.md`; its key facts are cumulative per-session snapshots in `<sessionId>.settings.json` under `~/.factory/sessions`, own-totals-only ingestion to avoid subagent double counting, and mtime-based recency.
- `cargo fmt --all --check`, `cargo check --workspace --all-targets`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, and `git diff --check` passed on the user's Mac with the temporary local-only `runtime_shaders` bypass, reverted before committing.
- Tests: 121 total (111 unit tests and 10 collector-to-storage integration tests), including new interactive-source matrix and client-attribution fixtures (desktop, IDE extension, absent originator, third-party originator, CLI baseline) and the unchanged Codex pipeline suite.

## M4-04 verification evidence

- `agentmeter-pricing` now owns the full estimate core: `ModelRates` are integer nano-USD **per token** (realistic catalog prices stay integral at that unit, so estimation never touches floating point), per-bucket costs use checked multiplication/addition, and overflow surfaces as a visibly unpriced fact instead of a truncated number.
- `RateDataset` carries source, version, reviewed canonical rates (optionally provider-qualified keys), and reviewed aliases. Matching precedence follows `docs/PLAN.md` section 8: exact provider-qualified match, then exact bare model match, then reviewed alias (recorded as `alias:<spelling>` in the pricing rule); everything else stays unpriced with rule `unpriced:no-match`. No fuzzy model matching exists anywhere.
- The bundled dataset ships **empty by design** (`agentmeter-reviewed@2026-08-19.0`): rates enter only after review against official provider pricing, and until then every model is visibly unpriced. The seed dataset is follow-up work with per-rate official documentation evidence.
- Storage gained the pricing ledger the v1 schema reserved: `record_pricing_snapshot` is idempotent by content hash, `events_for_pricing` projects every canonical event in deterministic order, and `replace_estimate_facts` swaps all API-equivalent estimates and unpriced markers in one transaction. Provider-reported and subscription-credit facts are never touched, and unpriced markers keep absence visible in overview data quality rather than reading as a valid zero.
- `agentmeter-app` gained `PricingService::reprice`, which records the snapshot, prices every event, and replaces the previous run wholesale — historical events reprice without re-ingesting source logs, and a changed dataset fully reverses the prior estimates.
- Tests: 131 total (121 unit tests and 10 collector-to-storage integration tests): exact per-bucket integer math, precedence ordering, alias rule recording, unpriced/overflow behavior, versioned empty bundled dataset, stable content hashing, snapshot idempotence, estimate replacement with provider-reported preservation and unpriced visibility, end-to-end service repricing reversibility, and data-directory failure classification.
- `cargo fmt --all --check`, `cargo check --workspace --all-targets`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, and `git diff --check` passed on the user's Mac with the temporary local-only `runtime_shaders` bypass, reverted before committing.

## M4-05 verification evidence

- Official pricing pages were fetched and quoted verbatim on 2026-08-19: developers.openai.com/api/docs/pricing, platform.kimi.ai/docs/pricing, platform.claude.com/docs/en/about-claude/pricing, and api-docs.deepseek.com/quick_start/pricing. Every seeded rate's source, listed prices, and documented inferences are recorded in `docs/research/rates-2026-08-19.md`.
- The bundled dataset `agentmeter-reviewed@2026-08-19.1` seeds 16 canonical models — gpt-5.3-codex plus the gpt-5.6 family (explicit cache-write tiers), kimi-k2.7-code/-highspeed and kimi-k3, seven Claude generations (5-minute cache-write tier, thinking at output rate), and deepseek-v4-flash/-pro (peak tier) — all as integer nano-USD per token (USD-per-million × 1_000, integral for every listed price including $56.25 and $15.625 writes).
- Four official aliases: `kimi-for-coding`/`-highspeed` → the kimi-k2.7-code family (kimi.com/code mapping) and the retired `deepseek-chat`/`deepseek-reasoner` → deepseek-v4-flash (official changelog), so historical logs price correctly.
- Documented conservative inferences, recorded in code and the sources file: cache writes bill at the input rate where no write price is listed (OpenAI codex, Kimi, DeepSeek); reasoning is priced as output where only one output rate exists; gpt-5.3-codex Fast mode and DeepSeek off-peak windows are not yet distinguishable, so such usage is under-/over-estimated respectively; claude-sonnet-4-7 carries its time-limited discount ending 2026-09-30 and must be rechecked.
- Models absent from official pages (for example any `codex-mini` variant) deliberately stay unpriced.
- Tests: 131 total (121 unit and 10 integration) — the bundled-dataset test now verifies seeded rates, alias pricing through `kimi-for-coding`, integral $56.25 conversion, and that unlisted models remain unpriced.
- `cargo fmt --all --check`, `cargo check --workspace --all-targets`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, and `git diff --check` passed on the user's Mac with the temporary local-only `runtime_shaders` bypass, reverted before committing.

## M4-06 verification evidence

- Storage projects canonical events into exact daily UTC activity rows. The application service deterministically rolls those rows into Monday-based weekly or calendar-month buckets and groups by client, provider, or model. Token and API-equivalent estimate totals use checked integer arithmetic; absent rates remain `None` with an explicit unpriced-event count rather than becoming zero.
- Desktop presentation state owns daily/weekly/monthly, tokens/cost, and client/provider/model selections. Time and dimension changes load immutable snapshots off the render path and reject stale completions; the metric toggle reuses the loaded snapshot. The GPUI route renders accessible controls and stacked, labeled series with semantic colors in both themes. English and Simplified Chinese cover loading, empty, error, control, estimate, and unpriced states.
- The GitHub macOS visual matrix captures a synthetic populated Activity view in all four locale/theme combinations without reading local Agent histories. CI run `32239288063` passed, including production-feature build and artifact upload. Inspection found the 1120-pixel window clipped on the runner's 1024-pixel display, so the follow-up candidate narrows the default window to 960 pixels. A macOS screen-capture permission dialog still obscures the artifact and prevents treating it as visual acceptance evidence; this is a CI capture limitation, not a claimed UI pass.
- Orb verification passed: `cargo fmt --all --check`, `cargo check --workspace --all-targets`, `cargo test --workspace` (135 tests: 125 unit tests and 10 collector-to-storage integration tests), `cargo clippy --workspace --all-targets -- -D warnings`, and `git diff --check`.
- Apple Silicon macOS validation passed with Rust 1.96.0 after the known local-only `runtime_shaders` bypass: workspace check and Clippy passed, and all 29 desktop tests passed. The bypass was not added to the candidate. This validation found and resolved the Activity scroll container's missing stable element ID before completion.

## M4-07 verification evidence

- Storage returns source-scoped session summaries ordered by recent activity without joining title, path, prompt, response, or event-provenance content. Token and event totals aggregate once even when an event has multiple cost kinds; providers/models are deterministic distinct sets, and equal native session IDs from different source objects remain separate.
- The application service exposes immutable summaries with checked token totals, stable content generations, adapter/source-kind/parser-version provenance, worst-fact confidence, separately labeled provider-reported and API-equivalent costs, and explicit unpriced-event counts.
- Portable desktop state rejects stale snapshots and errors. Content-free cards localize duration, missing projects/costs, confidence, and every loading/empty/error/detail label in English and Simplified Chinese. GPUI loads off the render path and renders a stable-ID scroll list using semantic theme colors.
- The hosted macOS visual matrix is configured to seed only synthetic session, usage, and cost rows and capture the populated Sessions route in all four locale/theme combinations; no local Agent history is read.
- Orb verification passed: `cargo fmt --all --check`, `cargo check --workspace --all-targets`, `cargo test --workspace` (139 tests: 129 unit tests and 10 collector-to-storage integration tests), `cargo clippy --workspace --all-targets -- -D warnings`, `git diff --check`, and the privacy-pattern scan (only policy examples and intentional `/fixture/home/...` paths matched).
- Apple Silicon validation of the exact candidate passed workspace check, 43 application/desktop tests, and Clippy with warnings denied using only the known local `runtime_shaders` bypass. Hosted CI run `32242486063` then passed Linux and production-feature macOS jobs, including desktop build and the 20-image visual matrix upload. Inspection confirmed populated English-light and Chinese-dark Sessions content, but the macOS screen-capture permission dialog still obscures the center, so the artifact is not claimed as complete visual acceptance.

## M4-08 verification evidence (deferred verification completed)

- The deferred candidate `d415dbb` received the full required validation on 2026-08-19 after the user's direct-commit request: on the user's Mac with the temporary local-only `runtime_shaders` bypass, `cargo fmt --all --check`, `cargo check --workspace --all-targets`, `cargo test --workspace` (142 tests), `cargo clippy --workspace --all-targets -- -D warnings`, and `git diff --check` all passed; the bypass was reverted before this documentation commit.
- Hosted CI run `32243890892` passed both the Linux gates and the production-feature macOS job (Metal Toolchain, all-target checks, desktop build, launch, and the extended visual matrix — now 28 images covering Activity, Sessions, Models, and Pricing with route-specific synthetic seed data).
- Reviewed artifacts: the Models route in Chinese dark renders every model card with token buckets, separately labeled costs, confidence, and pricing provenance, and visibly marks unknown models as unpriced (`unpriced:no-match`); the Pricing route in English light renders the dataset header (`agentmeter-reviewed@2026-08-19.1` with priced/unpriced counts) and the rate catalog with aliases and correct official rates (for example the $56.25 claude-opus-5-5 cache write). No permission dialogs or rendering defects appeared in the inspected captures, unlike the M4-07 run.
- Code review confirmed the established boundaries: `ModelsPricingService` owns the database work (including the idempotent bundled-dataset application when estimates are stale or incomplete), GPUI loads through the background executor with single-use request generations, and localization/theme rules are enforced by the portable tests included in the 142-test run.

## M4-09 verification evidence

- Storage gained `export_event_rows`, a per-event projection joining usage facts with their cost facts that deliberately excludes source paths, warnings, provenance notes, and diagnostic text; a fixture test proves path and warning text can never appear in either export format.
- The application layer gained `ExportService`: versioned JSON (`agentmeter-events-v1` envelope with generated-at and event count) and CSV (stable untranslated header) renderings, with costs serialized as exact minimal decimal strings from nano-USD (`0.12` stays `0.12`, whole dollars stay integral) — never floats. Files are written to `AgentMeter/exports/agentmeter-events-<UTC-stamp>-<content-hash>.<ext>` under the local data directory, each export an explicit user action; content-derived suffixes make repeated exports deterministic and diffable.
- Desktop Settings gained a localized Export section with two real buttons invoking the service on the background executor; portable `ExportState` rejects out-of-order completions, shows a running indicator, the written file name and event count on success, and a localized danger message on failure. Every new key ships in English and Simplified Chinese.
- The export field set settles open decision 5 in `docs/PLAN.md` section 18: event/session ids, UTC millisecond timestamp, client, provider, model, the five canonical token buckets with total and source-reported total, confidence, provider-reported and API-equivalent costs with unpriced flag, and pricing key/rule.
- Tests: 147 total (137 unit tests and 10 collector-to-storage integration tests), covering payload privacy, exact decimals and UTC file-name stamps, CSV header/row shape, stale-result rejection, running/result/error state transitions, data-directory failure classification, and both-locale label coverage.
- `cargo fmt --all --check`, `cargo check --workspace --all-targets`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, and `git diff --check` passed on the user's Mac with the temporary local-only `runtime_shaders` bypass, reverted before committing.

## M5-01 verification evidence

- `agentmeter-app` gained `IngestionService`: per-adapter discovery, source registration with stable ids (`adapter:source_key`), checkpoint-driven starts (Fresh/Resume/Rebuild with parser-version invalidation), transactional batch application, daily due-reconciliation rebuilt through the owning adapter, and adapter errors persisted as collection-failure diagnostics (Error health with RetryCollection remediation) instead of silent zeroes. Discovery-level failures stay in the returned summary because no source object exists to persist them against.
- The default adapter set is the Codex home, Pi sessions root, and both Kimi wire roots; Amp stays out of default scanning (local history is experimental, stream-json is an explicit capture), documented in code.
- Desktop wiring: `new()` now delegates to extracted `reload_*` helpers; startup runs one collection pass and refreshes every view snapshot afterward; the Sources route gains a real Rescan command (running indicator, localized failure banner). `IngestionUiState` rejects out-of-order scan completions.
- End-to-end tests drive real Kimi and Codex adapters from synthetic fixtures through the service: first scan ingests exactly one event per source, repeated scans are idempotent, appended bytes resume from checkpoints, and a corrupt compressed rollout surfaces as a persisted Error health state with remediation.
- Tests: 151 total (141 unit and 10 integration). `cargo fmt --all --check`, `cargo check --workspace --all-targets`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, and `git diff --check` passed on the user's Mac with the temporary local-only `runtime_shaders` bypass, reverted before committing.

## M5-02 verification evidence

- New benchmark harness at `crates/agentmeter-app/tests/performance.rs` (`#[ignore]`d; run with `cargo test -p agentmeter-app --release --test performance -- --ignored --nocapture`). Corpus: 40 Kimi agent journals × 2,500 usage events = 100,000 events, 18.47 MB, spread over 90 UTC days. Platform: the user's Apple Silicon Mac, release profile. The harness asserts the section 16 targets and prints per-phase timings.
- Measured (release): warm dashboard (overview service load, which embeds source health) **110 ms** against the <200 ms target; unchanged warm reconciliation **45.7 ms** against the <1 s target; append refresh (+100 lines to one source) 52 ms; cold collection of the whole corpus 11.0 s (bounded, no numeric target). The sources-only snapshot takes 37 ms. Projection rebuild determinism is asserted at scale.
- The first benchmark run exposed a real defect: `apply_ingest` rebuilt the entire daily projection (full-table GROUP BY) after every per-source run, putting unchanged warm reconciliation at 4.1 s. Appends now project incrementally per inserted event inside the same transaction (`upsert_daily_delta`), source-owned replacements keep the full rebuild, and zero-record warm appends touch nothing. A new storage test proves the incremental path matches a full rebuild exactly, including duplicate immunity.
- A corpus-realism lesson is recorded in the harness: per-second unique timestamps inflated `COUNT(DISTINCT date(...))` pathologically; realistic multi-day spread matters ("do not optimize against invented workloads").
- Tests: 152 total (142 unit and 10 integration). `cargo fmt --all --check`, `cargo check --workspace --all-targets`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, and `git diff --check` passed on the user's Mac with the temporary local-only `runtime_shaders` bypass, reverted before committing.

## M5-03 verification evidence

- `agentmeter-app` gained `CancellationToken` (atomic cooperative cancellation) and `scan_and_ingest_cancellable`: cancellation is checked between adapter runs, before every source-owned transaction, and per reconciliation target — every source either fully commits or never runs, so an abandoned scan never corrupts a checkpoint or half-applies a source. Cancelled scans return the partial `IngestionSummary` with a `cancelled` flag instead of an error.
- Tests prove the transaction-boundary guarantee: cancelling at the third check commits exactly the first source, and an uncancellable follow-up scan resumes and finishes the remaining source with exact totals; a pre-cancelled token processes nothing. Desktop tests cover the cancelled flag surfacing and stale-result rejection.
- The desktop cancels any in-flight scan when a new one starts, exposes a localized Cancel command beside the running indicator in Sources, and shows a localized "Scan cancelled" notice after partial runs; committed work still refreshes every view snapshot. New copy ships in English and Simplified Chinese.
- Tests: 154 total (144 unit and 10 integration). `cargo fmt --all --check`, `cargo check --workspace --all-targets`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, and `git diff --check` passed on the user's Mac with the temporary local-only `runtime_shaders` bypass, reverted before committing.

## M5-04 verification evidence

- Open decision 4 settled: history is the durable record — a new application test proves deleting a vendor journal keeps canonical events, the next scan simply no longer discovers the source, and no failure diagnostic is recorded. No automatic deletion exists in 1.0. A localized "Data and privacy" Settings card states the policy in English and Simplified Chinese.
- Open decision 1 settled: the bundle identifier is `com.topit.agentmeter`.
- `.github/workflows/release.yml` (manual `workflow_dispatch`) verifies the Metal Toolchain, builds the desktop app in release mode, assembles the `AgentMeter.app` bundle with the settled identifier, and produces an `AgentMeter.dmg` artifact; the signing hook refuses to run if signing secrets appear before the codesign/notarytool steps are implemented, so a half-signed package can never ship silently.
- Tests: 155 total (145 unit and 10 integration). `cargo fmt --all --check`, `cargo check --workspace --all-targets`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, and `git diff --check` passed on the user's Mac with the temporary local-only `runtime_shaders` bypass, reverted before committing.

## M5-05 verification evidence

- First real end-to-end validation: the unsigned DMG ran on the user's Mac and cold-collected 593 Codex rollouts into 74,682 canonical events (101 MB database) — the data pipeline is correct. The defect was experience, not collection: a minutes-long first scan left every view in its empty state with no sign of life, and two simultaneously open instances interleaved their scans (idempotent by design, but confusing).
- Warm relaunch after the first scan completes in under ten seconds, so only the first run and large incremental additions pay the full cost.
- `agentmeter-app` gained `ScanProgress`: atomic discovered/processed counters shared with the UI and threaded through `scan_and_ingest_reported`; cancellation tests now also assert the counters (partial scans report exactly the sources that started).
- The desktop shows live progress while scanning: a 400 ms notify poller drives a repaint, the Sources page displays processed/discovered counts beside the scanning label, and the Overview empty state is replaced by an accent-bordered "Collecting local usage…" card explaining the first-run delay in English and Simplified Chinese.
- Re-cut DMG fresh-database verification: a single instance cold-collected the same 593-rollout corpus into 74,736 events in roughly 35 seconds with live progress counters; the earlier "minutes-long" reading came from two simultaneously open instances interleaving scans plus a monitoring artifact (a locked-database read reported as zero). Local screenshots of the collecting state are blocked by the terminal's screen-recording TCC permission; the state machine is unit-tested and the normal-state matrix keeps running in CI.
- Tests: 155 total. `cargo fmt --all --check`, `cargo check --workspace --all-targets`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, and `git diff --check` passed on the user's Mac with the temporary local-only `runtime_shaders` bypass, reverted before committing.

## M5-06 verification evidence

- Repository privacy-pattern scan: no real home paths outside the ledger's own operational notes, no secret-shaped strings, no generated databases or exports tracked (57 tracked files). Fixture policy unchanged; the rate-dataset and adapter research notes carry only synthetic paths.
- The README was rewritten to describe the real product: local launch-and-rescan collection with progress and cancellation, the five supported v1.0 sources, no-silent-zeroes and facts-before-estimates guarantees, verified rate sourcing, local-only export, durable-history retention with its removal path, the unsigned-DMG download story with the right-click-open caveat and first-run timing, privacy commitments (metadata-only reading, zero network calls, zero telemetry), development layout, and the pinned GPUI/Metal toolchain note. The Chinese section mirrors the same facts.
- `git diff --check` clean; documentation-only change rides the same CI matrix.

## Development handoff

M4-08 adds deterministic lifetime model aggregation, current pricing-snapshot provenance, bundled-dataset refresh, stale-result-safe portable presentation, localized Models/Pricing routes, and synthetic CI visual fixtures. Focused application/desktop tests passed during development, but the exact final candidate did not receive the required full workspace or macOS checks because the user explicitly requested immediate submission after their validation-tool quota was exhausted.

Handoff point: the M5-06 privacy-and-README commit on GitHub `topit/agentmeter`, branch `main`, written directly after `0519819`. M5-05 was reconciled to `a61be6c` with CI green and release run `32370110427`. Remaining M5 work is external-input dependent: implement codesign/notarytool once the user's Developer ID certificate is configured, run the pre-packaging interactive accessibility pass after the local Metal Toolchain repair, re-verify the user's DMG test of the progress experience, settle open decisions 2/3/6/7 at the M5 exit review, then tag v0.1.0.
