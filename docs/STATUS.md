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

- Base revision: `8080c26` (`Persist provider-reported cost facts`)
- Branch: `main`, tracking GitHub `github/main`; M1 through M2-06 have been pushed
- Additional remote: Amp-hosted `origin` remains read-only for this work
- Amp project and `origin` still use `moeit/token-usage` pending a Puck-managed rename
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
| M2-07 Reconciliation report | coordinator | `8080c26` | complete collector facts | completed | deterministic reconciliation/cross-check fixtures; workspace checks | due-source rebuild gate and content-free versioned report verified; ready to commit and push |
| M2-08 Variant assessment | coordinator | M2-07 commit | upstream Codex/Pi contracts | queued | official-source evidence; variant fixtures where supported; workspace checks | assess Codex compressed/headless and Pi legacy location before support claims |

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

## Budget ledger

- Budget source: user settings, amount not provided
- Budget total: UNKNOWN
- Consumed: UNKNOWN
- Integration/verification reserve: 20% policy applies, exact amount UNKNOWN
- Threshold state: UNKNOWN
- Cost decision: serialize schema and contract work; use one bounded documentation worker only
