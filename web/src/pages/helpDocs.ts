export type HelpSectionId =
  | 'getting-started'
  | 'web-ui'
  | 'commands'
  | 'configuration'
  | 'coordinator'
  | 'troubleshooting';

export type HelpSection = {
  id: HelpSectionId;
  title: string;
  content: string;
};

export const HELP_SECTIONS: HelpSection[] = [
  {
    id: 'getting-started',
    title: 'Getting Started',
    content: `# Getting Started

The MACC Web UI helps you run the BMAD-lite workflow in one place.

## First steps

1. Open **Dashboard** to confirm coordinator and repository health.
2. Use **Init** if this project is not initialized yet.
3. Continue with **PRD**, **Plan**, then **Apply**.

## Keyboard shortcuts

- \`Ctrl+K\`: open command palette
- \`/\`: focus global search

For full project documentation, open [README.md](../README.md) and [MACC.md](../MACC.md).`,
  },
  {
    id: 'web-ui',
    title: 'Web UI',
    content: `# Web UI

## Navigation

- **Setup & Config**: setup and workflow pages.
- **Ops**: runtime and infrastructure views.
- **Support**: Help and About.

## Common patterns

- Use global search in the top bar to jump quickly.
- Open notifications for coordinator and workflow events.
- Use the contextual **?** button in the top bar to open page-specific help.`,
  },
  {
    id: 'commands',
    title: 'Commands',
    content: `# Commands

The command palette includes frequent navigation and coordinator actions.

\`\`\`bash
Ctrl+K                     # open command palette
G then ?                   # jump to Help from palette shortcuts
\`\`\`

Typical workflow commands are also available from page buttons (for example Plan run and Apply actions).`,
  },
  {
    id: 'configuration',
    title: 'Configuration',
    content: `# Configuration

Use the **Config** pages to manage tool adapters and standards.

## Pages

- **Tools**: adapter-level settings and health.
- **Standards**: coding standards and presets.
- **Skills**: install/remove skill packages.
- **Settings**: global web/coordinator/runtime settings.

When unsure, start from Settings and search for the key in global search.`,
  },
  {
    id: 'coordinator',
    title: 'Coordinator',
    content: `# Coordinator

Coordinator status is visible in Dashboard and the status strip.

## Operational pages

- **Live**: event stream and active task progress.
- **Logs**: detailed logs and filtering.
- **Locks**: lock graph and contention.
- **Worktrees / Registry**: worker and task assignments.

If coordinator actions fail, check Diagnostics then Logs.`,
  },
  {
    id: 'troubleshooting',
    title: 'Troubleshooting',
    content: `# Troubleshooting

## Common checks

1. Confirm API health and SSE connection on Dashboard.
2. Check Diagnostics for environment problems.
3. Review Logs for recent errors.
4. Inspect Worktrees and locks when tasks are blocked.

## Useful command examples

\`\`\`bash
make test
cargo test
pnpm --dir web test
\`\`\`

For full runbooks and architecture details, refer to [README.md](../README.md) and [MACC.md](../MACC.md).`,
  },
];

const ROUTE_HELP_SECTION_ENTRIES: Array<[prefix: string, section: HelpSectionId]> = [
  ['/welcome', 'getting-started'],
  ['/init', 'getting-started'],
  ['/dashboard', 'web-ui'],
  ['/config', 'configuration'],
  ['/prd', 'commands'],
  ['/plan', 'commands'],
  ['/apply', 'commands'],
  ['/ops/console', 'coordinator'],
  ['/ops/registry', 'coordinator'],
  ['/ops/worktrees', 'coordinator'],
  ['/ops/worker', 'coordinator'],
  ['/ops/live', 'coordinator'],
  ['/ops/locks', 'coordinator'],
  ['/ops/diagnostics', 'troubleshooting'],
  ['/ops/logs', 'troubleshooting'],
  ['/ops/backups', 'troubleshooting'],
  ['/ops/git', 'coordinator'],
  ['/about', 'web-ui'],
  ['/help', 'web-ui'],
];

export function getHelpSectionForRoute(pathname: string): HelpSectionId {
  const normalizedPath = pathname.trim().toLowerCase();

  for (const [prefix, section] of ROUTE_HELP_SECTION_ENTRIES) {
    if (normalizedPath.startsWith(prefix)) {
      return section;
    }
  }

  return 'getting-started';
}

export function getHelpSectionById(id: string | null): HelpSection | undefined {
  if (!id) {
    return undefined;
  }

  return HELP_SECTIONS.find((section) => section.id === id);
}
