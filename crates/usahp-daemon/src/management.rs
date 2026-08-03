use serde::Serialize;
use usahp_core::SwitchSnapshot;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CaptureAvailability {
    Available,
    PermissionRequired,
    Unavailable,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CaptureStatus {
    pub active: bool,
    pub availability: CaptureAvailability,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RequestOutcome {
    Accepted,
    Rejected { reason: String },
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ConnectionSnapshot {
    pub client_id: u64,
    pub peer: Option<String>,
    pub app_id: Option<String>,
    pub pid: Option<u32>,
    pub requested_mode: Option<String>,
    pub outcome: Option<RequestOutcome>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ActiveSessionSnapshot {
    pub client_id: u64,
    pub app_id: String,
    pub pid: Option<u32>,
    pub requested_mode: String,
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct BrokerSnapshot {
    pub capture: CaptureStatus,
    pub switches: Vec<SwitchSnapshot>,
    pub connections: Vec<ConnectionSnapshot>,
    pub active_session: Option<ActiveSessionSnapshot>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_status_serializes_as_an_in_process_management_shape() {
        let status = CaptureStatus {
            active: false,
            availability: CaptureAvailability::PermissionRequired,
            message: Some("grant Accessibility".into()),
        };
        assert_eq!(
            serde_json::to_value(status).unwrap(),
            serde_json::json!({
                "active": false,
                "availability": "permission_required",
                "message": "grant Accessibility"
            })
        );
    }
}
