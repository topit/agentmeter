use agentmeter_app::{
    ActivityDimension, ActivityGranularity, ActivitySnapshot, LocalDataErrorKind,
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ActivityMetric {
    #[default]
    Tokens,
    Cost,
}

#[derive(Debug, Eq, PartialEq)]
pub struct ActivityRequest(u64);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ActivityLoadState {
    #[default]
    Loading,
    Empty,
    Populated,
    Error(LocalDataErrorKind),
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ActivityState {
    latest_request: u64,
    load_state: ActivityLoadState,
    granularity: ActivityGranularity,
    dimension: ActivityDimension,
    metric: ActivityMetric,
    snapshot: Option<ActivitySnapshot>,
}

impl ActivityState {
    pub fn begin_request(
        &mut self,
        granularity: ActivityGranularity,
        dimension: ActivityDimension,
    ) -> ActivityRequest {
        self.latest_request = self
            .latest_request
            .checked_add(1)
            .expect("activity request generation overflowed");
        self.granularity = granularity;
        self.dimension = dimension;
        self.load_state = ActivityLoadState::Loading;
        ActivityRequest(self.latest_request)
    }

    pub fn apply_snapshot(&mut self, request: ActivityRequest, snapshot: ActivitySnapshot) -> bool {
        if request.0 != self.latest_request {
            return false;
        }
        self.load_state = if snapshot.points.is_empty() {
            ActivityLoadState::Empty
        } else {
            ActivityLoadState::Populated
        };
        self.snapshot = Some(snapshot);
        true
    }

    pub fn apply_error(&mut self, request: ActivityRequest, error: LocalDataErrorKind) -> bool {
        if request.0 != self.latest_request {
            return false;
        }
        self.load_state = ActivityLoadState::Error(error);
        true
    }

    pub fn set_metric(&mut self, metric: ActivityMetric) {
        self.metric = metric;
    }

    pub const fn load_state(&self) -> ActivityLoadState {
        self.load_state
    }

    pub const fn granularity(&self) -> ActivityGranularity {
        self.granularity
    }

    pub const fn dimension(&self) -> ActivityDimension {
        self.dimension
    }

    pub const fn metric(&self) -> ActivityMetric {
        self.metric
    }

    pub const fn snapshot(&self) -> Option<&ActivitySnapshot> {
        self.snapshot.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use agentmeter_app::{ActivityDimension, ActivityGranularity, ActivitySnapshot};

    use super::{ActivityLoadState, ActivityMetric, ActivityState};

    #[test]
    fn rejects_stale_results_and_keeps_the_latest_query_selection() {
        let mut state = ActivityState::default();
        let first = state.begin_request(ActivityGranularity::Daily, ActivityDimension::Client);
        let second = state.begin_request(ActivityGranularity::Weekly, ActivityDimension::Model);

        assert!(state.apply_snapshot(second, snapshot(2, ActivityGranularity::Weekly)));
        assert!(!state.apply_snapshot(first, snapshot(1, ActivityGranularity::Daily)));
        assert_eq!(state.snapshot().unwrap().generation, 2);
        assert_eq!(state.granularity(), ActivityGranularity::Weekly);
        assert_eq!(state.dimension(), ActivityDimension::Model);
        assert_eq!(state.load_state(), ActivityLoadState::Empty);
    }

    #[test]
    fn metric_toggle_does_not_reload_the_snapshot() {
        let mut state = ActivityState::default();
        let request = state.begin_request(ActivityGranularity::Daily, ActivityDimension::Provider);
        assert!(state.apply_snapshot(request, snapshot(7, ActivityGranularity::Daily)));

        state.set_metric(ActivityMetric::Cost);

        assert_eq!(state.metric(), ActivityMetric::Cost);
        assert_eq!(state.snapshot().unwrap().generation, 7);
    }

    fn snapshot(generation: u64, granularity: ActivityGranularity) -> ActivitySnapshot {
        ActivitySnapshot {
            generation,
            granularity,
            dimension: ActivityDimension::Client,
            points: Vec::new(),
        }
    }
}
