use chrono::{DateTime, Duration, Utc};

use super::{ClientIdentity, OwnershipRecord, OwnershipStore};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeartbeatConfig {
    pub ttl_seconds: u64,
}

impl Default for HeartbeatConfig {
    fn default() -> Self {
        Self { ttl_seconds: 60 }
    }
}

pub fn evict_stale_clients(store: &mut OwnershipStore, config: &HeartbeatConfig) {
    let cutoff = Utc::now() - Duration::seconds(config.ttl_seconds.min(i64::MAX as u64) as i64);

    for record in &mut store.records {
        record
            .viewers
            .retain(|viewer| !is_stale_client(viewer, cutoff));

        let stale_owner = record
            .owner
            .as_ref()
            .is_some_and(|owner| is_stale_client(owner, cutoff));
        if !stale_owner {
            continue;
        }

        if let Some(owner) = record.owner.take() {
            emit_owner_stale(record, &owner);
        }

        if let Some(next_owner) = promote_oldest_viewer(record) {
            record.owner = Some(next_owner);
        }
    }
}

fn is_stale_client(client: &ClientIdentity, cutoff: DateTime<Utc>) -> bool {
    parse_timestamp(&client.last_heartbeat)
        .map(|heartbeat| heartbeat < cutoff)
        .unwrap_or(true)
}

fn promote_oldest_viewer(record: &mut OwnershipRecord) -> Option<ClientIdentity> {
    let next_index = record
        .viewers
        .iter()
        .enumerate()
        .min_by_key(|(_, viewer)| {
            parse_timestamp(&viewer.connected_at).unwrap_or(DateTime::<Utc>::MAX_UTC)
        })
        .map(|(index, _)| index)?;

    Some(record.viewers.remove(next_index))
}

fn parse_timestamp(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|ts| ts.with_timezone(&Utc))
}

pub(crate) fn emit_owner_stale(_record: &OwnershipRecord, _owner: &ClientIdentity) {
    #[cfg(test)]
    test_support::record_owner_stale(_record, _owner);
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::cell::RefCell;

    use super::{ClientIdentity, OwnershipRecord};

    thread_local! {
        static EVENTS: RefCell<Vec<(String, String)>> = const { RefCell::new(Vec::new()) };
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
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, Utc};
    use std::path::PathBuf;

    use super::{HeartbeatConfig, evict_stale_clients, test_support::take_owner_stale_events};
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
