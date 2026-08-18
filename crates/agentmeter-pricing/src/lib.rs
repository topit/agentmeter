//! Reversible AgentMeter pricing of immutable token facts.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CostKind {
    ProviderReported,
    ApiEquivalentEstimate,
    SubscriptionCredit,
    Unpriced,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CostEstimate {
    pub usd: Option<f64>,
    pub kind: CostKind,
    pub pricing_key: Option<String>,
    pub pricing_source: Option<String>,
}
