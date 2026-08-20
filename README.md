# AgentMeter

> A local-first macOS app that explains token usage across your coding agents.

AgentMeter reads the usage records your coding agents already keep on this
machine, normalizes them into an auditable SQLite event ledger, and presents
token, cost, session, model, and collection-health views — in English and
简体中文, in light, dark, and system themes.

## What it does

- **Collects locally, on launch and on demand.** A progress indicator and a
  rescan command are built in; long first scans can be cancelled and resume
  safely. Vendor files are only ever read, never modified.
- **Supported sources (v1.0):** Codex CLI and the Codex desktop app (one
  store, `~/.codex`), Pi (`~/.pi/agent/sessions`), and Kimi / Kimi Code
  (`~/.kimi` and `~/.kimi-code` wire journals, both layouts). Amp's local
  history remains experimental and opt-in; more agents are planned after
  1.0 (see [docs/PLAN.md](docs/PLAN.md)).
- **No silent zeroes.** Missing folders, unsupported schemas, parser errors,
  and duplicate or partial data show up as explicit source states with
  remediation hints — collection failure never looks like "you used zero
  tokens".
- **Facts before estimates.** Token facts are immutable; costs are separate,
  reversible facts. Provider-reported costs and API-equivalent estimates are
  always labeled separately, and unknown models stay visibly unpriced
  instead of guessing. The bundled rate dataset only contains prices
  verified against official provider pricing pages
  ([sources](docs/research/rates-2026-08-19.md)).
- **Export what you want, when you want.** JSON and CSV exports of the
  normalized event ledger are written locally by an explicit command and
  contain usage facts only — never paths, diagnostics, or message content.
- **History is durable.** Events survive even if you delete the original
  agent files; nothing is auto-deleted. Remove `~/Library/Application
  Support/AgentMeter` to start fresh.

## Privacy

- AgentMeter is local-first: prompts, responses, code, and message content
  are never read or stored — only token counts and metadata.
- No telemetry, no analytics, no cloud sync, no network calls in v1.0.
  Nothing leaves this machine except the files you export yourself.
- Data lives in `~/Library/Application Support/AgentMeter/agentmeter.db`
  (plus `exports/`) with user-only filesystem permissions.

## Getting the app

Pre-beta builds are produced by
[CI](https://github.com/topit/agentmeter/actions/workflows/release.yml):
download the latest `AgentMeter-macos-*` artifact and open the DMG. Builds
are not yet signed — right-click → Open the first time. Signing and
notarization arrive with the 1.0 beta.

First launch reads your local history once (about half a minute for a
few hundred sessions); later launches are fast incremental refreshes.

## Development

The repository pins Rust 1.96.0.

```sh
cargo fmt --all --check
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

On macOS, run the desktop app with `cargo run -p agentmeter-desktop`. The
GPUI dependency is pinned to Zed revision
`c24358d96cdb4ce14ecbc088462295353b0103f0` and compiling it locally needs a
working Metal Toolchain (`xcrun metal --version`); CI provides one. A
synthetic-corpus performance benchmark lives in
`crates/agentmeter-app/tests/performance.rs` (run with `--ignored`).

```text
apps/desktop                   GPUI shell, views, localization, themes
crates/agentmeter-app          application services, ingestion orchestration
crates/agentmeter-core         normalized domain model
crates/agentmeter-collectors   source discovery and per-agent parsers
crates/agentmeter-storage      SQLite event ledger and aggregate queries
crates/agentmeter-pricing      reviewed rate datasets and estimates
```

See the [product and engineering plan](docs/PLAN.md), the
[execution ledger](docs/STATUS.md), [source support status](docs/SOURCES.md),
and the [fixture privacy policy](docs/FIXTURES.md).

## 中文说明

AgentMeter 是一个本地优先的 macOS 桌面应用，统一统计 coding agent 的
token、成本、会话和模型使用情况。当前支持 Codex（CLI 与桌面版）、Pi 和
Kimi / Kimi Code；首次启动会读取本地历史（几百个会话约半分钟），之后启动
均为快速增量刷新。

隐私承诺：不读取提示词、回复或代码内容，只统计 token 与元数据；无遥测、
无云同步，除你主动导出的文件外不会有任何数据离开本机。数据保存在
`~/Library/Application Support/AgentMeter/`。

界面支持英文与简体中文、亮色/暗色/跟随系统主题。详细范围与路线图见
[`docs/PLAN.md`](docs/PLAN.md)。
