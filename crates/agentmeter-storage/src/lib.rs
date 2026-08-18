//! Durable local storage for AgentMeter events.
//!
//! SQLite migrations and repositories will live here. The schema is defined
//! in `docs/PLAN.md` before the first migration is committed.

pub const INITIAL_SCHEMA_VERSION: u32 = 0;
