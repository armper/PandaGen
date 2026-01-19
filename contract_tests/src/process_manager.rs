//! Process Manager service contract tests
//!
//! These tests define the stable contract for the ProcessManager service.

use crate::test_helpers::*;
use core_types::ServiceId;
use ipc::SchemaVersion;
use serde::{Deserialize, Serialize};

// ===== ProcessManager Contract Version =====
const PROCESS_MANAGER_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

// ===== Action Identifiers =====
const ACTION_SPAWN: &str = "process_manager.spawn";
const ACTION_TERMINATE: &str = "process_manager.terminate";
const ACTION_GET_STATUS: &str = "process_manager.get_status";
const ACTION_LIST_PROCESSES: &str = "process_manager.list_processes";

// ===== Canonical Payload Structures =====

/// Restart policy (stable contract)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RestartPolicy {
    Never,
    Always,
    OnFailure,
}

/// Lifecycle state (stable contract)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum LifecycleState {
    Starting,
    Running,
    Stopping,
    Stopped,
    Failed,
    Restarting,
}

/// Spawn process request
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SpawnRequest {
    pub service_id: ServiceId,
    pub name: String,
    pub restart_policy: RestartPolicy,
}

/// Spawn process response
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SpawnResponse {
    pub task_id: String, // Simplified for contract test
}

/// Terminate process request
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TerminateRequest {
    pub task_id: String,
}

/// Get status request
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GetStatusRequest {
    pub task_id: String,
}

/// Get status response
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GetStatusResponse {
    pub task_id: String,
    pub state: LifecycleState,
}

/// List processes response
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ListProcessesResponse {
    pub processes: Vec<ProcessInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProcessInfo {
    pub task_id: String,
    pub service_id: ServiceId,
    pub name: String,
    pub state: LifecycleState,
}

// ===== Contract Tests =====

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spawn_contract() {
        let service_id = ServiceId::new();
        let request = SpawnRequest {
            service_id,
            name: "test_service".to_string(),
            restart_policy: RestartPolicy::OnFailure,
        };

        let envelope = create_test_envelope(
            service_id,
            ACTION_SPAWN,
            PROCESS_MANAGER_SCHEMA_VERSION,
            &request,
        );

        verify_envelope_contract(&envelope, ACTION_SPAWN, PROCESS_MANAGER_SCHEMA_VERSION);
        verify_major_version(&envelope, 1);

        let deserialized: SpawnRequest = envelope.payload.deserialize().unwrap();
        assert_eq!(deserialized, request);
    }

    #[test]
    fn test_terminate_contract() {
        let service_id = ServiceId::new();
        let request = TerminateRequest {
            task_id: "task-123".to_string(),
        };

        let envelope = create_test_envelope(
            service_id,
            ACTION_TERMINATE,
            PROCESS_MANAGER_SCHEMA_VERSION,
            &request,
        );

        verify_envelope_contract(&envelope, ACTION_TERMINATE, PROCESS_MANAGER_SCHEMA_VERSION);
        verify_major_version(&envelope, 1);

        let deserialized: TerminateRequest = envelope.payload.deserialize().unwrap();
        assert_eq!(deserialized, request);
    }

    #[test]
    fn test_get_status_contract() {
        let service_id = ServiceId::new();
        let request = GetStatusRequest {
            task_id: "task-123".to_string(),
        };

        let envelope = create_test_envelope(
            service_id,
            ACTION_GET_STATUS,
            PROCESS_MANAGER_SCHEMA_VERSION,
            &request,
        );

        verify_envelope_contract(&envelope, ACTION_GET_STATUS, PROCESS_MANAGER_SCHEMA_VERSION);
        verify_major_version(&envelope, 1);

        let deserialized: GetStatusRequest = envelope.payload.deserialize().unwrap();
        assert_eq!(deserialized, request);
    }

    #[test]
    fn test_list_processes_contract() {
        let service_id = ServiceId::new();
        let response = ListProcessesResponse {
            processes: vec![ProcessInfo {
                task_id: "task-123".to_string(),
                service_id: ServiceId::new(),
                name: "test_service".to_string(),
                state: LifecycleState::Running,
            }],
        };

        let envelope = create_test_envelope(
            service_id,
            ACTION_LIST_PROCESSES,
            PROCESS_MANAGER_SCHEMA_VERSION,
            &response,
        );

        verify_envelope_contract(
            &envelope,
            ACTION_LIST_PROCESSES,
            PROCESS_MANAGER_SCHEMA_VERSION,
        );
        verify_major_version(&envelope, 1);

        let deserialized: ListProcessesResponse = envelope.payload.deserialize().unwrap();
        assert_eq!(deserialized.processes.len(), response.processes.len());
    }

    #[test]
    fn test_restart_policy_enum_is_stable() {
        // These variants MUST NOT CHANGE without version bump
        assert_eq!(
            serde_json::to_string(&RestartPolicy::Never).unwrap(),
            r#""Never""#
        );
        assert_eq!(
            serde_json::to_string(&RestartPolicy::Always).unwrap(),
            r#""Always""#
        );
        assert_eq!(
            serde_json::to_string(&RestartPolicy::OnFailure).unwrap(),
            r#""OnFailure""#
        );
    }

    #[test]
    fn test_lifecycle_state_enum_is_stable() {
        // These variants MUST NOT CHANGE without version bump
        assert_eq!(
            serde_json::to_string(&LifecycleState::Starting).unwrap(),
            r#""Starting""#
        );
        assert_eq!(
            serde_json::to_string(&LifecycleState::Running).unwrap(),
            r#""Running""#
        );
        assert_eq!(
            serde_json::to_string(&LifecycleState::Stopping).unwrap(),
            r#""Stopping""#
        );
        assert_eq!(
            serde_json::to_string(&LifecycleState::Stopped).unwrap(),
            r#""Stopped""#
        );
        assert_eq!(
            serde_json::to_string(&LifecycleState::Failed).unwrap(),
            r#""Failed""#
        );
        assert_eq!(
            serde_json::to_string(&LifecycleState::Restarting).unwrap(),
            r#""Restarting""#
        );
    }

    #[test]
    fn test_action_identifiers_are_stable() {
        assert_eq!(ACTION_SPAWN, "process_manager.spawn");
        assert_eq!(ACTION_TERMINATE, "process_manager.terminate");
        assert_eq!(ACTION_GET_STATUS, "process_manager.get_status");
        assert_eq!(ACTION_LIST_PROCESSES, "process_manager.list_processes");
    }

    #[test]
    fn test_schema_version_is_stable() {
        assert_eq!(PROCESS_MANAGER_SCHEMA_VERSION.major, 1);
        assert_eq!(PROCESS_MANAGER_SCHEMA_VERSION.minor, 0);
    }
}
