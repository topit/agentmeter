use agentmeter_app::{
    LocalDataErrorKind, ModelRateSummary, ModelUsageSummary, ModelsPricingSnapshot,
};

use crate::{Locale, MessageKey, confidence_key};

#[derive(Debug, Eq, PartialEq)]
pub struct ModelsPricingRequest(u64);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ModelsPricingLoadState {
    #[default]
    Loading,
    Populated,
    Error(LocalDataErrorKind),
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ModelsPricingState {
    latest_request: u64,
    load_state: ModelsPricingLoadState,
    snapshot: Option<ModelsPricingSnapshot>,
}

impl ModelsPricingState {
    pub fn begin_request(&mut self) -> ModelsPricingRequest {
        self.latest_request = self
            .latest_request
            .checked_add(1)
            .expect("models/pricing request generation overflowed");
        self.load_state = ModelsPricingLoadState::Loading;
        ModelsPricingRequest(self.latest_request)
    }

    pub fn apply_snapshot(
        &mut self,
        request: ModelsPricingRequest,
        snapshot: ModelsPricingSnapshot,
    ) -> bool {
        if request.0 != self.latest_request {
            return false;
        }
        self.load_state = ModelsPricingLoadState::Populated;
        self.snapshot = Some(snapshot);
        true
    }

    pub fn apply_error(
        &mut self,
        request: ModelsPricingRequest,
        error: LocalDataErrorKind,
    ) -> bool {
        if request.0 != self.latest_request {
            return false;
        }
        self.load_state = ModelsPricingLoadState::Error(error);
        true
    }

    pub const fn load_state(&self) -> ModelsPricingLoadState {
        self.load_state
    }

