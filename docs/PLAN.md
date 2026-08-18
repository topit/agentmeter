# AgentMeter product and engineering plan

- Status: M2 collector implementation in progress; Amp, Codex, and Pi parser milestones implemented
- Product name: AgentMeter
- Primary platform: macOS
- Future platform: Windows
- UI: GPUI
- Locales: English and Simplified Chinese
- Themes: system, light, and dark

## 1. Product definition

AgentMeter is a local-first desktop application that discovers usage records written by coding agents, reconciles each source's semantics, stores normalized events in an auditable local ledger, and explains token usage, cost estimates, sessions, models, and collection health.

The product is not a cloud billing authority. It distinguishes provider-reported charges, estimated API-equivalent cost, subscription credits, and unknown pricing.

### Primary users

- Developers who use several coding agents on one machine.
- Power users comparing models, cache behavior, and agent efficiency.
- Developers who need to understand missing or incomplete statistics instead of seeing unexplained zeroes.
- Future users who want local export for their own analysis without uploading conversations.

### Core questions

The product should answer:

1. How many tokens did I use today, this week, and this month?
2. Which client, model, project, and session produced the usage?
3. How much was reported as charged, and what is only an estimate?
4. How effective was caching?
5. Why is an agent missing or partially counted?
6. Can a surprising total be traced back to its source and parser decision?

## 2. Goals and non-goals

### Goals

- Accurate, incremental, local collection from multiple agent formats.
- Explicit source health, completeness, confidence, and diagnostics.
- Durable SQLite history that survives source rotation or cleanup according to user retention policy.
- Reversible pricing independent from immutable token facts.
- Responsive native macOS interface with English/Chinese and light/dark/system themes.
- Portable core suitable for Windows.
- Export of normalized, privacy-reviewed aggregates and events.

### Non-goals for v1

- Running or orchestrating agents.
- Capturing prompt or response content for analytics.
- Team leaderboards or cloud synchronization.
- Acting as a transparent HTTP/TLS proxy by default.
- Matching provider invoices when the local source lacks billing tier or account data.
- Mac App Store distribution.
- Mobile or web frontend.

## 3. Product principles

1. **Local first:** raw agent data remains on the device.
2. **No silent zeroes:** absence, denied access, disabled telemetry, parsing failure, and genuine zero usage are distinct states.
3. **Facts before estimates:** token events are stored independently from price calculations.
4. **Provenance by default:** every event can be traced to source, parser version, and normalization confidence.
5. **Correct depth before broad coverage:** a source is supported only after fixture, dedup, incrementality, diagnostics, and UI acceptance tests pass.
6. **Replaceable frontend:** GPUI does not own the database or collector lifecycle.

## 4. System architecture

```text
┌──────────────────────── GPUI desktop ────────────────────────┐
│ overview · activity · sessions · sources · models · settings │
└─────────────────────────────┬─────────────────────────────────┘
                              │ commands / immutable snapshots
                    ┌─────────▼──────────┐
                    │ application service│
                    └─────┬──────┬───────┘
                          │      │
              ┌───────────▼─┐  ┌─▼─────────────────┐
              │ query layer │  │ ingestion service │
              └──────┬──────┘  └─┬───────────────┬─┘
                     │           │               │
              ┌──────▼─────┐ ┌───▼──────────┐ ┌──▼───────────┐
              │ SQLite/WAL │ │ adapters     │ │ pricing      │
              │ event ledger│ │ files/DB/API │ │ snapshots    │
              └────────────┘ └──────────────┘ └──────────────┘
```

### Crate responsibilities

- `agentmeter-core`: stable domain vocabulary, normalization invariants, source/client/provider/model identities.
- `agentmeter-collectors`: platform discovery and adapter-specific parsing, reconciliation, cursor state, and warnings.
- `agentmeter-storage`: SQLite migrations, transactional source ownership, event queries, and materialized projections.
- `agentmeter-pricing`: rate dataset cache, model matching, reported cost, estimates, and repricing.
- `desktop`: GPUI entities, views, chart rendering, localization, theme tokens, accessibility, and preferences.
- A future `platform-macos` crate will isolate status-item, launch-at-login, signing/update, and security-scoped access code.

### Process model

Start with one process and strict internal boundaries:

