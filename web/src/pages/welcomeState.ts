import type { ApiCoordinatorStatus } from '../api/models';

export function shouldShowDashboard(status: ApiCoordinatorStatus | null): boolean {
  if (!status) {
    return false;
  }

  return (
    status.total > 0 ||
    status.todo > 0 ||
    status.active > 0 ||
    status.blocked > 0 ||
    status.merged > 0 ||
    status.paused ||
    Boolean(status.latest_error) ||
    Boolean(status.failure_report)
  );
}
