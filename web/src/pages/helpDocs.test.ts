import { describe, expect, it } from 'vitest';
import { getHelpSectionForRoute } from './helpDocs';

describe('getHelpSectionForRoute', () => {
  it.each([
    ['/welcome', 'getting-started'],
    ['/config/tools', 'configuration'],
    ['/ops/logs', 'troubleshooting'],
    ['/plan', 'commands'],
    ['/ops/worktrees/worker-1', 'coordinator'],
    ['/unknown', 'getting-started'],
  ] as const)('maps %s to %s', (pathname, expected) => {
    expect(getHelpSectionForRoute(pathname)).toBe(expected);
  });
});
