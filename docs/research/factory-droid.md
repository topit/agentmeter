# Factory Droid local format — research note for the v1.1 adapter

Deferred from v1 on 2026-08-19. Research completed against Droid CLI 0.199.0
(real installation, reverse inspection of the shipped binary's bundled
JavaScript, and official docs). Facts below are the verified subset needed to
implement the adapter; treat anything not listed as unverified.

## Storage layout

- Root `~/.factory/` (Windows `%USERPROFILE%\.factory\`). Overrides found in
  the binary: `FACTORY_HOME_OVERRIDE`, `.factory-dev` via `FACTORY_ENV`.
- `sessions/<sanitized-cwd>/<sessionId>.jsonl` — append-only transcript,
  **contains no token data at all**.
- `sessions/<sanitized-cwd>/<sessionId>.settings.json` — the mutable
  "session settings snapshot" rewritten in place; carries cumulative
  `tokenUsage`. `.settings.json.bak` is a one-generation rolling backup.
- `sessions-index.json` — mutable cache (`{sessionId, mtime, settingsMtime,
  title, cwd, messagesCount, callingSessionId, callingToolUseId, tags}`);
  `callingSessionId` + `tags:[{name:"exec"|"subagent"}]` mark subagent
  lineage. Advisory only; reconcile against files.
- No SQLite; no per-turn usage history survives locally (only the log file
  has an accidental cumulative time-series, unsuitable as a source).

## Usage semantics

- Snapshot fields: `tokenUsage.{inputTokens, outputTokens,
  cacheCreationTokens, cacheReadTokens, thinkingTokens, factoryCredits}`.
  Values are **cumulative per session** (`+=` accumulation in the binary);
  never sum successive reads of the same file — replace per session id.
- Parent/child rollup: `inclusiveTokenUsage = tokenUsage + Σ
  childInclusiveTokenUsageBySessionId`. Always ingest **own `tokenUsage`**
  for every session (parents and children alike); using the inclusive total
  double counts subagents. Subagent sessions have their own settings files.
- `thinkingTokens` is reasoning reported separately from output
  (provider-dependent overlap, not resolvable locally).
- Model: single `model` string = **latest model only** (mid-session switches
  re-attribute everything to the final model — a provenance limit).
  `custom:` prefix marks BYOK models (leave unpriced). `providerLock` is the
  current provider. `factoryCredits` is subscription credits, never USD.
- Timestamps: settings files have only `providerLockTimestamp` (RFC 3339 ms,
  marks provider selection, not spend). File mtime is the only last-accrual
  marker; transcript lines have RFC 3339 ms timestamps for session span.
- Zero-total snapshots are valid (BYOK), not failures. Duplicate session ids
  across directories happen when Droid moves sessions; keep the greater
  mtime and warn.

## Community parsers (cross-checks)

- ccusage `rust/adapters/droid/`: `**/*.settings.json` under the sessions
  root, latest-snapshot-wins dedup keyed on `providerLockTimestamp` (mis
  ranks moved files), dates whole sessions by lock time.
- tokscale `sessions/droid.rs`: timestamp = max(mtime, lock time); apportions
  cumulative totals across transcript turns by byte weights as an estimate.
- Waku and Tokens do not support Droid.

## Recommended adapter shape (when v1.1 picks this up)

Mutable-snapshot source (Replace per session id in one transaction), one
cumulative record per session, own-totals only, mtime for recency,
`providerLockTimestamp` + raw model string in provenance, transcript span for
day attribution if needed (estimate-flagged), `.bak` fallback with warning,
duplicate-id latest-mtime-wins with warning. Also expects a
`DROID_SESSIONS_DIR`-style custom root override for testing.
