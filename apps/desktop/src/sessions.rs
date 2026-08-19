use agentmeter_app::{LocalDataErrorKind, SessionSummary, SessionsSnapshot};

use crate::{Locale, MessageKey, confidence_key};

#[derive(Debug, Eq, PartialEq)]
pub struct SessionsRequest(u64);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SessionsLoadState {
    #[default]
    Loading,
    Empty,
    Populated,
    Error(LocalDataErrorKind),
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SessionsState {
    latest_request: u64,
    load_state: SessionsLoadState,
    snapshot: Option<SessionsSnapshot>,
}

impl SessionsState {
    pub fn begin_request(&mut self) -> SessionsRequest {
        self.latest_request = self
            .latest_request
            .checked_add(1)
            .expect("sessions request generation overflowed");
        self.load_state = SessionsLoadState::Loading;
        SessionsRequest(self.latest_request)
    }

    pub fn apply_snapshot(&mut self, request: SessionsRequest, snapshot: SessionsSnapshot) -> bool {
        if request.0 != self.latest_request {
            return false;
        }
        self.load_state = if snapshot.sessions.is_empty() {
            SessionsLoadState::Empty
        } else {
            SessionsLoadState::Populated
        };
        self.snapshot = Some(snapshot);
        true
    }

    pub fn apply_error(&mut self, request: SessionsRequest, error: LocalDataErrorKind) -> bool {
        if request.0 != self.latest_request {
            return false;
        }
        self.load_state = SessionsLoadState::Error(error);
        true
    }

    pub const fn load_state(&self) -> SessionsLoadState {
        self.load_state
    }

    pub const fn snapshot(&self) -> Option<&SessionsSnapshot> {
        self.snapshot.as_ref()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionCard {
    pub session_id: String,
    pub detail: Vec<(MessageKey, String)>,
    pub unpriced: bool,
}

impl SessionCard {
    pub fn from_summary(summary: &SessionSummary, locale: Locale) -> Self {
        let unavailable = || locale.text(MessageKey::NotAvailable).to_owned();
        let list = |values: &[String]| {
            if values.is_empty() {
                unavailable()
            } else {
                values.join(", ")
            }
        };
        Self {
            session_id: summary.session_id.clone(),
            detail: vec![
                (
                    MessageKey::SessionsStarted,
                    locale.format_unix_ms_utc(summary.started_at_unix_ms),
                ),
                (
                    MessageKey::SessionsDuration,
                    locale.format_duration_ms(
                        summary
                            .ended_at_unix_ms
                            .saturating_sub(summary.started_at_unix_ms),
                    ),
                ),
                (
                    MessageKey::SessionsProject,
                    summary.project.clone().unwrap_or_else(unavailable),
                ),
                (MessageKey::SessionsClient, summary.client.clone()),
                (MessageKey::SessionsProvider, list(&summary.providers)),
                (MessageKey::SessionsModel, list(&summary.models)),
                (
                    MessageKey::TotalTokens,
                    locale.format_count(summary.total_tokens),
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
                (MessageKey::SessionsAdapter, summary.adapter_id.clone()),
                (MessageKey::SessionsSourceKind, summary.source_kind.clone()),
                (
                    MessageKey::SessionsParserVersion,
                    locale.format_count(u64::from(summary.parser_version)),
                ),
            ],
            unpriced: summary.unpriced_event_count != 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use agentmeter_app::{SessionSummary, SessionsSnapshot};
    use agentmeter_core::{DataConfidence, NanoUsd};

    use super::{SessionCard, SessionsLoadState, SessionsState};
    use crate::{Locale, MessageKey};

    #[test]
    fn rejects_stale_snapshots_and_errors() {
        let mut state = SessionsState::default();
        let first = state.begin_request();
        let second = state.begin_request();
        assert!(state.apply_snapshot(second, snapshot(2, Vec::new())));
        assert!(!state.apply_snapshot(first, snapshot(1, vec![summary()])));
        assert_eq!(state.snapshot().unwrap().generation, 2);
        assert_eq!(state.load_state(), SessionsLoadState::Empty);
    }

    #[test]
    fn builds_localized_content_free_session_cards() {
        for locale in [Locale::En, Locale::ZhCn] {
            let card = SessionCard::from_summary(&summary(), locale);
            assert_eq!(card.session_id, "session-synthetic");
            assert!(card.unpriced);
            assert!(card.detail.contains(&(
                MessageKey::SessionsDuration,
                locale.format_duration_ms(90_000)
            )));
            assert!(card.detail.contains(&(
                MessageKey::SessionsProvider,
                "provider-a, provider-b".to_owned()
            )));
            assert!(card.detail.contains(&(
                MessageKey::ApiEquivalentCost,
                locale.format_usd(NanoUsd::from_nanos(100_000_000))
            )));
            assert!(!format!("{card:?}").contains("prompt"));
        }
    }

    fn snapshot(generation: u64, sessions: Vec<SessionSummary>) -> SessionsSnapshot {
        SessionsSnapshot {
            generation,
            sessions,
        }
    }

    fn summary() -> SessionSummary {
        SessionSummary {
            source_object_id: "source-synthetic".into(),
            session_id: "session-synthetic".into(),
            adapter_id: "synthetic".into(),
            source_kind: "jsonl".into(),
            parser_version: 3,
            client: "client-a".into(),
            project: None,
            started_at_unix_ms: 1_704_067_200_000,
            ended_at_unix_ms: 1_704_067_290_000,
            total_tokens: 1234,
            event_count: 2,
            confidence: DataConfidence::Derived,
            providers: vec!["provider-a".into(), "provider-b".into()],
            models: vec!["model-a".into(), "model-b".into()],
            provider_reported_usd: None,
            api_equivalent_estimate_usd: Some(NanoUsd::from_nanos(100_000_000)),
            unpriced_event_count: 1,
        }
    }
}
