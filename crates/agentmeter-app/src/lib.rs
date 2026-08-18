//! Application services that coordinate portable AgentMeter core crates.

use std::path::{Path, PathBuf};

use agentmeter_core::{OverviewSnapshot, SourceHealthSnapshot};
use agentmeter_storage::{Database, StorageError};

/// Why a local data service could not produce a snapshot. The kinds are
/// presentation-safe: they never embed paths or raw database errors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalDataErrorKind {
    DataDirectory,
    Database,
}

/// Immutable headline facts for the Overview screen.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OverviewService {
    database_path: PathBuf,
}

impl OverviewService {
    pub fn in_data_directory(data_directory: impl AsRef<Path>) -> Self {
        Self {
            database_path: local_database_path(data_directory.as_ref()),
        }
    }

    pub fn load(&self) -> Result<OverviewSnapshot, LocalDataServiceError> {
        open_local_database(&self.database_path)?
            .overview_snapshot()
            .map_err(LocalDataServiceError::Database)
    }
}

/// Immutable per-source collection health for the Sources screen.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourcesService {
    database_path: PathBuf,
}

impl SourcesService {
    pub fn in_data_directory(data_directory: impl AsRef<Path>) -> Self {
        Self {
            database_path: local_database_path(data_directory.as_ref()),
        }
    }

    pub fn load(&self) -> Result<SourceHealthSnapshot, LocalDataServiceError> {
        open_local_database(&self.database_path)?
            .source_health_snapshot()
            .map_err(LocalDataServiceError::Database)
    }
}

#[derive(Debug)]
pub enum LocalDataServiceError {
    DataDirectory(std::io::Error),
    Database(StorageError),
}

impl LocalDataServiceError {
    pub const fn kind(&self) -> LocalDataErrorKind {
        match self {
            Self::DataDirectory(_) => LocalDataErrorKind::DataDirectory,
            Self::Database(_) => LocalDataErrorKind::Database,
        }
    }
}

fn local_database_path(data_directory: &Path) -> PathBuf {
    data_directory.join("AgentMeter").join("agentmeter.db")
}

fn open_local_database(database_path: &Path) -> Result<Database, LocalDataServiceError> {
    let parent = database_path
        .parent()
        .expect("AgentMeter database path must have a parent directory");
    std::fs::create_dir_all(parent).map_err(LocalDataServiceError::DataDirectory)?;
    Database::open(database_path).map_err(LocalDataServiceError::Database)
}

#[cfg(test)]
mod tests {
    use agentmeter_core::{SourceHealthState, SourcePermissionState, SourceRemediation};
    use agentmeter_storage::{Database, SourceInstallationRegistration, SourceRegistration};
    use tempfile::tempdir;

    use super::{LocalDataErrorKind, OverviewService, SourcesService};

    #[test]
    fn creates_and_loads_the_local_database_for_overview() {
        let directory = tempdir().unwrap();
        let service = OverviewService::in_data_directory(directory.path());

        let snapshot = service.load().unwrap();

        assert_eq!(snapshot.event_count, 0);
        assert!(directory.path().join("AgentMeter/agentmeter.db").is_file());
    }

    #[test]
    fn loads_an_empty_sources_snapshot_with_a_stable_generation() {
        let directory = tempdir().unwrap();
        let service = SourcesService::in_data_directory(directory.path());

        let first = service.load().unwrap();
        let second = service.load().unwrap();

        assert!(first.sources.is_empty());
        assert_eq!(first, second, "unchanged data must not change the snapshot");
    }

    #[test]
    fn loads_registered_source_health_from_the_local_database() {
        let directory = tempdir().unwrap();
        let database_path = directory.path().join("AgentMeter/agentmeter.db");
        std::fs::create_dir_all(database_path.parent().unwrap()).unwrap();
        let mut database = Database::open(&database_path).unwrap();
        database
            .register_installation(&SourceInstallationRegistration {
                installation_id: "installation-synthetic".into(),
                adapter_id: "amp".into(),
                platform: "macos".into(),
                root_path: "/fixture/home/.config/amp".into(),
                discovery_method: "default".into(),
                enabled: true,
                permission: SourcePermissionState::Granted,
            })
            .unwrap();
        database
            .register_source(&SourceRegistration {
                installation_id: "installation-synthetic".into(),
                source_object_id: "amp-stream-synthetic".into(),
                adapter_id: "amp".into(),
                platform: "macos".into(),
                root_path: "/fixture/home/.config/amp".into(),
                discovery_method: "default".into(),
                native_path: "/fixture/home/.config/amp/agent.jsonl".into(),
                kind: "jsonl".into(),
            })
            .unwrap();

        let snapshot = SourcesService::in_data_directory(directory.path())
            .load()
            .unwrap();

        assert_eq!(snapshot.sources.len(), 1);
        let source = &snapshot.sources[0];
        assert_eq!(source.adapter_id, "amp");
        assert_eq!(
            source.native_path.as_deref(),
            Some("/fixture/home/.config/amp/agent.jsonl")
        );
        assert_eq!(source.state, SourceHealthState::SetupRequired);
        assert_eq!(source.remediation, Some(SourceRemediation::RetryCollection));
    }

    #[test]
    fn classifies_an_unavailable_data_directory_without_exposing_its_path() {
        let directory = tempdir().unwrap();
        std::fs::write(directory.path().join("AgentMeter"), b"not a directory").unwrap();

        let error = OverviewService::in_data_directory(directory.path())
            .load()
            .unwrap_err();

        assert_eq!(error.kind(), LocalDataErrorKind::DataDirectory);
    }

    #[test]
    fn classifies_sources_data_directory_and_database_failures() {
        let directory = tempdir().unwrap();
        std::fs::write(directory.path().join("AgentMeter"), b"not a directory").unwrap();
        let error = SourcesService::in_data_directory(directory.path())
            .load()
            .unwrap_err();
        assert_eq!(error.kind(), LocalDataErrorKind::DataDirectory);

        let directory = tempdir().unwrap();
        std::fs::create_dir_all(directory.path().join("AgentMeter")).unwrap();
        std::fs::write(
            directory.path().join("AgentMeter/agentmeter.db"),
            b"not a sqlite database",
        )
        .unwrap();
        let error = SourcesService::in_data_directory(directory.path())
            .load()
            .unwrap_err();
        assert_eq!(error.kind(), LocalDataErrorKind::Database);
    }
}
