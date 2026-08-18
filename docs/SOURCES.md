# Source support status

AgentMeter distinguishes implemented parsers from product-level support. A source is **supported** only after it satisfies the discovery, fixture, reconciliation, diagnostics, cross-check, i18n, and UI criteria in [`PLAN.md`](PLAN.md#definition-of-supported).

| Source path | Contract | Status | Limitations |
|---|---|---|---|
| Amp `--stream-json` capture | [Official Amp Streaming JSON documentation](https://ampcode.com/news/streaming-json) | parser implemented; not yet product-supported | opt-in capture only; the documented stream omits event timestamps, model IDs, and stable per-event IDs |
| Amp local `threads/T-*.json` | undocumented local implementation | research only | schema is not an Amp compatibility contract; must remain experimental until fixture and cross-check requirements pass |
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
