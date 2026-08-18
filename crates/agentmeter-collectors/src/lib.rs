//! AgentMeter source discovery and ingestion contracts.

pub mod reference;

use std::path::PathBuf;

use agentmeter_core::UsageRecord;

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
    pub source_len: u64,
    pub prefix_fingerprint: Option<String>,
    pub parser_state: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum IngestMode {
    #[default]
    Append,
    Replace,
}

#[derive(Clone, Copy, Debug)]
pub enum IngestStart<'a> {
    Fresh,
    Resume(&'a SourceCheckpoint),
    Rebuild,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct IngestBatch {
    pub mode: IngestMode,
    pub records: Vec<UsageRecord>,
    pub checkpoint: SourceCheckpoint,
    pub source_fingerprint: String,
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

impl CollectorError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// Each adapter owns discovery and source-specific reconciliation. Database
/// transactions, pricing, and presentation remain outside the adapter.
pub trait CollectorAdapter {
    fn id(&self) -> &'static str;
    fn parser_version(&self) -> u32;
    fn discover(&self) -> Result<Vec<SourceCandidate>, CollectorError>;
    fn ingest(
        &self,
        source: &SourceCandidate,
        start: IngestStart<'_>,
    ) -> Result<IngestBatch, CollectorError>;
}
