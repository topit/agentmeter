# Source support status

AgentMeter distinguishes implemented parsers from product-level support. A source is **supported** only after it satisfies the discovery, fixture, reconciliation, diagnostics, cross-check, i18n, and UI criteria in [`PLAN.md`](PLAN.md#definition-of-supported).

## Periodic reconciliation and cross-checks

- Enabled sources with granted permission become due according to their latest successful full `Replace`; ordinary incremental appends do not reset that clock. Due append-only sources are reopened with `IngestStart::Rebuild`, and their canonical events, provenance, costs, and checkpoint are replaced atomically.
- The versioned reconciliation JSON reports canonical token buckets and confidence counts per source. Source-native totals are compared only across events that supplied them, with explicit match, mismatch, partial, and unavailable states.
- Reviewed aggregate expectations can be labeled only as source UI/CLI, Waku, Tokens, Tokscale, ccusage, or synthetic fixture evidence. Checks are sorted deterministically by adapter and reference kind.
- Export deliberately excludes filesystem paths, models, session/event IDs, warnings, normalization notes, and source excerpts. Amp, Codex, and Pi pipeline fixtures verify their adapter totals through this same report path.

| Source path | Contract | Status | Limitations |
|---|---|---|---|
| Amp `--stream-json` capture | [Official Amp Streaming JSON documentation](https://ampcode.com/news/streaming-json) | parser implemented; not yet product-supported | opt-in capture only; the documented stream omits event timestamps, model IDs, and stable per-event IDs |
| Amp local `threads/T-*.json` | undocumented local implementation, cross-checked against [Tokens](https://github.com/missuo/tokens) and [Tokscale](https://github.com/junhoyeo/tokscale) | experimental parser implemented; not product-supported | whole-file replacement; schema is not an Amp compatibility contract; credit values are diagnosed but not yet treated as USD cost |
| Codex CLI rollout JSONL | [official OpenAI Codex rollout/protocol source](https://github.com/openai/codex) | incremental parser and lineage reconciliation implemented; not product-supported | compressed rollouts, headless variants, and product-level health/UI acceptance remain pending |
| Pi coding agent session JSONL | [official Pi session manager and AI types](https://github.com/badlogic/pi-mono) | incremental parser, fork reconciliation, and provider cost ingestion implemented; not product-supported | legacy one-directory discovery and product-level health/UI acceptance remain pending |

## Amp Stream JSON normalization

- Only top-level assistant events are counted; child/tool events with a non-null `parent_tool_use_id` are excluded.
- `input_tokens`, `output_tokens`, `cache_read_input_tokens`, and `cache_creation_input_tokens` map to mutually exclusive canonical buckets.
- Amp's observed `usage.iterations` extension is handled by selecting only the final iteration, matching Waku's live-driver behavior. This extension is not part of the published Amp persistence contract and is recorded as a distinct schema variant.
- The official stream does not expose a per-event timestamp. AgentMeter uses the capture file's modification time and records `TimestampOrigin::FileModified` plus a normalization note.
- The official stream example does not expose a model or provider. Missing values remain `unknown`/unpriced rather than being guessed.
- The stream has no native per-event ID. AgentMeter derives a versioned ID from the source-stable session ID and byte offset; prefix verification and source-owned replacement preserve correctness after rewrites.

The collector deserializes only routing and usage fields. Prompt, response, thinking, and tool content are ignored and are never copied into the canonical ledger or diagnostics.

## Experimental Amp local history

- Discovery uses `XDG_DATA_HOME/amp/threads/T-*.json`, falling back to `<home>/.local/share/amp/threads`. A custom root can be supplied directly.
- Every thread JSON file is an independent mutable source. A successful scan replaces only that file's source-owned events; malformed or incompatible JSON fails visibly and preserves the previous ledger state.
- `usageLedger.events` is authoritative when present. Assistant `messages[].usage` rows are matched first by `toMessageId`/`messageId`, then by exact model and token buckets; unmatched assistant rows are retained so a partial ledger does not lose usage.
- Ledger RFC 3339 timestamps are preferred. Missing timestamps fall back to thread creation time, then file modification time, with provenance indicating the fallback.
- Negative counters are clamped to zero, marked `Derived`, and diagnosed. Empty or overflowing totals are skipped with a warning rather than recorded as genuine zero usage.
- Models and providers are never guessed. Missing models remain `unknown`; providers remain unset.
- Amp `credits` are not assumed to be dollars. Their presence is diagnosed until pricing storage can preserve the source value and semantics separately.

All fixtures are inline, deterministic, and synthetic. They were manually constructed on 2026-08-18 from the observed dual ledger/message shape, contain no prompt/response/tool content, and cross-check the same reconciliation outcomes covered by Tokens and Tokscale: full-ledger deduplication, partial-ledger completion, message-ID precedence, and timestamp fallback.

## Codex CLI rollout JSONL

- Discovery honors a supplied Codex home and recursively scans both `sessions` and `archived_sessions`. The filename is the stable adapter source key across archive movement; if both copies temporarily exist, the active copy wins.
- The parser follows OpenAI Codex's `session_meta`, `turn_context`, and `event_msg/token_count` contracts. It preserves thread, model, provider, ordinal, byte offset, timestamp source, and raw usage statistics without deserializing transcript content.
- `last_token_usage` is the preferred per-request increment. `total_token_usage` is persisted in parser state as a cumulative watermark; equal snapshots are skipped, total-only snapshots become component-wise deltas, and regressions are diagnosed as reset boundaries.
- Codex input includes cached input, and reasoning output is included in output. Canonical mutually exclusive buckets are therefore: uncached input = input − cache read − cache write, and non-reasoning output = output − reasoning. Underflow or a source-total mismatch lowers confidence to `Derived` and produces a warning.
- Ordinals provide native record identity within one immutable rollout. Event IDs include the stable filename-derived source key so revert branches that reuse ordinals remain distinct while archive movement preserves identity. Legacy records use the same source scope plus byte offset and complete line bytes.
- Byte checkpoints include parser state and verified prefix continuity. A partial trailing record is retried without advancing the checkpoint; complete malformed records are diagnosed while later records remain collectible; truncation or rewrite triggers source-owned replacement.
- Paginated lineage follows the first canonical `session_meta.history_base`. Its `thread_id` resolves the parent rollout ID from the canonical filename, while `end_ordinal_exclusive` and `end_byte_offset` validate the inherited boundary. The last inherited cumulative snapshot seeds the child parser, so only post-fork deltas are emitted. Parent lookup spans active and archived trees and rejects missing parents, invalid cutoffs, non-paginated segments, and cycles.
- Legacy copied forks use `forked_from_id` only to locate the parent thread. AgentMeter suppresses a leading child usage sequence only while its timestamp and complete token snapshots exactly match the parent prefix as it existed at fork time. A missing parent, invalid timestamp, empty match set, or divergence is retained at `Derived` confidence with a visible warning; no timing-density heuristic silently discards usage.
- Only the first `session_meta` owns thread/provider/lineage metadata. Copied historical metadata cannot replace the child identity.

Synthetic fixtures cover active/archive precedence, official token normalization, cumulative-only deltas, equal-snapshot deduplication, resumed parser state, rewrite recovery, null usage info, malformed middle records, incomplete tails, archived-parent paginated lineage, exact legacy replay, missing parents, cycles, and revert ordinal reuse. The storage pipeline proves parent plus child totals are counted once. Expected behavior was cross-checked against Waku, Tokens/Tokscale, and ccusage; official Codex source remains authoritative.

Codex remains **not supported** at product level. Compressed rollout and headless variants still need a contract assessment and fixtures; source-health, i18n, and UI acceptance coverage also remain required.

## Pi coding-agent session JSONL

- The contract was verified against `badlogic/pi-mono` commit `2509b5c037d366979f2febfce4174b88aeaadc6a`. Discovery recursively scans a supplied sessions root. The default resolver honors `PI_CODING_AGENT_SESSION_DIR`, then `PI_CODING_AGENT_DIR/sessions`, then `<home>/.pi/agent/sessions`; custom settings and `--session-dir` are represented by supplying their resolved root.
- The first `type: "session"` header owns the session ID, format version, and optional `parentSession`. Versions 1–3 are accepted; versions newer than 3 produce a visible unsupported-schema warning and no valid zero. Normal appends use byte checkpoints and prefix continuity; truncation or migration rewrite triggers source-owned replacement.
- Assistant `message.usage` records preserve the native entry ID, message timestamp, provider, and `responseModel` when present (otherwise requested `model`). Every physical branch entry is counted because abandoned branches still represent API work; the `parentId` tree is a context projection, not a billing deletion.
- Canonical input, cache-read, and cache-write map directly. Pi reasoning is a subset of output, so non-reasoning output is `output − reasoning`. `cacheWrite1h` is a subset of cache write and is not added again. Negative counters, subset underflow, total disagreement, invalid timestamps, and zero/overflow totals are diagnosed.
- `compaction` and `branch_summary` usage are additional summarization calls and are counted with unknown provider/model rather than attributed by guesswork. Nested tool-result usage is excluded because Pi defines it as tool work outside main-context accounting.
- Forks and clones copy native entries into a new file and set `parentSession`. AgentMeter locates the discovered parent by filename and suppresses only matching native entry IDs with identical usage facts. Missing parents or changed inherited facts remain visible and lower retained usage to `Derived`; entry IDs are never globally deduplicated across unrelated sessions.
- Pi's provider-reported USD cost is retained as a separate `ProviderReported` fact, never as an API-equivalent estimate. Source decimals are parsed exactly into integer nano-USD; the source `total` is authoritative when present, otherwise valid component buckets are summed. Negative, over-precise, overflowing, or structurally incomplete amounts produce warnings without discarding valid token usage.
- Usage, provenance, cost facts, and checkpoints enter SQLite in one transaction. Replays require the stored cost to match event identity, source replacement removes stale costs through source-owned event deletion, and parser version 2 transactionally rebuilds prior Pi checkpoints to populate the new facts.

All Pi fixtures are deterministic and synthetic, omit message/tool/summary content, and model only schema fields needed for discovery, usage, lineage, malformed/truncated input, schema drift, and storage reconciliation. Waku was used only to contrast live context occupancy; Tokens, Tokscale, and ccusage were cross-checks. Official Pi source remains authoritative.

Pi remains **not supported** at product level until the retained legacy location is assessed and source-health/i18n/UI acceptance criteria pass.
