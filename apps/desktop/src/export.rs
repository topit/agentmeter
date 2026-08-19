use agentmeter_app::{ExportSummary, LocalDataErrorKind};

#[derive(Debug, Eq, PartialEq)]
pub struct ExportRequest(u64);

/// Presentation state for the Settings export section. Exports are explicit
/// user actions that run through the application service off the render
/// path; single-use request generations reject out-of-order completions so a
/// slow older export can never overwrite a newer result.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ExportState {
    latest_request: u64,
    running: bool,
    summary: Option<ExportSummary>,
    error: Option<LocalDataErrorKind>,
}

impl ExportState {
    pub fn begin_export(&mut self) -> ExportRequest {
        self.latest_request = self
            .latest_request
            .checked_add(1)
            .expect("export request generation overflowed");
        self.running = true;
        self.error = None;
        ExportRequest(self.latest_request)
    }

    pub fn apply_result(
        &mut self,
        request: ExportRequest,
        result: Result<ExportSummary, LocalDataErrorKind>,
    ) -> bool {
        if request.0 != self.latest_request {
            return false;
        }
        self.running = false;
        match result {
            Ok(summary) => {
                self.summary = Some(summary);
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
        true
    }

    pub const fn running(&self) -> bool {
        self.running
    }

    pub const fn summary(&self) -> Option<&ExportSummary> {
        self.summary.as_ref()
    }

    pub const fn error(&self) -> Option<LocalDataErrorKind> {
        self.error
    }
}

#[cfg(test)]
mod tests {
    use agentmeter_app::{ExportFormat, ExportSummary, LocalDataErrorKind};

    use super::ExportState;

    fn summary(format: ExportFormat) -> ExportSummary {
        ExportSummary {
            format,
            file_name: "agentmeter-events-synthetic".into(),
            event_count: 7,
        }
    }

    #[test]
    fn rejects_an_out_of_order_export_result() {
        let mut state = ExportState::default();
        let stale = state.begin_export();
        let current = state.begin_export();

        assert!(!state.apply_result(stale, Ok(summary(ExportFormat::Csv))));
        assert!(state.apply_result(current, Ok(summary(ExportFormat::Json))));
        assert_eq!(state.summary().unwrap().format, ExportFormat::Json);
        assert!(!state.running());
    }

    #[test]
    fn records_success_and_failure_with_fresh_requests() {
        let mut state = ExportState::default();

        let failed = state.begin_export();
        assert!(state.running());
        assert!(state.apply_result(failed, Err(LocalDataErrorKind::Database)));
        assert_eq!(state.error(), Some(LocalDataErrorKind::Database));
        assert_eq!(state.summary(), None);

        let succeeded = state.begin_export();
        assert_eq!(state.error(), None, "a new export clears the stale error");
        assert!(state.apply_result(succeeded, Ok(summary(ExportFormat::Json))));
        assert_eq!(state.error(), None);
        assert_eq!(state.summary().unwrap().event_count, 7);
        assert_eq!(
            state.summary().unwrap().file_name,
            "agentmeter-events-synthetic"
        );
    }
}
