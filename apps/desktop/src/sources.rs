use agentmeter_app::LocalDataErrorKind;
use agentmeter_core::{SourceHealth, SourceHealthSnapshot, SourceHealthState};

use crate::{Locale, MessageKey, health_state_key, permission_key, remediation_key};

#[derive(Debug, Eq, PartialEq)]
pub struct SourcesRequest(u64);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SourcesLoadState {
    #[default]
    Loading,
    /// No configured installations. Sources that need setup are NOT empty:
    /// a discovered-but-unreadable source must stay visible.
    Empty,
    Populated,
    Error(LocalDataErrorKind),
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SourcesState {
    latest_request: u64,
    load_state: SourcesLoadState,
    snapshot: Option<SourceHealthSnapshot>,
}

impl SourcesState {
    pub fn begin_request(&mut self) -> SourcesRequest {
        self.latest_request = self
            .latest_request
            .checked_add(1)
            .expect("sources request generation overflowed");
        self.load_state = SourcesLoadState::Loading;
        SourcesRequest(self.latest_request)
    }

    pub fn apply_snapshot(
        &mut self,
        request: SourcesRequest,
        snapshot: SourceHealthSnapshot,
    ) -> bool {
        if request.0 != self.latest_request {
            return false;
        }
        self.load_state = if snapshot.sources.is_empty() {
            SourcesLoadState::Empty
        } else {
            SourcesLoadState::Populated
        };
        self.snapshot = Some(snapshot);
        true
    }

    pub fn apply_error(&mut self, request: SourcesRequest, error: LocalDataErrorKind) -> bool {
        if request.0 != self.latest_request {
            return false;
        }
        self.load_state = SourcesLoadState::Error(error);
        true
    }

    pub const fn load_state(&self) -> SourcesLoadState {
        self.load_state
    }

    pub const fn snapshot(&self) -> Option<&SourceHealthSnapshot> {
        self.snapshot.as_ref()
    }
}

/// Presentation-ready text for one source. Localization and formatting happen
/// here so the GPUI layer only lays out strings and never derives state from
/// diagnostic prose: status, permission, and remediation come only from the
/// typed snapshot fields.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceCard {
    pub adapter_id: String,
    /// Native source identity when the installation exposes one; the
    /// installation root otherwise. Local-only fact, never exported.
    pub identity: String,
    pub state: SourceHealthState,
    pub status_label: &'static str,
    pub remediation_label: Option<&'static str>,
    pub detail: Vec<(MessageKey, String)>,
    /// Verbatim source-native warnings and error text; untranslated by policy.
    pub warnings: Vec<String>,
    pub error: Option<String>,
}

