# AgentMeter repository guidance

## Mission

Build a local-first desktop application that accurately explains coding-agent token usage. Correctness, provenance, and visible collection health are more important than maximizing the number of agent logos shown in the UI.

The product is macOS-first and GPUI-based. Keep collection, domain, storage, and pricing code portable so a Windows frontend remains possible.

## Source of truth

- `docs/PLAN.md` owns product scope, architecture, milestones, and acceptance criteria.
- `README.md` is the concise public overview and onboarding document.
- Source format claims must be backed by fixtures, upstream source code, or official documentation.
- Do not call a client supported until its plan acceptance criteria are met.

## Workspace boundaries

- `apps/desktop`: presentation state, GPUI views, localization, themes, and user interaction.
- `crates/agentmeter-core`: platform-independent domain types and normalization invariants.
- `crates/agentmeter-collectors`: discovery and source-specific parsing/reconciliation.
- `crates/agentmeter-storage`: SQLite migrations, repositories, and aggregate queries.
- `crates/agentmeter-pricing`: pricing datasets, matching, confidence, and repricing.

Dependencies must point inward: desktop may depend on core services; core crates must never depend on GPUI. Agent-specific formats belong in collectors, not in storage or views.

## Required checks

Run these before completing code changes:

```sh
cargo fmt --all --check
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

For a collector change, also run that adapter's fixture tests. For a schema change, test a new database and migration from every retained schema fixture.

## Collector rules

- Treat vendor files and databases as read-only. Never rewrite, migrate, delete, or lock them for writing.
- Separate client/harness, API provider, model, account/transport, and device identity.
- Preserve raw source totals and semantics before normalization.
- Canonical token buckets must be mutually exclusive. If source reasoning is included in output, normalize it once and retain the original semantics in provenance.
- Prefer source-native event IDs. Scope fallback fingerprints to source lineage and version the fingerprint algorithm.
- JSONL adapters should checkpoint byte offsets and parser state, verify prefix continuity, and recover transactionally after truncation.
- Mutable snapshots must replace only the events owned by that source in one transaction.
- SQLite adapters must account for WAL state and use read-only connections.
- Persist parser warnings and schema mismatches. Never convert collection failure into a valid zero.
- Every adapter needs sanitized fixtures for normal, duplicate, malformed, truncated, and schema-drift cases.
- Do not add proxy interception as the default collection method. Optional OpenTelemetry or API integrations must be explicit and documented.

## Storage and pricing rules

- Canonical storage is the normalized event ledger; daily and model totals are rebuildable projections.
- A parser-version change must invalidate only affected sources.
- Token facts are immutable unless their owning source is transactionally re-ingested.
- Store provider-reported cost separately from API-equivalent estimates and subscription credits.
- Every estimate records the pricing key, source, dataset version, tier rule, and confidence.
- Unknown or ambiguous model pricing remains unpriced; do not silently choose the nearest expensive model.

## Internationalization

- The shipped locales are English (`en`) and Simplified Chinese (`zh-CN`). English is the fallback.
- All user-visible text, including errors, empty states, menus, accessibility labels, chart labels, and notifications, must use localization keys.
- Do not construct sentences by concatenating translated fragments. Use complete messages with named parameters once formatting support is added.
- Model IDs, file paths, source-native error excerpts, and user data are not translated.
- Every new message key must have both English and Chinese text in the same change.
- Test locale selection, fallback behavior, interpolation, plural-sensitive messages, and layouts with longer strings.

## Theme and UI rules

- Support `System`, `Light`, and `Dark` modes.
- Views use semantic theme tokens; never embed literal light/dark colors in a component.
- Agent series colors must remain distinguishable in both themes and must not be the only carrier of status.
- Verify overview, source health, empty, loading, partial-data, and error states in both light and dark themes.
- Keep filesystem, database, parsing, and pricing work off the render path. UI consumes immutable snapshots and rejects stale asynchronous responses.
- Collection health is a first-class UI feature. Show last successful scan, last event, parser version, permissions, and actionable warnings.

## Privacy and security

- Do not log prompts, responses, code, secrets, complete environment variables, or unredacted home paths.
- No usage data leaves the machine without explicit user action and a reviewable payload.
- Outbound pricing/update requests must be documented and independently disableable.
- Prefer Developer ID distribution for macOS v1; do not assume App Store sandbox access to agent directories.

## Scope discipline

- Implement one verified adapter completely before broadening source count.
- Avoid empty abstractions beyond the boundaries already established here.
- Do not add a dependency until code uses it and its license/platform implications are understood.
- Pin GPUI to a tested revision when the first window lands; do not track an unreviewed moving branch.
- Keep generated usage databases, real session logs, and unsanitized fixtures out of Git.