    pub const fn snapshot(&self) -> Option<&ModelsPricingSnapshot> {
        self.snapshot.as_ref()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelCard {
    pub model: String,
    pub detail: Vec<(MessageKey, String)>,
    pub unpriced: bool,
}

impl ModelCard {
    pub fn from_summary(summary: &ModelUsageSummary, locale: Locale) -> Self {
        let unavailable = || locale.text(MessageKey::NotAvailable).to_owned();
        let list = |values: &[String]| {
            if values.is_empty() {
                unavailable()
            } else {
                values.join(", ")
            }
        };
        let cache_denominator = summary
            .tokens
            .input
            .saturating_add(summary.tokens.cache_read);
        Self {
            model: summary.model.clone(),
            detail: vec![
                (
                    MessageKey::ModelsProvider,
                    summary.provider.clone().unwrap_or_else(unavailable),
                ),
                (MessageKey::ModelsClients, list(&summary.clients)),
                (
                    MessageKey::TotalTokens,
                    locale.format_count(summary.total_tokens),
                ),
                (
                    MessageKey::ModelsInputTokens,
                    locale.format_count(summary.tokens.input),
                ),
                (
                    MessageKey::ModelsOutputTokens,
                    locale.format_count(summary.tokens.output),
                ),
                (
                    MessageKey::ModelsCacheReadTokens,
                    locale.format_count(summary.tokens.cache_read),
                ),
                (
                    MessageKey::ModelsCacheWriteTokens,
                    locale.format_count(summary.tokens.cache_write),
                ),
                (
                    MessageKey::ModelsReasoningTokens,
                    locale.format_count(summary.tokens.reasoning),
                ),
                (
                    MessageKey::ModelsCacheEfficiency,
                    locale.format_ratio(summary.tokens.cache_read, cache_denominator),
                ),
                (
                    MessageKey::SessionsEvents,
                    locale.format_count(summary.event_count),
                ),
                (
                    MessageKey::ProviderReportedCost,
                    summary
                        .provider_reported_usd
                        .map(|cost| locale.format_usd(cost))
                        .unwrap_or_else(unavailable),
                ),
                (
                    MessageKey::ApiEquivalentCost,
                    summary
                        .api_equivalent_estimate_usd
                        .map(|cost| locale.format_usd(cost))
                        .unwrap_or_else(unavailable),
                ),
                (
                    MessageKey::SessionsConfidence,
                    locale.text(confidence_key(summary.confidence)).to_owned(),
                ),
                (MessageKey::ModelsPricingKey, list(&summary.pricing_keys)),
                (MessageKey::ModelsPricingRule, list(&summary.pricing_rules)),
                (
                    MessageKey::ModelsPricingConfidence,
                    summary
                        .pricing_confidence
                        .map(|confidence| locale.text(confidence_key(confidence)).to_owned())
                        .unwrap_or_else(unavailable),
                ),
            ],
            unpriced: summary.unpriced_event_count != 0,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RateCard {
    pub key: String,
    pub aliases: String,
    pub detail: Vec<(MessageKey, String)>,
}

impl RateCard {
    pub fn from_summary(summary: &ModelRateSummary, locale: Locale) -> Self {
        Self {
            key: summary.key.clone(),
            aliases: if summary.aliases.is_empty() {
                locale.text(MessageKey::NotAvailable).to_owned()
            } else {
                summary.aliases.join(", ")
            },
            detail: vec![
                (
                    MessageKey::PricingInputRate,
                    locale.format_usd(summary.input_per_million),
                ),
                (
                    MessageKey::PricingOutputRate,
                    locale.format_usd(summary.output_per_million),
                ),
                (
                    MessageKey::PricingCacheReadRate,
                    locale.format_usd(summary.cache_read_per_million),
                ),
                (
                    MessageKey::PricingCacheWriteRate,
                    locale.format_usd(summary.cache_write_per_million),
                ),
                (
                    MessageKey::PricingReasoningRate,
                    locale.format_usd(summary.reasoning_per_million),
                ),
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use agentmeter_app::{
        LocalDataErrorKind, ModelRateSummary, ModelUsageSummary, ModelsPricingSnapshot,
        PricingApplicationSummary,
    };
    use agentmeter_core::{DataConfidence, NanoUsd, TokenBreakdown};

    use super::{ModelCard, ModelsPricingLoadState, ModelsPricingState, RateCard};
    use crate::{Locale, MessageKey};

    #[test]
    fn rejects_stale_snapshots_and_errors() {
        let mut state = ModelsPricingState::default();
        let first = state.begin_request();
        let second = state.begin_request();
        assert!(state.apply_snapshot(second, snapshot(2)));
        assert!(!state.apply_snapshot(first, snapshot(1)));
        let stale_error = state.begin_request();
        let latest = state.begin_request();
        assert!(state.apply_snapshot(latest, snapshot(3)));
        assert!(!state.apply_error(stale_error, LocalDataErrorKind::Database));
        assert_eq!(state.snapshot().unwrap().generation, 3);
        assert_eq!(state.load_state(), ModelsPricingLoadState::Populated);
    }

    #[test]
    fn builds_localized_model_and_rate_cards() {
        for locale in [Locale::En, Locale::ZhCn] {
            let model = ModelCard::from_summary(&model(), locale);
            assert_eq!(model.model, "model-synthetic");
            assert!(model.unpriced);
            assert!(model.detail.contains(&(
                MessageKey::ModelsCacheEfficiency,
                locale.format_ratio(30, 130)
            )));
            assert!(model.detail.contains(&(
                MessageKey::ModelsPricingRule,
                "unpriced:no-match".to_owned()
            )));
            let rate = RateCard::from_summary(&rate(), locale);
            assert_eq!(rate.key, "provider/model-synthetic");
            assert_eq!(rate.aliases, "model-alias");
            assert!(rate.detail.contains(&(
                MessageKey::PricingInputRate,
                locale.format_usd(NanoUsd::from_nanos(1_000_000_000))
            )));
        }
    }

    fn snapshot(generation: u64) -> ModelsPricingSnapshot {
        ModelsPricingSnapshot {
            generation,
            dataset_source: "synthetic-reviewed".into(),
            dataset_version: "1".into(),
            rates: vec![rate()],
            models: vec![model()],
            applied: Some(PricingApplicationSummary {
                source: "synthetic-reviewed".into(),
                version: "1".into(),
                content_hash: "synthetic-content".into(),
                dataset_updated_at_unix_ms: 1_704_067_200_000,
                priced_event_count: 0,
                unpriced_event_count: 1,
            }),
        }
    }

    fn model() -> ModelUsageSummary {
        ModelUsageSummary {
            provider: Some("provider".into()),
            model: "model-synthetic".into(),
            clients: vec!["client".into()],
            tokens: TokenBreakdown {
                input: 100,
                output: 20,
                cache_read: 30,
                cache_write: 4,
                reasoning: 5,
            },
            total_tokens: 159,
            event_count: 2,
            confidence: DataConfidence::Derived,
            provider_reported_usd: None,
            api_equivalent_estimate_usd: None,
            unpriced_event_count: 1,
            pricing_keys: Vec::new(),
            pricing_rules: vec!["unpriced:no-match".into()],
            pricing_confidence: Some(DataConfidence::Estimated),
        }
    }

    fn rate() -> ModelRateSummary {
        ModelRateSummary {
            key: "provider/model-synthetic".into(),
            aliases: vec!["model-alias".into()],
            input_per_million: NanoUsd::from_nanos(1_000_000_000),
            output_per_million: NanoUsd::from_nanos(2_000_000_000),
            cache_read_per_million: NanoUsd::from_nanos(100_000_000),
            cache_write_per_million: NanoUsd::from_nanos(1_250_000_000),
            reasoning_per_million: NanoUsd::from_nanos(2_000_000_000),
        }
    }
}