- one serialized SQLite writer;
- background discovery/parse workers;
- read-only aggregate queries;
- debounced filesystem invalidation;
- immutable UI snapshots with generation IDs;
- periodic full reconciliation.

The storage layer selects enabled, permission-granted sources due for full reconciliation from their most recent successful source replacement. Incremental appends never postpone this deadline. The process coordinator passes those sources to adapters with `IngestStart::Rebuild`; a successful `Replace` transaction advances the full-reconciliation watermark without a second scheduling table.

Split out a helper process only if measured requirements show that collection must survive UI crashes or upgrades. Avoid helper signing and IPC complexity in the first release.

Source health is a portable core snapshot populated by storage, not a GPUI-owned state machine. Each immutable generation contains installation/source identity, enabled and permission state, parser version, last scan/success/event, latest changed-record count, warnings, error, status, and structured remediation. States are `Healthy`, `Partial`, `SetupRequired`, `UnsupportedSchema`, `Error`, and `Disabled`. Snapshot generation is a stable fingerprint of exposed health facts so asynchronous consumers can reject stale responses without a schema-only revision counter. Local paths may be shown in the Sources UI but must be redacted before diagnostics export.

## 5. Ingestion contract

Each collector adapter must provide:

1. stable adapter ID and parser version;
2. OS-specific default paths and supported environment overrides;
3. source discovery with canonical path and permission state;
4. source kind: append-only JSONL, mutable JSON, SQLite/WAL, or API/OTel;
5. source fingerprint dependencies, including sidecars and WAL files;
6. incremental checkpoint and parser state;
7. normalized usage events plus source-native totals;
8. semantic event identity and dedup/replay policy;
9. warnings, timestamp origin, and confidence;
10. sanitized fixtures and schema-drift tests.

### Refresh lifecycle

1. Discover installations and configured custom roots.
2. Compare source identity, metadata, fingerprint, parser version, and checkpoint.
3. Parse only changed/new data where the format permits.
4. Reconcile source-local duplicates and cumulative snapshots.
5. Validate normalized buckets and timestamps.
6. Commit events, checkpoint, ownership, and diagnostics atomically.
7. Update affected aggregate projections.
8. Notify the UI with a new snapshot generation.

Filesystem watcher events are invalidation hints, not truth. Queue overflow, truncation, source movement, and application startup trigger reconciliation.

Reconciliation exports are versioned, deterministic JSON snapshots. They contain adapter/source identity, parser version, aggregate canonical token buckets, confidence counts, source-total coverage and mismatch state, plus reviewed aggregate expectations from source UIs/CLIs or named reference parsers. They must not contain paths, model/session/event IDs, warnings, provenance notes, or source excerpts.

### Token normalization

Canonical buckets are mutually exclusive:

- uncached input;
- non-reasoning output;
- cache read;
- cache write/creation;
- reasoning.

Adapters preserve source totals and flags describing whether cache was included in input or reasoning was included in output. If semantics cannot be resolved, retain the raw facts, lower confidence, and avoid a misleading normalized total.

## 6. Database plan

The first SQLite migration should contain these logical tables; names may be refined during migration design.

### Canonical tables

- `source_installations`: adapter, platform, root, discovery method, permission and enabled state.
- `source_objects`: canonical path/identity, kind, fingerprint, parser version, cursor, parser state, and last scan.
- `sessions`: client, source, native ID, project/workspace, start/end, title when available.
- `usage_events`: stable ID, session, timestamp, client, provider, model, canonical token buckets, source total, and confidence.
- `event_provenance`: source object, record offset/native ID, schema variant, timestamp origin, normalization notes.
- `ingest_runs`: start/end, result, changed objects, event counts, warnings, and errors.
- `pricing_snapshots`: source, version/hash, fetch time, expiry, and offline/stale state.
- `event_costs`: event, kind, USD value, pricing key, dataset, rule/tier, and confidence.
- `preferences`: locale, theme, custom roots, retention, privacy, and update settings.

### Projections

- daily usage by client/model/provider/project;
- session totals;
- model and client lifetime totals;
- source health summary;
- token category and cache-efficiency summaries.

Projections are disposable and rebuildable from canonical events. Migrations never modify vendor-owned source data.

Recommended SQLite defaults: WAL, foreign keys on, busy timeout, and `synchronous=NORMAL`.

