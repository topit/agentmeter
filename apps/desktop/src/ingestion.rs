use std::sync::Arc;

use agentmeter_app::{IngestionSummary, LocalDataErrorKind, ScanProgress};

#[derive(Debug, Eq, PartialEq)]
pub struct IngestionRequest(u64);

/// Presentation state for collection runs. Scans execute through the
/// application service off the render path; out-of-order completions are
/// rejected so an older scan can never mark a newer one finished.
#[derive(Clone, Debug, Default)]
pub struct IngestionUiState {
    latest_request: u64,
    running: bool,
    cancelled: bool,
    progress: Arc<ScanProgress>,
    last_summary: Option<IngestionSummary>,
    error: Option<LocalDataErrorKind>,
}

impl IngestionUiState {
    pub fn begin_scan(&mut self) -> (IngestionRequest, Arc<ScanProgress>) {
        self.latest_request = self
            .latest_request
            .checked_add(1)
            .expect("ingestion request generation overflowed");
        self.running = true;
        self.cancelled = false;
        self.error = None;
        let progress = ScanProgress::shared();
        self.progress = progress.clone();
        (IngestionRequest(self.latest_request), progress)
    }

    pub fn apply_scan_result(
        &mut self,
        request: IngestionRequest,
        result: Result<IngestionSummary, LocalDataErrorKind>,
    ) -> bool {
        if request.0 != self.latest_request {
            return false;
        }
        self.running = false;
        match result {
            Ok(summary) => {
                self.cancelled = summary.cancelled;
                self.last_summary = Some(summary);
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
        true
    }

    pub const fn running(&self) -> bool {
        self.running
    }

    pub const fn cancelled(&self) -> bool {
        self.cancelled
    }

    /// Live (processed, discovered) counts for the running scan.
    pub fn progress(&self) -> (u64, u64) {
        (self.progress.processed(), self.progress.discovered())
    }

    pub const fn error(&self) -> Option<LocalDataErrorKind> {
        self.error
    }

    pub const fn last_summary(&self) -> Option<&IngestionSummary> {
        self.last_summary.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use agentmeter_app::{AdapterRunSummary, IngestionSummary, LocalDataErrorKind};

    use super::IngestionUiState;

    fn summary() -> IngestionSummary {
        IngestionSummary {
            runs: vec![AdapterRunSummary {
                adapter_id: "kimi-wire".into(),
                discovered_sources: 1,
                ingested_sources: 1,
                failed_sources: 0,
                reconciled_sources: 0,
                discovery_error: None,
            }],
            cancelled: false,
        }
    }

    #[test]
    fn rejects_an_out_of_order_scan_result() {
        let mut state = IngestionUiState::default();
        let (stale, _) = state.begin_scan();
        let (current, _) = state.begin_scan();

        assert!(!state.apply_scan_result(stale, Ok(summary())));
        assert!(state.apply_scan_result(current, Err(LocalDataErrorKind::Database)));
        assert!(state.error().is_some());
        assert_eq!(state.last_summary(), None);
        assert!(!state.running());
    }

    #[test]
    fn records_the_latest_summary_and_clears_stale_errors() {
        let mut state = IngestionUiState::default();

        let (failed, _) = state.begin_scan();
        assert!(state.running());
        assert!(state.apply_scan_result(failed, Err(LocalDataErrorKind::Database)));

        let (succeeded, _) = state.begin_scan();
        assert_eq!(state.error(), None, "a new scan clears the stale error");
        assert!(state.apply_scan_result(succeeded, Ok(summary())));

        assert!(!state.running());
        assert!(state.error().is_none());
        assert_eq!(
            state
                .last_summary()
                .and_then(|summary| summary.runs.first())
                .map(|run| run.adapter_id.as_str()),
            Some("kimi-wire")
        );
    }
}
