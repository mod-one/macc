export type ToolState = {
  tool_id: string; state: string; configured: boolean; connection_verified: boolean;
  available_in_run: boolean; connected_in_run: boolean; consulted: boolean;
  consultations_observed: number; diagnostic: string;
};

export function accessStates(value: unknown): ToolState[] | null {
  if (!value || typeof value !== 'object' || !('tool_states' in value) || !Array.isArray(value.tool_states)) return null;
  return value.tool_states as ToolState[];
}
