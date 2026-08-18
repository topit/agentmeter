# Collector fixture policy

AgentMeter collector tests use fixtures to preserve source schemas and parser edge cases without preserving user activity. Committed fixtures must be synthetic by default or, only when a schema cannot otherwise be reproduced accurately, **irreversibly sanitized**. Raw agent logs must never enter Git, including history, patches, review attachments, or test-failure output.

This policy applies to fixture files, generated snapshots, expected outputs, test names, comments, diagnostics, and provenance metadata.

## Preferred workflow: synthetic fixtures

1. Document the schema behavior being tested from upstream source code, official documentation, or an approved observation of the format. Do not copy payload values from a user log.
2. Build the smallest record from invented values. Use obviously fictional, deterministic identities such as `session-synthetic-001`, neutral payload markers such as `[synthetic]`, fixed UTC timestamps, and deliberate boundary token counts.
3. Preserve only fields needed to exercise parsing, source semantics, event identity, normalization, checkpoints, or diagnostics.
4. Derive variants mechanically where practical (append, duplicate, reorder, truncate, or mutate a schema field) so their relationship is reviewable.
5. Assert normalized facts, source-native totals, provenance, warnings, and checkpoint/reconciliation behavior. Tests must not print whole records on failure.

Synthetic values must not be modeled on a real person's names, IDs, directory layout, projects, conversations, or code. Random-looking values are not proof of safety; use recognizable fixture namespaces instead.

## Exceptional workflow: irreversible sanitization

Use a real record only when an adapter author cannot faithfully construct the relevant schema from authoritative information. Keep the original outside the repository and never stage it. Sanitization is reconstruction by allowlist, not search-and-replace redaction:

1. Identify the minimal structural fields and edge condition required by the test.
2. Create a new file containing only those allowlisted fields; do not edit a copy of the original in place.
3. Delete all prompts, responses, tool inputs/outputs, code, free-form text, environment data, URLs, hostnames, paths, and unknown fields.
4. Replace every person-, account-, device-, project-, workspace-, source-, and session-linked identifier with deterministic synthetic values. Preserve equality relationships only within the fixture family when the test needs them.
5. Replace timestamps with fixed dates while retaining only required ordering, equality, timezone, or boundary behavior. Replace numeric usage and cost values unless the exact boundary is necessary and cannot identify a real subject.
6. Recreate paths under an unmistakably fictional portable root (for example, `/fixture/home/...`), retaining only separators, extensions, or nesting required by the parser. Never retain a username or home-directory suffix.
7. Manually compare the reconstructed file against this policy, then have a second reviewer inspect the complete fixture and metadata. Do not submit the original, a raw-versus-sanitized diff, or a hash of the original.
8. Remove temporary working copies after review according to the owner's local data-handling policy.

If a sensitive value cannot be confidently classified or safely transformed, omit the field and keep the adapter unsupported until a safe fixture can be produced. Encryption, encoding, hashing, tokenization without destruction of the key, and visually masking part of a value are not irreversible sanitization.

## Content rules

### Prohibited in Git

- prompts, responses, reasoning text, message summaries, tool transcripts, or code;
- secrets, credentials, cookies, tokens, authorization material, private URLs, or complete environment variables;
- real names, email addresses, account/subscription IDs, device IDs, hostnames, project names, repository remotes, workspace IDs, or session IDs;
- real home paths or path fragments that identify a user, organization, or private project;
- opaque or unknown payloads retained “for realism,” binary database pages, memory dumps, or unreviewed vendor databases;
- raw logs, screenshots of logs, sanitizer input, reversible mappings, or hashes that enable correlation to a real record.

This prohibition also covers malformed fixtures: invalid syntax is not permission to retain an unparsed sensitive tail.

### Allowed structural data

Schema keys, enum values, documented provider/model/client identifiers, booleans, nulls, delimiters, encodings, and source format/version markers may remain. Stable IDs, timestamps, token counts, costs, offsets, and boundary values may remain only when needed for correctness **and** they cannot identify or correlate to a real person, account, device, source, or session. Otherwise replace them with synthetic equivalents.