## 7. Source roadmap

| Source | Expected local source | Initial strategy | Main risk |
|---|---|---|---|
| Amp | data root `threads/*.json` | reconcile `usageLedger` with assistant usage | credits and timestamps are not always dollar/time facts |
| Codex CLI | `sessions` and `archived_sessions` JSONL | cumulative-to-delta state machine, fork/replay handling | replay and archive duplication |
| Pi | session JSONL | assistant usage and embedded reported cost | missing provider and timestamp variants |
| Kimi/Kimi Code | old/new `wire.jsonl` layouts | old status replacement and new turn-scope events | cumulative/session usage duplication |
| Factory Droid | session settings snapshots | latest snapshot per session | no per-turn history |
| Grok Build CLI | session `updates.jsonl` + summary | event IDs, model usage, recorded cost | reasoning/output semantics and missing summary |
| Codex Desktop | probe shared Codex home, then containers | reuse parser only after fixture confirmation | separate/sandboxed storage may differ |
| Copilot CLI | session state and optional OTel | detect capability and guide setup | telemetry may not exist retrospectively |
| DeepSeek Harness | unknown until product/fixtures confirmed | dedicated adapter or explicit telemetry integration | no stable generic local contract |
| Cursor | private DB or explicit export/API cache | research after stable MVP | schema and authentication churn |
| Grok bot/remote agents | export/API/OTel | opt-in import | no local source on the desktop |

Amp's documented `--stream-json` output is implemented as a separate opt-in, append-only source. It is authoritative for token field names but does not expose event timestamps, model/provider identity, or a native per-event ID. Local `threads/*.json` parsing remains an explicitly experimental path because Amp does not publish that persistence schema; third-party parsers are cross-check evidence, not a compatibility guarantee.

### Definition of supported

A source is marked supported only when:

- default and custom-root discovery work;
- permission/missing/empty states are distinguishable;
- real sanitized fixtures cover known schemas;
- duplicate, replay, malformed, truncated, and changed-schema cases are tested;
- warm refresh is incremental or has a documented bounded replacement strategy;
- parser diagnostics appear in Sources;
- totals are cross-checked against the source's own UI/CLI for representative fixtures;
- English/Chinese health and remediation messages exist.

## 8. Pricing plan

Cost kinds:

- `ProviderReported`: source explicitly reports a currency amount.
- `ApiEquivalentEstimate`: computed from a versioned rate dataset.
- `SubscriptionCredit`: source-specific credit/quota that is not USD spend.
- `Unpriced`: model or tier cannot be matched safely.

Canonical USD amounts use integer nano-USD (USD × 10⁹) so decimal source facts, event identity, and fixture assertions never depend on binary floating point. SQLite's existing USD representation is converted only at the storage boundary. Subscription credits without explicit USD semantics carry no USD amount.

Pricing precedence:

1. retain provider-reported cost;
2. exact provider/model/tier rate match;
3. reviewed alias match with visible confidence;
4. unpriced.

Do not silently fuzzy-match ambiguous bare model family names. Historical events can be repriced without re-ingesting source logs. The UI must label estimates as estimates and show dataset freshness.

## 9. Information architecture

### Overview

- total tokens, estimated/reported cost, active days, sessions;
- first-class collection-health strip;
- usage-over-time chart by client/model/provider;
- token category and cache efficiency;
- today summary and agent ranking;
- data-quality breakdown: exact, derived, estimated, unpriced.

### Activity

- daily/weekly/monthly/hourly views;
- tokens/cost toggle;
- stack/group by client, provider, model, or project;
- selected-day drilldown.

### Sessions

- session list, duration, project, client, model, token and cost totals;
- source provenance and confidence;
- no prompt/response display in v1.

### Sources

- installation/path, adapter and parser version;
- enabled, permission, last scan, last event, records changed;
- healthy, partial, setup required, unsupported schema, and error states;
- remediation and privacy-safe diagnostics export;
- custom path configuration and rescan/rebuild controls.

### Models and pricing

- client/provider/model separation;
- exact model IDs and optional display aliases;
- token categories, cache efficiency, and cost;
- reported/estimated/unpriced split;
- pricing source, match key, confidence, and last update.

### Settings

- language: system, English, Simplified Chinese;
- appearance: system, light, dark;
- custom roots, retention, exports, offline mode;
- update checks and any future anonymous telemetry disabled by default.

