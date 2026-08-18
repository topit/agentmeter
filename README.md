# AgentMeter

> A local-first desktop app that explains token usage across coding agents.

AgentMeter reads usage records that agent tools already keep on the local machine, normalizes them into an auditable event ledger, and presents token, cost, session, model, and collection-health views. The first supported desktop platform is macOS; the portable Rust core is designed to support Windows later.

AgentMeter has a tested SQLite event ledger and synthetic reference collectors. It does not collect real agent usage yet.

## Product principles

- **Local first:** prompts, code, session contents, and paths stay on the device.
- **No silent zeroes:** missing permissions, disabled telemetry, unsupported schemas, and parser errors are visible source states.
- **Facts before estimates:** immutable token facts are stored independently from reversible price estimates.
- **Client ≠ provider ≠ model:** Pi using DeepSeek through OpenRouter remains three distinct dimensions.
- **Auditable ingestion:** every normalized event retains source and parser provenance.
- **Native and accessible:** GPUI on macOS first, with English and Simplified Chinese plus light, dark, and system themes.

## Planned source coverage

The first implementation wave targets Amp, Codex CLI, Pi, Kimi/Kimi Code, Factory Droid, and Grok Build CLI. Codex Desktop, DeepSeek Harness, GitHub Copilot CLI, Cursor, and remote bots follow according to the availability and stability of their local usage records.

“Supported” will mean that AgentMeter can identify a source, explain its data quality, parse fixtures, incrementally refresh it, and detect schema drift. Merely finding a directory does not count as support.

## Architecture

```text
apps/desktop              GPUI desktop shell, localization, themes
crates/agentmeter-app          application services and background queries
crates/agentmeter-core         normalized domain model
crates/agentmeter-collectors   source discovery and adapter contracts
crates/agentmeter-storage      SQLite event ledger and aggregate queries
crates/agentmeter-pricing      reported and estimated cost provenance
```

The macOS application pins GPUI to Zed revision `c24358d96cdb4ce14ecbc088462295353b0103f0`. GPUI remains isolated to the macOS target; portable navigation, localization, and theme state can be tested on other platforms without pulling presentation concerns into the core crates.

See the [complete product and engineering plan](docs/PLAN.md), [source support status](docs/SOURCES.md), [current implementation status](docs/STATUS.md), and [fixture privacy policy](docs/FIXTURES.md).

## Development

The repository pins Rust 1.96.0. In an Amp orb, `.agents/setup` installs Rust and the Linux packages needed by GPUI.

```sh
cargo fmt --all --check
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

On macOS, run the current GPUI navigation shell with `cargo run -p agentmeter-desktop`. Other platforms retain a small console fallback for portable contract checks:

```sh
cargo run -p agentmeter-desktop
LANG=zh_CN.UTF-8 cargo run -p agentmeter-desktop
```

## 中文说明

AgentMeter 是一个本地优先的桌面应用，用来统一统计 Amp、Codex、Pi、Kimi Code、Factory Droid 等 coding agent 的 token、成本、会话和模型使用情况。

首个正式版本以 macOS 为目标，界面使用 GPUI。核心采集、存储和计价逻辑保持纯 Rust、跨平台，为后续 Windows 版本做准备。应用从第一天支持英文、简体中文、亮色、暗色和跟随系统主题。

目前仓库已完成 SQLite 事件账本和合成参考采集器，还没有开始读取真实 agent 数据。详细范围、里程碑、验收标准和风险见 [`docs/PLAN.md`](docs/PLAN.md)。
