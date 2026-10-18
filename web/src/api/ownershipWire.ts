import type { ApiClientIdentity, ApiOwnershipRecord, ApiProcessKind } from './models';

// The HTTP API uses flat camelCase DTOs; UI stores use nested domain records.
type Identity = { clientId: string; kind: ApiClientIdentity['kind']; connectedAt: string; lastHeartbeat: string };
export type OwnershipWire = {
  kind: ApiProcessKind; projectRoot: string; pid: number | null; startedAt: string;
  owner: Identity | null; viewers: Identity[];
  takeoverRequest: { requestId: string; requester: Identity; requestedAt: string } | null;
};
const identity = (value: Identity): ApiClientIdentity => ({
  client_id: value.clientId, kind: value.kind,
  connected_at: value.connectedAt, last_heartbeat: value.lastHeartbeat,
});
export function ownershipRecord(value: OwnershipWire | ApiOwnershipRecord): ApiOwnershipRecord {
  // Retain compatibility with existing domain-shaped transports and fixtures.
  if ('process' in value) return value;
  return {
    process: { kind: value.kind, project_root: value.projectRoot, pid: value.pid },
    started_at: value.startedAt, owner: value.owner ? identity(value.owner) : null,
    viewers: value.viewers.map(identity),
    takeover_request: value.takeoverRequest ? {
      request_id: value.takeoverRequest.requestId,
      requester: identity(value.takeoverRequest.requester), requested_at: value.takeoverRequest.requestedAt,
    } : null,
  };
}
