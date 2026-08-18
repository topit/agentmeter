use agentmeter_core::OverviewSnapshot;

#[derive(Debug, Eq, PartialEq)]
pub struct OverviewRequest(u64);

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OverviewState {
    latest_request: u64,
    snapshot: Option<OverviewSnapshot>,
}

impl OverviewState {
    pub fn begin_request(&mut self) -> OverviewRequest {
        self.latest_request = self
            .latest_request
            .checked_add(1)
            .expect("overview request generation overflowed");
        OverviewRequest(self.latest_request)
    }

    pub fn apply_snapshot(&mut self, request: OverviewRequest, snapshot: OverviewSnapshot) -> bool {
        if request.0 != self.latest_request {
            return false;
        }
        self.snapshot = Some(snapshot);
        true
    }

    pub const fn snapshot(&self) -> Option<&OverviewSnapshot> {
        self.snapshot.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use agentmeter_core::{
        OverviewCostSummary, OverviewDataQuality, OverviewSnapshot, SourceHealthSnapshot,
        TokenBreakdown,
    };

    use super::OverviewState;

    #[test]
    fn rejects_an_out_of_order_snapshot() {
        let mut state = OverviewState::default();
        let first = state.begin_request();
        let second = state.begin_request();

        assert!(state.apply_snapshot(second, snapshot(2)));
        assert!(!state.apply_snapshot(first, snapshot(1)));
        assert_eq!(state.snapshot().unwrap().generation, 2);
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
}
