# Implementation status

This file is the durable execution ledger for the current milestone. Product and architecture authority remains in `docs/PLAN.md`.

## Authority envelope

- Mode: implementation
- Authorized repository: this AgentMeter checkout
- Allowed mutations: edit and verify repository files; local commits only when requested or needed for an explicit handoff
- External mutations: no push, deploy, release, repository rename, or production data operation
- Data boundary: synthetic fixtures only; do not copy real prompts, responses, code, account identifiers, secrets, or home paths into the repository

## Operational reality

- Base revision: `01c9532d0e1b358b7c2a79aa1b59dd9a8faae499`
- Branch: `main`, two local commits ahead of the Amp-hosted `origin/main`
- Additional remote: `github` points to `git@github.com:topit/agentmeter.git`; nothing has been pushed by this milestone
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

## Budget ledger

- Budget source: user settings, amount not provided
- Budget total: UNKNOWN
- Consumed: UNKNOWN
- Integration/verification reserve: 20% policy applies, exact amount UNKNOWN
- Threshold state: UNKNOWN
- Cost decision: serialize schema and contract work; use one bounded documentation worker only
