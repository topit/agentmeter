use agentmeter_app::OverviewLoadErrorKind;
use agentmeter_core::{OverviewSnapshot, SourceHealthState};

#[derive(Debug, Eq, PartialEq)]
pub struct OverviewRequest(u64);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum OverviewLoadState {
    #[default]
    Loading,
    Empty,
    Populated,
    Partial,
    Error(OverviewLoadErrorKind),
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OverviewState {
    latest_request: u64,
    load_state: OverviewLoadState,
    snapshot: Option<OverviewSnapshot>,
}

impl OverviewState {
    pub fn begin_request(&mut self) -> OverviewRequest {
        self.latest_request = self
            .latest_request
            .checked_add(1)
            .expect("overview request generation overflowed");
        self.load_state = OverviewLoadState::Loading;
        OverviewRequest(self.latest_request)
    }

    pub fn apply_snapshot(&mut self, request: OverviewRequest, snapshot: OverviewSnapshot) -> bool {
        if request.0 != self.latest_request {
            return false;
        }
        self.load_state = classify_snapshot(&snapshot);
        self.snapshot = Some(snapshot);
        true
    }

    pub fn apply_error(&mut self, request: OverviewRequest, error: OverviewLoadErrorKind) -> bool {
        if request.0 != self.latest_request {
            return false;
        }
        self.load_state = OverviewLoadState::Error(error);
        true
    }

    pub const fn load_state(&self) -> OverviewLoadState {
        self.load_state
    }

    pub const fn snapshot(&self) -> Option<&OverviewSnapshot> {
        self.snapshot.as_ref()
    }
}

fn classify_snapshot(snapshot: &OverviewSnapshot) -> OverviewLoadState {
    if snapshot.source_health.sources.iter().any(|source| {
        matches!(
            source.state,
            SourceHealthState::Partial
                | SourceHealthState::SetupRequired
                | SourceHealthState::UnsupportedSchema
                | SourceHealthState::Error
        )
    }) {
        OverviewLoadState::Partial
    } else if snapshot.event_count == 0 {
        OverviewLoadState::Empty
    } else {
        OverviewLoadState::Populated
    }
}

#[cfg(test)]
mod tests {
    use agentmeter_app::OverviewLoadErrorKind;
    use agentmeter_core::{
        OverviewCostSummary, OverviewDataQuality, OverviewSnapshot, SourceHealth,
        SourceHealthSnapshot, SourceHealthState, SourcePermissionState, TokenBreakdown,
    };

    use super::{OverviewLoadState, OverviewState};

    #[test]
    fn rejects_an_out_of_order_snapshot() {
        let mut state = OverviewState::default();
        let first = state.begin_request();
        let second = state.begin_request();

        assert!(state.apply_snapshot(second, snapshot(2)));
        assert!(!state.apply_snapshot(first, snapshot(1)));
        assert_eq!(state.snapshot().unwrap().generation, 2);
        assert_eq!(state.load_state(), OverviewLoadState::Empty);
    }

    #[test]
    fn classifies_populated_partial_and_error_states() {
        let mut state = OverviewState::default();
        let populated = state.begin_request();
        let mut populated_snapshot = snapshot(1);
        populated_snapshot.event_count = 1;
        assert!(state.apply_snapshot(populated, populated_snapshot));
        assert_eq!(state.load_state(), OverviewLoadState::Populated);

        let partial = state.begin_request();
        let mut partial_snapshot = snapshot(2);
        partial_snapshot.event_count = 1;
        partial_snapshot
            .source_health
            .sources
            .push(source_health(SourceHealthState::UnsupportedSchema));
        assert!(state.apply_snapshot(partial, partial_snapshot));
        assert_eq!(state.load_state(), OverviewLoadState::Partial);

        let setup_required = state.begin_request();
        let mut no_usage = snapshot(3);
        no_usage
            .source_health
            .sources
            .push(source_health(SourceHealthState::SetupRequired));
        assert!(state.apply_snapshot(setup_required, no_usage));
        assert_eq!(state.load_state(), OverviewLoadState::Partial);

        let failed = state.begin_request();
        assert!(state.apply_error(failed, OverviewLoadErrorKind::Database));
        assert_eq!(
            state.load_state(),
            OverviewLoadState::Error(OverviewLoadErrorKind::Database)
        );
    }

    fn snapshot(generation: u64) -> OverviewSnapshot {
        OverviewSnapshot {
            generation,
            tokens: TokenBreakdown::default(),
            event_count: 0,
            session_count: 0,
            active_days: 0,
            model_count: 0,
            costs: OverviewCostSummary::default(),
            data_quality: OverviewDataQuality::default(),
            source_health: SourceHealthSnapshot {
                generation: 0,
                sources: Vec::new(),
            },
        }
    }

    fn source_health(state: SourceHealthState) -> SourceHealth {
        SourceHealth {
            installation_id: "installation-synthetic".into(),
            source_object_id: None,
            adapter_id: "synthetic".into(),
            root_path: "/fixture/agentmeter".into(),
            native_path: None,
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
