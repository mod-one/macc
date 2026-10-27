use chrono::{DateTime, Duration, Utc};

use super::{ClientIdentity, OwnershipRecord, OwnershipStore};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TakeoverDefaultResponse {
    Deny,
    AutoAccept,
    AdminTakeover,
}

impl TakeoverDefaultResponse {
    pub fn from_str_lossy(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto_accept" | "auto-accept" | "accept" => Self::AutoAccept,
            "admin_takeover" | "admin-takeover" | "admin" => Self::AdminTakeover,
            _ => Self::Deny,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeartbeatConfig {
    pub ttl_seconds: u64,
    /// Seconds before a pending takeover request is auto-resolved.
    /// `0` disables timeout resolution.
    pub takeover_timeout_seconds: u64,
    /// Policy applied when a pending takeover times out.
    pub takeover_default_response: TakeoverDefaultResponse,
}

impl Default for HeartbeatConfig {
    fn default() -> Self {
        Self {
            // 15s: short enough that a killed client clears quickly, long enough
            // to survive a slow tick cycle without false eviction.
            ttl_seconds: 15,
            takeover_timeout_seconds: 60,
            takeover_default_response: TakeoverDefaultResponse::Deny,
        }
    }
}

/// Grace window (in milliseconds) for the current owner to automatically
/// respond with `heartbeat` after a takeover request is emitted. If the owner
/// does not answer within this window, it is considered absent and loses
/// ownership immediately.
pub const OWNER_HEARTBEAT_VERIFY_MS: i64 = 1500;

pub fn owner_responded_to_takeover(
    owner: &ClientIdentity,
    request: &super::TakeoverRequest,
) -> bool {
    match (
        parse_timestamp(&owner.last_heartbeat),
        parse_timestamp(&request.requested_at),
    ) {
        (Some(hb), Some(req)) => hb > req,
        _ => false,
    }
}

pub fn evict_stale_clients(store: &mut OwnershipStore, config: &HeartbeatConfig) {
    let now = Utc::now();
    let cutoff = now - Duration::seconds(config.ttl_seconds.min(i64::MAX as u64) as i64);
    let verify_cutoff = now - Duration::milliseconds(OWNER_HEARTBEAT_VERIFY_MS);

    for record in &mut store.records {
        // 1. Evict viewers that are stale or belong to a verified dead local PID.
        record
            .viewers
            .retain(|viewer| !is_dead_local_client(viewer) && !is_stale_client(viewer, cutoff));

        // 2. If the current owner is a verified dead local PID, evict it in 0ms.
        if record
            .owner
            .as_ref()
            .is_some_and(is_dead_local_client)
        {
            if let Some(dead_owner) = record.owner.take() {
                emit_owner_stale(record, &dead_owner);
            }
            if let Some(req) = record.takeover_request.take() {
                record
                    .viewers
                    .retain(|v| v.client_id != req.requester.client_id);
                record.owner = Some(req.requester);
            } else if let Some(next_owner) = promote_oldest_viewer(record) {
                record.owner = Some(next_owner);
            }
        }

        // 3. Check pending takeover requests:
        //    - First, verify whether the current owner responded automatically via
        //      `heartbeat` (`owner.last_heartbeat > req.requested_at`). If the
        //      owner failed to respond within `OWNER_HEARTBEAT_VERIFY_MS`, the
        //      owner is considered absent and loses ownership immediately.
        //    - Otherwise, if the owner did respond with a heartbeat, apply the
        //      configured `takeover_timeout_seconds` policy for user decision.
        if let Some(req) = record.takeover_request.clone() {
            let requested_at = parse_timestamp(&req.requested_at);
            let owner_responded = record
                .owner
                .as_ref()
                .is_some_and(|owner| owner_responded_to_takeover(owner, &req));

            if !owner_responded {
                let verify_expired = requested_at.map(|ts| ts < verify_cutoff).unwrap_or(true);
                if verify_expired {
                    if let Some(absent_owner) = record.owner.take() {
                        emit_owner_stale(record, &absent_owner);
                    }
                    let requester = req.requester.clone();
                    record
                        .viewers
                        .retain(|v| v.client_id != requester.client_id);
                    record.owner = Some(requester);
                    record.takeover_request = None;
                }
            } else if config.takeover_timeout_seconds > 0 {
                let takeover_cutoff = now
                    - Duration::seconds(
                        config.takeover_timeout_seconds.min(i64::MAX as u64) as i64,
                    );
                let expired = requested_at.map(|ts| ts < takeover_cutoff).unwrap_or(true);
                if expired {
                    match config.takeover_default_response {
                        TakeoverDefaultResponse::Deny => {
                            emit_takeover_timeout(record, &req, "deny");
                            record.takeover_request = None;
                        }
                        TakeoverDefaultResponse::AutoAccept
                        | TakeoverDefaultResponse::AdminTakeover => {
                            let policy = if matches!(
                                config.takeover_default_response,
                                TakeoverDefaultResponse::AutoAccept
                            ) {
                                "auto_accept"
                            } else {
                                "admin_takeover"
                            };
                            emit_takeover_timeout(record, &req, policy);
                            let requester = req.requester.clone();
                            record
                                .viewers
                                .retain(|v| v.client_id != requester.client_id);
                            record.owner = Some(requester);
                            record.takeover_request = None;
                        }
                    }
                }
            }
        }

        // 4. Evict stale owner (TTL exceeded) and promote highest-priority oldest viewer.
        let stale_owner = record
            .owner
            .as_ref()
            .is_some_and(|owner| is_stale_client(owner, cutoff));
        if stale_owner {
            if let Some(owner) = record.owner.take() {
                emit_owner_stale(record, &owner);
            }
            if let Some(next_owner) = promote_oldest_viewer(record) {
                record.owner = Some(next_owner);
            }
        } else if record
            .owner
            .as_ref()
            .is_some_and(|owner| owner.kind == super::ClientKind::Cli)
            && record
                .viewers
                .iter()
                .any(|viewer| viewer.kind.is_interactive_ui())
        {
            // 5. TUI and Web clients have priority over CLI clients.
            if let Some(interactive_owner) = promote_oldest_viewer(record) {
                if let Some(displaced_cli) = record.owner.replace(interactive_owner) {
                    if !record
                        .viewers
                        .iter()
                        .any(|v| v.client_id == displaced_cli.client_id)
                    {
                        record.viewers.push(displaced_cli);
                    }
                }
            }
        }
    }
}

fn is_dead_local_client(client: &ClientIdentity) -> bool {
    if let Some(rest) = client.client_id.strip_prefix("tui-pid-") {
        if let Some(pid_str) = rest.split('-').next() {
            if let Ok(pid) = pid_str.parse::<i64>() {
                return !crate::coordinator::helpers::is_pid_running(pid);
            }
        }
    }
    false
}

fn is_stale_client(client: &ClientIdentity, cutoff: DateTime<Utc>) -> bool {
    parse_timestamp(&client.last_heartbeat)
        .map(|heartbeat| heartbeat < cutoff)
        .unwrap_or(true)
}

pub(crate) fn promote_oldest_viewer(record: &mut OwnershipRecord) -> Option<ClientIdentity> {
    let next_index = record
        .viewers
        .iter()
        .enumerate()
        .min_by_key(|(_, viewer)| {
            let tier = if viewer.kind.is_interactive_ui() {
                0u8
            } else {
                1u8
            };
            let connected =
                parse_timestamp(&viewer.connected_at).unwrap_or(DateTime::<Utc>::MAX_UTC);
            (tier, connected)
        })
        .map(|(index, _)| index)?;

    Some(record.viewers.remove(next_index))
}

fn parse_timestamp(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|ts| ts.with_timezone(&Utc))
}

pub(crate) fn emit_owner_stale(record: &OwnershipRecord, owner: &ClientIdentity) {
    if record.process.project_root.exists() {
        super::events::emit_owner_stale(&record.process.project_root, &record.process, owner);
    }
    #[cfg(test)]
    test_support::record_owner_stale(record, owner);
}

pub(crate) fn emit_takeover_timeout(
    record: &OwnershipRecord,
    request: &super::TakeoverRequest,
    policy: &str,
) {
    super::events::emit_takeover_timeout(
        &record.process.project_root,
        &record.process,
        &request.requester,
        &request.request_id,
        policy,
    );
    #[cfg(test)]
    test_support::record_takeover_timeout(record, request, policy);
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::cell::RefCell;

    use super::{ClientIdentity, OwnershipRecord};
    use crate::process_ownership::TakeoverRequest;

    thread_local! {
        static EVENTS: RefCell<Vec<(String, String)>> = const { RefCell::new(Vec::new()) };
        static TAKEOVER_EVENTS: RefCell<Vec<(String, String, String)>> = const { RefCell::new(Vec::new()) };
    }

    pub fn record_owner_stale(record: &OwnershipRecord, owner: &ClientIdentity) {
        EVENTS.with(|events| {
            events.borrow_mut().push((
                record.process.project_root.to_string_lossy().into_owned(),
                owner.client_id.clone(),
            ));
        });
    }

    pub fn take_owner_stale_events() -> Vec<(String, String)> {
        EVENTS.with(|events| std::mem::take(&mut *events.borrow_mut()))
    }

    pub fn record_takeover_timeout(
        record: &OwnershipRecord,
        request: &TakeoverRequest,
        policy: &str,
    ) {
        TAKEOVER_EVENTS.with(|events| {
            events.borrow_mut().push((
                record.process.project_root.to_string_lossy().into_owned(),
                request.requester.client_id.clone(),
                policy.to_string(),
            ));
        });
    }

    pub fn take_takeover_timeout_events() -> Vec<(String, String, String)> {
        TAKEOVER_EVENTS.with(|events| std::mem::take(&mut *events.borrow_mut()))
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, Utc};
    use std::path::PathBuf;

    use super::{evict_stale_clients, test_support::take_owner_stale_events, HeartbeatConfig};
    use crate::process_ownership::{
        ClientIdentity, ClientKind, OwnershipRecord, OwnershipStore, ProcessHandle, ProcessKind,
    };

    #[test]
    fn all_fresh_entries_are_kept() {
        let mut store = OwnershipStore::default();
        let owner = client("owner", 50, 10);
        let viewer = client("viewer", 40, 5);
        store.upsert_record(record(Some(owner.clone()), vec![viewer.clone()]));

        evict_stale_clients(&mut store, &HeartbeatConfig::default());

        let record = &store.records[0];
        assert_eq!(record.owner.as_ref(), Some(&owner));
        assert_eq!(record.viewers, vec![viewer]);
        assert!(take_owner_stale_events().is_empty());
    }

    #[test]
    fn stale_viewer_is_removed() {
        let mut store = OwnershipStore::default();
        let fresh_viewer = client("viewer-fresh", 30, 10);
        store.upsert_record(record(
            Some(client("owner", 50, 10)),
            vec![client("viewer-stale", 120, 120), fresh_viewer.clone()],
        ));

        evict_stale_clients(&mut store, &HeartbeatConfig::default());

        assert_eq!(store.records[0].viewers, vec![fresh_viewer]);
        assert!(take_owner_stale_events().is_empty());
    }

    #[test]
    fn stale_owner_promotes_oldest_non_stale_viewer() {
        let mut store = OwnershipStore::default();
        let oldest_viewer = client("viewer-oldest", 55, 5);
        let newer_viewer = client("viewer-newer", 20, 5);
        store.upsert_record(record(
            Some(client("owner-stale", 120, 120)),
            vec![newer_viewer.clone(), oldest_viewer.clone()],
        ));

        evict_stale_clients(&mut store, &HeartbeatConfig::default());

        let record = &store.records[0];
        assert_eq!(record.owner.as_ref(), Some(&oldest_viewer));
        assert_eq!(record.viewers, vec![newer_viewer]);
        assert_eq!(
            take_owner_stale_events(),
            vec![("/tmp/project/repo".to_string(), "owner-stale".to_string())]
        );
    }

    #[test]
    fn stale_owner_without_viewers_clears_owner() {
        let mut store = OwnershipStore::default();
        store.upsert_record(record(Some(client("owner-stale", 120, 120)), Vec::new()));

        evict_stale_clients(&mut store, &HeartbeatConfig::default());

        let record = &store.records[0];
        assert!(record.owner.is_none());
        assert!(record.viewers.is_empty());
        assert_eq!(
            take_owner_stale_events(),
            vec![("/tmp/project/repo".to_string(), "owner-stale".to_string())]
        );
    }

    fn record(owner: Option<ClientIdentity>, viewers: Vec<ClientIdentity>) -> OwnershipRecord {
        OwnershipRecord {
            process: ProcessHandle {
                kind: ProcessKind::Coordinator,
                project_root: PathBuf::from("/tmp/project/repo"),
                pid: Some(1000),
            },
            owner,
            viewers,
            takeover_request: None,
            started_at: Utc::now().to_rfc3339(),
        }
    }

    #[test]
    fn takeover_timeout_deny_policy_clears_pending_request() {
        use super::TakeoverDefaultResponse;
        use crate::process_ownership::TakeoverRequest;

        let mut store = OwnershipStore::default();
        let mut rec = record(Some(client("owner", 5, 5)), vec![]);
        rec.takeover_request = Some(TakeoverRequest {
            request_id: "req-1".into(),
            requester: client("requester", 200, 200),
            requested_at: (Utc::now() - Duration::seconds(200)).to_rfc3339(),
        });
        store.upsert_record(rec);

        let cfg = HeartbeatConfig {
            ttl_seconds: 60,
            takeover_timeout_seconds: 30,
            takeover_default_response: TakeoverDefaultResponse::Deny,
        };
        evict_stale_clients(&mut store, &cfg);

        assert!(store.records[0].takeover_request.is_none());
        assert_eq!(
            store.records[0]
                .owner
                .as_ref()
                .map(|o| o.client_id.as_str()),
            Some("owner")
        );
        let events = super::test_support::take_takeover_timeout_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].2, "deny");
    }

    #[test]
    fn takeover_timeout_auto_accept_transfers_ownership() {
        use super::TakeoverDefaultResponse;
        use crate::process_ownership::TakeoverRequest;

        let mut store = OwnershipStore::default();
        let mut rec = record(Some(client("owner", 5, 5)), vec![]);
        // Requester heartbeat is fresh so it is not evicted as stale after promotion.
        rec.takeover_request = Some(TakeoverRequest {
            request_id: "req-1".into(),
            requester: client("requester", 200, 5),
            requested_at: (Utc::now() - Duration::seconds(200)).to_rfc3339(),
        });
        store.upsert_record(rec);

        let cfg = HeartbeatConfig {
            ttl_seconds: 60,
            takeover_timeout_seconds: 30,
            takeover_default_response: TakeoverDefaultResponse::AutoAccept,
        };
        evict_stale_clients(&mut store, &cfg);

        assert!(store.records[0].takeover_request.is_none());
        assert_eq!(
            store.records[0]
                .owner
                .as_ref()
                .map(|o| o.client_id.as_str()),
            Some("requester")
        );
        let events = super::test_support::take_takeover_timeout_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].2, "auto_accept");
    }

    fn client(
        client_id: &str,
        connected_age_seconds: i64,
        heartbeat_age_seconds: i64,
    ) -> ClientIdentity {
        ClientIdentity {
            client_id: client_id.to_string(),
            kind: ClientKind::Tui,
            connected_at: (Utc::now() - Duration::seconds(connected_age_seconds)).to_rfc3339(),
            last_heartbeat: (Utc::now() - Duration::seconds(heartbeat_age_seconds)).to_rfc3339(),
        }
    }
}
