//! Application services that coordinate portable AgentMeter core crates.

use std::path::{Path, PathBuf};

use agentmeter_core::OverviewSnapshot;
use agentmeter_storage::{Database, StorageError};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OverviewLoadErrorKind {
    DataDirectory,
    Database,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OverviewService {
    database_path: PathBuf,
}

impl OverviewService {
    pub fn in_data_directory(data_directory: impl AsRef<Path>) -> Self {
        Self {
            database_path: data_directory
                .as_ref()
                .join("AgentMeter")
                .join("agentmeter.db"),
        }
    }

    pub fn load(&self) -> Result<OverviewSnapshot, OverviewServiceError> {
        let parent = self
            .database_path
            .parent()
            .expect("AgentMeter database path must have a parent directory");
        std::fs::create_dir_all(parent).map_err(OverviewServiceError::DataDirectory)?;
        Database::open(&self.database_path)
            .and_then(|database| database.overview_snapshot())
            .map_err(OverviewServiceError::Database)
    }
}

#[derive(Debug)]
pub enum OverviewServiceError {
    DataDirectory(std::io::Error),
    Database(StorageError),
}

impl OverviewServiceError {
    pub const fn kind(&self) -> OverviewLoadErrorKind {
        match self {
            Self::DataDirectory(_) => OverviewLoadErrorKind::DataDirectory,
            Self::Database(_) => OverviewLoadErrorKind::Database,
        }
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::{OverviewLoadErrorKind, OverviewService};

    #[test]
    fn creates_and_loads_the_local_database() {
        let directory = tempdir().unwrap();
        let service = OverviewService::in_data_directory(directory.path());

        let snapshot = service.load().unwrap();

        assert_eq!(snapshot.event_count, 0);
        assert!(directory.path().join("AgentMeter/agentmeter.db").is_file());
    }

    #[test]
    fn classifies_an_unavailable_data_directory_without_exposing_its_path() {
        let directory = tempdir().unwrap();
        std::fs::write(directory.path().join("AgentMeter"), b"not a directory").unwrap();

        let error = OverviewService::in_data_directory(directory.path())
            .load()
            .unwrap_err();

        assert_eq!(error.kind(), OverviewLoadErrorKind::DataDirectory);
    }
}
