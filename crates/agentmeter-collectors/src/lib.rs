//! AgentMeter source discovery and ingestion contracts.

use std::path::PathBuf;

use agentmeter_core::UsageEvent;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceKind {
    AppendOnlyJsonl,
    MutableJson,
    Sqlite,
    Api,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceCandidate {
    pub path: PathBuf,
    pub kind: SourceKind,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SourceCheckpoint {
    pub byte_offset: Option<u64>,
    pub parser_state: Vec<u8>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct IngestBatch {
    pub events: Vec<UsageEvent>,
    pub checkpoint: SourceCheckpoint,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CollectorError {
    pub message: String,
}

impl std::fmt::Display for CollectorError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for CollectorError {}

/// Each adapter owns discovery and source-specific reconciliation. Database
/// transactions, pricing, and presentation remain outside the adapter.
pub trait CollectorAdapter {
    fn id(&self) -> &'static str;
    fn parser_version(&self) -> u32;
    fn discover(&self) -> Result<Vec<SourceCandidate>, CollectorError>;
    fn ingest(
        &self,
        source: &SourceCandidate,
        checkpoint: Option<&SourceCheckpoint>,
    ) -> Result<IngestBatch, CollectorError>;
}