Preserving source semantics does not mean preserving source content. A fixture may model cache/read/write/reasoning buckets, cumulative totals, timestamp origin, record identity, and malformed structure while all payload text is absent or synthetic.

## Layout, names, and provenance

Place future collector fixtures beneath the owning adapter, using the repository convention established by that adapter (prefer `crates/agentmeter-collectors/tests/fixtures/<adapter>/<schema>/`). Use lowercase kebab-case names that state behavior, not user or source identity, for example:

```text
normal.jsonl
empty.jsonl
malformed-middle.jsonl
truncated-final-record.jsonl
duplicate-event.jsonl
replay-fork.jsonl
schema-drift-unknown-field.jsonl
```

Keep related multi-step states in a named directory with ordered files such as `01-initial.jsonl`, `02-appended.jsonl`, and `03-rewritten.jsonl`. Expected outputs should live beside the test or in a clearly named `expected` directory and must obey the same privacy rules.

Each fixture family must have reviewable provenance metadata in a nearby manifest or test documentation containing:

- adapter ID, parser/schema variant, and fixture purpose;
- classification: `synthetic` or `irreversibly-sanitized`;
- generator/tool version or manual construction method;
- upstream documentation/source-code reference and version when available;
- synthetic transformations and intentionally preserved boundary semantics;
- creation or last-review date and reviewer identity by repository handle;
- expected records, warnings, and reconciliation behavior.

Do not record the original file name, machine path, user identity, private issue link, raw-record checksum, or any reversible mapping. Provenance demonstrates review; it must not create a new identifier leak.

## Required coverage and validation

For every supported schema, tests should cover normal and empty input, malformed records, unknown fields, truncation, append, rewrite, duplicates, timestamp fallback, and schema drift. Add replay/fork, cumulative-to-delta, source rotation, or large-history cases where the format supports them. A large-history fixture should be generated deterministically from synthetic templates rather than committed from a real corpus.

Edge-case fixtures must be constructed safely:

- **Malformed:** synthesize invalid delimiters, types, or required fields; use neutral text and remove any trailing raw bytes.
- **Truncated:** cut a synthetic record at a deterministic byte boundary.
- **Duplicate:** repeat or regenerate a synthetic record with the equality relationships needed by deduplication.
- **Replay/fork:** reuse synthetic event IDs and timestamps across explicit synthetic branches.
- **Schema drift:** add, remove, rename, or change the type of invented fields; never retain unknown real payloads merely to test forwarding.

Validation must confirm parser results and diagnostics, semantic event identity, dedup/replay behavior, prefix continuity and truncation recovery for JSONL, replacement ownership for mutable snapshots, source-native and normalized totals, timestamp origin, confidence, and checkpoint behavior as applicable. Unknown or failed parsing must produce diagnostics rather than a valid zero.

## Pre-commit review checklist

- [ ] The fixture is minimal and synthetic, or its manifest justifies irreversible sanitization.
- [ ] No raw source file was staged, committed, attached, or included in a patch/history.
- [ ] Prompts, responses, reasoning, code, tool payloads, free-form text, secrets, and environment data are absent.
- [ ] Names, account/device/project/workspace/session identifiers, hostnames, URLs, and home paths are absent or unmistakably synthetic.
- [ ] Unknown fields and malformed/truncated tails contain no retained source payload.
- [ ] Remaining IDs, dates, counts, costs, and boundaries are necessary, non-identifying, and non-correlatable.
- [ ] Fixture names, expected outputs, comments, diagnostics, snapshots, and metadata pass the same review.
- [ ] Provenance metadata states classification, purpose, method, references, transformations, expectations, date, and reviewer without identifying the original.
- [ ] Tests cover the intended normal or edge behavior and do not dump complete records on failure.
- [ ] A second reviewer inspected every irreversibly sanitized fixture in full.
- [ ] Repository secret/privacy scanning and the adapter's focused fixture tests pass; `git diff --check` reports no whitespace errors.

A reviewer must reject a fixture when safety depends on context unavailable in the change, when sanitization is merely cosmetic, or when parser coverage can be achieved with synthetic data instead.