## 10. Internationalization

### Scope

Ship `en` and `zh-CN`. Unsupported system locales fall back to English. Users can override the system language without restarting if GPUI state permits a reliable live refresh; otherwise show that restart is required.

All visible strings are localized, including:

- navigation, menus, dialogs, tooltips, notifications;
- accessibility names and descriptions;
- chart axes, date ranges, units, empty/loading/error states;
- source health, parser warnings, and remediation;
- export field descriptions, but not stable machine-readable field names.

### Implementation direction

The foundation uses typed keys and exhaustive English/Chinese matches. Before parameterized copy grows, move catalog content to reviewed locale resources while retaining typed/generated keys. Add named interpolation, locale-aware number/date/currency formatting, and plural handling. Never concatenate translated fragments.

Use binary units only where technically correct. Token displays use locale-aware decimal grouping and compact notation without changing raw export values.

### Verification

- compile/test parity for every key;
- fallback tests;
- snapshot or rendered-state checks in both locales;
- pseudo-long-string checks for clipping;
- CJK font fallback and baseline verification;
- locale-aware date, number, currency, and plural tests.

## 11. Theme system

Modes: `System`, `Light`, and `Dark`.

Components consume semantic tokens such as:

- background, surface, elevated surface;
- primary, secondary, muted text;
- border, divider, focus ring;
- accent, selection, hover, pressed;
- success, warning, danger, info;
- chart grid and stable categorical series colors.

No component should branch on theme or embed light/dark literals. System appearance changes update the resolved palette and repaint existing windows. Theme preference is independent from locale.

Charts must meet contrast requirements in both themes. Status always includes icon/text in addition to color. Verify light/dark versions of overview, source health, empty, loading, partial, and error states.

## 12. Platform and distribution

### macOS v1

- GPUI dashboard window;
- Developer ID signed/notarized DMG or Homebrew distribution;
- non-sandboxed read-only access to user-selected and known agent locations;
- native menu and optional launch at login;
- status-item support only after a small AppKit bridge is justified;
- updater selected separately because GPUI does not provide one.

### Windows later

- keep core crates free from macOS and GPUI dependencies;
- add Windows path resolvers and fixture tests from the first relevant adapter;
- CI compile/test portable crates on Windows before the Windows UI milestone;
- isolate notification-area, startup, installer, updater, and path permissions behind platform modules.

GPUI is pre-1.0. Pin a tested revision when the first real window is implemented and update it deliberately with visual and platform regression checks.

## 13. Privacy and security

- Never persist prompts, responses, code, secrets, or complete environment dumps.
- Store paths only where required for source ownership; redact home prefixes in UI diagnostics and exports by default.
- Use vendor stores read-only and avoid long-lived locks.
- Database and logs use user-only filesystem permissions.
- Pricing and update requests are documented, minimal, independently disableable, and contain no usage payload.
- No cloud sync or analytics in v1.
- Diagnostic export shows an exact preview and requires explicit user action.

## 14. Testing strategy

### Domain tests

- exclusive token normalization and overflow;
- source semantics conversion;
- client/provider/model identity;
- cost provenance and repricing.

### Adapter fixtures

For each schema: normal, empty, malformed line, unknown field, truncation, append, rewrite, duplicate, replay/fork where relevant, timestamp fallback, and large-history performance.

Fixtures must be synthetic or irreversibly sanitized. Keep stable source IDs and boundary values while removing prompts, responses, paths, account identifiers, and secrets.

### Storage tests

- fresh schema and retained migrations;
- source-scoped transactional replacement;
- crash/rollback behavior;
- checkpoint recovery and parser-version invalidation;
- projection rebuild equality;
- WAL concurrency and database corruption handling.

### UI tests

- both locales and themes;
- representative healthy, partial, setup-required, unsupported, and error sources;
- empty, loading, refresh, stale-result rejection, and large datasets;
- keyboard navigation, focus, text contrast, and screen-reader labels;
- chart selection and non-color status cues.

### Cross-checks

Representative adapter totals should be compared with upstream agent output and at least one independent parser such as Waku, Tokens, or ccusage. Differences require documented semantic explanations rather than test-specific constants.

## 15. Milestones

### M0 — Repository foundation (complete)