impl SourceCard {
    pub fn from_health(health: &SourceHealth, locale: Locale) -> Self {
        let unavailable = || locale.text(MessageKey::NotAvailable).to_owned();
        let timestamp = |unix_ms: Option<i64>| {
            unix_ms
                .map(|value| locale.format_unix_ms_utc(value))
                .unwrap_or_else(unavailable)
        };
        let optional = |value: Option<&str>| {
            value
                .map(|value| value.to_owned())
                .unwrap_or_else(unavailable)
        };
        let detail = vec![
            (MessageKey::SourcesSourcePath, health.root_path.to_owned()),
            (
                MessageKey::SourcesSourceKind,
                optional(health.source_kind.as_deref()),
            ),
            (
                MessageKey::SourcesParserVersion,
                health
                    .parser_version
                    .map(|version| locale.format_count(u64::from(version)))
                    .unwrap_or_else(unavailable),
            ),
            (
                MessageKey::SourcesPermission,
                locale.text(permission_key(health.permission)).to_owned(),
            ),
            (
                MessageKey::SourcesLastScan,
                timestamp(health.last_scan_unix_ms),
            ),
            (
                MessageKey::SourcesLastSuccess,
                timestamp(health.last_success_unix_ms),
            ),
            (
                MessageKey::SourcesLastEvent,
                timestamp(health.last_event_unix_ms),
            ),
            (
                MessageKey::SourcesRecordsChanged,
                locale.format_count(health.records_changed),
            ),
            (
                MessageKey::SourcesWarnings,
                locale.format_count(health.warnings.len() as u64),
            ),
        ];
        Self {
            adapter_id: health.adapter_id.to_owned(),
            identity: health
                .native_path
                .clone()
                .unwrap_or_else(|| health.root_path.to_owned()),
            state: health.state,
            status_label: locale.text(health_state_key(health.state)),
            remediation_label: health
                .remediation
                .map(|remediation| locale.text(remediation_key(remediation))),
            detail,
            warnings: health.warnings.to_owned(),
            error: health.error.to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use agentmeter_app::LocalDataErrorKind;
    use agentmeter_core::{
        SourceHealth, SourceHealthSnapshot, SourceHealthState, SourcePermissionState,
        SourceRemediation,
    };

    use super::{SourceCard, SourcesLoadState, SourcesState};
    use crate::Locale;

    #[test]
    fn rejects_an_out_of_order_sources_snapshot() {
        let mut state = SourcesState::default();
        let first = state.begin_request();
        let second = state.begin_request();

        assert!(state.apply_snapshot(second, snapshot(&[])));
        assert!(!state.apply_snapshot(first, snapshot(&[source(SourceHealthState::Healthy)])));
        assert_eq!(state.load_state(), SourcesLoadState::Empty);
        assert_eq!(
            state.snapshot().map(|snapshot| snapshot.sources.len()),
            Some(0),
            "a stale completion must not replace the current snapshot"
        );
    }

    #[test]
    fn rejects_an_out_of_order_sources_error() {
        let mut state = SourcesState::default();
        let first = state.begin_request();
        let second = state.begin_request();

        assert!(state.apply_error(second, LocalDataErrorKind::Database));
        assert!(!state.apply_error(first, LocalDataErrorKind::DataDirectory));
        assert_eq!(
            state.load_state(),
            SourcesLoadState::Error(LocalDataErrorKind::Database)
        );
    }

    #[test]
    fn setup_required_and_errored_sources_take_precedence_over_empty() {
        let mut state = SourcesState::default();

        let setup = state.begin_request();
        assert!(state.apply_snapshot(setup, snapshot(&[source(SourceHealthState::SetupRequired)])));
        assert_eq!(
            state.load_state(),
            SourcesLoadState::Populated,
            "a discovered source needing setup must not read as 'no sources'"
        );

        let errored = state.begin_request();
        assert!(state.apply_snapshot(errored, snapshot(&[source(SourceHealthState::Error)])));
        assert_eq!(state.load_state(), SourcesLoadState::Populated);

        let empty = state.begin_request();
        assert!(state.apply_snapshot(empty, snapshot(&[])));
        assert_eq!(state.load_state(), SourcesLoadState::Empty);
    }

    #[test]
    fn builds_localized_cards_from_typed_health_without_parsing_diagnostics() {
        let mut denied = source(SourceHealthState::SetupRequired);
        denied.permission = SourcePermissionState::Denied;
        denied.remediation = Some(SourceRemediation::GrantPermission);
        denied.native_path = None;
        denied.source_kind = Some("jsonl".into());
        denied.parser_version = Some(3);
        denied.last_scan_unix_ms = Some(1_787_011_200_000);
        denied.last_success_unix_ms = None;
        denied.last_event_unix_ms = Some(1_787_046_083_000);
        denied.records_changed = 1_234;
        denied.warnings = vec!["synthetic warning".into()];
        denied.error = Some("synthetic [permission] prose".into());

        for locale in [Locale::En, Locale::ZhCn] {
            let card = SourceCard::from_health(&denied, locale);

            assert_eq!(card.adapter_id, "synthetic");
            assert_eq!(card.identity, "/fixture/agentmeter");
            assert_eq!(card.state, SourceHealthState::SetupRequired);
            assert_eq!(
                card.status_label,
                locale.text(crate::MessageKey::HealthSetupRequired)
            );
            assert_eq!(
                card.remediation_label,
                Some(locale.text(crate::MessageKey::RemediationGrantPermission))
            );
            let values: Vec<(crate::MessageKey, &str)> = card
                .detail
                .iter()
                .map(|(key, value)| (*key, value.as_str()))
                .collect();
            assert!(values.contains(&(crate::MessageKey::SourcesLastScan, "2026-08-18 00:00 UTC")));
            assert!(values.contains(&(
                crate::MessageKey::SourcesLastSuccess,
                locale.text(crate::MessageKey::NotAvailable)
            )));
            assert!(values.contains(&(crate::MessageKey::SourcesParserVersion, "3")));
            assert!(values.contains(&(
                crate::MessageKey::SourcesPermission,
                locale.text(crate::MessageKey::PermissionDenied)
            )));
            assert!(values.contains(&(crate::MessageKey::SourcesRecordsChanged, "1,234")));
            assert_eq!(card.warnings, vec!["synthetic warning".to_owned()]);
            assert_eq!(card.error.as_deref(), Some("synthetic [permission] prose"));
        }
    }

    #[test]
    fn cards_keep_disabled_sources_visible_with_their_typed_state() {
        let disabled = source(SourceHealthState::Disabled);
        for locale in [Locale::En, Locale::ZhCn] {
            let card = SourceCard::from_health(&disabled, locale);
            assert_eq!(card.state, SourceHealthState::Disabled);
            assert_eq!(
                card.status_label,
                locale.text(crate::MessageKey::HealthDisabled)
            );
            assert_eq!(card.remediation_label, None);
        }
    }

    fn snapshot(sources: &[SourceHealth]) -> SourceHealthSnapshot {
        SourceHealthSnapshot {
            generation: sources.len() as u64,
            sources: sources.to_vec(),
        }
    }

    fn source(state: SourceHealthState) -> SourceHealth {
        SourceHealth {
            installation_id: "installation-synthetic".into(),
            source_object_id: Some("source-synthetic".into()),
            adapter_id: "synthetic".into(),
            root_path: "/fixture/agentmeter".into(),
            native_path: Some("/fixture/agentmeter/agent.jsonl".into()),
            source_kind: None,
            enabled: true,
            permission: SourcePermissionState::Granted,
            parser_version: None,
            last_scan_unix_ms: None,
            last_success_unix_ms: None,
            last_event_unix_ms: None,
            records_changed: 0,
            warnings: Vec::new(),
            error: None,
            state,
            remediation: None,
        }
    }
}
