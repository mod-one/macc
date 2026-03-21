import React, { useEffect, useState, useCallback } from 'react';
import { 
  getToolCooldowns, 
  setToolCooldown, 
  clearToolCooldown,
  getConfig 
} from '../api/client';
import type { 
  ApiToolCooldownEntry 
} from '../api/models';
import { Button } from './Button';
import * as Icons from './icons';
import { cn } from './styles';

interface ToolCooldownPanelProps {
  className?: string;
}

export const ToolCooldownPanel: React.FC<ToolCooldownPanelProps> = ({ className }) => {
  const [cooldowns, setCooldowns] = useState<ApiToolCooldownEntry[]>([]);
  const [enabledTools, setEnabledTools] = useState<string[]>([]);
  const [isBusy, setIsBusy] = useState(false);
  
  // Form state
  const [selectedTool, setSelectedTool] = useState('');
  const [durationSeconds, setDurationSeconds] = useState(3600);

  const fetchCooldowns = useCallback(async () => {
    try {
      const result = await getToolCooldowns();
      if (result.tool_cooldowns) {
        setCooldowns(result.tool_cooldowns);
      }
    } catch (err) {
      console.error('Failed to fetch cooldowns:', err);
    }
  }, []);

  const fetchConfig = useCallback(async () => {
    try {
      const config = await getConfig();
      setEnabledTools(config.enabledTools);
      if (config.enabledTools.length > 0 && !selectedTool) {
        setSelectedTool(config.enabledTools[0]);
      }
    } catch (err) {
      console.error('Failed to fetch config:', err);
    }
  }, [selectedTool]);

  useEffect(() => {
    fetchCooldowns();
    fetchConfig();
    
    const interval = setInterval(fetchCooldowns, 30000);
    return () => clearInterval(interval);
  }, [fetchCooldowns, fetchConfig]);

  // Real-time countdown
  useEffect(() => {
    if (cooldowns.length === 0) return;

    const timer = setInterval(() => {
      setCooldowns(prev => prev.map(c => ({
        ...c,
        remaining_seconds: Math.max(0, c.remaining_seconds - 1)
      })));
    }, 1000);

    return () => clearInterval(timer);
  }, [cooldowns.length]);

  const handleClear = async (toolId: string) => {
    setIsBusy(true);
    try {
      await clearToolCooldown(toolId);
      await fetchCooldowns();
    } catch (err) {
      console.error('Failed to clear cooldown:', err);
    } finally {
      setIsBusy(false);
    }
  };

  const handleSet = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!selectedTool) return;
    
    setIsBusy(true);
    try {
      await setToolCooldown(selectedTool, durationSeconds);
      await fetchCooldowns();
    } catch (err) {
      console.error('Failed to set cooldown:', err);
    } finally {
      setIsBusy(false);
    }
  };

  const formatDuration = (seconds: number) => {
    const h = Math.floor(seconds / 3600);
    const m = Math.floor((seconds % 3600) / 60);
    const s = seconds % 60;
    
    const parts = [];
    if (h > 0) parts.push(`${h}h`);
    if (m > 0 || h > 0) parts.push(`${m}m`);
    parts.push(`${s}s`);
    
    return parts.join(' ');
  };

  const presets = [
    { label: '1h', value: 3600 },
    { label: '2h', value: 7200 },
    { label: '6h', value: 21600 },
    { label: '12h', value: 43200 },
    { label: '24h', value: 86400 },
  ];

  return (
    <div className={cn("space-y-6", className)}>
      {/* Cooldown Table */}
      <div className="rounded-xl border border-[var(--border)] bg-[var(--bg-card)] overflow-hidden">
        <table className="w-full text-left text-sm">
          <thead className="bg-[var(--bg-secondary)] border-b border-[var(--border)]">
            <tr>
              <th className="px-4 py-3 font-semibold text-[var(--text-secondary)]">Tool</th>
              <th className="px-4 py-3 font-semibold text-[var(--text-secondary)] text-right">Remaining</th>
              <th className="px-4 py-3 font-semibold text-[var(--text-secondary)] hidden sm:table-cell">Until</th>
              <th className="px-4 py-3 font-semibold text-[var(--text-secondary)] hidden lg:table-cell text-right">Backoff</th>
              <th className="px-4 py-3 font-semibold text-[var(--text-secondary)] text-right">Actions</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-[var(--border)]">
            {cooldowns.length === 0 ? (
              <tr>
                <td colSpan={5} className="px-4 py-8 text-center text-[var(--text-muted)] italic">
                  No active cooldowns.
                </td>
              </tr>
            ) : (
              cooldowns.map((c) => (
                <tr key={c.tool_id} className="hover:bg-[var(--bg-secondary)]/50 transition-colors">
                  <td className="px-4 py-3">
                    <div className="flex items-center gap-2">
                      <span className="font-medium text-[var(--text-primary)]">{c.tool_id}</span>
                      {c.remaining_seconds > 0 && (
                        <span className="h-2 w-2 rounded-full bg-amber-500 animate-pulse" />
                      )}
                    </div>
                  </td>
                  <td className="px-4 py-3 text-right font-mono text-amber-500">
                    {formatDuration(c.remaining_seconds)}
                  </td>
                  <td className="px-4 py-3 text-[var(--text-secondary)] hidden sm:table-cell">
                    {new Date(c.throttled_until).toLocaleTimeString()}
                  </td>
                  <td className="px-4 py-3 text-right text-[var(--text-muted)] hidden lg:table-cell">
                    {formatDuration(c.backoff_seconds)}
                  </td>
                  <td className="px-4 py-3 text-right">
                    <Button
                      onClick={() => handleClear(c.tool_id)}
                      disabled={isBusy}
                      className="h-8 px-3 text-xs border-[var(--border)] bg-[var(--bg-secondary)] hover:bg-rose-500/10 hover:text-rose-500 hover:border-rose-500/50"
                    >
                      Clear
                    </Button>
                  </td>
                </tr>
              ))
            )}
          </tbody>
        </table>
      </div>

      {/* Set Cooldown Form */}
      <form onSubmit={handleSet} className="p-5 rounded-2xl border border-[var(--border)] bg-[var(--bg-card)] shadow-sm space-y-4">
        <h4 className="text-xs font-bold uppercase tracking-wider text-[var(--text-muted)] flex items-center gap-2">
          <Icons.ClockIcon className="h-3 w-3" />
          Set Manual Cooldown
        </h4>
        
        <div className="grid gap-4 sm:grid-cols-[1fr_1fr_auto] items-end">
          <div className="space-y-1.5">
            <label className="text-[10px] font-bold uppercase text-[var(--text-secondary)] px-1">Tool Name</label>
            <select
              value={selectedTool}
              onChange={(e) => setSelectedTool(e.target.value)}
              className="w-full bg-[var(--bg-secondary)] border border-[var(--border)] rounded-lg px-3 h-10 text-sm text-[var(--text-primary)] focus:outline-none focus:ring-2 focus:ring-[var(--accent)]/50"
            >
              {enabledTools.map(t => (
                <option key={t} value={t}>{t}</option>
              ))}
            </select>
          </div>

          <div className="space-y-1.5">
            <label className="text-[10px] font-bold uppercase text-[var(--text-secondary)] px-1">Duration (seconds)</label>
            <input
              type="number"
              value={durationSeconds}
              onChange={(e) => setDurationSeconds(parseInt(e.target.value) || 0)}
              className="w-full bg-[var(--bg-secondary)] border border-[var(--border)] rounded-lg px-3 h-10 text-sm text-[var(--text-primary)] focus:outline-none focus:ring-2 focus:ring-[var(--accent)]/50"
            />
          </div>

          <Button 
            type="submit" 
            disabled={isBusy || !selectedTool}
            className="h-10 px-6 bg-[var(--accent)] text-white hover:bg-[var(--accent-hover)] font-semibold shadow-lg shadow-[var(--accent)]/20"
          >
            Set Cooldown
          </Button>
        </div>

        <div className="flex flex-wrap gap-2">
          {presets.map(p => (
            <button
              key={p.value}
              type="button"
              onClick={() => setDurationSeconds(p.value)}
              className={cn(
                "px-3 py-1 rounded-full text-xs font-medium border transition-all",
                durationSeconds === p.value 
                  ? "bg-[var(--accent)]/10 border-[var(--accent)] text-[var(--accent)]" 
                  : "bg-[var(--bg-secondary)] border-[var(--border)] text-[var(--text-secondary)] hover:border-[var(--text-muted)]"
              )}
            >
              {p.label}
            </button>
          ))}
        </div>
      </form>
    </div>
  );
};
