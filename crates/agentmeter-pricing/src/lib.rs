//! Reversible AgentMeter pricing of immutable token facts.

pub use agentmeter_core::CostKind;
use agentmeter_core::NanoUsd;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CostEstimate {
    pub usd: Option<NanoUsd>,
    pub kind: CostKind,
    pub pricing_key: Option<String>,
    pub pricing_source: Option<String>,
}
