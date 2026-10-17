import { useEffect, useState } from 'react';
import { supervisorErrors } from './supervisorValidation';
import { getToolDescriptors } from '../../api/client';
import type { ApiToolDescriptor } from '../../api/models';
import type { ApiConfigResponse, ApiConfigUpdateRequest, SupervisorConfig } from '../../api/models';


const inputStyle = { width: '100%', padding: 8, border: '1px solid var(--border)', borderRadius: 4, color: 'var(--text-primary)', background: 'var(--bg-secondary)' };

export default function SupervisorSettings({ draft, update }: {
  draft: ApiConfigResponse;
  update: (patch: Partial<ApiConfigUpdateRequest>) => void;
}) {
  const [tools, setTools] = useState<ApiToolDescriptor[]>([]);
  const [toolError, setToolError] = useState<string | null>(null);
  useEffect(() => {
    let active = true;
    getToolDescriptors().then(value => { if (active) setTools(value); }).catch(error => { if (active) setToolError(String(error)); });
    return () => { active = false; };
  }, []);
  const config = draft.supervisor;
  if (!config) return <p>Supervisor configuration is unavailable. Reload with a current MACC server.</p>;
  const set = (patch: Partial<SupervisorConfig>) => update({ supervisor: { ...config, ...patch } });
  const issues = supervisorErrors(config);
  const selectedTool = config.tool ?? draft.coordinatorTool ?? draft.enabledTools[0];
  const descriptor = tools.find(t => t.id === selectedTool);
  const suggestions = (key: string) => {
    const field = descriptor?.fields.find(f => key === 'model' ? f.id === 'model' : f.id.toLowerCase().includes('effort'));
    return field?.kind.type === 'enum' ? field.kind.options : [];
  };
  return <section aria-label="Supervisor settings" style={{ display: 'grid', gap: 18 }}>
    <h2>Supervisor</h2>
    <p>Effective tool: {selectedTool ?? 'none enabled'}. Model and effort inherit its settings when left empty.</p>
    {toolError && <p role="status">Tool suggestions unavailable: {toolError}. You can still enter overrides.</p>}
    {['model', 'effort'].map(key => <datalist key={key} id={`supervisor-options-${key}`}>{suggestions(key).map(v => <option key={v} value={v} />)}</datalist>)}
    <p>Diagnose → repair → tests → AI verification → commit and integrate → resume. Changes apply at the next intervention; an active intervention keeps its current settings.</p>
    {(!config.tests_enabled || !config.ai_verification_enabled) && <p role="status">Automatic integration remains enabled with the selected checks. {config.tests_enabled ? '' : 'Tests are skipped. '}{config.ai_verification_enabled ? '' : 'AI verification is skipped.'}</p>}
    {!config.repair_enabled && <p role="status">Diagnosis only: source repairs remain waiting until automatic repair is enabled.</p>}
    {!config.auto_restart && <p>After integration, tasks are requeued and the coordinator stays stopped.</p>}
    {issues.length > 0 && <ul role="alert">{issues.map(e => <li key={e}>{e}</li>)}</ul>}
    {(draft.supervisorFields ?? []).filter(f => f.key !== 'validation_commands').map(field => {
      const value = config[field.key];
      return <div key={field.key}>
        <label htmlFor={`supervisor-${field.key}`}>{field.label}</label>
        {field.kind === 'bool' ? <input id={`supervisor-${field.key}`} type="checkbox" checked={value === true} onChange={e => set({ [field.key]: e.target.checked })} />
          : field.key === 'tool' ? <select id="supervisor-tool" style={inputStyle} value={config.tool ?? ''} onChange={e => set({ tool: e.target.value || null })}>
            <option value="">Automatic (coordinator, then first enabled tool)</option>
            {Array.from(new Set([...draft.enabledTools, ...(config.tool ? [config.tool] : [])])).map(id => <option key={id} value={id}>{id}</option>)}
          </select>
          : <input id={`supervisor-${field.key}`} style={inputStyle} list={field.key === 'model' || field.key === 'effort' ? `supervisor-options-${field.key}` : undefined} type={field.kind === 'number' ? 'number' : 'text'} min={field.kind === 'number' ? 1 : undefined} step={field.kind === 'number' ? 1 : undefined} value={typeof value === 'string' || typeof value === 'number' ? value : ''} placeholder={field.kind === 'optional' ? 'Inherit tool setting' : undefined} onChange={e => set({ [field.key]: field.kind === 'number' ? Number(e.target.value) : field.kind === 'optional' ? e.target.value || null : e.target.value })} />}
        <p style={{ fontSize: 12, color: 'var(--text-muted)' }}>{field.help}</p>
      </div>;
    })}
    <fieldset>
      {!config.tests_enabled && <p>Commands stay editable for later use; they will not run while tests are disabled.</p>}
      <legend>Validation commands</legend>
      <p>Run from the isolated worktree root. Use relative subproject paths, for example: cd web &amp;&amp; npm test. An empty list enables root workflow detection.</p>
      {config.validation_commands.map((command, index) => <div key={index} style={{ display: 'flex', gap: 8, marginBottom: 8 }}>
        <input aria-label={`Validation command ${index + 1}`} style={inputStyle} value={command} onChange={e => set({ validation_commands: config.validation_commands.map((c, i) => i === index ? e.target.value : c) })} />
        <button type="button" aria-label={`Remove validation command ${index + 1}`} onClick={() => set({ validation_commands: config.validation_commands.filter((_, i) => i !== index) })}>Remove</button>
        <button type="button" disabled={index === 0} aria-label={`Move validation command ${index + 1} up`} onClick={() => {
          const commands = [...config.validation_commands];
          [commands[index - 1], commands[index]] = [commands[index], commands[index - 1]];
          set({ validation_commands: commands });
        }}>↑</button>
      </div>)}
      <button type="button" onClick={() => set({ validation_commands: [...config.validation_commands, ''] })}>Add validation command</button>
    </fieldset>
    <details><summary>Last saved readiness (commands are not executed)</summary>
      <ul>{(draft.supervisorReadiness ?? []).map(message => <li key={message}>{message}</li>)}</ul>
      <p>Save to refresh discovery. CLI: macc supervisor config check</p>
    </details>
  </section>;
}