- Rust workspace and pinned toolchain;
- crate boundaries;
- initial token, collector, pricing, i18n, and theme contracts;
- README, repository guidance, and complete plan;
- orb setup and baseline checks.

Exit: clean setup from a fresh orb and all workspace checks pass.

### M1 — Fixture lab and SQLite ledger (complete)

- sanitized fixture policy/tooling;
- schema v1 and migrations;
- source ownership, checkpoints, diagnostics, projections;
- synthetic reference adapter proving append and snapshot paths;
- exportable reconciliation report.

Exit: append, rewrite, truncation, parser upgrade, rollback, and projection rebuild are tested.

### M2 — First complete collectors

- Amp, Codex CLI, and Pi;
- source health model;
- incremental refresh and periodic reconciliation;
- cross-check report against source totals/reference parsers.

Exit: all “supported” criteria pass for three adapters.

### M3 — GPUI application shell

- pin tested GPUI revision;
- window, navigation, overview, sources, and settings;
- SQLite query snapshots and stale-result protection;
- English/Chinese and system/light/dark support;
- accessible source remediation flows.

Exit: representative UI states verified in four locale/theme combinations per theme matrix, including both themes and both locales.

### M4 — Coverage and pricing

- Kimi/Kimi Code, Factory Droid, Grok Build;
- versioned pricing snapshots and reversible estimates;
- activity, sessions, models, and pricing views;
- JSON/CSV export.

Exit: six complete collectors, visible pricing provenance, and no silent unknown pricing.

### M5 — macOS beta

- performance and retention work;
- signing, notarization, DMG/Homebrew packaging;
- launch behavior, permissions, diagnostics, crash recovery;
- privacy review and user documentation.

Exit: representative large histories refresh incrementally, cold rebuild is bounded and cancellable, and beta package passes clean-machine testing.

### M6 — Extended sources and Windows preparation

- investigate Codex Desktop, Copilot CLI, DeepSeek Harness, Cursor, and remote imports;
- Windows path and core CI coverage;
- assess GPUI Windows shell and platform integrations.

Exit: source support is based on fixtures/contracts, and a Windows go/no-go report identifies remaining platform gaps.

## 16. Performance targets

Initial targets, to be validated with fixture corpora:

- dashboard from warm database: under 200 ms to first useful snapshot;
- unchanged warm source reconciliation: under 1 second for common histories;
- append refresh: proportional to appended bytes, not full history;
- UI interaction remains responsive during scan/reprice/rebuild;
- source diagnostics explain work skipped, parsed, replaced, and failed;
- database projections can be fully rebuilt and compared deterministically.

Do not optimize against invented workloads. Record benchmark fixture sizes and platform details.

## 17. Risks and mitigations

| Risk | Mitigation |
|---|---|
| private agent schemas change | parser versions, fixtures, diagnostics, transactional source rebuild |
| duplicated cumulative/replay records | source-specific state machines and provenance-backed IDs |
| cost is mistaken for actual spend | explicit cost kind and confidence everywhere |
| broad source list hides broken collection | strict support definition and health-first UI |
| GPUI breaking changes | pinned revision and replaceable presentation boundary |
| macOS permissions block hidden/container data | Developer ID v1, permission diagnostics, explicit custom roots |
| dark/CJK support arrives too late | typed locale/theme foundation and milestone acceptance matrix |
| Windows assumptions leak into core | platform path resolvers and portable-crate CI |
| real fixtures leak private data | synthetic/sanitized fixture policy and repository review |

## 18. Open product decisions

These decisions do not block M1 but should be settled before macOS beta:

1. Bundle identifier.
2. Dock application only versus optional menu-bar status item.
3. Default headline cost: provider-reported where available versus API-equivalent estimate.
4. Whether historical events remain after vendor source files are deleted, and default retention period.
5. Which normalized event fields are included in exports by default.
6. Update mechanism and distribution channels.
7. Whether any anonymous product telemetry is ever offered; current default and v1 plan are none.

## 19. Immediate next implementation step

Begin M3 by selecting and pinning a tested GPUI revision, then land the first macOS window and navigation shell without moving collection or storage work onto the render path. The first UI commit must preserve the existing English/Chinese and System/Light/Dark contracts and establish a portable presentation boundary before overview data is connected.
