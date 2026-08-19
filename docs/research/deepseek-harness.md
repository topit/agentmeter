# DeepSeek Harness (dsh) local format — research note for the v1.1 adapter

Deferred from v1 on 2026-08-19 (user decision: finish core scope first).
Researched against `deepseek-ai/deepseek-harness` at commit `99f6f02`
(2026-08-17, tag `dsh-v0.1.0-rc.7`); the npm package is `@deepseek-ai/dsh`,
command `dsh`.

## Verdict

A first-party DeepSeek agent harness with a real local contract: an
append-only, versioned JSONL session log with per-request provider, model,
and token usage. The roadmap risk "no stable local contract" should be
re-labeled: the format is v0/pre-release with explicit refusal semantics —
a parser-version invalidation concern, not a blocker. A dedicated read-only
adapter is the right strategy; no proxy interception and no API usage
endpoint exists (platform.deepseek.com shows account usage web-only).

## Storage layout

- Home `~/.dsh` (`DSH_HOME` env override; blank treated as unset).
- Sessions: `~/.dsh/sessions/--<normalized-cwd>--/<encoded-session-id>/`
  with `session.jsonl.zstd` by default (checksummed zstd frames: one header
  frame + one frame per append batch — frame boundaries map cleanly onto
  checkpointing) or `session.jsonl` when compression is `none`.
- Line 1 is an immutable header `{type:"session", version, id, cwd?,
  createdAt, parentSession?, seedLength?, origin?, delegationDepth,
  agentPreset?}`. Later lines are `{type, seq, time, data, ignorable?,
  surfaceOp?, sourceEventSeqs?}` with contiguous `seq`.

## Usage semantics

- `assistant/message` events carry `usage?:
  {inputTokens, outputTokens, cacheReadTokens?, cacheWriteTokens?,
  reasoningTokens?}` and `message.source = {kind:"model", provider, model}`
  (provider route e.g. `deepseek-official`; a dormant `llm-pi-ai` adapter
  means non-DeepSeek providers can appear — attribute per event).
- Buckets are already mutually exclusive: DeepSeek's wire semantics are
  `prompt_tokens = cache_hit + cache_miss`, and dsh subtracts cache hits in
  `mapUsage` — record the semantics in provenance anyway.
- Compaction summarization requests also consume tokens and are logged with
  their own provider/model/usage — count both event kinds.
- Skip packed chunk rows (`text-chunks`, `reasoning-chunks`,
  `tool-call-chunks`) when reading usage.

## Stability and hazards

- `SESSION_FORMAT_VERSION = 0` ("pre-release, no compatibility implied");
  backends refuse other header versions. Unknown non-`ignorable` event types
  refuse rather than skip. The adapter should mirror these refusals: gate on
  header version, persist warnings for `ignorable` unknown events, and
  surface version drift instead of parsing through it.
- Append-only with fsync; torn-tail recovery via synthetic closers; no
  deletion API; one live writer per session. Fork/parent linkage via
  `parentSession` in the header.
- Privacy: session lines contain full message content. The adapter must
  extract usage only, never log content, and fixtures must be sanitized.

## Community parsers

- tokscale supports dsh (reads `~/.dsh/sessions/**/session.jsonl.zstd`,
  honors `DSH_HOME`) — independent corroboration of paths and format.
- ccusage, Waku, Tokens: no dsh support found.
