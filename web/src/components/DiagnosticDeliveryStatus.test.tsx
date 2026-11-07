import { render, screen } from '@testing-library/react';
import { expect, it } from 'vitest';
import { DiagnosticDeliveryStatus } from './DiagnosticDeliveryStatus';
it('separates completed diagnostic delivery from feature acceptance and pending assistance', () => {
  render(<DiagnosticDeliveryStatus delivery={{report_path:'reports/review.json',report_digest:'sha256:example',completed:true,feature_accepted:false,release_authorized:false,unverified_decisions:['decision-1']}} />);
  expect(screen.getByRole('status')).toHaveTextContent('Diagnostic report complete');
  expect(screen.getByText('Feature changes requested')).toBeInTheDocument();
  expect(screen.getByText('External verification pending')).toBeInTheDocument();
  expect(screen.getByText('Release not authorized')).toBeInTheDocument();
  expect(screen.queryByText('Feature accepted')).not.toBeInTheDocument();
});
