type ToolState = {
  tool_id: string; state: string; configured: boolean; connection_verified: boolean;
  available_in_run: boolean; connected_in_run: boolean; consulted: boolean;
  consultations_observed: number; diagnostic: string;
};

export function accessStates(value: unknown): ToolState[] | null {
  if (!value || typeof value !== 'object' || !('tool_states' in value) || !Array.isArray(value.tool_states)) return null;
  return value.tool_states as ToolState[];
}

export default function McpAccessStatus({ rows }: { rows: ToolState[] }) {
  const state = (enabled: boolean) => enabled ? 'Yes' : 'No';
  return <section aria-label="Scoped MCP access">
    <p>Apply verifies the registered server. Live run observations show actual access separately. Refresh the inspection to update these states.</p>
    {rows.length === 0 ? <p>No enabled tools in this project.</p> : <div style={{ overflowX: 'auto' }}><table>
      <caption>Memory MCP access by tool</caption>
      <thead><tr><th scope="col">Tool</th><th scope="col">State</th><th scope="col">Configured</th><th scope="col">Connection verified</th><th scope="col">Available in run</th><th scope="col">Catalog connected in run</th><th scope="col">Memory consulted</th></tr></thead>
      <tbody>{rows.map(row => <tr key={row.tool_id}><th scope="row">{row.tool_id}</th><td>{row.state.replaceAll('_', ' ')}</td><td>{state(row.configured)}</td><td>{state(row.connection_verified)}</td><td>{state(row.available_in_run)}</td><td>{state(row.connected_in_run)}</td><td>{state(row.consulted)} ({row.consultations_observed} observed calls)</td></tr>)}</tbody>
    </table></div>}
    {rows.map(row => <p key={row.tool_id}><strong>{row.tool_id}</strong>: {row.diagnostic}</p>)}
    <p>Control reads never count as agent consultation. A consultation does not prove the memory was useful. Live credentials are never displayed.</p>
  </section>;
}
