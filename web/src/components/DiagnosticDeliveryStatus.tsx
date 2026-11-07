import type { ApiCoordinatorRunSummary } from '../api/models';
type Delivery = NonNullable<ApiCoordinatorRunSummary['diagnostic_delivery']>;
export function DiagnosticDeliveryStatus({ delivery }: { delivery: Delivery }) {
  return <div role="status" style={{ marginTop: 4 }}>
    <span>{delivery.completed ? 'Diagnostic report complete' : 'Diagnostic report incomplete'}</span>{' · '}
    <span>{delivery.feature_accepted ? 'Feature accepted' : 'Feature changes requested'}</span>{' · '}
    <span>{delivery.unverified_decisions.length > 0 ? 'External verification pending' : 'External verification recorded'}</span>{' · '}
    <span>{delivery.release_authorized ? 'Release authorized' : 'Release not authorized'}</span>
    <div>{delivery.report_path}</div>
  </div>;
}
