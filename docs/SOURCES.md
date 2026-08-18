# Source support status

AgentMeter distinguishes implemented parsers from product-level support. A source is **supported** only after it satisfies the discovery, fixture, reconciliation, diagnostics, cross-check, i18n, and UI criteria in [`PLAN.md`](PLAN.md#definition-of-supported).

| Source path | Contract | Status | Limitations |
|---|---|---|---|
| Amp `--stream-json` capture | [Official Amp Streaming JSON documentation](https://ampcode.com/news/streaming-json) | parser implemented; not yet product-supported | opt-in capture only; the documented stream omits event timestamps, model IDs, and stable per-event IDs |
| Amp local `threads/T-*.json` | undocumented local implementation, cross-checked against [Tokens](https://github.com/missuo/tokens) and [Tokscale](https://github.com/junhoyeo/tokscale) | experimental parser implemented; not product-supported | whole-file replacement; schema is not an Amp compatibility contract; credit values are diagnosed but not yet treated as USD cost |
| Codex CLI | upstream source and local JSONL fixtures pending | planned for M2 | not implemented |
| Pi | upstream source and local JSONL fixtures pending | planned for M2 | not implemented |

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
