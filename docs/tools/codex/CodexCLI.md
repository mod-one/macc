# Codex CLI

Codex CLI is OpenAI's coding agent that you can run locally from your terminal. It can read, change, and run code on your machine in the selected directory.
It's [open source](https://github.com/openai/codex) and built in Rust for speed and efficiency.

ChatGPT Plus, Pro, Business, Edu, and Enterprise plans include Codex. Learn more about [what's included](https://developers.openai.com/codex/pricing).

<YouTubeEmbed
  title="Codex CLI overview"
  videoId="iqNzfK4_meQ"
  class="max-w-md"
/>
<br />

## CLI setup

<CliSetupSteps client:load />

The Codex CLI is available on macOS, Windows, and Linux. On Windows, run Codex
  natively in PowerShell with the Windows sandbox, or use WSL2 when you need a
  Linux-native environment. For setup details, see the 
  <a href="/codex/windows">Windows setup guide</a>.

If you're new to Codex, read the [best practices guide](https://developers.openai.com/codex/learn/best-practices).

---

## Work with the Codex CLI

<BentoContainer>
  <BentoContent href="/codex/cli/features#running-in-interactive-mode">

### Run Codex interactively

Run `codex` to start an interactive terminal UI (TUI) session.

  </BentoContent>
  <BentoContent href="/codex/cli/features#models-reasoning">

### Control model and reasoning

Use `/model` to switch between GPT-5.4, GPT-5.3-Codex, and other available models, or adjust reasoning levels.

  </BentoContent>
  <BentoContent href="/codex/cli/features#image-inputs">

### Image inputs

Attach screenshots or design specs so Codex reads them alongside your prompt.

  </BentoContent>
  <BentoContent href="/codex/cli/features#image-generation">

### Image generation

Generate or edit images directly in the CLI, and attach references when you want Codex to iterate on an existing asset.

  </BentoContent>

  <BentoContent href="/codex/cli/features#running-local-code-review">

### Run local code review

Get your code reviewed by a separate Codex agent before you commit or push your changes.

  </BentoContent>

  <BentoContent href="/codex/subagents">

### Use subagents

Use subagents to parallelize complex tasks.

  </BentoContent>

  <BentoContent href="/codex/cli/features#web-search">

### Web search

Use Codex to search the web and get up-to-date information for your task.

  </BentoContent>

  <BentoContent href="/codex/cli/features#working-with-codex-cloud">

### Codex Cloud tasks

Launch a Codex Cloud task, choose environments, and apply the resulting diffs without leaving your terminal.

  </BentoContent>

  <BentoContent href="/codex/noninteractive">

### Scripting Codex

Automate repeatable workflows by scripting Codex with the `exec` command.

  </BentoContent>
  <BentoContent href="/codex/mcp">

### Model Context Protocol

Give Codex access to additional third-party tools and context with Model Context Protocol (MCP).

  </BentoContent>
  
  <BentoContent href="/codex/cli/features#approval-modes">

### Approval modes

Choose the approval mode that matches your comfort level before Codex edits or runs commands.

  </BentoContent>
</BentoContainer>


# Codex CLI features

Codex supports workflows beyond chat. Use this guide to learn what each one unlocks and when to use it.

## Running in interactive mode

Codex launches into a full-screen terminal UI that can read your repository, make edits, and run commands as you iterate together. Use it whenever you want a conversational workflow where you can review Codex's actions in real time.

```bash
codex
```

You can also specify an initial prompt on the command line.

```bash
codex "Explain this codebase to me"
```

Once the session is open, you can:

- Send prompts, code snippets, or screenshots (see [image inputs](#image-inputs)) directly into the composer.
- Watch Codex explain its plan before making a change, and approve or reject steps inline.
- Read syntax-highlighted markdown code blocks and diffs in the TUI, then use `/theme` to preview and save a preferred theme.
- Use `/clear` to wipe the terminal and start a fresh chat, or press <kbd>Ctrl</kbd>+<kbd>L</kbd> to clear the screen without starting a new conversation.
- Use `/copy` or press <kbd>Ctrl</kbd>+<kbd>O</kbd> to copy the latest completed Codex output. If a turn is still running, Codex copies the most recent finished output instead of in-progress text.
- Press <kbd>Tab</kbd> while Codex is running to queue follow-up text, slash commands, or `!` shell commands for the next turn.
- Navigate draft history in the composer with <kbd>Up</kbd>/<kbd>Down</kbd>; Codex restores prior draft text and image placeholders.
- Press <kbd>Ctrl</kbd>+<kbd>R</kbd> to search prompt history from the composer, then press <kbd>Enter</kbd> to accept a match or <kbd>Esc</kbd> to cancel.
- Press <kbd>Ctrl</kbd>+<kbd>C</kbd> or use `/exit` to close the interactive session when you're done.

## Resuming conversations

Codex stores your transcripts locally so you can pick up where you left off instead of repeating context. Use the `resume` subcommand when you want to reopen an earlier thread with the same repository state and instructions.

- `codex resume` launches a picker of recent interactive sessions. Highlight a run to see its summary and press <kbd>Enter</kbd> to reopen it.
- `codex resume --all` shows sessions beyond the current working directory, so you can reopen any local run.
- `codex resume --last` skips the picker and jumps straight to your most recent session from the current working directory (add `--all` to ignore the current working directory filter).
- `codex resume <SESSION_ID>` targets a specific run. You can copy the ID from the picker, `/status`, or the files under `~/.codex/sessions/`.

Non-interactive automation runs can resume too:

```bash
codex exec resume --last "Fix the race conditions you found"
codex exec resume 7f9f9a2e-1b3c-4c7a-9b0e-.... "Implement the plan"
```

Each resumed run keeps the original transcript, plan history, and approvals, so Codex can use prior context while you supply new instructions. Override the working directory with `--cd` or add extra roots with `--add-dir` if you need to steer the environment before resuming.

## Connect the TUI to a remote app server

Remote TUI mode lets you run the Codex app server on one machine and use the
Codex terminal UI from another machine. Start the app server with a WebSocket
listener:

```bash
codex app-server --listen ws://127.0.0.1:4500
```

Then connect the TUI to that endpoint:

```bash
codex --remote ws://127.0.0.1:4500
```

For access from another machine, bind the app server to a reachable interface
and configure WebSocket auth before remote use:

```bash
TOKEN_FILE="$HOME/.codex/app-server-token"
openssl rand -base64 32 > "$TOKEN_FILE"
chmod 600 "$TOKEN_FILE"
codex app-server --listen ws://0.0.0.0:4500 --ws-auth capability-token --ws-token-file "$TOKEN_FILE"
```

`--remote` accepts explicit `ws://host:port`, `wss://host:port`, `unix://`, and
`unix://PATH` addresses. Use `unix://` for Codex's default local Unix socket or
`unix://PATH` for an explicit local socket path. Plain WebSocket connections are
appropriate for localhost and SSH port-forwarding workflows. For non-local
clients, use WebSocket auth and put the connection behind TLS.

Codex supports these WebSocket authentication modes:

- Capability token: start the server with `--ws-auth capability-token` and
  either `--ws-token-file /absolute/path` or `--ws-token-sha256 HEX`.
- Signed bearer token: start the server with
  `--ws-auth signed-bearer-token --ws-shared-secret-file /absolute/path`, plus
  optional `--ws-issuer`, `--ws-audience`, and `--ws-max-clock-skew-seconds`.

The TUI sends the remote auth token as an `Authorization: Bearer <token>` header
during the WebSocket handshake. Codex only accepts remote auth tokens over
`wss://` URLs or local-only `ws://` URLs.

```bash
export CODEX_REMOTE_TOKEN="$(cat "$TOKEN_FILE")"
codex --remote wss://remote-host:4500 --remote-auth-token-env CODEX_REMOTE_TOKEN
```

For SSH remote projects in the Codex app, use
[Remote connections](https://developers.openai.com/codex/remote-connections). For managed remote-control
clients, `codex remote-control` starts an app-server process with
remote-control support enabled.

## Models and reasoning

For most tasks in Codex, `gpt-5.5` is the recommended model. It's OpenAI's newest frontier model for complex coding, computer
use, knowledge work, and research workflows, with stronger planning, tool use,
and follow-through on multi-step tasks. For extra fast tasks, ChatGPT Pro subscribers have
access to the GPT-5.3-Codex-Spark model in research preview.

Switch models mid-session with the `/model` command, or specify one when launching the CLI.

```bash
codex --model gpt-5.5
```


[Learn more about the models available in Codex](https://developers.openai.com/codex/models).

## Feature flags

Codex includes a small set of feature flags. Use the `features` subcommand to inspect what's available and to persist changes in your configuration.

```bash
codex features list
codex features enable unified_exec
codex features disable shell_snapshot
```

`codex features enable <feature>` and `codex features disable <feature>` write
to `$CODEX_HOME/config.toml`. The `features` subcommand doesn't accept
`--profile`.

## Subagents

Use Codex subagent workflows to parallelize larger tasks. For setup, role configuration (`[agents]` in `config.toml`), and examples, see [Subagents](https://developers.openai.com/codex/subagents).

Codex only spawns subagents when you explicitly ask it to. Because each
subagent does its own model and tool work, subagent workflows consume more
tokens than comparable single-agent runs.

## Image inputs

Attach screenshots or design specs so Codex can read image details alongside your prompt. You can paste images into the interactive composer or provide files on the command line.

```bash
codex -i screenshot.png "Explain this error"
```

```bash
codex --image img1.png,img2.jpg "Summarize these diagrams"
```

Codex accepts common formats such as PNG and JPEG. Use comma-separated filenames for two or more images, and combine them with text instructions to add context.

## Image generation

Ask Codex to generate or edit images directly in the CLI. This works well for assets such as icons, banners, illustrations, sprite sheets, and placeholder art. If you want Codex to transform or extend an existing asset, attach a reference image with your prompt.

You can ask in natural language or explicitly invoke the image generation skill by including `$imagegen` in your prompt.

Built-in image generation uses `gpt-image-2`, counts toward your general Codex usage limits, and uses included limits 3-5x faster on average than similar turns without image generation, depending on image quality and size. For details, see [Pricing](https://developers.openai.com/codex/pricing#image-generation-usage-limits). For prompting tips and model details, see the [image generation guide](https://developers.openai.com/api/docs/guides/image-generation).

For larger batches of image generation, set `OPENAI_API_KEY` in your environment variables and ask Codex to generate images through the API so API pricing applies instead.

## Syntax highlighting and themes

The TUI syntax-highlights fenced markdown code blocks and file diffs so code is easier to scan during reviews and debugging.

Use `/theme` to open the theme picker, preview themes live, and save your selection to `tui.theme` in `~/.codex/config.toml`. You can also add custom `.tmTheme` files under `$CODEX_HOME/themes` and select them in the picker.

## Running local code review

Type `/review` in the CLI to open Codex's review presets. The CLI launches a dedicated reviewer that reads the diff you select and reports prioritized, actionable findings without touching your working tree. By default it uses the current session model; set `review_model` in `config.toml` to override.

- **Review against a base branch** lets you pick a local branch; Codex finds the merge base against its upstream, diffs your work, and highlights the biggest risks before you open a pull request.
- **Review uncommitted changes** inspects everything that's staged, not staged, or not tracked so you can address issues before committing.
- **Review a commit** lists recent commits and has Codex read the exact change set for the SHA you choose.
- **Custom review instructions** accepts your own wording (for example, "Focus on accessibility regressions") and runs the same reviewer with that prompt.

Each run shows up as its own turn in the transcript, so you can rerun reviews as the code evolves and compare the feedback.

## Web search

Codex ships with a first-party web search tool. For local tasks in the Codex CLI, Codex enables web search by default and serves results from a web search cache. The cache is an OpenAI-maintained index of web results, so cached mode returns pre-indexed results instead of fetching live pages. This reduces exposure to prompt injection from arbitrary live content, but you should still treat web results as untrusted. If you are using `--yolo` or another [full access sandbox setting](https://developers.openai.com/codex/agent-approvals-security), web search defaults to live results. To fetch the most recent data, pass `--search` for a single run or set `web_search = "live"` in [Config basics](https://developers.openai.com/codex/config-basic). You can also set `web_search = "disabled"` to turn the tool off.

You'll see `web_search` items in the transcript or `codex exec --json` output whenever Codex looks something up.

## Running with an input prompt

When you just need a quick answer, run Codex with a single prompt and skip the interactive UI.

```bash
codex "explain this codebase"
```

Codex will read the working directory, craft a plan, and stream the response back to your terminal before exiting. Pair this with flags like `--path` to target a specific directory or `--model` to dial in the behavior up front.

## Shell completions

Speed up everyday usage by installing the generated completion scripts for your shell:

```bash
codex completion bash
codex completion zsh
codex completion fish
```

Run the completion script in your shell configuration file to set up completions for new sessions. For example, if you use `zsh`, you can add the following to the end of your `~/.zshrc` file:

```bash
# ~/.zshrc
eval "$(codex completion zsh)"
```

Start a new session, type `codex`, and press <kbd>Tab</kbd> to see the completions. If you see a `command not found: compdef` error, add `autoload -Uz compinit && compinit` to your `~/.zshrc` file before the `eval "$(codex completion zsh)"` line, then restart your shell.

## Approval modes

Approval modes define how much Codex can do without stopping for confirmation. Use `/permissions` inside an interactive session to switch modes as your comfort level changes.

- **Auto** (default) lets Codex read files, edit, and run commands within the working directory. It still asks before touching anything outside that scope or using the network.
- **Read-only** keeps Codex in a consultative mode. It can browse files but won't make changes or run commands until you approve a plan.
- **Full Access** grants Codex the ability to work across your machine, including network access, without asking. Use it sparingly and only when you trust the repository and task.

Codex always surfaces a transcript of its actions, so you can review or roll back changes with your usual git workflow.

## Scripting Codex

Automate workflows or wire Codex into your existing scripts with the `exec` subcommand. This runs Codex non-interactively, piping the final plan and results back to `stdout`.

```bash
codex exec "fix the CI failure"
```

Combine `exec` with shell scripting to build custom workflows, such as automatically updating changelogs, sorting issues, or enforcing editorial checks before a PR ships.

## Working with Codex cloud

The `codex cloud` command lets you triage and launch [Codex cloud tasks](https://developers.openai.com/codex/cloud) without leaving the terminal. Run it with no arguments to open an interactive picker, browse active or finished tasks, and apply the changes to your local project.

You can also start a task directly from the terminal:

```bash
codex cloud exec --env ENV_ID "Summarize open bugs"
```

Add `--attempts` (1–4) to request best-of-N runs when you want Codex cloud to generate more than one solution. For example, `codex cloud exec --env ENV_ID --attempts 3 "Summarize open bugs"`.

Environment IDs come from your Codex cloud configuration—use `codex cloud` and press <kbd>Ctrl</kbd>+<kbd>O</kbd> to choose an environment or the web dashboard to confirm the exact value. Authentication follows your existing CLI login, and the command exits non-zero if submission fails so you can wire it into scripts or CI.

## Slash commands

Slash commands give you quick access to specialized workflows like `/review`, `/fork`, `/side`, or your own reusable prompts. Codex ships with a curated set of built-ins, and you can create custom ones for team-specific tasks or personal shortcuts.

See the [slash commands guide](https://developers.openai.com/codex/guides/slash-commands) to browse the catalog of built-ins, learn how to author custom commands, and understand where they live on disk.

## Prompt editor

When you're drafting a longer prompt, it can be easier to switch to a full editor and then send the result back to the composer.

In the prompt input, press <kbd>Ctrl</kbd>+<kbd>G</kbd> to open the editor defined by the `VISUAL` environment variable (or `EDITOR` if `VISUAL` isn't set).

## Model Context Protocol (MCP)

Connect Codex to more tools by configuring Model Context Protocol servers. Add STDIO or streaming HTTP servers in `~/.codex/config.toml`, or manage them with the `codex mcp` CLI commands—Codex launches them automatically when a session starts and exposes their tools next to the built-ins. You can even run Codex itself as an MCP server when you need it inside another agent.

See [Model Context Protocol](https://developers.openai.com/codex/mcp) for example configurations, supported auth flows, and a more detailed guide.

## Tips and shortcuts

- Type `@` in the composer to open a fuzzy file search over the workspace root; press <kbd>Tab</kbd> or <kbd>Enter</kbd> to drop the highlighted path into your message.
- Press <kbd>Enter</kbd> while Codex is running to inject new instructions into the current turn, or press <kbd>Tab</kbd> to queue follow-up input for the next turn. Queued input can be a normal prompt, a slash command such as `/review`, or a `!` shell command. Codex parses queued slash commands when they run.
- Prefix a line with `!` to run a local shell command (for example, `!ls`). Codex treats the output like a user-provided command result and still applies your approval and sandbox settings.
- Tap <kbd>Esc</kbd> twice while the composer is empty to edit your previous user message. Continue pressing <kbd>Esc</kbd> to walk further back in the transcript, then hit <kbd>Enter</kbd> to fork from that point.
- Launch Codex from any directory using `codex --cd <path>` to set the working root without running `cd` first. The active path appears in the TUI header.
- Expose more writable roots with `--add-dir` (for example, `codex --cd apps/frontend --add-dir ../backend --add-dir ../shared`) when you need to coordinate changes across more than one project.
- Make sure your environment is already set up before launching Codex so it doesn't spend tokens probing what to activate. For example, source your Python virtual environment (or other language environments), start any required daemons, and export the environment variables you expect to use ahead of time.


---


# Command line options

export const globalFlagOptions = [
  {
    key: "PROMPT",
    type: "string",
    description:
      "Optional text instruction to start the session. Omit to launch the TUI without a pre-filled message.",
  },
  {
    key: "--image, -i",
    type: "path[,path...]",
    description:
      "Attach one or more image files to the initial prompt. Separate multiple paths with commas or repeat the flag.",
  },
  {
    key: "--model, -m",
    type: "string",
    description:
      "Override the model set in configuration (for example `gpt-5.4`).",
  },
  {
    key: "--oss",
    type: "boolean",
    defaultValue: "false",
    description:
      'Use the local open source model provider (equivalent to `-c model_provider="oss"`). Validates that Ollama is running.',
  },
  {
    key: "--profile, -p",
    type: "string",
    description:
      "Layer `$CODEX_HOME/profile-name.config.toml` on top of the base user config.",
  },
  {
    key: "--sandbox, -s",
    type: "read-only | workspace-write | danger-full-access",
    description:
      "Select the sandbox policy for model-generated shell commands.",
  },
  {
    key: "--ask-for-approval, -a",
    type: "untrusted | on-request | never",
    description:
      "Control when Codex pauses for human approval before running a command. `on-failure` is deprecated; prefer `on-request` for interactive runs or `never` for non-interactive runs.",
  },
  {
    key: "--dangerously-bypass-approvals-and-sandbox, --yolo",
    type: "boolean",
    defaultValue: "false",
    description:
      "Run every command without approvals or sandboxing. Only use inside an externally hardened environment.",
  },
  {
    key: "--dangerously-bypass-hook-trust",
    type: "boolean",
    defaultValue: "false",
    description:
      "Run enabled hooks without requiring persisted hook trust for this invocation. Intended only for automation that already vets hook sources.",
  },
  {
    key: "--cd, -C",
    type: "path",
    description:
      "Set the working directory for the agent before it starts processing your request.",
  },
  {
    key: "--search",
    type: "boolean",
    defaultValue: "false",
    description:
      'Enable live web search (sets `web_search = "live"` instead of the default `"cached"`).',
  },
  {
    key: "--add-dir",
    type: "path",
    description:
      "Grant additional directories write access alongside the main workspace. Repeat for multiple paths.",
  },
  {
    key: "--no-alt-screen",
    type: "boolean",
    defaultValue: "false",
    description:
      "Disable alternate screen mode for the TUI (overrides `tui.alternate_screen` for this run).",
  },
  {
    key: "--remote",
    type: "ws://host:port | wss://host:port | unix:// | unix://PATH",
    description:
      "Connect the interactive TUI to a remote app-server endpoint over WebSocket or a Unix socket. Supported for `codex`, `codex resume`, and `codex fork`; other subcommands reject remote mode.",
  },
  {
    key: "--remote-auth-token-env",
    type: "ENV_VAR",
    description:
      "Read a bearer token from this environment variable and send it when connecting with `--remote`. Requires `--remote`; tokens are only sent over `wss://` URLs or local-only `ws://` URLs.",
  },
  {
    key: "--strict-config",
    type: "boolean",
    defaultValue: "false",
    description:
      "Error when `config.toml` contains fields this Codex version does not recognize. Supported by runtime commands such as `codex`, `exec`, `review`, `resume`, `fork`, `app-server`, `mcp-server`, and `exec-server`.",
  },
  {
    key: "--enable",
    type: "feature",
    description:
      "Force-enable a feature flag (translates to `-c features.<name>=true`). Repeatable.",
  },
  {
    key: "--disable",
    type: "feature",
    description:
      "Force-disable a feature flag (translates to `-c features.<name>=false`). Repeatable.",
  },
  {
    key: "--config, -c",
    type: "key=value",
    description:
      "Override configuration values. Values parse as TOML if possible; otherwise the literal string is used.",
  },
];

export const commandOverview = [
  {
    key: "codex",
    href: "/codex/cli/reference#codex-interactive",
    type: "stable",
    description:
      "Launch the terminal UI. Accepts the global flags above plus an optional prompt or image attachments.",
  },
  {
    key: "codex app-server",
    href: "/codex/cli/reference#codex-app-server",
    type: "experimental",
    description:
      "Launch the Codex app server for local development or debugging over stdio, WebSocket, or a Unix socket.",
  },
  {
    key: "codex remote-control",
    href: "/codex/cli/reference#codex-remote-control",
    type: "experimental",
    description:
      "Ensure the local app-server daemon is running with remote-control support enabled.",
  },
  {
    key: "codex app",
    href: "/codex/cli/reference#codex-app",
    type: "stable",
    description:
      "Launch the Codex desktop app on macOS or Windows. On macOS, Codex can open a workspace path; on Windows, Codex prints the path to open.",
  },
  {
    key: "codex debug app-server send-message-v2",
    href: "/codex/cli/reference#codex-debug-app-server-send-message-v2",
    type: "experimental",
    description:
      "Debug app-server by sending a single V2 message through the built-in test client.",
  },
  {
    key: "codex debug models",
    href: "/codex/cli/reference#codex-debug-models",
    type: "experimental",
    description:
      "Print the raw model catalog Codex sees, including an option to inspect only the bundled catalog.",
  },
  {
    key: "codex apply",
    href: "/codex/cli/reference#codex-apply",
    type: "stable",
    description:
      "Apply the latest diff generated by a Codex Cloud task to your local working tree. Alias: `codex a`.",
  },
  {
    key: "codex cloud",
    href: "/codex/cli/reference#codex-cloud",
    type: "experimental",
    description:
      "Browse or execute Codex Cloud tasks from the terminal without opening the TUI. Alias: `codex cloud-tasks`.",
  },
  {
    key: "codex completion",
    href: "/codex/cli/reference#codex-completion",
    type: "stable",
    description:
      "Generate shell completion scripts for Bash, Zsh, Fish, or PowerShell.",
  },
  {
    key: "codex doctor",
    href: "/codex/cli/reference#codex-doctor",
    type: "stable",
    description:
      "Generate a diagnostic report for local installation, config, auth, runtime, Git, terminal, app-server, and thread inventory issues.",
  },
  {
    key: "codex features",
    href: "/codex/cli/reference#codex-features",
    type: "stable",
    description:
      "List feature flags and persistently enable or disable them in `config.toml`.",
  },
  {
    key: "codex exec",
    href: "/codex/cli/reference#codex-exec",
    type: "stable",
    description:
      "Run Codex non-interactively. Alias: `codex e`. Stream results to stdout or JSONL and optionally resume previous sessions.",
  },
  {
    key: "codex execpolicy",
    href: "/codex/cli/reference#codex-execpolicy",
    type: "experimental",
    description:
      "Evaluate execpolicy rule files and see whether a command would be allowed, prompted, or blocked.",
  },
  {
    key: "codex login",
    href: "/codex/cli/reference#codex-login",
    type: "stable",
    description:
      "Authenticate Codex using ChatGPT OAuth, device auth, an API key, or an access token piped over stdin.",
  },
  {
    key: "codex logout",
    href: "/codex/cli/reference#codex-logout",
    type: "stable",
    description: "Remove stored authentication credentials.",
  },
  {
    key: "codex mcp",
    href: "/codex/cli/reference#codex-mcp",
    type: "experimental",
    description:
      "Manage Model Context Protocol servers (list, add, remove, authenticate).",
  },
  {
    key: "codex plugin marketplace",
    href: "/codex/cli/reference#codex-plugin-marketplace",
    type: "experimental",
    description:
      "Add, list, upgrade, or remove plugin marketplaces from Git or local sources.",
  },
  {
    key: "codex mcp-server",
    href: "/codex/cli/reference#codex-mcp-server",
    type: "experimental",
    description:
      "Run Codex itself as an MCP server over stdio. Useful when another agent consumes Codex.",
  },
  {
    key: "codex resume",
    href: "/codex/cli/reference#codex-resume",
    type: "stable",
    description:
      "Continue a previous interactive session by ID or resume the most recent conversation.",
  },
  {
    key: "codex fork",
    href: "/codex/cli/reference#codex-fork",
    type: "stable",
    description:
      "Fork a previous interactive session into a new thread, preserving the original transcript.",
  },
  {
    key: "codex sandbox",
    href: "/codex/cli/reference#codex-sandbox",
    type: "experimental",
    description:
      "Run arbitrary commands inside Codex-provided macOS, Linux, or Windows sandboxes.",
  },
  {
    key: "codex update",
    href: "/codex/cli/reference#codex-update",
    type: "stable",
    description:
      "Check for and apply a Codex CLI update when the installed release supports self-update.",
  },
];

export const execOptions = [
  {
    key: "PROMPT",
    type: "string | - (read stdin)",
    description:
      "Initial instruction for the task. Use `-` to pipe the prompt from stdin.",
  },
  {
    key: "--image, -i",
    type: "path[,path...]",
    description:
      "Attach images to the first message. Repeatable; supports comma-separated lists.",
  },
  {
    key: "--model, -m",
    type: "string",
    description: "Override the configured model for this run.",
  },
  {
    key: "--oss",
    type: "boolean",
    defaultValue: "false",
    description:
      "Use the local open source provider (requires a running Ollama instance).",
  },
  {
    key: "--sandbox, -s",
    type: "read-only | workspace-write | danger-full-access",
    description:
      "Sandbox policy for model-generated commands. Defaults to configuration.",
  },
  {
    key: "--profile, -p",
    type: "string",
    description:
      "Layer `$CODEX_HOME/profile-name.config.toml` on top of the base user config.",
  },
  {
    key: "--full-auto",
    type: "boolean",
    defaultValue: "false",
    description:
      "Deprecated compatibility flag. Prefer `--sandbox workspace-write`; Codex prints a warning when this flag is used.",
  },
  {
    key: "--dangerously-bypass-approvals-and-sandbox, --yolo",
    type: "boolean",
    defaultValue: "false",
    description:
      "Bypass approval prompts and sandboxing. Dangerous—only use inside an isolated runner.",
  },
  {
    key: "--dangerously-bypass-hook-trust",
    type: "boolean",
    defaultValue: "false",
    description:
      "Run enabled hooks without requiring persisted hook trust for this invocation. Intended only for automation that already vets hook sources.",
  },
  {
    key: "--cd, -C",
    type: "path",
    description: "Set the workspace root before executing the task.",
  },
  {
    key: "--skip-git-repo-check",
    type: "boolean",
    defaultValue: "false",
    description:
      "Allow running outside a Git repository (useful for one-off directories).",
  },
  {
    key: "--ephemeral",
    type: "boolean",
    defaultValue: "false",
    description: "Run without persisting session rollout files to disk.",
  },
  {
    key: "--ignore-user-config",
    type: "boolean",
    defaultValue: "false",
    description:
      "Do not load `$CODEX_HOME/config.toml`. Authentication still uses `CODEX_HOME`.",
  },
  {
    key: "--ignore-rules",
    type: "boolean",
    defaultValue: "false",
    description:
      "Do not load user or project execpolicy `.rules` files for this run.",
  },
  {
    key: "--output-schema",
    type: "path",
    description:
      "JSON Schema file describing the expected final response shape. Codex validates tool output against it.",
  },
  {
    key: "--color",
    type: "always | never | auto",
    defaultValue: "auto",
    description: "Control ANSI color in stdout.",
  },
  {
    key: "--json, --experimental-json",
    type: "boolean",
    defaultValue: "false",
    description:
      "Print newline-delimited JSON events instead of formatted text.",
  },
  {
    key: "--output-last-message, -o",
    type: "path",
    description:
      "Write the assistant’s final message to a file. Useful for downstream scripting.",
  },
  {
    key: "Resume subcommand",
    type: "codex exec resume [SESSION_ID]",
    description:
      "Resume an exec session by ID or add `--last` to continue the most recent session from the current working directory. Add `--all` to consider sessions from any directory. Accepts an optional follow-up prompt.",
  },
  {
    key: "-c, --config",
    type: "key=value",
    description:
      "Inline configuration override for the non-interactive run (repeatable).",
  },
];

export const appServerOptions = [
  {
    key: "--listen",
    type: "stdio:// | ws://IP:PORT | unix:// | unix://PATH | off",
    defaultValue: "stdio://",
    description:
      "Transport listener URL. Use `stdio://` for JSONL, `ws://IP:PORT` for a TCP WebSocket endpoint, `unix://` for the default Unix socket, `unix://PATH` for a custom Unix socket, or `off` to disable the local transport.",
  },
  {
    key: "--ws-auth",
    type: "capability-token | signed-bearer-token",
    description:
      "Authentication mode for app-server WebSocket clients. If omitted, WebSocket auth is disabled; non-local listeners warn during startup.",
  },
  {
    key: "--ws-token-file",
    type: "absolute path",
    description:
      "File containing the shared capability token. Use with `--ws-auth capability-token` unless you provide `--ws-token-sha256` instead.",
  },
  {
    key: "--ws-token-sha256",
    type: "hexadecimal SHA-256 digest",
    description:
      "Expected SHA-256 digest for capability-token authentication. Use instead of `--ws-token-file` when the client token comes from another source.",
  },
  {
    key: "--ws-shared-secret-file",
    type: "absolute path",
    description:
      "File containing the HMAC shared secret used to validate signed JWT bearer tokens. Required with `--ws-auth signed-bearer-token`.",
  },
  {
    key: "--ws-issuer",
    type: "string",
    description:
      "Expected `iss` claim for signed bearer tokens. Requires `--ws-auth signed-bearer-token`.",
  },
  {
    key: "--ws-audience",
    type: "string",
    description:
      "Expected `aud` claim for signed bearer tokens. Requires `--ws-auth signed-bearer-token`.",
  },
  {
    key: "--ws-max-clock-skew-seconds",
    type: "number",
    defaultValue: "30",
    description:
      "Clock skew allowance when validating signed bearer token `exp` and `nbf` claims. Requires `--ws-auth signed-bearer-token`.",
  },
  {
    key: "--analytics-default-enabled",
    type: "boolean",
    defaultValue: "false",
    description:
      "Defaults analytics to enabled for first-party app-server clients unless the user opts out in config.",
  },
];

export const appOptions = [
  {
    key: "PATH",
    type: "path",
    defaultValue: ".",
    description:
      "Workspace path for Codex Desktop. On macOS, Codex opens this path; on Windows, Codex prints the path.",
  },
  {
    key: "--download-url",
    type: "url",
    description:
      "Advanced override for the Codex desktop installer URL used during install.",
  },
];

export const debugAppServerSendMessageV2Options = [
  {
    key: "USER_MESSAGE",
    type: "string",
    description:
      "Message text sent to app-server through the built-in V2 test-client flow.",
  },
];

export const debugModelsOptions = [
  {
    key: "--bundled",
    type: "boolean",
    defaultValue: "false",
    description:
      "Skip refresh and print only the model catalog bundled with the current Codex binary.",
  },
];

export const doctorOptions = [
  {
    key: "--json",
    type: "boolean",
    defaultValue: "false",
    description: "Emit a redacted machine-readable support report.",
  },
  {
    key: "--summary",
    type: "boolean",
    defaultValue: "false",
    description: "Show grouped check rows and the final count summary only.",
  },
  {
    key: "--all",
    type: "boolean",
    defaultValue: "false",
    description: "Expand long lists in the detailed human-readable report.",
  },
  {
    key: "--no-color",
    type: "boolean",
    defaultValue: "false",
    description: "Disable ANSI color in human-readable output.",
  },
  {
    key: "--ascii",
    type: "boolean",
    defaultValue: "false",
    description:
      "Use ASCII status labels and separators in human-readable output.",
  },
];

export const resumeOptions = [
  {
    key: "SESSION_ID",
    type: "uuid",
    description:
      "Resume the specified session. Omit and use `--last` to continue the most recent session.",
  },
  {
    key: "--last",
    type: "boolean",
    defaultValue: "false",
    description:
      "Skip the picker and resume the most recent conversation from the current working directory.",
  },
  {
    key: "--all",
    type: "boolean",
    defaultValue: "false",
    description:
      "Include sessions outside the current working directory when selecting the most recent session.",
  },
];

export const featuresOptions = [
  {
    key: "List subcommand",
    type: "codex features list",
    description:
      "Show known feature flags, their maturity stage, and their effective state.",
  },
  {
    key: "Enable subcommand",
    type: "codex features enable <feature>",
    description:
      "Persistently enable a feature flag in `$CODEX_HOME/config.toml`.",
  },
  {
    key: "Disable subcommand",
    type: "codex features disable <feature>",
    description:
      "Persistently disable a feature flag in `$CODEX_HOME/config.toml`.",
  },
];

export const execResumeOptions = [
  {
    key: "SESSION_ID",
    type: "uuid",
    description:
      "Resume the specified session. Omit and use `--last` to continue the most recent session.",
  },
  {
    key: "--last",
    type: "boolean",
    defaultValue: "false",
    description:
      "Resume the most recent conversation from the current working directory.",
  },
  {
    key: "--all",
    type: "boolean",
    defaultValue: "false",
    description:
      "Include sessions outside the current working directory when selecting the most recent session.",
  },
  {
    key: "--image, -i",
    type: "path[,path...]",
    description:
      "Attach one or more images to the follow-up prompt. Separate multiple paths with commas or repeat the flag.",
  },
  {
    key: "PROMPT",
    type: "string | - (read stdin)",
    description:
      "Optional follow-up instruction sent immediately after resuming.",
  },
];

export const forkOptions = [
  {
    key: "SESSION_ID",
    type: "uuid",
    description:
      "Fork the specified session. Omit and use `--last` to fork the most recent session.",
  },
  {
    key: "--last",
    type: "boolean",
    defaultValue: "false",
    description:
      "Skip the picker and fork the most recent conversation automatically.",
  },
  {
    key: "--all",
    type: "boolean",
    defaultValue: "false",
    description:
      "Show sessions beyond the current working directory in the picker.",
  },
];

export const execpolicyOptions = [
  {
    key: "--rules, -r",
    type: "path (repeatable)",
    description:
      "Path to an execpolicy rule file to evaluate. Provide multiple flags to combine rules across files.",
  },
  {
    key: "--pretty",
    type: "boolean",
    defaultValue: "false",
    description: "Pretty-print the JSON result.",
  },
  {
    key: "COMMAND...",
    type: "var-args",
    description: "Command to be checked against the specified policies.",
  },
];

export const loginOptions = [
  {
    key: "--with-api-key",
    type: "boolean",
    description:
      "Read an API key from stdin (for example `printenv OPENAI_API_KEY | codex login --with-api-key`).",
  },
  {
    key: "--with-access-token",
    type: "boolean",
    description:
      "Read an access token from stdin (for example `printenv CODEX_ACCESS_TOKEN | codex login --with-access-token`).",
  },
  {
    key: "--device-auth",
    type: "boolean",
    description:
      "Use OAuth device code flow instead of launching a browser window.",
  },
  {
    key: "status subcommand",
    type: "codex login status",
    description:
      "Print the active authentication mode and exit with 0 when logged in.",
  },
];

export const applyOptions = [
  {
    key: "TASK_ID",
    type: "string",
    description:
      "Identifier of the Codex Cloud task whose diff should be applied.",
  },
];

export const sandboxMacOptions = [
  {
    key: "--profile, -p",
    type: "NAME",
    description:
      "Layer `$CODEX_HOME/NAME.config.toml` on top of the base user config.",
  },
  {
    key: "--permissions-profile",
    type: "NAME",
    description:
      "Apply a named permissions profile from the active configuration stack.",
  },
  {
    key: "--cd, -C",
    type: "DIR",
    description:
      "Working directory used for profile resolution and command execution. Requires `--permissions-profile`.",
  },
  {
    key: "--include-managed-config",
    type: "boolean",
    defaultValue: "false",
    description:
      "Include managed requirements while resolving an explicit permissions profile. Requires `--permissions-profile`.",
  },
  {
    key: "--allow-unix-socket",
    type: "path",
    description:
      "Allow the sandboxed command to bind or connect Unix sockets rooted at this path. Repeat to allow multiple paths.",
  },
  {
    key: "--log-denials",
    type: "boolean",
    defaultValue: "false",
    description:
      "Capture macOS sandbox denials with `log stream` while the command runs and print them after exit.",
  },
  {
    key: "--config, -c",
    type: "key=value",
    description:
      "Pass configuration overrides into the sandboxed run (repeatable).",
  },
  {
    key: "COMMAND...",
    type: "var-args",
    description:
      "Shell command to execute under macOS Seatbelt. Everything after `--` is forwarded.",
  },
];

export const sandboxLinuxOptions = [
  {
    key: "--profile, -p",
    type: "NAME",
    description:
      "Layer `$CODEX_HOME/NAME.config.toml` on top of the base user config.",
  },
  {
    key: "--permissions-profile",
    type: "NAME",
    description:
      "Apply a named permissions profile from the active configuration stack.",
  },
  {
    key: "--cd, -C",
    type: "DIR",
    description:
      "Working directory used for profile resolution and command execution. Requires `--permissions-profile`.",
  },
  {
    key: "--include-managed-config",
    type: "boolean",
    defaultValue: "false",
    description:
      "Include managed requirements while resolving an explicit permissions profile. Requires `--permissions-profile`.",
  },
  {
    key: "--config, -c",
    type: "key=value",
    description:
      "Configuration overrides applied before launching the sandbox (repeatable).",
  },
  {
    key: "COMMAND...",
    type: "var-args",
    description:
      "Command to execute under Landlock + seccomp. Provide the executable after `--`.",
  },
];

export const sandboxWindowsOptions = [
  {
    key: "--profile, -p",
    type: "NAME",
    description:
      "Layer `$CODEX_HOME/NAME.config.toml` on top of the base user config.",
  },
  {
    key: "--permissions-profile",
    type: "NAME",
    description:
      "Apply a named permissions profile from the active configuration stack.",
  },
  {
    key: "--cd, -C",
    type: "DIR",
    description:
      "Working directory used for profile resolution and command execution. Requires `--permissions-profile`.",
  },
  {
    key: "--include-managed-config",
    type: "boolean",
    defaultValue: "false",
    description:
      "Include managed requirements while resolving an explicit permissions profile. Requires `--permissions-profile`.",
  },
  {
    key: "--config, -c",
    type: "key=value",
    description:
      "Configuration overrides applied before launching the sandbox (repeatable).",
  },
  {
    key: "COMMAND...",
    type: "var-args",
    description:
      "Command to execute under the native Windows sandbox. Provide the executable after `--`.",
  },
];

export const completionOptions = [
  {
    key: "SHELL",
    type: "bash | zsh | fish | power-shell | elvish",
    defaultValue: "bash",
    description: "Shell to generate completions for. Output prints to stdout.",
  },
];

export const cloudExecOptions = [
  {
    key: "QUERY",
    type: "string",
    description:
      "Task prompt. If omitted, Codex prompts interactively for details.",
  },
  {
    key: "--env",
    type: "ENV_ID",
    description:
      "Target Codex Cloud environment identifier (required). Use `codex cloud` to list options.",
  },
  {
    key: "--attempts",
    type: "1-4",
    defaultValue: "1",
    description:
      "Number of assistant attempts (best-of-N) Codex Cloud should run.",
  },
];

export const cloudListOptions = [
  {
    key: "--env",
    type: "ENV_ID",
    description: "Filter tasks by environment identifier.",
  },
  {
    key: "--limit",
    type: "1-20",
    defaultValue: "20",
    description: "Maximum number of tasks to return.",
  },
  {
    key: "--cursor",
    type: "string",
    description: "Pagination cursor returned by a previous request.",
  },
  {
    key: "--json",
    type: "boolean",
    defaultValue: "false",
    description: "Emit machine-readable JSON instead of plain text.",
  },
];

export const mcpCommands = [
  {
    key: "list",
    type: "--json",
    description:
      "List configured MCP servers. Add `--json` for machine-readable output.",
  },
  {
    key: "get <name>",
    type: "--json",
    description:
      "Show a specific server configuration. `--json` prints the raw config entry.",
  },
  {
    key: "add <name>",
    type: "-- <command...> | --url <value>",
    description:
      "Register a server using a stdio launcher command or a streamable HTTP URL. Supports `--env KEY=VALUE` for stdio transports.",
  },
  {
    key: "remove <name>",
    description: "Delete a stored MCP server definition.",
  },
  {
    key: "login <name>",
    type: "--scopes scope1,scope2",
    description:
      "Start an OAuth login for a streamable HTTP server (servers that support OAuth only).",
  },
  {
    key: "logout <name>",
    description:
      "Remove stored OAuth credentials for a streamable HTTP server.",
  },
];

export const mcpAddOptions = [
  {
    key: "COMMAND...",
    type: "stdio transport",
    description:
      "Executable plus arguments to launch the MCP server. Provide after `--`.",
  },
  {
    key: "--env KEY=VALUE",
    type: "repeatable",
    description:
      "Environment variable assignments applied when launching a stdio server.",
  },
  {
    key: "--url",
    type: "https://…",
    description:
      "Register a streamable HTTP server instead of stdio. Mutually exclusive with `COMMAND...`.",
  },
  {
    key: "--bearer-token-env-var",
    type: "ENV_VAR",
    description:
      "Environment variable whose value is sent as a bearer token when connecting to a streamable HTTP server.",
  },
  {
    key: "--oauth-client-id",
    type: "CLIENT_ID",
    description:
      "OAuth client identifier for a streamable HTTP MCP server. Requires `--url`.",
  },
  {
    key: "--oauth-resource",
    type: "RESOURCE",
    description:
      "OAuth resource parameter to include during login for a streamable HTTP MCP server. Requires `--url`.",
  },
];

export const marketplaceCommands = [
  {
    key: "add <source>",
    type: "[--ref REF] [--sparse PATH]",
    description:
      "Install a plugin marketplace from GitHub shorthand, a Git URL, an SSH URL, or a local marketplace root directory. `--sparse` is supported only for Git sources and can be repeated.",
  },
  {
    key: "list",
    description:
      "Show plugin marketplaces Codex is currently considering and the root path for each marketplace.",
  },
  {
    key: "upgrade [marketplace-name]",
    description:
      "Refresh one configured Git marketplace, or all configured Git marketplaces when no name is provided.",
  },
  {
    key: "remove <marketplace-name>",
    description: "Remove a configured plugin marketplace.",
  },
];

## How to read this reference

This page catalogs every documented Codex CLI command and flag. Use the interactive tables to search by key or description. Each section indicates whether the option is stable or experimental and calls out risky combinations.

The CLI inherits most defaults from <code>~/.codex/config.toml</code>. Any
  <code>-c key=value</code> overrides you pass at the command line take
  precedence for that invocation. See [Config
  basics](https://developers.openai.com/codex/config-basic#configuration-precedence) for more information.

## Global flags

<ConfigTable client:load options={globalFlagOptions} />

These options apply to the base `codex` command. Most propagate to commands;
see the notes above or the relevant command help for exceptions. For propagated
flags, follow the relevant command help. For example, `codex exec --oss ...`
applies `--oss` to `exec`.

## Command overview

The Maturity column uses feature maturity labels such as Experimental, Beta,
  and Stable. See [Feature Maturity](https://developers.openai.com/codex/feature-maturity) for how to
  interpret these labels.

<ConfigTable
  client:load
  options={commandOverview}
  secondColumnTitle="Maturity"
  secondColumnVariant="maturity"
/>

## Command details

### `codex` (interactive)

Running `codex` with no subcommand launches the interactive terminal UI (TUI). The agent accepts the global flags above plus image attachments. Web search defaults to cached mode; use `--search` to switch to live browsing. For low-friction local work, use `--sandbox workspace-write --ask-for-approval on-request`.

Use `--remote ws://host:port` or `--remote wss://host:port` to connect the TUI to an app server started with `codex app-server --listen ws://IP:PORT`. For a local Unix socket, use `--remote unix://` for the default socket or `--remote unix://PATH` for an explicit path. Add `--remote-auth-token-env <ENV_VAR>` when the server requires a bearer token for WebSocket authentication.

### `codex app-server`

Launch the Codex app server locally. This is primarily for development and debugging and may change without notice.

<ConfigTable client:load options={appServerOptions} />

`codex app-server --listen stdio://` keeps the default JSONL-over-stdio behavior. `--listen ws://IP:PORT` enables WebSocket transport for app-server clients. The server accepts `ws://` listen URLs; use TLS termination or a secure proxy when clients connect with `wss://`. Use `--listen unix://` to accept WebSocket handshakes on Codex's default Unix socket, or `--listen unix:///absolute/path.sock` to choose a socket path. If you generate schemas for client bindings, add `--experimental` to include gated fields and methods.

### `codex remote-control`

Ensure the app-server daemon is running with remote-control support enabled.
Managed remote-control clients and SSH remote workflows use this command; it's
not a replacement for `codex app-server --listen` when you are building a local
protocol client.

### `codex app`

Launch Codex Desktop from the terminal on macOS or Windows. On macOS, Codex can open a specific workspace path; on Windows, Codex prints the path to open.

<ConfigTable client:load options={appOptions} />

`codex app` opens an installed Codex Desktop app, or starts the installer when
the app is missing. On macOS, Codex opens the provided workspace path; on
Windows, it prints the path to open after installation.

### `codex debug app-server send-message-v2`

Send one message through app-server's V2 thread/turn flow using the built-in app-server test client.

<ConfigTable client:load options={debugAppServerSendMessageV2Options} />

This debug flow initializes with `experimentalApi: true`, starts a thread, sends a turn, and streams server notifications. Use it to reproduce and inspect app-server protocol behavior locally.

### `codex debug models`

Print the raw model catalog Codex sees as JSON.

<ConfigTable client:load options={debugModelsOptions} />

Use `--bundled` when you want to inspect only the catalog bundled with the current binary, without refreshing from the remote models endpoint.

### `codex apply`

Apply the most recent diff from a Codex cloud task to your local repository. You must authenticate and have access to the task.

<ConfigTable client:load options={applyOptions} />

Codex prints the patched files and exits non-zero if `git apply` fails (for example, due to conflicts).

### `codex cloud`

Interact with Codex cloud tasks from the terminal. The default command opens an interactive picker; `codex cloud exec` submits a task directly, and `codex cloud list` returns recent tasks for scripting or quick inspection.

<ConfigTable client:load options={cloudExecOptions} />

Authentication follows the same credentials as the main CLI. Codex exits non-zero if the task submission fails.

#### `codex cloud list`

List recent cloud tasks with optional filtering and pagination.

<ConfigTable client:load options={cloudListOptions} />

Plain-text output prints a task URL followed by status details. Use `--json` for automation. The JSON payload contains a `tasks` array plus an optional `cursor` value. Each task includes `id`, `url`, `title`, `status`, `updated_at`, `environment_id`, `environment_label`, `summary`, `is_review`, and `attempt_total`.

### `codex completion`

Generate shell completion scripts and redirect the output to the appropriate location, for example `codex completion zsh > "${fpath[1]}/_codex"`.

<ConfigTable client:load options={completionOptions} />

### `codex doctor`

Generate a local diagnostic report before filing a support issue or
while investigating a broken Codex installation. The report checks installation,
configuration, authentication, runtime, Git, terminal, app-server, and thread
inventory health.

<ConfigTable client:load options={doctorOptions} />

### `codex features`

Manage feature flags stored in `$CODEX_HOME/config.toml`. The `enable` and
`disable` commands persist changes so they apply to future sessions. The
`features` subcommand doesn't accept `--profile`.

<ConfigTable client:load options={featuresOptions} />

### `codex exec`

Use `codex exec` (or the short form `codex e`) for scripted or CI-style runs that should finish without human interaction.

<ConfigTable client:load options={execOptions} />

Codex writes formatted output by default. Add `--json` to receive newline-delimited JSON events (one per state change). The optional `resume` subcommand lets you continue non-interactive tasks. Use `--last` to pick the most recent session from the current working directory, or add `--all` to search across all sessions:

<ConfigTable client:load options={execResumeOptions} />

### `codex execpolicy`

Check `execpolicy` rule files before you save them. `codex execpolicy check` accepts one or more `--rules` flags (for example, files under `~/.codex/rules`) and emits JSON showing the strictest decision and any matching rules. Add `--pretty` to format the output. The `execpolicy` command is currently in preview.

<ConfigTable client:load options={execpolicyOptions} />

### `codex login`

Authenticate the CLI with a ChatGPT account, API key, or access token. With no flags, Codex opens a browser for the ChatGPT OAuth flow.

<ConfigTable client:load options={loginOptions} />

`codex login status` exits with `0` when credentials are present, which is helpful in automation scripts.

### `codex logout`

Remove saved credentials for both API key and ChatGPT authentication. This command has no flags.

### `codex mcp`

Manage Model Context Protocol server entries stored in `~/.codex/config.toml`.

<ConfigTable client:load options={mcpCommands} />

The `add` subcommand supports both stdio and streamable HTTP transports:

<ConfigTable client:load options={mcpAddOptions} />

OAuth actions (`login`, `logout`) only work with streamable HTTP servers (and only when the server supports OAuth).

### `codex plugin marketplace`

Manage plugin marketplace sources that Codex can browse and install from.

<ConfigTable client:load options={marketplaceCommands} />

`codex plugin marketplace add` accepts GitHub shorthand such as `owner/repo` or
`owner/repo@ref`, HTTP or HTTPS Git URLs, SSH Git URLs, and local marketplace
root directories. Use `--ref` to pin a Git ref, and repeat `--sparse PATH` to
use a sparse checkout for Git-backed marketplace repositories.

`codex plugin marketplace list` prints in-scope marketplace names and roots,
including implicitly discovered default marketplaces and configured marketplace
snapshots.

### `codex mcp-server`

Run Codex as an MCP server over stdio so that other tools can connect. This command inherits global configuration overrides and exits when the downstream client closes the connection.

### `codex resume`

Continue an interactive session by ID or resume the most recent conversation. `codex resume` scopes `--last` to the current working directory unless you pass `--all`. It accepts the same global flags as `codex`, including model and sandbox overrides.

<ConfigTable client:load options={resumeOptions} />

### `codex fork`

Fork a previous interactive session into a new thread. By default, `codex fork` opens the session picker; add `--last` to fork your most recent session instead.

<ConfigTable client:load options={forkOptions} />

### `codex sandbox`

Use the sandbox helper to run a command under the same policies Codex uses internally.

#### macOS seatbelt

<ConfigTable client:load options={sandboxMacOptions} />

#### Linux Landlock

<ConfigTable client:load options={sandboxLinuxOptions} />

#### Windows

<ConfigTable client:load options={sandboxWindowsOptions} />

### `codex update`

Check for and apply a Codex CLI update when the installed release supports self-update. Debug builds print a message telling you to install a release build instead.

## Flag combinations and safety tips

- Use `--sandbox workspace-write` for unattended local work that can stay inside the workspace, and avoid `--dangerously-bypass-approvals-and-sandbox` unless you are inside a dedicated sandbox VM.
- When you need to grant Codex write access to more directories, prefer `--add-dir` rather than forcing `--sandbox danger-full-access`.
- Pair `--json` with `--output-last-message` in CI to capture machine-readable progress and a final natural-language summary.

## Related resources

- [Codex CLI overview](https://developers.openai.com/codex/cli): installation, upgrades, and quick tips.
- [Config basics](https://developers.openai.com/codex/config-basic): persist defaults like the model and provider.
- [Advanced Config](https://developers.openai.com/codex/config-advanced): profiles, providers, sandbox tuning, and integrations.
- [AGENTS.md](https://developers.openai.com/codex/guides/agents-md): conceptual overview of Codex agent capabilities and best practices.


---


# Slash commands in Codex CLI

Slash commands give you fast, keyboard-first control over Codex. Type `/` in
the composer to open the slash popup, choose a command, and Codex will perform
actions such as switching models, adjusting permissions, or summarizing long
conversations without leaving the terminal.

This guide shows you how to:

- Find the right built-in slash command for a task
- Steer an active session with commands like `/model`, `/fast`,
  `/personality`, `/permissions`, `/approve`, `/raw`, `/agent`, and `/status`

## Built-in slash commands

Codex ships with the following commands. Open the slash popup and start typing
the command name to filter the list.

When a task is already running, you can type a slash command and press `Tab` to
queue it for the next turn. Codex parses queued slash commands when they run, so
command menus and errors appear after the current turn finishes. Slash
completion still works before you queue the command.

| Command                                                                         | Purpose                                                         | When to use it                                                                                             |
| ------------------------------------------------------------------------------- | --------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| [`/permissions`](#update-permissions-with-permissions)                          | Set what Codex can do without asking first.                     | Relax or tighten approval requirements mid-session, such as switching between Auto and Read Only.          |
| [`/ide`](#include-ide-context-with-ide)                                         | Include open files, current selection, and other IDE context.   | Pull editor context into the next prompt without re-explaining what's open in your IDE.                    |
| [`/keymap`](#remap-tui-shortcuts-with-keymap)                                   | Remap TUI keyboard shortcuts.                                   | Inspect and persist custom shortcut bindings in `config.toml`.                                             |
| [`/vim`](#toggle-vim-mode-with-vim)                                             | Toggle Vim mode for the composer.                               | Switch between Vim normal/insert behavior and the default composer editing mode.                           |
| [`/sandbox-add-read-dir`](#grant-sandbox-read-access-with-sandbox-add-read-dir) | Grant sandbox read access to an extra directory (Windows only). | Unblock commands that need to read an absolute directory path outside the current readable roots.          |
| [`/agent`](#switch-agent-threads-with-agent)                                    | Switch the active agent thread.                                 | Inspect or continue work in a spawned subagent thread.                                                     |
| [`/apps`](#browse-apps-with-apps)                                               | Browse apps (connectors) and insert them into your prompt.      | Attach an app as `$app-slug` before asking Codex to use it.                                                |
| [`/plugins`](#browse-plugins-with-plugins)                                      | Browse installed and discoverable plugins.                      | Inspect plugin tools, install suggested plugins, or manage plugin availability.                            |
| [`/hooks`](#review-hooks-with-hooks)                                            | Review lifecycle hooks.                                         | Inspect configured hooks, trust new or changed hooks, or disable non-managed hooks before they run.        |
| [`/clear`](#clear-the-terminal-and-start-a-new-chat-with-clear)                 | Clear the terminal and start a fresh chat.                      | Reset the visible UI and conversation together when you want a fresh start.                                |
| [`/compact`](#keep-transcripts-lean-with-compact)                               | Summarize the visible conversation to free tokens.              | Use after long runs so Codex retains key points without blowing the context window.                        |
| [`/copy`](#copy-the-latest-response-with-copy)                                  | Copy the latest completed Codex output.                         | Grab the latest finished response or plan text without manually selecting it. You can also press `Ctrl+O`. |
| [`/diff`](#review-changes-with-diff)                                            | Show the Git diff, including files Git isn't tracking yet.      | Review Codex's edits before you commit or run tests.                                                       |
| [`/exit`](#exit-the-cli-with-quit-or-exit)                                      | Exit the CLI (same as `/quit`).                                 | Alternative spelling; both commands exit the session.                                                      |
| [`/experimental`](#toggle-experimental-features-with-experimental)              | Toggle experimental features.                                   | Enable optional features such as subagents from the CLI.                                                   |
| [`/approve`](#approve-an-auto-review-denial-with-approve)                       | Approve one retry of a recent auto review denial.               | Retry a command or action that the auto reviewer denied.                                                   |
| [`/memories`](#configure-memories-with-memories)                                | Configure memory use and generation.                            | Turn memory injection or memory generation on or off without leaving the TUI.                              |
| [`/skills`](#use-skills-with-skills)                                            | Browse and use skills.                                          | Improve task-specific behavior by selecting a relevant local skill.                                        |
| [`/hooks`](#view-lifecycle-hooks-with-hooks)                                    | View and manage lifecycle hooks.                                | Inspect hook configuration loaded into the current session.                                                |
| [`/feedback`](#send-feedback-with-feedback)                                     | Send logs to the Codex maintainers.                             | Report issues or share diagnostics with support.                                                           |
| [`/init`](#generate-agentsmd-with-init)                                         | Generate an `AGENTS.md` scaffold in the current directory.      | Capture persistent instructions for the repository or subdirectory you're working in.                      |
| [`/logout`](#sign-out-with-logout)                                              | Sign out of Codex.                                              | Clear local credentials when using a shared machine.                                                       |
| [`/mcp`](#list-mcp-tools-with-mcp)                                              | List configured Model Context Protocol (MCP) tools.             | Check which external tools Codex can call during the session; add `verbose` for server details.            |
| [`/mention`](#highlight-files-with-mention)                                     | Attach a file to the conversation.                              | Point Codex at specific files or folders you want it to inspect next.                                      |
| [`/model`](#set-the-active-model-with-model)                                    | Choose the active model (and reasoning effort, when available). | Switch between general-purpose models (`gpt-4.1-mini`) and deeper reasoning models before running a task.  |
| [`/fast`](#toggle-fast-mode-with-fast)                                          | Toggle a Fast service tier when the model catalog exposes one.  | Turn the current model's Fast tier on or off, or check whether the thread is using it.                     |
| [`/plan`](#switch-to-plan-mode-with-plan)                                       | Switch to plan mode and optionally send a prompt.               | Ask Codex to propose an execution plan before implementation work starts.                                  |
| [`/goal`](#set-or-view-a-task-goal-with-goal)                                   | Set, pause, resume, view, or clear a task goal.                 | Give Codex a persistent target to track while a larger task runs.                                          |
| [`/personality`](#set-a-communication-style-with-personality)                   | Choose a communication style for responses.                     | Make Codex more concise, more explanatory, or more collaborative without changing your instructions.       |
| [`/ps`](#check-background-terminals-with-ps)                                    | Show experimental background terminals and their recent output. | Check long-running commands without leaving the main transcript.                                           |
| [`/stop`](#stop-background-terminals-with-stop)                                 | Stop all background terminals.                                  | Cancel background terminal work started by the current session.                                            |
| [`/fork`](#fork-the-current-conversation-with-fork)                             | Fork the current conversation into a new thread.                | Branch the active session to explore a new approach without losing the current transcript.                 |
| [`/side`, `/btw`](#start-a-side-conversation-with-side)                         | Start an ephemeral side conversation.                           | Ask a focused follow-up without disrupting the main thread's transcript.                                   |
| [`/raw`](#toggle-raw-scrollback-with-raw)                                       | Toggle raw scrollback mode.                                     | Make terminal selection and copying less formatted while reviewing long output.                            |
| [`/resume`](#resume-a-saved-conversation-with-resume)                           | Resume a saved conversation from your session list.             | Continue work from a previous CLI session without starting over.                                           |
| [`/new`](#start-a-new-conversation-with-new)                                    | Start a new conversation inside the same CLI session.           | Reset the chat context without leaving the CLI when you want a fresh prompt in the same repo.              |
| [`/quit`](#exit-the-cli-with-quit-or-exit)                                      | Exit the CLI.                                                   | Leave the session immediately.                                                                             |
| [`/review`](#ask-for-a-working-tree-review-with-review)                         | Ask Codex to review your working tree.                          | Run after Codex completes work or when you want a second set of eyes on local changes.                     |
| [`/status`](#inspect-the-session-with-status)                                   | Display session configuration and token usage.                  | Confirm the active model, approval policy, writable roots, and remaining context capacity.                 |
| [`/debug-config`](#inspect-config-layers-with-debug-config)                     | Print config layer and requirements diagnostics.                | Debug precedence and policy requirements, including experimental network constraints.                      |
| [`/statusline`](#configure-footer-items-with-statusline)                        | Configure TUI status-line fields interactively.                 | Pick and reorder footer items (model/context/limits/git/tokens/session) and persist in config.toml.        |
| [`/title`](#configure-terminal-title-items-with-title)                          | Configure terminal window or tab title fields interactively.    | Pick and reorder title items such as project, status, thread, branch, model, and task progress.            |
| [`/theme`](#choose-a-syntax-theme-with-theme)                                   | Choose a syntax-highlighting theme.                             | Preview and persist a terminal syntax-highlighting theme.                                                  |

`/quit` and `/exit` both exit the CLI. Use them only after you have saved or
committed any important work.

Use `/permissions` to adjust what Codex can do without asking first. Use
`/approve` only when you need to retry a recent action that automatic review
denied.

## Control your session with slash commands

The following workflows keep your session on track without restarting Codex.

### Set the active model with `/model`

1. Start Codex and open the composer.
2. Type `/model` and press Enter.
3. Choose a model such as `gpt-4.1-mini` or `gpt-4.1` from the popup.

Expected: Codex confirms the new model in the transcript. Run `/status` to verify the change.

### Toggle Fast mode with `/fast`

1. Type `/fast on`, `/fast off`, or `/fast status`.
2. If you want the setting to persist, confirm the update when Codex offers to save it.

Expected: Codex reports whether the current model's Fast service tier is on or
off for the current thread. In the TUI footer, you can also show a Fast mode
status-line item with `/statusline`.

Fast tier commands are catalog-driven. If the current model doesn't advertise a
Fast tier, Codex won't show `/fast`.

### Set a communication style with `/personality`

Use `/personality` to change how Codex communicates without rewriting your prompt.

1. In an active conversation, type `/personality` and press Enter.
2. Choose a style from the popup.

Expected: Codex confirms the new style in the transcript and uses it for later
responses in the thread.

Codex supports `friendly`, `pragmatic`, and `none` personalities. Use `none`
to disable personality instructions.

If the active model doesn't support personality-specific instructions, Codex hides this command.

### Switch to plan mode with `/plan`

1. Type `/plan` and press Enter to switch the active conversation into plan
   mode.
2. Optional: provide inline prompt text (for example, `/plan Propose a
migration plan for this service`).
3. You can paste content or attach images while using inline `/plan` arguments.

Expected: Codex enters plan mode and uses your optional inline prompt as the first planning request.

While a task is already running, `/plan` is temporarily unavailable.

### Set or view a task goal with `/goal`

1. Type `/goal <objective>` to set the goal, for example `/goal Finish the migration and keep tests green`.
2. Type `/goal` to view the current goal.
3. Use `/goal pause`, `/goal resume`, or `/goal clear` to pause, resume, or remove it.

Expected: Codex keeps the goal attached to the active thread while work continues.

Goal objectives must be non-empty and at most 4,000 characters. For longer
instructions, put the details in a file and point the goal at that file.

### Toggle experimental features with `/experimental`

1. Type `/experimental` and press Enter.
2. Toggle the features you want (for example, Apps or Smart Approvals), then restart Codex if the prompt asks you to.

Expected: Codex saves your feature choices to config and applies them on restart.

### Approve an auto review denial with `/approve`

Use `/approve` when the automatic reviewer denied a recent action and you want
Codex to retry it once.

1. Type `/approve`.
2. Confirm the retry when Codex shows the relevant denied action.

Expected: Codex retries that denied action once under the current session
policy.

### Configure memories with `/memories`

1. Type `/memories`.
2. Choose whether Codex should use existing memories, generate new memories, or
   keep memory behavior disabled.

Expected: Codex updates the relevant memory settings for future sessions.

### Use skills with `/skills`

1. Type `/skills`.
2. Pick the skill you want Codex to apply.

Expected: Codex inserts the selected skill context so the next request follows
that skill's instructions.

### View lifecycle hooks with `/hooks`

1. Type `/hooks`.
2. Review the loaded lifecycle hook configuration.

Expected: Codex shows the hooks that can run in the current session.

### Clear the terminal and start a new chat with `/clear`

1. Type `/clear` and press Enter.

Expected: Codex clears the terminal, resets the visible transcript, and starts
a fresh chat in the same CLI session.

Unlike <kbd>Ctrl</kbd>+<kbd>L</kbd>, `/clear` starts a new conversation.

<kbd>Ctrl</kbd>+<kbd>L</kbd> only clears the terminal view and keeps the current
chat. Codex disables both actions while a task is in progress.

### Update permissions with `/permissions`

1. Type `/permissions` and press Enter.
2. Select the approval preset that matches your comfort level, for example
   `Auto` for hands-off runs or `Read Only` to review edits. When named
   permission profiles are active, the picker also shows configured custom
   profiles and their descriptions.

Expected: Codex announces the updated policy. Future actions respect the
updated approval mode until you change it again.

### Include IDE context with `/ide`

1. Type `/ide`.
2. Add optional inline text if you want to explain what Codex should do with the
   current IDE selection or open files.

Expected: Codex includes available IDE context in the next prompt.

### Toggle Vim mode with `/vim`

1. Type `/vim`.
2. Continue editing in the composer.

Expected: Codex toggles composer Vim mode for the current session. To make Vim
mode the default for new sessions, set `tui.vim_mode_default = true` in
`config.toml`.

### Copy the latest response with `/copy`

1. Type `/copy` and press Enter.

Expected: Codex copies the latest completed Codex output to your clipboard.

If a turn is still running, `/copy` uses the latest completed output instead of
the in-progress response. The command is unavailable before the first completed
Codex output and immediately after a rollback.

You can also press <kbd>Ctrl</kbd>+<kbd>O</kbd> from the main TUI to copy the
latest completed response without opening the slash command menu.

### Toggle raw scrollback with `/raw`

1. Type `/raw`, `/raw on`, or `/raw off`.

Expected: Codex toggles raw scrollback mode, which makes terminal selection and
copying more direct. You can also use the default <kbd>Alt</kbd>+<kbd>R</kbd>
binding or persist the default with `tui.raw_output_mode = true`.

### Grant sandbox read access with `/sandbox-add-read-dir`

This command is available only when running the CLI natively on Windows.

1. Type `/sandbox-add-read-dir C:\absolute\directory\path` and press Enter.
2. Confirm the path is an existing absolute directory.

Expected: Codex refreshes the Windows sandbox policy and grants read access to
that directory for later commands that run in the sandbox.

### Inspect the session with `/status`

1. In any conversation, type `/status`.
2. Review the output for the active model, approval policy, writable roots, and
   current token usage. When the TUI connects remotely, the output also
   shows the remote address and the server version.

Expected: Codex prints a summary confirming that it's operating where you
expect.

### Inspect config layers with `/debug-config`

1. Type `/debug-config`.
2. Review the output for config layer order (lowest precedence first), on/off
   state, and policy sources.

Expected: Codex prints layer diagnostics plus policy details such as
`allowed_approval_policies`, `allowed_sandbox_modes`, `mcp_servers`, `rules`,
`enforce_residency`, and `experimental_network` when configured.

Use this output to debug why an effective setting differs from `config.toml`.

### Configure footer items with `/statusline`

1. Type `/statusline`.
2. Use the picker to toggle and reorder items, then confirm.

Expected: The footer status line updates immediately and persists to
`tui.status_line` in `config.toml`.

Available status-line items include model, model+reasoning, context stats, rate
limits, git branch, token counters, session id, current directory/project root,
and Codex version.

### Configure terminal title items with `/title`

1. Type `/title`.
2. Use the picker to toggle and reorder items, then confirm.

Expected: The terminal window or tab title updates immediately and persists to
`tui.terminal_title` in `config.toml`.

Available title items include app name, project, spinner, status, thread, git
branch, model, and task progress.

### Choose a syntax theme with `/theme`

1. Type `/theme`.
2. Preview a theme from the picker, then confirm.

Expected: Codex updates syntax highlighting and persists the choice to
`tui.theme` in `config.toml`.

### Remap TUI shortcuts with `/keymap`

Use `/keymap` to inspect, update, and persist keyboard shortcut bindings for the TUI.

1. Type `/keymap`.
2. Pick the shortcut context and action you want to change.
3. Enter the new binding or remove the existing one.

Expected: Codex updates the active keymap and writes the custom binding to `tui.keymap` in `config.toml`.

Key bindings use names such as `ctrl-a`, `shift-enter`, and `page-down`. Context-specific bindings override `tui.keymap.global`; an empty binding list unbinds the action.

### Check background terminals with `/ps`

1. Type `/ps`.
2. Review the list of background terminals and their status.

Expected: Codex shows each background terminal's command plus up to three
recent, non-empty output lines so you can gauge progress at a glance.

Background terminals appear when `unified_exec` is in use; otherwise, the list may be empty.

### Stop background terminals with `/stop`

1. Type `/stop`.
2. Confirm if Codex asks before stopping the listed terminals.

Expected: Codex stops all background terminals for the current session. `/clean`
is still available as an alias for `/stop`.

### Keep transcripts lean with `/compact`

1. After a long exchange, type `/compact`.
2. Confirm when Codex offers to summarize the conversation so far.

Expected: Codex replaces earlier turns with a concise summary, freeing context
while keeping critical details.

### Review changes with `/diff`

1. Type `/diff` to inspect the Git diff.
2. Scroll through the output inside the CLI to review edits and added files.

Expected: Codex shows changes you've staged, changes you haven't staged yet,
and files Git hasn't started tracking, so you can decide what to keep.

### Highlight files with `/mention`

1. Type `/mention` followed by a path, for example `/mention src/lib/api.ts`.
2. Select the matching result from the popup.

Expected: Codex adds the file to the conversation, ensuring follow-up turns reference it directly.

### Start a new conversation with `/new`

1. Type `/new` and press Enter.

Expected: Codex starts a fresh conversation in the same CLI session, so you
can switch tasks without leaving your terminal.

Unlike `/clear`, `/new` doesn't clear the current terminal view first.

### Resume a saved conversation with `/resume`

1. Type `/resume` and press Enter.
2. Choose the session you want from the saved-session picker.

Expected: Codex reloads the selected conversation's transcript so you can pick
up where you left off, keeping the original history intact.

### Fork the current conversation with `/fork`

1. Type `/fork` and press Enter.

Expected: Codex clones the current conversation into a new thread with a fresh
ID, leaving the original transcript untouched so you can explore an alternative
approach in parallel.

If you need to fork a saved session instead of the current one, run
`codex fork` in your terminal to open the session picker.

### Start a side conversation with `/side`

Use `/side` to start an ephemeral fork from the current conversation without switching away from the main task.

1. Type `/side` to open a side conversation.
2. Optionally add inline text, for example `/side Check whether this plan has an obvious risk`.
3. Return to the parent thread after the focused detour finishes.

Expected: Codex opens a side conversation whose transcript is separate from the parent thread. While you are in side mode, the TUI continues to show parent-thread status so you can see whether the main task is still running.

`/side` is unavailable inside another side conversation and during review mode.

### Generate `AGENTS.md` with `/init`

1. Run `/init` in the directory where you want Codex to look for persistent instructions.
2. Review the generated `AGENTS.md`, then edit it to match your repository conventions.

Expected: Codex creates an `AGENTS.md` scaffold you can refine and commit for
future sessions.

### Ask for a working tree review with `/review`

1. Type `/review`.
2. Follow up with `/diff` if you want to inspect the exact file changes.

Expected: Codex summarizes issues it finds in your working tree, focusing on
behavior changes and missing tests. It uses the current session model unless
you set `review_model` in `config.toml`.

### List MCP tools with `/mcp`

1. Type `/mcp`.
2. Review the list to confirm which MCP servers and tools are available.

Expected: You see the configured Model Context Protocol (MCP) tools Codex can call in this session.

Use `/mcp verbose` to include detailed server diagnostics. If you pass anything other than `verbose`, Codex shows the command usage.

### Browse apps with `/apps`

1. Type `/apps`.
2. Pick an app from the list.

Expected: Codex inserts the app mention into the composer as `$app-slug`, so
you can immediately ask Codex to use it.

### Browse plugins with `/plugins`

1. Type `/plugins`.
2. Choose a marketplace tab, then pick a plugin to inspect its capabilities or available actions.

Expected: Codex opens the plugin browser so you can review installed plugins,
discoverable plugins that your configuration allows, and installed plugin state.
Press <kbd>Space</kbd> on an installed plugin to toggle its enabled state.

### Review hooks with `/hooks`

1. Type `/hooks`.
2. Choose a hook event to inspect the matching handlers.
3. Trust, disable, or re-enable non-managed hooks as needed.

Expected: Codex opens the hook browser so you can review configured lifecycle
hooks. Managed hooks appear as managed and can't be disabled from the user hook
browser.

### Switch agent threads with `/agent`

1. Type `/agent` and press Enter.
2. Select the thread you want from the picker.

Expected: Codex switches the active thread so you can inspect or continue that
agent's work.

### Send feedback with `/feedback`

1. Type `/feedback` and press Enter.
2. Follow the prompts to include logs or diagnostics.

Expected: Codex collects the requested diagnostics and submits them to the
maintainers.

### Sign out with `/logout`

1. Type `/logout` and press Enter.

Expected: Codex clears local credentials for the current user session.

### Exit the CLI with `/quit` or `/exit`

1. Type `/quit` (or `/exit`) and press Enter.

Expected: Codex exits immediately. Save or commit any important work first.


---


# Config basics

Codex reads configuration details from more than one location. Your personal defaults live in `~/.codex/config.toml`, and you can add project overrides with `.codex/config.toml` files. For security, Codex loads project `.codex/` layers only when you trust the project.

## Codex configuration file

Codex stores user-level configuration at `~/.codex/config.toml`. To scope settings to a specific project or subfolder, add a `.codex/config.toml` file in your repo.

To open the configuration file from the Codex IDE extension, select the gear icon in the top-right corner, then select **Codex Settings > Open config.toml**.

The CLI and IDE extension share the same configuration layers. You can use them to:

- Set the default model and provider.
- Configure [approval policies and sandbox settings](https://developers.openai.com/codex/agent-approvals-security#sandbox-and-approvals).
- Configure [MCP servers](https://developers.openai.com/codex/mcp).

## Configuration precedence

Codex resolves values in this order (highest precedence first):

1. CLI flags and `--config` overrides
2. Project config files: `.codex/config.toml`, ordered from the project root down to your current working directory (closest wins; trusted projects only)
3. [Profile](https://developers.openai.com/codex/config-advanced#profiles) files selected with `--profile profile-name` (`~/.codex/profile-name.config.toml`)
4. User config: `~/.codex/config.toml`
5. System config (if present): `/etc/codex/config.toml` on Unix
6. Built-in defaults

Use that precedence to set shared defaults in `config.toml` and keep [profile files](https://developers.openai.com/codex/config-advanced#profiles) focused on the values that differ.

If you mark a project as untrusted, Codex skips project-scoped `.codex/` layers, including project-local config, hooks, and rules. User and system config still load, including user/global hooks and rules.

For one-off overrides via `-c`/`--config` (including TOML quoting rules), see [Advanced Config](https://developers.openai.com/codex/config-advanced#one-off-overrides-from-the-cli).

On managed machines, your organization may also enforce constraints via
  `requirements.toml` (for example, disallowing `approval_policy = "never"` or
  `sandbox_mode = "danger-full-access"`). See [Managed
  configuration](https://developers.openai.com/codex/enterprise/managed-configuration) and [Admin-enforced
  requirements](https://developers.openai.com/codex/enterprise/managed-configuration#admin-enforced-requirements-requirementstoml).

## Common configuration options

Here are a few options people change most often:

#### Default model

Choose the model Codex uses by default in the CLI and IDE.

```toml
model = "gpt-5.5"
```


#### Approval prompts

Control when Codex pauses to ask before running generated commands.

```toml
approval_policy = "on-request"
```

For behavior differences between `untrusted`, `on-request`, and `never`, see [Run without approval prompts](https://developers.openai.com/codex/agent-approvals-security#run-without-approval-prompts) and [Common sandbox and approval combinations](https://developers.openai.com/codex/agent-approvals-security#common-sandbox-and-approval-combinations).

#### Sandbox level

Adjust how much filesystem and network access Codex has while executing commands.

```toml
sandbox_mode = "workspace-write"
```

For mode-by-mode behavior (including protected `.git`/`.codex` paths and network defaults), see [Sandbox and approvals](https://developers.openai.com/codex/agent-approvals-security#sandbox-and-approvals), [Protected paths in writable roots](https://developers.openai.com/codex/agent-approvals-security#protected-paths-in-writable-roots), and [Network access](https://developers.openai.com/codex/agent-approvals-security#network-access).

#### Permission profiles

Codex also supports named permission profiles for reusable filesystem and
network policies. Built-in profiles are `:read-only`, `:workspace`, and
`:danger-full-access`. Custom profiles use `[permissions.<name>]` tables and a
matching `default_permissions` value. See [Permissions](https://developers.openai.com/codex/permissions).

#### Windows sandbox mode

When running Codex natively on Windows, set the native sandbox mode to `elevated` in the `windows` table. Use `unelevated` only if you don't have administrator permissions or if elevated setup fails.

```toml
[windows]
sandbox = "elevated"   # Recommended
# sandbox = "unelevated" # Fallback if admin permissions/setup are unavailable
```

#### Web search mode

Codex enables web search by default for local tasks and serves results from a web search cache. The cache is an OpenAI-maintained index of web results, so cached mode returns pre-indexed results instead of fetching live pages. This reduces exposure to prompt injection from arbitrary live content, but you should still treat web results as untrusted. If you are using `--yolo` or another [full access sandbox setting](https://developers.openai.com/codex/agent-approvals-security#common-sandbox-and-approval-combinations), web search defaults to live results. Choose a mode with `web_search`:

- `"cached"` (default) serves results from the web search cache.
- `"live"` fetches the most recent data from the web (same as `--search`).
- `"disabled"` turns off the web search tool.

```toml
web_search = "cached"  # default; serves results from the web search cache
# web_search = "live"  # fetch the most recent data from the web (same as --search)
# web_search = "disabled"
```

#### Reasoning effort

Tune how much reasoning effort the model applies when supported.

```toml
model_reasoning_effort = "high"
```

#### Communication style

Set a default communication style for supported models.

```toml
personality = "friendly" # or "pragmatic" or "none"
```

You can override this later in an active session with `/personality` or per thread/turn when using the app-server APIs.

#### TUI keymap

Customize terminal shortcuts under `tui.keymap`. Selected composer actions fall back to matching `tui.keymap.global` bindings; context-specific bindings take precedence when supported. An empty list unbinds the action.

```toml
[tui.keymap.global]
open_transcript = "ctrl-t"

[tui.keymap.composer]
submit = ["enter", "ctrl-m"]

[tui.keymap.chat]
interrupt_turn = "f12"
```

#### Command environment

Control which environment variables Codex forwards to spawned commands.

```toml
[shell_environment_policy]
include_only = ["PATH", "HOME"]
```

#### Log directory

Override where Codex writes local log files. Setting `log_dir` explicitly also
enables the opt-in plaintext TUI log, `codex-tui.log`, in that directory.

```toml
log_dir = "/absolute/path/to/codex-logs"
```

For one-off runs, you can also set it from the CLI:

```bash
codex -c log_dir=./.codex-log
```

## Feature flags

Use the `[features]` table in `config.toml` to toggle optional and experimental capabilities.

```toml
[features]
shell_snapshot = true           # Speed up repeated commands
```

### Supported features

| Key                  |        Default        | Maturity     | Description                                                                              |
| -------------------- | :-------------------: | ------------ | ---------------------------------------------------------------------------------------- |
| `apps`               |         false         | Experimental | Enable ChatGPT Apps/connectors support                                                   |
| `codex_git_commit`   |         false         | Experimental | Enable Codex-generated git commits and commit attribution trailers                       |
| `hooks`              |         true          | Stable       | Enable lifecycle hooks from `hooks.json` or inline `[hooks]`. See [Hooks](https://developers.openai.com/codex/hooks). |
| `fast_mode`          |         true          | Stable       | Enable Fast mode selection and the `service_tier = "fast"` path                          |
| `memories`           |         false         | Stable       | Enable [Memories](https://developers.openai.com/codex/memories)                                                       |
| `multi_agent`        |         true          | Stable       | Enable subagent collaboration tools                                                      |
| `personality`        |         true          | Stable       | Enable personality selection controls                                                    |
| `shell_snapshot`     |         true          | Stable       | Snapshot your shell environment to speed up repeated commands                            |
| `shell_tool`         |         true          | Stable       | Enable the default `shell` tool                                                          |
| `unified_exec`       | `true` except Windows | Stable       | Use the unified PTY-backed exec tool                                                     |
| `undo`               |         false         | Stable       | Enable undo via per-turn git ghost snapshots                                             |
| `web_search`         |         true          | Deprecated   | Legacy toggle; prefer the top-level `web_search` setting                                 |
| `web_search_cached`  |         false         | Deprecated   | Legacy toggle that maps to `web_search = "cached"` when unset                            |
| `web_search_request` |         false         | Deprecated   | Legacy toggle that maps to `web_search = "live"` when unset                              |

The Maturity column uses feature maturity labels such as Experimental, Beta,
  and Stable. See [Feature Maturity](https://developers.openai.com/codex/feature-maturity) for how to
  interpret these labels.

Omit feature keys to keep their defaults.

For lifecycle hook configuration, see [Hooks](https://developers.openai.com/codex/hooks).

### Enabling features

- In `config.toml`, add `feature_name = true` under `[features]`.
- From the CLI, run `codex --enable feature_name`.
- To enable more than one feature, run `codex --enable feature_a --enable feature_b`.
- To disable a feature, set the key to `false` in `config.toml`.


---


# Advanced Configuration

Use these options when you need more control over providers, policies, and integrations. For a quick start, see [Config basics](https://developers.openai.com/codex/config-basic).

For background on project guidance, reusable capabilities, custom slash commands, subagent workflows, and integrations, see [Customization](https://developers.openai.com/codex/concepts/customization). For configuration keys, see [Configuration Reference](https://developers.openai.com/codex/config-reference).

## Profiles

Profiles let you save named configuration layers and switch between them from
the CLI. When you pass `--profile profile-name`, Codex loads
`~/.codex/config.toml`, then overlays `~/.codex/profile-name.config.toml`.
Profile names can contain letters, numbers, hyphens, and underscores.

Create a separate TOML file for each profile. Use top-level config keys in the
profile file; don't nest them under `[profiles.profile-name]`.

```toml
# ~/.codex/deep-review.config.toml
model = "gpt-5.5"
model_reasoning_effort = "xhigh"
approval_policy = "on-request"
model_catalog_json = "/Users/me/.codex/model-catalogs/deep-review.json"
```

```shell
codex --profile deep-review
codex exec --profile deep-review "review this change"
```

Because the profile file is a layer above your base user config and below
project and CLI config, it only needs the values that differ from your base
config. Profile files can also override `model_catalog_json`; Codex uses the
profile value when both files set it.

In Codex 0.134.0 and later, `--profile` no longer reads `[profiles.profile-name]`
from `config.toml`, and the top-level `profile = "profile-name"` selector is no
longer supported. Move legacy profile settings into
`~/.codex/profile-name.config.toml`, then remove the matching
`[profiles.profile-name]` table and `profile = "profile-name"` selector from
`config.toml`.

## One-off overrides from the CLI

In addition to editing `~/.codex/config.toml`, you can override configuration for a single run from the CLI:

- Prefer dedicated flags when they exist (for example, `--model`).
- Use `-c` / `--config` when you need to override an arbitrary key.

Examples:

```shell
# Dedicated flag
codex --model gpt-5.4

# Generic key/value override (value is TOML, not JSON)
codex --config model='"gpt-5.4"'
codex --config sandbox_workspace_write.network_access=true
codex --config 'shell_environment_policy.include_only=["PATH","HOME"]'
```

Notes:

- Keys can use dot notation to set nested values (for example, `mcp_servers.context7.enabled=false`).
- `--config` values are parsed as TOML. When in doubt, quote the value so your shell doesn't split it on spaces.
- If the value can't be parsed as TOML, Codex treats it as a string.

## Config and state locations

Codex stores its local state under `CODEX_HOME` (defaults to `~/.codex`).

Common files you may see there:

- `config.toml` (your local configuration)
- `auth.json` (if you use file-based credential storage) or your OS keychain/keyring
- `history.jsonl` (if history persistence is enabled)
- Other per-user state such as logs and caches

For authentication details (including credential storage modes), see [Authentication](https://developers.openai.com/codex/auth). For the full list of configuration keys, see [Configuration Reference](https://developers.openai.com/codex/config-reference).

For shared defaults, rules, and skills checked into repos or system paths, see [Team Config](https://developers.openai.com/codex/enterprise/admin-setup#team-config).

If you just need to point the built-in OpenAI provider at an LLM proxy, router, or data-residency enabled project, set `openai_base_url` in `config.toml` instead of defining a new provider. This changes the base URL for the built-in `openai` provider without requiring a separate `model_providers.<id>` entry.

```toml
openai_base_url = "https://us.api.openai.com/v1"
```

## Project config files (`.codex/config.toml`)

In addition to your user config, Codex reads project-scoped overrides from `.codex/config.toml` files inside your repo. Codex walks from the project root to your current working directory and loads every `.codex/config.toml` it finds. If multiple files define the same key, the closest file to your working directory wins.

For security, Codex loads project-scoped config files only when the project is trusted. If the project is untrusted, Codex ignores project `.codex/` layers, including `.codex/config.toml`, project-local hooks, and project-local rules. User and system layers remain separate and still load.

Relative paths inside a project config (for example, `model_instructions_file`) are resolved relative to the `.codex/` folder that contains the `config.toml`.

Project config files can't override settings that redirect credentials, alter
host-owned app request metadata, change provider auth, select config profiles,
or run machine-local notification/telemetry commands. Codex ignores the
following keys in project-local `.codex/config.toml` and prints a startup
warning when it sees them: `openai_base_url`, `chatgpt_base_url`,
`apps_mcp_product_sku`, `model_provider`, `model_providers`, `notify`,
`profile`, `profiles`, `experimental_realtime_ws_base_url`, and `otel`. Set
provider, notification, and telemetry keys in your user-level
`~/.codex/config.toml`; select config profiles with `--profile profile-name`
and `~/.codex/profile-name.config.toml`.

## Hooks

Codex can also load lifecycle hooks from either `hooks.json` files or inline
`[hooks]` tables in `config.toml` files that sit next to active config layers.

In practice, the four most useful locations are:

- `~/.codex/hooks.json`
- `~/.codex/config.toml`
- `<repo>/.codex/hooks.json`
- `<repo>/.codex/config.toml`

Project-local hooks load only when the project `.codex/` layer is trusted.
User-level hooks remain independent of project trust.

Inline TOML hooks use the same event structure as `hooks.json`:

```toml
[[hooks.PreToolUse]]
matcher = "^Bash$"

[[hooks.PreToolUse.hooks]]
type = "command"
command = '/usr/bin/python3 "$(git rev-parse --show-toplevel)/.codex/hooks/pre_tool_use_policy.py"'
timeout = 30
statusMessage = "Checking Bash command"
```

If a single layer contains both `hooks.json` and inline `[hooks]`, Codex loads
both and warns. Prefer one representation per layer.

For the current event list, input fields, output behavior, and limitations, see
[Hooks](https://developers.openai.com/codex/hooks).

## Agent roles (`[agents]` in `config.toml`)

For subagent role configuration (`[agents]` in `config.toml`), see [Subagents](https://developers.openai.com/codex/subagents).

## Project root detection

Codex discovers project configuration (for example, `.codex/` layers and `AGENTS.md`) by walking up from the working directory until it reaches a project root.

By default, Codex treats a directory containing `.git` as the project root. To customize this behavior, set `project_root_markers` in `config.toml`:

```toml
# Treat a directory as the project root when it contains any of these markers.
project_root_markers = [".git", ".hg", ".sl"]
```

Set `project_root_markers = []` to skip searching parent directories and treat the current working directory as the project root.

## Custom model providers

A model provider defines how Codex connects to a model (base URL, wire API, authentication, and optional HTTP headers). Custom providers can't reuse the reserved built-in provider IDs: `openai`, `ollama`, and `lmstudio`.

Define additional providers and point `model_provider` at them:

```toml
model = "gpt-5.4"
model_provider = "proxy"

[model_providers.proxy]
name = "OpenAI using LLM proxy"
base_url = "http://proxy.example.com"
env_key = "OPENAI_API_KEY"

[model_providers.local_ollama]
name = "Ollama"
base_url = "http://localhost:11434/v1"

[model_providers.mistral]
name = "Mistral"
base_url = "https://api.mistral.ai/v1"
env_key = "MISTRAL_API_KEY"
```

Add request headers when needed:

```toml
[model_providers.example]
http_headers = { "X-Example-Header" = "example-value" }
env_http_headers = { "X-Example-Features" = "EXAMPLE_FEATURES" }
```

Use command-backed authentication when a provider needs Codex to fetch bearer tokens from an external credential helper:

```toml
[model_providers.proxy]
name = "OpenAI using LLM proxy"
base_url = "https://proxy.example.com/v1"
wire_api = "responses"

[model_providers.proxy.auth]
command = "/usr/local/bin/fetch-codex-token"
args = ["--audience", "codex"]
timeout_ms = 5000
refresh_interval_ms = 300000
```

The auth command receives no `stdin` and must print the token to stdout. Codex trims surrounding whitespace, treats an empty token as an error, and refreshes proactively at `refresh_interval_ms`; set `refresh_interval_ms = 0` to refresh only after an authentication retry. Don't combine `[model_providers.<id>.auth]` with `env_key`, `experimental_bearer_token`, or `requires_openai_auth`.

### Amazon Bedrock provider

Codex includes a built-in `amazon-bedrock` model provider. Set it directly as
`model_provider`; unlike custom providers, this built-in provider supports only
the nested AWS profile and region overrides.

```toml
model_provider = "amazon-bedrock"
model = "<bedrock-model-id>"

[model_providers.amazon-bedrock.aws]
profile = "default"
region = "eu-central-1"
```

If you omit `profile`, Codex uses the standard AWS credential chain. Set
`region` to the supported Bedrock region that should handle requests.

For the full setup flow, authentication options, supported models, and feature
availability, see [Use Codex with Amazon Bedrock](https://developers.openai.com/codex/amazon-bedrock).

## OSS mode (local providers)

Codex can run against a local "open source" provider (for example, Ollama or LM Studio) when you pass `--oss`. If you pass `--oss` without specifying a provider, Codex uses `oss_provider` as the default.

```toml
# Default local provider used with `--oss`
oss_provider = "ollama" # or "lmstudio"
```

## Azure provider and per-provider tuning

```toml
[model_providers.azure]
name = "Azure"
base_url = "https://YOUR_PROJECT_NAME.openai.azure.com/openai"
env_key = "AZURE_OPENAI_API_KEY"
query_params = { api-version = "2025-04-01-preview" }
wire_api = "responses"
request_max_retries = 4
stream_max_retries = 10
stream_idle_timeout_ms = 300000
```

To change the base URL for the built-in OpenAI provider, use `openai_base_url`; don't create `[model_providers.openai]`, because you can't override built-in provider IDs.

## ChatGPT customers using data residency

Projects created with [data residency](https://help.openai.com/en/articles/9903489-data-residency-and-inference-residency-for-chatgpt) enabled can create a model provider to update the base_url with the [correct prefix](https://platform.openai.com/docs/guides/your-data#which-models-and-features-are-eligible-for-data-residency).

```toml
model_provider = "openaidr"
[model_providers.openaidr]
name = "OpenAI Data Residency"
base_url = "https://us.api.openai.com/v1" # Replace 'us' with domain prefix
```

## Model reasoning, verbosity, and limits

```toml
model_reasoning_summary = "none"          # Disable summaries
model_verbosity = "low"                   # Shorten responses
model_supports_reasoning_summaries = true # Force reasoning
model_context_window = 128000             # Context window size
```

`model_verbosity` applies only to providers using the Responses API. Chat Completions providers will ignore the setting.

## Approval policies and sandbox modes

Pick approval strictness (affects when Codex pauses) and sandbox level (affects file/network access).

For operational details to keep in mind while editing `config.toml`, see [Common sandbox and approval combinations](https://developers.openai.com/codex/agent-approvals-security#common-sandbox-and-approval-combinations), [Protected paths in writable roots](https://developers.openai.com/codex/agent-approvals-security#protected-paths-in-writable-roots), and [Network access](https://developers.openai.com/codex/agent-approvals-security#network-access).

For beta permission profiles that configure filesystem and network access together, see [Permissions](https://developers.openai.com/codex/permissions).

You can also use a granular approval policy (`approval_policy = { granular = { ... } }`) to allow or auto-reject individual prompt categories. This is useful when you want normal interactive approvals for some cases but want others, such as `request_permissions` or skill-script prompts, to fail closed automatically.

Set `approvals_reviewer = "auto_review"` to route eligible interactive approval
requests through automatic review. This changes the reviewer, not the sandbox
boundary.

Use `[auto_review].policy` for local reviewer policy instructions. Managed
`guardian_policy_config` takes precedence.

```toml
approval_policy = "untrusted"   # Other options: on-request, never, or { granular = { ... } }
approvals_reviewer = "user"     # Or "auto_review" for automatic review
sandbox_mode = "workspace-write"
allow_login_shell = false       # Optional hardening: disallow login shells for shell tools

# Example granular approval policy:
# approval_policy = { granular = {
#   sandbox_approval = true,
#   rules = true,
#   mcp_elicitations = true,
#   request_permissions = false,
#   skill_approval = false
# } }

[sandbox_workspace_write]
exclude_tmpdir_env_var = false  # Allow $TMPDIR
exclude_slash_tmp = false       # Allow /tmp
writable_roots = ["/Users/YOU/.pyenv/shims"]
network_access = false          # Opt in to outbound network

[auto_review]
policy = """
Use your organization's automatic review policy.
"""
```

### Named permission profiles

For built-in profiles, custom profile syntax, and the full filesystem and
network configuration model, see [Permissions](https://developers.openai.com/codex/permissions).

For the complete key list and requirements constraints, see
[Configuration Reference](https://developers.openai.com/codex/config-reference) and
[Managed configuration](https://developers.openai.com/codex/enterprise/managed-configuration).

In workspace-write mode, some environments keep `.git/` and `.codex/`
  read-only even when the rest of the workspace is writable. This is why
  commands like `git commit` may still require approval to run outside the
  sandbox. If you want Codex to skip specific commands (for example, block `git
  commit` outside the sandbox), use
  <a href="/codex/rules">rules</a>.

Disable sandboxing entirely (use only if your environment already isolates processes):

```toml
sandbox_mode = "danger-full-access"
```

## Shell environment policy

`shell_environment_policy` controls which environment variables Codex passes to any subprocess it launches (for example, when running a tool-command the model proposes). Start from a clean start (`inherit = "none"`) or a trimmed set (`inherit = "core"`), then layer on excludes, includes, and overrides to avoid leaking secrets while still providing the paths, keys, or flags your tasks need.

```toml
[shell_environment_policy]
inherit = "none"
set = { PATH = "/usr/bin", MY_FLAG = "1" }
ignore_default_excludes = false
exclude = ["AWS_*", "AZURE_*"]
include_only = ["PATH", "HOME"]
```

Patterns are case-insensitive globs (`*`, `?`, `[A-Z]`); `ignore_default_excludes = false` keeps the automatic KEY/SECRET/TOKEN filter before your includes/excludes run.

## MCP servers

See the dedicated [MCP documentation](https://developers.openai.com/codex/mcp) for configuration details.

## Observability and telemetry

Enable OpenTelemetry (OTel) log export to track Codex runs (API requests, SSE/events, prompts, tool approvals/results). Disabled by default; opt in via `[otel]`:

```toml
[otel]
environment = "staging"   # defaults to "dev"
exporter = "none"         # set to otlp-http or otlp-grpc to send events
log_user_prompt = false   # redact user prompts unless explicitly enabled
```

Choose an exporter:

```toml
[otel]
exporter = { otlp-http = {
  endpoint = "https://otel.example.com/v1/logs",
  protocol = "binary",
  headers = { "x-otlp-api-key" = "${OTLP_TOKEN}" }
}}
```

```toml
[otel]
exporter = { otlp-grpc = {
  endpoint = "https://otel.example.com:4317",
  headers = { "x-otlp-meta" = "abc123" }
}}
```

If `exporter = "none"` Codex records events but sends nothing. Exporters batch asynchronously and flush on shutdown. Event metadata includes service name, CLI version, env tag, conversation id, model, sandbox/approval settings, and per-event fields (see [Config Reference](https://developers.openai.com/codex/config-reference)).

### What gets emitted

Codex emits structured log events for runs and tool usage. Representative event types include:

- `codex.conversation_starts` (model, reasoning settings, sandbox/approval policy)
- `codex.api_request` (attempt, status/success, duration, and error details)
- `codex.sse_event` (stream event kind, success/failure, duration, plus token counts on `response.completed`)
- `codex.websocket_request` and `codex.websocket_event` (request duration plus per-message kind/success/error)
- `codex.user_prompt` (length; content redacted unless explicitly enabled)
- `codex.tool_decision` (approved/denied and whether the decision came from config vs user)
- `codex.tool_result` (duration, success, output snippet)

### OTel metrics emitted

When the OTel metrics pipeline is enabled, Codex emits counters and duration histograms for API, stream, and tool activity.

Each metric below also includes default metadata tags: `auth_mode`, `originator`, `session_source`, `model`, and `app.version`.

| Metric                                | Type      | Fields              | Description                                                       |
| ------------------------------------- | --------- | ------------------- | ----------------------------------------------------------------- |
| `codex.api_request`                   | counter   | `status`, `success` | API request count by HTTP status and success/failure.             |
| `codex.api_request.duration_ms`       | histogram | `status`, `success` | API request duration in milliseconds.                             |
| `codex.sse_event`                     | counter   | `kind`, `success`   | SSE event count by event kind and success/failure.                |
| `codex.sse_event.duration_ms`         | histogram | `kind`, `success`   | SSE event processing duration in milliseconds.                    |
| `codex.websocket.request`             | counter   | `success`           | WebSocket request count by success/failure.                       |
| `codex.websocket.request.duration_ms` | histogram | `success`           | WebSocket request duration in milliseconds.                       |
| `codex.websocket.event`               | counter   | `kind`, `success`   | WebSocket message/event count by type and success/failure.        |
| `codex.websocket.event.duration_ms`   | histogram | `kind`, `success`   | WebSocket message/event processing duration in milliseconds.      |
| `codex.tool.call`                     | counter   | `tool`, `success`   | Tool invocation count by tool name and success/failure.           |
| `codex.tool.call.duration_ms`         | histogram | `tool`, `success`   | Tool execution duration in milliseconds by tool name and outcome. |

For more security and privacy guidance around telemetry, see [Security](https://developers.openai.com/codex/agent-approvals-security#monitoring-and-telemetry).

### Metrics

By default, Codex periodically sends a small amount of anonymous usage and health data back to OpenAI. This helps detect when Codex isn't working correctly and shows what features and configuration options are being used, so the Codex team can focus on what matters most. These metrics don't contain any personally identifiable information (PII). Metrics collection is independent of OTel log/trace export.

If you want to disable metrics collection entirely across Codex surfaces on a machine, set the analytics flag in your config:

```toml
[analytics]
enabled = false
```

Each metric includes its own fields plus the default context fields below.

#### Default context fields (applies to every event/metric)

- `auth_mode`: `swic` | `api` | `unknown`.
- `model`: name of the model used.
- `app.version`: Codex version.

#### Metrics catalog

Each metric includes the required fields plus the default context fields above. Metric names below omit the `codex.` prefix.
Most metric names are centralized in `codex-rs/otel/src/metrics/names.rs`; feature-specific metrics emitted outside that file are included here too.
If a metric includes the `tool` field, it reflects the internal tool used (for example, `apply_patch` or `shell`) and doesn't contain the actual shell command or patch `codex` is trying to apply.

#### Runtime and model transport

| Metric                                          | Type      | Fields               | Description                                                  |
| ----------------------------------------------- | --------- | -------------------- | ------------------------------------------------------------ |
| `api_request`                                   | counter   | `status`, `success`  | API request count by HTTP status and success/failure.        |
| `api_request.duration_ms`                       | histogram | `status`, `success`  | API request duration in milliseconds.                        |
| `sse_event`                                     | counter   | `kind`, `success`    | SSE event count by event kind and success/failure.           |
| `sse_event.duration_ms`                         | histogram | `kind`, `success`    | SSE event processing duration in milliseconds.               |
| `websocket.request`                             | counter   | `success`            | WebSocket request count by success/failure.                  |
| `websocket.request.duration_ms`                 | histogram | `success`            | WebSocket request duration in milliseconds.                  |
| `websocket.event`                               | counter   | `kind`, `success`    | WebSocket message/event count by type and success/failure.   |
| `websocket.event.duration_ms`                   | histogram | `kind`, `success`    | WebSocket message/event processing duration in milliseconds. |
| `responses_api_overhead.duration_ms`            | histogram |                      | Responses API overhead timing from WebSocket responses.      |
| `responses_api_inference_time.duration_ms`      | histogram |                      | Responses API inference timing from WebSocket responses.     |
| `responses_api_engine_iapi_ttft.duration_ms`    | histogram |                      | Responses API engine IAPI time-to-first-token timing.        |
| `responses_api_engine_service_ttft.duration_ms` | histogram |                      | Responses API engine service time-to-first-token timing.     |
| `responses_api_engine_iapi_tbt.duration_ms`     | histogram |                      | Responses API engine IAPI time-between-token timing.         |
| `responses_api_engine_service_tbt.duration_ms`  | histogram |                      | Responses API engine service time-between-token timing.      |
| `transport.fallback_to_http`                    | counter   | `from_wire_api`      | WebSocket-to-HTTP fallback count.                            |
| `remote_models.fetch_update.duration_ms`        | histogram |                      | Time to fetch remote model definitions.                      |
| `remote_models.load_cache.duration_ms`          | histogram |                      | Time to load the remote model cache.                         |
| `startup_prewarm.duration_ms`                   | histogram | `status`             | Startup prewarm duration by outcome.                         |
| `startup_prewarm.age_at_first_turn_ms`          | histogram | `status`             | Startup prewarm age when the first real turn resolves it.    |
| `cloud_requirements.fetch.duration_ms`          | histogram |                      | Workspace-managed cloud requirements fetch duration.         |
| `cloud_requirements.fetch_attempt`              | counter   | See note             | Workspace-managed cloud requirements fetch attempts.         |
| `cloud_requirements.fetch_final`                | counter   | See note             | Final workspace-managed cloud requirements fetch outcome.    |
| `cloud_requirements.load`                       | counter   | `trigger`, `outcome` | Workspace-managed cloud requirements load outcome.           |

The `cloud_requirements.fetch_attempt` metric includes `trigger`, `attempt`, `outcome`, and `status_code` fields. The `cloud_requirements.fetch_final` metric includes `trigger`, `outcome`, `reason`, `attempt_count`, and `status_code` fields.

#### Turn and tool activity

| Metric                                 | Type      | Fields                                                                    | Description                                                                                                      |
| -------------------------------------- | --------- | ------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| `turn.e2e_duration_ms`                 | histogram |                                                                           | End-to-end time for a full turn.                                                                                 |
| `turn.ttft.duration_ms`                | histogram |                                                                           | Time to first token for a turn.                                                                                  |
| `turn.ttfm.duration_ms`                | histogram |                                                                           | Time to first model output item for a turn.                                                                      |
| `turn.network_proxy`                   | counter   | `active`, `tmp_mem_enabled`                                               | Whether the managed network proxy was active for the turn.                                                       |
| `turn.memory`                          | counter   | `read_allowed`, `feature_enabled`, `config_use_memories`, `has_citations` | Per-turn memory read availability and memory citation usage.                                                     |
| `turn.tool.call`                       | histogram | `tmp_mem_enabled`                                                         | Number of tool calls in the turn.                                                                                |
| `turn.token_usage`                     | histogram | `token_type`, `tmp_mem_enabled`                                           | Per-turn token usage by token type (`total`, `input`, `cached_input`, `output`, or `reasoning_output`).          |
| `tool.call`                            | counter   | `tool`, `success`                                                         | Tool invocation count by tool name and success/failure.                                                          |
| `tool.call.duration_ms`                | histogram | `tool`, `success`                                                         | Tool execution duration in milliseconds by tool name and outcome.                                                |
| `tool.unified_exec`                    | counter   | `tty`                                                                     | Unified exec tool calls by TTY mode.                                                                             |
| `approval.requested`                   | counter   | `tool`, `approved`                                                        | Tool approval request result (`approved`, `approved_with_amendment`, `approved_for_session`, `denied`, `abort`). |
| `mcp.call`                             | counter   | See note                                                                  | MCP tool invocation result.                                                                                      |
| `mcp.call.duration_ms`                 | histogram | See note                                                                  | MCP tool invocation duration.                                                                                    |
| `mcp.tools.list.duration_ms`           | histogram | `cache`                                                                   | MCP tool-list duration, including cache hit/miss state.                                                          |
| `mcp.tools.fetch_uncached.duration_ms` | histogram |                                                                           | Duration of MCP tool fetches that miss the cache.                                                                |
| `mcp.tools.cache_write.duration_ms`    | histogram |                                                                           | Duration of Codex Apps MCP tool-cache writes.                                                                    |
| `hooks.run`                            | counter   | `hook_name`, `source`, `status`                                           | Hook run count by hook name, source, and status.                                                                 |
| `hooks.run.duration_ms`                | histogram | `hook_name`, `source`, `status`                                           | Hook run duration in milliseconds.                                                                               |

The `mcp.call` and `mcp.call.duration_ms` metrics include `status`; normal tool-call emissions also include `tool`, plus `connector_id` and `connector_name` when available. Blocked Codex Apps MCP calls may emit `mcp.call` with only `status`.

#### Threads, tasks, and features

| Metric                            | Type      | Fields                | Description                                                                      |
| --------------------------------- | --------- | --------------------- | -------------------------------------------------------------------------------- |
| `feature.state`                   | counter   | `feature`, `value`    | Feature values that differ from defaults (emit one row per non-default).         |
| `status_line`                     | counter   |                       | Session started with a configured status line.                                   |
| `model_warning`                   | counter   |                       | Warning sent to the model.                                                       |
| `thread.started`                  | counter   | `is_git`              | New thread created, tagged by whether the working directory is in a Git repo.    |
| `conversation.turn.count`         | counter   |                       | User/assistant turns per thread, recorded at the end of the thread.              |
| `thread.fork`                     | counter   | `source`              | New thread created by forking an existing thread.                                |
| `thread.rename`                   | counter   |                       | Thread renamed.                                                                  |
| `thread.side`                     | counter   | `source`              | Side conversation created.                                                       |
| `thread.skills.enabled_total`     | histogram |                       | Number of skills enabled for a new thread.                                       |
| `thread.skills.kept_total`        | histogram |                       | Number of enabled skills kept after prompt rendering.                            |
| `thread.skills.truncated`         | histogram |                       | Whether skill rendering truncated the enabled skills list (`1` or `0`).          |
| `task.compact`                    | counter   | `type`                | Number of compactions per type (`remote` or `local`), including manual and auto. |
| `task.review`                     | counter   |                       | Number of reviews triggered.                                                     |
| `task.undo`                       | counter   |                       | Number of undo actions triggered.                                                |
| `task.user_shell`                 | counter   |                       | Number of user shell actions (`!` in the TUI for example).                       |
| `shell_snapshot`                  | counter   | See note              | Whether taking a shell snapshot succeeded.                                       |
| `shell_snapshot.duration_ms`      | histogram | `success`             | Time to take a shell snapshot.                                                   |
| `skill.injected`                  | counter   | `status`, `skill`     | Skill injection outcomes by skill.                                               |
| `plugins.startup_sync`            | counter   | `transport`, `status` | Curated plugin startup sync attempts.                                            |
| `plugins.startup_sync.final`      | counter   | `transport`, `status` | Final curated plugin startup sync outcome.                                       |
| `multi_agent.spawn`               | counter   | `role`                | Agent spawns by role.                                                            |
| `multi_agent.resume`              | counter   |                       | Agent resumes.                                                                   |
| `multi_agent.nickname_pool_reset` | counter   |                       | Agent nickname pool resets.                                                      |

The `shell_snapshot` metric includes `success` and, on failures, `failure_reason`.

#### Memory and local state

| Metric                         | Type      | Fields                    | Description                                               |
| ------------------------------ | --------- | ------------------------- | --------------------------------------------------------- |
| `memory.phase1`                | counter   | `status`                  | Memory phase 1 job counts by status.                      |
| `memory.phase1.e2e_ms`         | histogram |                           | End-to-end duration for memory phase 1.                   |
| `memory.phase1.output`         | counter   |                           | Memory phase 1 outputs written.                           |
| `memory.phase1.token_usage`    | histogram | `token_type`              | Memory phase 1 token usage by token type.                 |
| `memory.phase2`                | counter   | `status`                  | Memory phase 2 job counts by status.                      |
| `memory.phase2.e2e_ms`         | histogram |                           | End-to-end duration for memory phase 2.                   |
| `memory.phase2.input`          | counter   |                           | Memory phase 2 input count.                               |
| `memory.phase2.token_usage`    | histogram | `token_type`              | Memory phase 2 token usage by token type.                 |
| `memories.usage`               | counter   | `kind`, `tool`, `success` | Memory usage by kind, tool, and success/failure.          |
| `external_agent_config.detect` | counter   | See note                  | External agent config detections by migration item type.  |
| `external_agent_config.import` | counter   | See note                  | External agent config imports by migration item type.     |
| `db.backfill`                  | counter   | `status`                  | Initial state DB backfill results (`upserted`, `failed`). |
| `db.backfill.duration_ms`      | histogram | `status`                  | Duration of the initial state DB backfill.                |
| `db.error`                     | counter   | `stage`                   | Errors during state DB operations.                        |

The `external_agent_config.detect` and `external_agent_config.import` metrics include `migration_type`; skills migrations also include `skills_count`.

#### Windows sandbox

| Metric                                           | Type      | Fields                                    | Description                                           |
| ------------------------------------------------ | --------- | ----------------------------------------- | ----------------------------------------------------- |
| `windows_sandbox.setup_success`                  | counter   | `originator`, `mode`                      | Windows sandbox setup successes.                      |
| `windows_sandbox.setup_failure`                  | counter   | `originator`, `mode`                      | Windows sandbox setup failures.                       |
| `windows_sandbox.setup_duration_ms`              | histogram | `result`, `originator`, `mode`            | Windows sandbox setup duration.                       |
| `windows_sandbox.elevated_setup_success`         | counter   |                                           | Elevated Windows sandbox setup successes.             |
| `windows_sandbox.elevated_setup_failure`         | counter   | See note                                  | Elevated Windows sandbox setup failures.              |
| `windows_sandbox.elevated_setup_canceled`        | counter   | See note                                  | Canceled elevated Windows sandbox setup attempts.     |
| `windows_sandbox.elevated_setup_duration_ms`     | histogram | `result`                                  | Elevated Windows sandbox setup duration.              |
| `windows_sandbox.elevated_prompt_shown`          | counter   |                                           | Elevated sandbox setup prompt shown.                  |
| `windows_sandbox.elevated_prompt_accept`         | counter   |                                           | Elevated sandbox setup prompt accepted.               |
| `windows_sandbox.elevated_prompt_use_legacy`     | counter   |                                           | User chose legacy sandbox from the elevated prompt.   |
| `windows_sandbox.elevated_prompt_quit`           | counter   |                                           | User quit from the elevated prompt.                   |
| `windows_sandbox.fallback_prompt_shown`          | counter   |                                           | Fallback sandbox prompt shown.                        |
| `windows_sandbox.fallback_retry_elevated`        | counter   |                                           | User retried elevated setup from the fallback prompt. |
| `windows_sandbox.fallback_use_legacy`            | counter   |                                           | User chose legacy sandbox from the fallback prompt.   |
| `windows_sandbox.fallback_prompt_quit`           | counter   |                                           | User quit from the fallback prompt.                   |
| `windows_sandbox.legacy_setup_preflight_failed`  | counter   | See note                                  | Legacy Windows sandbox setup preflight failure.       |
| `windows_sandbox.setup_elevated_sandbox_command` | counter   |                                           | Elevated sandbox setup command invoked.               |
| `windows_sandbox.createprocessasuserw_failed`    | counter   | `error_code`, `path_kind`, `exe`, `level` | Windows `CreateProcessAsUserW` failures.              |

The elevated setup failure metrics include `code` and `message` when Windows setup failure details are available, and may include `originator` when emitted from the shared setup path. The `windows_sandbox.legacy_setup_preflight_failed` metric includes `originator` when emitted from the shared setup path, but fallback-prompt preflight failures may not include any fields.

### Feedback controls

By default, Codex lets users send feedback from `/feedback`. To disable feedback collection across Codex surfaces on a machine, update your config:

```toml
[feedback]
enabled = false
```

When disabled, `/feedback` shows a disabled message and Codex rejects feedback submissions.

### Hide or surface reasoning events

If you want to reduce noisy "reasoning" output (for example in CI logs), you can suppress it:

```toml
hide_agent_reasoning = true
```

If you want to surface raw reasoning content when a model emits it:

```toml
show_raw_agent_reasoning = true
```

Enable raw reasoning only if it's acceptable for your workflow. Some models/providers (like `gpt-oss`) don't emit raw reasoning; in that case, this setting has no visible effect.

## Notifications

Use `notify` to trigger an external program whenever Codex emits supported events (currently only `agent-turn-complete`). This is handy for desktop toasts, chat webhooks, CI updates, or any side-channel alerting that the built-in TUI notifications don't cover.

```toml
notify = ["python3", "/path/to/notify.py"]
```

Example `notify.py` (truncated) that reacts to `agent-turn-complete`:

```python
#!/usr/bin/env python3
import json, subprocess, sys

def main() -> int:
    notification = json.loads(sys.argv[1])
    if notification.get("type") != "agent-turn-complete":
        return 0
    title = f"Codex: {notification.get('last-assistant-message', 'Turn Complete!')}"
    message = " ".join(notification.get("input-messages", []))
    subprocess.check_output([
        "terminal-notifier",
        "-title", title,
        "-message", message,
        "-group", "codex-" + notification.get("thread-id", ""),
        "-activate", "com.googlecode.iterm2",
    ])
    return 0

if __name__ == "__main__":
    sys.exit(main())
```

The script receives a single JSON argument. Common fields include:

- `type` (currently `agent-turn-complete`)
- `thread-id` (session identifier)
- `turn-id` (turn identifier)
- `cwd` (working directory)
- `input-messages` (user messages that led to the turn)
- `last-assistant-message` (last assistant message text)

Place the script somewhere on disk and point `notify` to it.

#### `notify` vs `tui.notifications`

- `notify` runs an external program (good for webhooks, desktop notifiers, CI hooks).
- `tui.notifications` is built in to the TUI and can optionally filter by event type (for example, `agent-turn-complete` and `approval-requested`).
- `tui.notification_method` controls how the TUI emits terminal notifications (`auto`, `osc9`, or `bel`).
- `tui.notification_condition` controls whether TUI notifications fire only when
  the terminal is `unfocused` or `always`.

In `auto` mode, Codex prefers OSC 9 notifications (a terminal escape sequence some terminals interpret as a desktop notification) and falls back to BEL (`\x07`) otherwise.

See [Configuration Reference](https://developers.openai.com/codex/config-reference) for the exact keys.

## History persistence

By default, Codex saves local session transcripts under `CODEX_HOME` (for example, `~/.codex/history.jsonl`). To disable local history persistence:

```toml
[history]
persistence = "none"
```

To cap the history file size, set `history.max_bytes`. When the file exceeds the cap, Codex drops the oldest entries and compacts the file while keeping the newest records.

```toml
[history]
max_bytes = 104857600 # 100 MiB
```

## Clickable citations

If you use a terminal/editor integration that supports it, Codex can render file citations as clickable links. Configure `file_opener` to pick the URI scheme Codex uses:

```toml
file_opener = "vscode" # or cursor, windsurf, vscode-insiders, none
```

Example: a citation like `/home/user/project/main.py:42` can be rewritten into a clickable `vscode://file/...:42` link.

## Project instructions discovery

Codex reads `AGENTS.md` (and related files) and includes a limited amount of project guidance in the first turn of a session. Two knobs control how this works:

- `project_doc_max_bytes`: how much to read from each `AGENTS.md` file
- `project_doc_fallback_filenames`: additional filenames to try when `AGENTS.md` is missing at a directory level

For a detailed walkthrough, see [Custom instructions with AGENTS.md](https://developers.openai.com/codex/guides/agents-md).

## TUI options

Running `codex` with no subcommand launches the interactive terminal UI (TUI). Codex exposes some TUI-specific configuration under `[tui]`, including:

- `tui.notifications`: enable/disable notifications (or restrict to specific types)
- `tui.notification_method`: choose `auto`, `osc9`, or `bel` for terminal notifications
- `tui.notification_condition`: choose `unfocused` or `always` for when
  notifications fire
- `tui.animations`: enable/disable ASCII animations and shimmer effects
- `tui.alternate_screen`: control alternate screen usage (set to `never` to keep terminal scrollback)
- `tui.show_tooltips`: show or hide onboarding tooltips on the welcome screen

`tui.notification_method` defaults to `auto`. In `auto` mode, Codex prefers OSC 9 notifications (a terminal escape sequence some terminals interpret as a desktop notification) when the terminal appears to support them, and falls back to BEL (`\x07`) otherwise.

See [Configuration Reference](https://developers.openai.com/codex/config-reference) for the full key list.


---


# Configuration Reference

Use this page as a searchable reference for Codex configuration files. For conceptual guidance and examples, start with [Config basics](https://developers.openai.com/codex/config-basic) and [Advanced Config](https://developers.openai.com/codex/config-advanced).

## `config.toml`

User-level configuration lives in `~/.codex/config.toml`. You can also add project-scoped overrides in `.codex/config.toml` files. Codex loads project-scoped config files only when you trust the project.

Project-scoped config can't override machine-local provider, auth,
host-owned app request metadata, notification, configuration profile selection,
or telemetry routing keys. Codex ignores `openai_base_url`,
`chatgpt_base_url`, `apps_mcp_product_sku`, `model_provider`,
`model_providers`, `notify`, `profile`, `profiles`,
`experimental_realtime_ws_base_url`, and `otel` when they appear in a
project-local `.codex/config.toml`; put provider, notification, and telemetry
keys in user-level config instead. Config [profile files](https://developers.openai.com/codex/config-advanced#profiles) live next to
`config.toml` as `$CODEX_HOME/profile-name.config.toml`; select one with
`--profile profile-name`.

For sandbox and approval keys (`approval_policy`, `sandbox_mode`, and `sandbox_workspace_write.*`), pair this reference with [Sandbox and approvals](https://developers.openai.com/codex/agent-approvals-security#sandbox-and-approvals), [Protected paths in writable roots](https://developers.openai.com/codex/agent-approvals-security#protected-paths-in-writable-roots), and [Network access](https://developers.openai.com/codex/agent-approvals-security#network-access). For beta permission profiles, see [Permissions](https://developers.openai.com/codex/permissions).

<ConfigTable
  options={[
    {
      key: "model",
      type: "string",
      description: "Model to use (e.g., `gpt-5.5`).",
    },
    {
      key: "review_model",
      type: "string",
      description:
        "Optional model override used by `/review` (defaults to the current session model).",
    },
    {
      key: "model_provider",
      type: "string",
      description: "Provider id from `model_providers` (default: `openai`).",
    },
    {
      key: "openai_base_url",
      type: "string",
      description:
        "Base URL override for the built-in `openai` model provider.",
    },
    {
      key: "model_context_window",
      type: "number",
      description: "Context window tokens available to the active model.",
    },
    {
      key: "model_auto_compact_token_limit",
      type: "number",
      description:
        "Token threshold that triggers automatic history compaction (unset uses model defaults).",
    },
    {
      key: "model_catalog_json",
      type: "string (path)",
      description:
        "Optional path to a JSON model catalog loaded on startup. A selected `$CODEX_HOME/profile-name.config.toml` profile file can override this per profile.",
    },
    {
      key: "oss_provider",
      type: "lmstudio | ollama",
      description:
        "Default local provider used when running with `--oss` (defaults to prompting if unset).",
    },
    {
      key: "approval_policy",
      type: "untrusted | on-request | never | { granular = { sandbox_approval = bool, rules = bool, mcp_elicitations = bool, request_permissions = bool, skill_approval = bool } }",
      description:
        "Controls when Codex pauses for approval before executing commands. You can also use `approval_policy = { granular = { ... } }` to allow or auto-reject specific prompt categories while keeping other prompts interactive. `on-failure` is deprecated; use `on-request` for interactive runs or `never` for non-interactive runs.",
    },
    {
      key: "approval_policy.granular.sandbox_approval",
      type: "boolean",
      description:
        "When `true`, sandbox escalation approval prompts are allowed to surface.",
    },
    {
      key: "approval_policy.granular.rules",
      type: "boolean",
      description:
        "When `true`, approvals triggered by execpolicy `prompt` rules are allowed to surface.",
    },
    {
      key: "approval_policy.granular.mcp_elicitations",
      type: "boolean",
      description:
        "When `true`, MCP elicitation prompts are allowed to surface instead of being auto-rejected.",
    },
    {
      key: "approval_policy.granular.request_permissions",
      type: "boolean",
      description:
        "When `true`, prompts from the `request_permissions` tool are allowed to surface.",
    },
    {
      key: "approval_policy.granular.skill_approval",
      type: "boolean",
      description:
        "When `true`, skill-script approval prompts are allowed to surface.",
    },
    {
      key: "approvals_reviewer",
      type: "user | auto_review",
      description:
        "Who reviews eligible approval prompts under `on-request` or granular approval policies. Defaults to `user`; `auto_review` uses the reviewer subagent. This setting doesn't change sandboxing or review actions already allowed inside the sandbox.",
    },
    {
      key: "auto_review.policy",
      type: "string",
      description:
        "Local Markdown policy instructions for automatic review. Managed `guardian_policy_config` takes precedence. Blank values are ignored.",
    },
    {
      key: "allow_login_shell",
      type: "boolean",
      description:
        "Allow shell-based tools to use login-shell semantics. Defaults to `true`; when `false`, `login = true` requests are rejected and omitted `login` defaults to non-login shells.",
    },
    {
      key: "sandbox_mode",
      type: "read-only | workspace-write | danger-full-access",
      description:
        "Sandbox policy for filesystem and network access during command execution.",
    },
    {
      key: "sandbox_workspace_write.writable_roots",
      type: "array<string>",
      description:
        'Additional writable roots when `sandbox_mode = "workspace-write"`.',
    },
    {
      key: "sandbox_workspace_write.network_access",
      type: "boolean",
      description:
        "Allow outbound network access inside the workspace-write sandbox.",
    },
    {
      key: "sandbox_workspace_write.exclude_tmpdir_env_var",
      type: "boolean",
      description:
        "Exclude `$TMPDIR` from writable roots in workspace-write mode.",
    },
    {
      key: "sandbox_workspace_write.exclude_slash_tmp",
      type: "boolean",
      description:
        "Exclude `/tmp` from writable roots in workspace-write mode.",
    },
    {
      key: "windows.sandbox",
      type: "unelevated | elevated",
      description:
        "Windows-only native sandbox mode when running Codex natively on Windows.",
    },
    {
      key: "windows.sandbox_private_desktop",
      type: "boolean",
      description:
        "Run the final sandboxed child process on a private desktop by default on native Windows. Set `false` only for compatibility with the older `Winsta0\\\\Default` behavior.",
    },
    {
      key: "notify",
      type: "array<string>",
      description:
        "Command invoked for notifications; receives a JSON payload from Codex.",
    },
    {
      key: "check_for_update_on_startup",
      type: "boolean",
      description:
        "Check for Codex updates on startup (set to false only when updates are centrally managed).",
    },
    {
      key: "feedback.enabled",
      type: "boolean",
      description:
        "Enable feedback submission via `/feedback` across Codex surfaces (default: true).",
    },
    {
      key: "analytics.enabled",
      type: "boolean",
      description:
        "Enable or disable analytics for this machine/profile. When unset, the client default applies.",
    },
    {
      key: "instructions",
      type: "string",
      description:
        "Reserved for future use; prefer `model_instructions_file` or `AGENTS.md`.",
    },
    {
      key: "developer_instructions",
      type: "string",
      description:
        "Additional developer instructions injected into the session (optional).",
    },
    {
      key: "log_dir",
      type: "string (path)",
      description:
        "Directory where Codex writes log files; defaults to `$CODEX_HOME/log`. Setting this explicitly also enables the opt-in plaintext TUI log, `codex-tui.log`, in that directory.",
    },
    {
      key: "sqlite_home",
      type: "string (path)",
      description:
        "Directory where Codex stores the SQLite-backed state DB used by agent jobs and other resumable runtime state.",
    },
    {
      key: "compact_prompt",
      type: "string",
      description: "Inline override for the history compaction prompt.",
    },
    {
      key: "commit_attribution",
      type: "string",
      description:
        'Commit co-author trailer used when `[features].codex_git_commit` is enabled. Defaults to `Codex <noreply@openai.com>`; set `""` to disable.',
    },
    {
      key: "model_instructions_file",
      type: "string (path)",
      description:
        "Replacement for built-in instructions instead of `AGENTS.md`.",
    },
    {
      key: "personality",
      type: "none | friendly | pragmatic",
      description:
        "Default communication style for models that advertise `supportsPersonality`; can be overridden per thread/turn or via `/personality`.",
    },
    {
      key: "service_tier",
      type: "string",
      description:
        "Preferred service tier for new turns. Built-in values include `flex` and `fast`; legacy `fast` config maps to the request value `priority`, and catalog-provided tier IDs can also be stored.",
    },
    {
      key: "experimental_compact_prompt_file",
      type: "string (path)",
      description:
        "Load the compaction prompt override from a file (experimental).",
    },
    {
      key: "skills.config",
      type: "array<object>",
      description: "Per-skill enablement overrides stored in config.toml.",
    },
    {
      key: "skills.config.<index>.path",
      type: "string (path)",
      description: "Path to a skill folder containing `SKILL.md`.",
    },
    {
      key: "skills.config.<index>.enabled",
      type: "boolean",
      description: "Enable or disable the referenced skill.",
    },
    {
      key: "apps.<id>.enabled",
      type: "boolean",
      description:
        "Enable or disable a specific app/connector by id (default: true).",
    },
    {
      key: "apps._default.enabled",
      type: "boolean",
      description:
        "Default app enabled state for all apps unless overridden per app.",
    },
    {
      key: "apps._default.destructive_enabled",
      type: "boolean",
      description:
        "Default allow/deny for app tools with `destructive_hint = true`.",
    },
    {
      key: "apps._default.open_world_enabled",
      type: "boolean",
      description:
        "Default allow/deny for app tools with `open_world_hint = true`.",
    },
    {
      key: "apps.<id>.destructive_enabled",
      type: "boolean",
      description:
        "Allow or block tools in this app that advertise `destructive_hint = true`.",
    },
    {
      key: "apps.<id>.open_world_enabled",
      type: "boolean",
      description:
        "Allow or block tools in this app that advertise `open_world_hint = true`.",
    },
    {
      key: "apps.<id>.default_tools_enabled",
      type: "boolean",
      description:
        "Default enabled state for tools in this app unless a per-tool override exists.",
    },
    {
      key: "apps.<id>.default_tools_approval_mode",
      type: "auto | prompt | approve",
      description:
        "Default approval behavior for tools in this app unless a per-tool override exists.",
    },
    {
      key: "apps.<id>.tools.<tool>.enabled",
      type: "boolean",
      description:
        "Per-tool enabled override for an app tool (for example `repos/list`).",
    },
    {
      key: "apps.<id>.tools.<tool>.approval_mode",
      type: "auto | prompt | approve",
      description: "Per-tool approval behavior override for a single app tool.",
    },
    {
      key: "tool_suggest.discoverables",
      type: "array<table>",
      description:
        'Allow tool suggestions for additional discoverable connectors or plugins. Each entry uses `type = "connector"` or `"plugin"` and an `id`.',
    },
    {
      key: "tool_suggest.disabled_tools",
      type: "array<table>",
      description:
        'Disable suggestions for specific discoverable connectors or plugins. Each entry uses `type = "connector"` or `"plugin"` and an `id`.',
    },
    {
      key: "features.apps",
      type: "boolean",
      description: "Enable ChatGPT Apps/connectors support (experimental).",
    },
    {
      key: "features.hooks",
      type: "boolean",
      description:
        "Enable lifecycle hooks loaded from `hooks.json` or inline `[hooks]` config. `features.codex_hooks` is a deprecated alias.",
    },
    {
      key: "features.codex_git_commit",
      type: "boolean",
      description:
        "Enable Codex-generated git commits. When enabled, Codex uses `commit_attribution` to append a `Co-authored-by:` trailer to generated commit messages.",
    },
    {
      key: "hooks",
      type: "table",
      description:
        "Lifecycle hooks configured inline in `config.toml`. Uses the same event schema as `hooks.json`; see the Hooks guide for examples and supported events.",
    },
    {
      key: "hooks.<Event>",
      type: "array<table>",
      description:
        "Matcher groups for hook events such as `PreToolUse`, `PermissionRequest`, `PostToolUse`, `PreCompact`, `PostCompact`, `SessionStart`, `SubagentStart`, `SubagentStop`, `UserPromptSubmit`, or `Stop`.",
    },
    {
      key: "hooks.<Event>[].hooks",
      type: "array<table>",
      description:
        "Hook handlers for a matcher group. Command hooks are currently supported; prompt and agent hook handlers are parsed but skipped.",
    },
    {
      key: "hooks.<Event>[].hooks[].commandWindows",
      type: "string",
      description:
        "Windows-only command override for command hooks. The TOML alias `command_windows` is also accepted.",
    },
    {
      key: "features.memories",
      type: "boolean",
      description: "Enable [Memories](https://developers.openai.com/codex/memories) (off by default).",
    },
    {
      key: "mcp_servers.<id>.command",
      type: "string",
      description: "Launcher command for an MCP stdio server.",
    },
    {
      key: "mcp_servers.<id>.args",
      type: "array<string>",
      description: "Arguments passed to the MCP stdio server command.",
    },
    {
      key: "mcp_servers.<id>.env",
      type: "map<string,string>",
      description: "Environment variables forwarded to the MCP stdio server.",
    },
    {
      key: "mcp_servers.<id>.env_vars",
      type: 'array<string | { name = string, source = "local" | "remote" }>',
      description:
        'Additional environment variables to whitelist for an MCP stdio server. String entries default to `source = "local"`; use `source = "remote"` only with executor-backed remote stdio.',
    },
    {
      key: "mcp_servers.<id>.cwd",
      type: "string",
      description: "Working directory for the MCP stdio server process.",
    },
    {
      key: "mcp_servers.<id>.url",
      type: "string",
      description: "Endpoint for an MCP streamable HTTP server.",
    },
    {
      key: "mcp_servers.<id>.bearer_token_env_var",
      type: "string",
      description:
        "Environment variable sourcing the bearer token for an MCP HTTP server.",
    },
    {
      key: "mcp_servers.<id>.http_headers",
      type: "map<string,string>",
      description: "Static HTTP headers included with each MCP HTTP request.",
    },
    {
      key: "mcp_servers.<id>.env_http_headers",
      type: "map<string,string>",
      description:
        "HTTP headers populated from environment variables for an MCP HTTP server.",
    },
    {
      key: "mcp_servers.<id>.enabled",
      type: "boolean",
      description: "Disable an MCP server without removing its configuration.",
    },
    {
      key: "mcp_servers.<id>.required",
      type: "boolean",
      description:
        "When true, fail startup/resume if this enabled MCP server cannot initialize.",
    },
    {
      key: "mcp_servers.<id>.startup_timeout_sec",
      type: "number",
      description:
        "Override the default 10s startup timeout for an MCP server.",
    },
    {
      key: "mcp_servers.<id>.startup_timeout_ms",
      type: "number",
      description: "Alias for `startup_timeout_sec` in milliseconds.",
    },
    {
      key: "mcp_servers.<id>.tool_timeout_sec",
      type: "number",
      description:
        "Override the default 60s per-tool timeout for an MCP server.",
    },
    {
      key: "mcp_servers.<id>.enabled_tools",
      type: "array<string>",
      description: "Allow list of tool names exposed by the MCP server.",
    },
    {
      key: "mcp_servers.<id>.disabled_tools",
      type: "array<string>",
      description:
        "Deny list applied after `enabled_tools` for the MCP server.",
    },
    {
      key: "mcp_servers.<id>.default_tools_approval_mode",
      type: "auto | prompt | approve",
      description:
        "Default approval behavior for MCP tools on this server unless a per-tool override exists.",
    },
    {
      key: "mcp_servers.<id>.tools.<tool>.approval_mode",
      type: "auto | prompt | approve",
      description:
        "Per-tool approval behavior override for one MCP tool on this server.",
    },
    {
      key: "mcp_servers.<id>.scopes",
      type: "array<string>",
      description:
        "OAuth scopes to request when authenticating to that MCP server.",
    },
    {
      key: "mcp_servers.<id>.oauth_resource",
      type: "string",
      description:
        "Optional RFC 8707 OAuth resource parameter to include during MCP login.",
    },
    {
      key: "mcp_servers.<id>.experimental_environment",
      type: "local | remote",
      description:
        "Experimental placement for an MCP server. `remote` starts stdio servers through a remote executor environment; streamable HTTP remote placement is not implemented.",
    },
    {
      key: "agents.max_threads",
      type: "number",
      description:
        "Maximum number of agent threads that can be open concurrently. Defaults to `6` when unset.",
    },
    {
      key: "agents.max_depth",
      type: "number",
      description:
        "Maximum nesting depth allowed for spawned agent threads (root sessions start at depth 0; default: 1).",
    },
    {
      key: "agents.job_max_runtime_seconds",
      type: "number",
      description:
        "Default per-worker timeout for `spawn_agents_on_csv` jobs. When unset, the tool falls back to 1800 seconds per worker.",
    },
    {
      key: "agents.<name>.description",
      type: "string",
      description:
        "Role guidance shown to Codex when choosing and spawning that agent type.",
    },
    {
      key: "agents.<name>.config_file",
      type: "string (path)",
      description:
        "Path to a TOML config layer for that role; relative paths resolve from the config file that declares the role.",
    },
    {
      key: "agents.<name>.nickname_candidates",
      type: "array<string>",
      description:
        "Optional pool of display nicknames for spawned agents in that role.",
    },
    {
      key: "memories.generate_memories",
      type: "boolean",
      description:
        "When `false`, newly created threads are not stored as memory-generation inputs. Defaults to `true`.",
    },
    {
      key: "memories.use_memories",
      type: "boolean",
      description:
        "When `false`, Codex skips injecting existing memories into future sessions. Defaults to `true`.",
    },
    {
      key: "memories.disable_on_external_context",
      type: "boolean",
      description:
        "When `true`, threads that use external context such as MCP tool calls, web search, or tool search are kept out of memory generation. Defaults to `false`. Legacy alias: `memories.no_memories_if_mcp_or_web_search`.",
    },
    {
      key: "memories.max_raw_memories_for_consolidation",
      type: "number",
      description:
        "Maximum recent raw memories retained for global consolidation. Defaults to `256` and is capped at `4096`.",
    },
    {
      key: "memories.max_unused_days",
      type: "number",
      description:
        "Maximum days since a memory was last used before it becomes ineligible for consolidation. Defaults to `30` and is clamped to `0`-`365`.",
    },
    {
      key: "memories.max_rollout_age_days",
      type: "number",
      description:
        "Maximum age of threads considered for memory generation. Defaults to `30` and is clamped to `0`-`90`.",
    },
    {
      key: "memories.max_rollouts_per_startup",
      type: "number",
      description:
        "Maximum rollout candidates processed per startup pass. Defaults to `16` and is capped at `128`.",
    },
    {
      key: "memories.min_rollout_idle_hours",
      type: "number",
      description:
        "Minimum idle time before a thread is considered for memory generation. Defaults to `6` and is clamped to `1`-`48`.",
    },
    {
      key: "memories.min_rate_limit_remaining_percent",
      type: "number",
      description:
        "Minimum remaining percentage required in Codex rate-limit windows before memory generation starts. Defaults to `25` and is clamped to `0`-`100`.",
    },
    {
      key: "memories.extract_model",
      type: "string",
      description: "Optional model override for per-thread memory extraction.",
    },
    {
      key: "memories.consolidation_model",
      type: "string",
      description: "Optional model override for global memory consolidation.",
    },
    {
      key: "features.unified_exec",
      type: "boolean",
      description:
        "Use the unified PTY-backed exec tool (stable; enabled by default except on Windows).",
    },
    {
      key: "features.shell_snapshot",
      type: "boolean",
      description:
        "Snapshot shell environment to speed up repeated commands (stable; on by default).",
    },
    {
      key: "features.undo",
      type: "boolean",
      description: "Enable undo support (stable; off by default).",
    },
    {
      key: "features.multi_agent",
      type: "boolean",
      description:
        "Enable multi-agent collaboration tools (`spawn_agent`, `send_input`, `resume_agent`, `wait_agent`, and `close_agent`) (stable; on by default).",
    },
    {
      key: "features.personality",
      type: "boolean",
      description:
        "Enable personality selection controls (stable; on by default).",
    },
    {
      key: "features.network_proxy",
      type: "boolean | table",
      description:
        "Enable sandboxed networking. Use a table form when setting network policy options such as `domains` (experimental; off by default).",
    },
    {
      key: "features.network_proxy.enabled",
      type: "boolean",
      description: "Enable sandboxed networking. Defaults to `false`.",
    },
    {
      key: "features.network_proxy.domains",
      type: "map<string, allow | deny>",
      description:
        "Domain policy for sandboxed networking. Unset by default, which means no external destinations are allowed until you add `allow` rules. Supports exact hosts, `*.example.com` for subdomains only, `**.example.com` for apex plus subdomains, and global `*` allow rules; prefer scoped rules because `*` broadly opens public outbound access. Add `deny` rules for blocked destinations; `deny` wins on conflicts.",
    },
    {
      key: "features.network_proxy.unix_sockets",
      type: "map<string, allow | deny>",
      description:
        "Unix socket policy for sandboxed networking. Unset by default; add `allow` entries for permitted sockets.",
    },
    {
      key: "features.network_proxy.allow_local_binding",
      type: "boolean",
      description:
        "Allow broader local/private-network access. Defaults to `false`; exact local IP literal or `localhost` allow rules can still permit specific local targets.",
    },
    {
      key: "features.network_proxy.enable_socks5",
      type: "boolean",
      description: "Expose SOCKS5 support. Defaults to `true`.",
    },
    {
      key: "features.network_proxy.enable_socks5_udp",
      type: "boolean",
      description: "Allow UDP over SOCKS5. Defaults to `true`.",
    },
    {
      key: "features.network_proxy.allow_upstream_proxy",
      type: "boolean",
      description:
        "Allow chaining through an upstream proxy from the environment. Defaults to `true`.",
    },
    {
      key: "features.network_proxy.dangerously_allow_non_loopback_proxy",
      type: "boolean",
      description:
        "Permit non-loopback listener addresses. Defaults to `false`; enabling it can expose proxy listeners beyond localhost.",
    },
    {
      key: "features.network_proxy.dangerously_allow_all_unix_sockets",
      type: "boolean",
      description:
        "Permit arbitrary Unix socket destinations instead of allowlist-only access. Defaults to `false`; use only in tightly controlled environments.",
    },
    {
      key: "features.network_proxy.proxy_url",
      type: "string",
      description:
        'HTTP listener URL for sandboxed networking. Defaults to `"http://127.0.0.1:3128"`.',
    },
    {
      key: "features.network_proxy.socks_url",
      type: "string",
      description:
        'SOCKS5 listener URL. Defaults to `"http://127.0.0.1:8081"`.',
    },
    {
      key: "features.web_search",
      type: "boolean",
      description:
        "Deprecated legacy toggle; prefer the top-level `web_search` setting.",
    },
    {
      key: "features.web_search_cached",
      type: "boolean",
      description:
        'Deprecated legacy toggle. When `web_search` is unset, true maps to `web_search = "cached"`.',
    },
    {
      key: "features.web_search_request",
      type: "boolean",
      description:
        'Deprecated legacy toggle. When `web_search` is unset, true maps to `web_search = "live"`.',
    },
    {
      key: "features.shell_tool",
      type: "boolean",
      description:
        "Enable the default `shell` tool for running commands (stable; on by default).",
    },
    {
      key: "features.enable_request_compression",
      type: "boolean",
      description:
        "Compress streaming request bodies with zstd when supported (stable; on by default).",
    },
    {
      key: "features.skill_mcp_dependency_install",
      type: "boolean",
      description:
        "Allow prompting and installing missing MCP dependencies for skills (stable; on by default).",
    },
    {
      key: "features.fast_mode",
      type: "boolean",
      description:
        "Enable model-catalog service tier selection in the TUI, including Fast-tier commands when the active model advertises them (stable; on by default).",
    },
    {
      key: "features.prevent_idle_sleep",
      type: "boolean",
      description:
        "Prevent the machine from sleeping while a turn is actively running (experimental; off by default).",
    },
    {
      key: "suppress_unstable_features_warning",
      type: "boolean",
      description:
        "Suppress the warning that appears when under-development feature flags are enabled.",
    },
    {
      key: "model_providers.<id>",
      type: "table",
      description:
        "Custom provider definition. Built-in provider IDs (`openai`, `ollama`, and `lmstudio`) are reserved and cannot be overridden.",
    },
    {
      key: "model_providers.<id>.name",
      type: "string",
      description: "Display name for a custom model provider.",
    },
    {
      key: "model_providers.<id>.base_url",
      type: "string",
      description: "API base URL for the model provider.",
    },
    {
      key: "model_providers.<id>.env_key",
      type: "string",
      description: "Environment variable supplying the provider API key.",
    },
    {
      key: "model_providers.<id>.env_key_instructions",
      type: "string",
      description: "Optional setup guidance for the provider API key.",
    },
    {
      key: "model_providers.<id>.experimental_bearer_token",
      type: "string",
      description:
        "Direct bearer token for the provider (discouraged; use `env_key`).",
    },
    {
      key: "model_providers.<id>.requires_openai_auth",
      type: "boolean",
      description:
        "The provider uses OpenAI authentication (defaults to false).",
    },
    {
      key: "model_providers.<id>.wire_api",
      type: "responses",
      description:
        "Protocol used by the provider. `responses` is the only supported value, and it is the default when omitted.",
    },
    {
      key: "model_providers.<id>.query_params",
      type: "map<string,string>",
      description: "Extra query parameters appended to provider requests.",
    },
    {
      key: "model_providers.<id>.http_headers",
      type: "map<string,string>",
      description: "Static HTTP headers added to provider requests.",
    },
    {
      key: "model_providers.<id>.env_http_headers",
      type: "map<string,string>",
      description:
        "HTTP headers populated from environment variables when present.",
    },
    {
      key: "model_providers.<id>.request_max_retries",
      type: "number",
      description:
        "Retry count for HTTP requests to the provider (default: 4).",
    },
    {
      key: "model_providers.<id>.stream_max_retries",
      type: "number",
      description: "Retry count for SSE streaming interruptions (default: 5).",
    },
    {
      key: "model_providers.<id>.stream_idle_timeout_ms",
      type: "number",
      description:
        "Idle timeout for SSE streams in milliseconds (default: 300000).",
    },
    {
      key: "model_providers.<id>.supports_websockets",
      type: "boolean",
      description:
        "Whether that provider supports the Responses API WebSocket transport.",
    },
    {
      key: "model_providers.<id>.auth",
      type: "table",
      description:
        "Command-backed bearer token configuration for a custom provider. Do not combine with `env_key`, `experimental_bearer_token`, or `requires_openai_auth`.",
    },
    {
      key: "model_providers.<id>.auth.command",
      type: "string",
      description:
        "Command to run when Codex needs a bearer token. The command must print the token to stdout.",
    },
    {
      key: "model_providers.<id>.auth.args",
      type: "array<string>",
      description: "Arguments passed to the token command.",
    },
    {
      key: "model_providers.<id>.auth.timeout_ms",
      type: "number",
      description:
        "Maximum token command runtime in milliseconds (default: 5000).",
    },
    {
      key: "model_providers.<id>.auth.refresh_interval_ms",
      type: "number",
      description:
        "How often Codex proactively refreshes the token in milliseconds (default: 300000). Set to `0` to refresh only after an authentication retry.",
    },
    {
      key: "model_providers.<id>.auth.cwd",
      type: "string (path)",
      description: "Working directory for the token command.",
    },
    {
      key: "model_providers.amazon-bedrock.aws.profile",
      type: "string",
      description:
        "AWS profile name used by the built-in `amazon-bedrock` provider.",
    },
    {
      key: "model_providers.amazon-bedrock.aws.region",
      type: "string",
      description: "AWS region used by the built-in `amazon-bedrock` provider.",
    },
    {
      key: "model_reasoning_effort",
      type: "minimal | low | medium | high | xhigh",
      description:
        "Adjust reasoning effort for supported models (Responses API only; `xhigh` is model-dependent).",
    },
    {
      key: "plan_mode_reasoning_effort",
      type: "none | minimal | low | medium | high | xhigh",
      description:
        "Plan-mode-specific reasoning override. When unset, Plan mode uses its built-in preset default.",
    },
    {
      key: "model_reasoning_summary",
      type: "auto | concise | detailed | none",
      description:
        "Select reasoning summary detail or disable summaries entirely.",
    },
    {
      key: "model_verbosity",
      type: "low | medium | high",
      description:
        "Optional GPT-5 Responses API verbosity override; when unset, the selected model/preset default is used.",
    },
    {
      key: "model_supports_reasoning_summaries",
      type: "boolean",
      description: "Force Codex to send or not send reasoning metadata.",
    },
    {
      key: "shell_environment_policy.inherit",
      type: "all | core | none",
      description:
        "Baseline environment inheritance when spawning subprocesses.",
    },
    {
      key: "shell_environment_policy.ignore_default_excludes",
      type: "boolean",
      description:
        "Keep variables containing KEY/SECRET/TOKEN before other filters run.",
    },
    {
      key: "shell_environment_policy.exclude",
      type: "array<string>",
      description:
        "Glob patterns for removing environment variables after the defaults.",
    },
    {
      key: "shell_environment_policy.include_only",
      type: "array<string>",
      description:
        "Whitelist of patterns; when set only matching variables are kept.",
    },
    {
      key: "shell_environment_policy.set",
      type: "map<string,string>",
      description:
        "Explicit environment overrides injected into every subprocess.",
    },
    {
      key: "shell_environment_policy.experimental_use_profile",
      type: "boolean",
      description: "Use the user shell profile when spawning subprocesses.",
    },
    {
      key: "project_root_markers",
      type: "array<string>",
      description:
        "List of project root marker filenames; used when searching parent directories for the project root.",
    },
    {
      key: "project_doc_max_bytes",
      type: "number",
      description:
        "Maximum bytes read from `AGENTS.md` when building project instructions.",
    },
    {
      key: "project_doc_fallback_filenames",
      type: "array<string>",
      description: "Additional filenames to try when `AGENTS.md` is missing.",
    },
    {
      key: "history.persistence",
      type: "save-all | none",
      description:
        "Control whether Codex saves session transcripts to history.jsonl.",
    },
    {
      key: "tool_output_token_limit",
      type: "number",
      description:
        "Token budget for storing individual tool/function outputs in history.",
    },
    {
      key: "background_terminal_max_timeout",
      type: "number",
      description:
        "Maximum poll window in milliseconds for empty `write_stdin` polls (background terminal polling). Default: `300000` (5 minutes). Replaces the older `background_terminal_timeout` key.",
    },
    {
      key: "history.max_bytes",
      type: "number",
      description:
        "If set, caps the history file size in bytes by dropping oldest entries.",
    },
    {
      key: "file_opener",
      type: "vscode | vscode-insiders | windsurf | cursor | none",
      description:
        "URI scheme used to open citations from Codex output (default: `vscode`).",
    },
    {
      key: "otel.environment",
      type: "string",
      description:
        "Environment tag applied to emitted OpenTelemetry events (default: `dev`).",
    },
    {
      key: "otel.exporter",
      type: "none | otlp-http | otlp-grpc",
      description:
        "Select the OpenTelemetry exporter and provide any endpoint metadata.",
    },
    {
      key: "otel.trace_exporter",
      type: "none | otlp-http | otlp-grpc",
      description:
        "Select the OpenTelemetry trace exporter and provide any endpoint metadata.",
    },
    {
      key: "otel.metrics_exporter",
      type: "none | statsig | otlp-http | otlp-grpc",
      description:
        "Select the OpenTelemetry metrics exporter (defaults to `statsig`).",
    },
    {
      key: "otel.log_user_prompt",
      type: "boolean",
      description:
        "Opt in to exporting raw user prompts with OpenTelemetry logs.",
    },
    {
      key: "otel.exporter.<id>.endpoint",
      type: "string",
      description: "Exporter endpoint for OTEL logs.",
    },
    {
      key: "otel.exporter.<id>.protocol",
      type: "binary | json",
      description: "Protocol used by the OTLP/HTTP exporter.",
    },
    {
      key: "otel.exporter.<id>.headers",
      type: "map<string,string>",
      description: "Static headers included with OTEL exporter requests.",
    },
    {
      key: "otel.trace_exporter.<id>.endpoint",
      type: "string",
      description: "Trace exporter endpoint for OTEL logs.",
    },
    {
      key: "otel.trace_exporter.<id>.protocol",
      type: "binary | json",
      description: "Protocol used by the OTLP/HTTP trace exporter.",
    },
    {
      key: "otel.trace_exporter.<id>.headers",
      type: "map<string,string>",
      description: "Static headers included with OTEL trace exporter requests.",
    },
    {
      key: "otel.exporter.<id>.tls.ca-certificate",
      type: "string",
      description: "CA certificate path for OTEL exporter TLS.",
    },
    {
      key: "otel.exporter.<id>.tls.client-certificate",
      type: "string",
      description: "Client certificate path for OTEL exporter TLS.",
    },
    {
      key: "otel.exporter.<id>.tls.client-private-key",
      type: "string",
      description: "Client private key path for OTEL exporter TLS.",
    },
    {
      key: "otel.trace_exporter.<id>.tls.ca-certificate",
      type: "string",
      description: "CA certificate path for OTEL trace exporter TLS.",
    },
    {
      key: "otel.trace_exporter.<id>.tls.client-certificate",
      type: "string",
      description: "Client certificate path for OTEL trace exporter TLS.",
    },
    {
      key: "otel.trace_exporter.<id>.tls.client-private-key",
      type: "string",
      description: "Client private key path for OTEL trace exporter TLS.",
    },
    {
      key: "tui",
      type: "table",
      description:
        "TUI-specific options such as enabling inline desktop notifications.",
    },
    {
      key: "tui.notifications",
      type: "boolean | array<string>",
      description:
        "Enable TUI notifications; optionally restrict to specific event types.",
    },
    {
      key: "tui.notification_method",
      type: "auto | osc9 | bel",
      description:
        "Notification method for terminal notifications (default: auto).",
    },
    {
      key: "tui.notification_condition",
      type: "unfocused | always",
      description:
        "Control whether TUI notifications fire only when the terminal is unfocused or regardless of focus. Defaults to `unfocused`.",
    },
    {
      key: "tui.animations",
      type: "boolean",
      description:
        "Enable terminal animations (welcome screen, shimmer, spinner) (default: true).",
    },
    {
      key: "tui.alternate_screen",
      type: "auto | always | never",
      description:
        "Control alternate screen usage for the TUI (default: auto; auto skips it in Zellij to preserve scrollback).",
    },
    {
      key: "tui.vim_mode_default",
      type: "boolean",
      description:
        "Start the composer in Vim normal mode instead of insert mode (default: false). You can still toggle it per session with `/vim`.",
    },
    {
      key: "tui.raw_output_mode",
      type: "boolean",
      description:
        "Start the TUI in raw scrollback mode for copy-friendly terminal selection (default: false). You can toggle it with `/raw` or the default `alt-r` key binding.",
    },
    {
      key: "tui.show_tooltips",
      type: "boolean",
      description:
        "Show onboarding tooltips in the TUI welcome screen (default: true).",
    },
    {
      key: "tui.status_line",
      type: "array<string> | null",
      description:
        "Ordered list of TUI footer status-line item identifiers. `null` disables the status line.",
    },
    {
      key: "tui.terminal_title",
      type: "array<string> | null",
      description:
        'Ordered list of terminal window/tab title item identifiers. Defaults to `["spinner", "project"]`; `null` disables title updates.',
    },
    {
      key: "tui.theme",
      type: "string",
      description:
        "Syntax-highlighting theme override (kebab-case theme name).",
    },
    {
      key: "tui.keymap.<context>.<action>",
      type: "string | array<string>",
      description:
        "Keyboard shortcut binding for a TUI action. Supported contexts include `global`, `chat`, `composer`, `editor`, `vim_normal`, `vim_operator`, `vim_text_object`, `pager`, `list`, and `approval`. Selected composer actions fall back to matching `tui.keymap.global` bindings; context-specific bindings take precedence when supported.",
    },
    {
      key: "tui.keymap.<context>.<action> = []",
      type: "empty array",
      description:
        "Unbind the action in that keymap context. Key names use normalized strings such as `ctrl-a`, `shift-enter`, `page-down`, or `minus`.",
    },
    {
      key: "plugins.<plugin>.mcp_servers.<server>.enabled",
      type: "boolean",
      description:
        "Enable or disable an MCP server bundled by an installed plugin without changing the plugin manifest.",
    },
    {
      key: "plugins.<plugin>.mcp_servers.<server>.default_tools_approval_mode",
      type: "auto | prompt | approve",
      description:
        "Default approval behavior for tools on a plugin-provided MCP server.",
    },
    {
      key: "plugins.<plugin>.mcp_servers.<server>.enabled_tools",
      type: "array<string>",
      description:
        "Allow list of tools exposed from a plugin-provided MCP server.",
    },
    {
      key: "plugins.<plugin>.mcp_servers.<server>.disabled_tools",
      type: "array<string>",
      description:
        "Deny list applied after `enabled_tools` for a plugin-provided MCP server.",
    },
    {
      key: "plugins.<plugin>.mcp_servers.<server>.tools.<tool>.approval_mode",
      type: "auto | prompt | approve",
      description:
        "Per-tool approval behavior override for a plugin-provided MCP tool.",
    },
    {
      key: "tui.model_availability_nux.<model>",
      type: "integer",
      description: "Internal startup-tooltip state keyed by model slug.",
    },
    {
      key: "hide_agent_reasoning",
      type: "boolean",
      description:
        "Suppress reasoning events in both the TUI and `codex exec` output.",
    },
    {
      key: "show_raw_agent_reasoning",
      type: "boolean",
      description:
        "Surface raw reasoning content when the active model emits it.",
    },
    {
      key: "disable_paste_burst",
      type: "boolean",
      description: "Disable burst-paste detection in the TUI.",
    },
    {
      key: "windows_wsl_setup_acknowledged",
      type: "boolean",
      description: "Track Windows onboarding acknowledgement (Windows only).",
    },
    {
      key: "chatgpt_base_url",
      type: "string",
      description: "Override the base URL used during the ChatGPT login flow.",
    },
    {
      key: "cli_auth_credentials_store",
      type: "file | keyring | auto",
      description:
        "Control where the CLI stores cached credentials (file-based auth.json vs OS keychain).",
    },
    {
      key: "mcp_oauth_credentials_store",
      type: "auto | file | keyring",
      description: "Preferred store for MCP OAuth credentials.",
    },
    {
      key: "mcp_oauth_callback_port",
      type: "integer",
      description:
        "Optional fixed port for the local HTTP callback server used during MCP OAuth login. When unset, Codex binds to an ephemeral port chosen by the OS.",
    },
    {
      key: "mcp_oauth_callback_url",
      type: "string",
      description:
        "Optional redirect URI override for MCP OAuth login (for example, a devbox ingress URL). `mcp_oauth_callback_port` still controls the callback listener port.",
    },
    {
      key: "experimental_use_unified_exec_tool",
      type: "boolean",
      description:
        "Legacy name for enabling unified exec; prefer `[features].unified_exec` or `codex --enable unified_exec`.",
    },
    {
      key: "tools.web_search",
      type: 'boolean | { context_size = "low|medium|high", allowed_domains = [string], location = { country, region, city, timezone } }',
      description:
        "Optional web search tool configuration. The legacy boolean form is still accepted, but the object form lets you set search context size, allowed domains, and approximate user location.",
    },
    {
      key: "tools.view_image",
      type: "boolean",
      description: "Enable the local-image attachment tool `view_image`.",
    },
    {
      key: "web_search",
      type: "disabled | cached | live",
      description:
        'Web search mode (default: `"cached"`; cached uses an OpenAI-maintained index and does not fetch live pages; if you use `--yolo` or another full access sandbox setting, it defaults to `"live"`). Use `"live"` to fetch the most recent data from the web, or `"disabled"` to remove the tool.',
    },
    {
      key: "default_permissions",
      type: "string",
      description:
        "Name of the default permissions profile to apply to sandboxed tool calls. Built-ins are `:read-only`, `:workspace`, and `:danger-full-access`; custom profile names require matching `[permissions.<name>]` tables. Don't combine with `sandbox_mode` or `[sandbox_workspace_write]`.",
    },
    {
      key: "permissions.<name>.description",
      type: "string",
      description:
        "Human-readable description for this named profile. A profile does not inherit its parent's description through `extends`.",
    },
    {
      key: "permissions.<name>.extends",
      type: "string",
      description:
        "Optional parent profile applied before this named profile. Set it to another named profile, `:read-only`, or `:workspace`; `:danger-full-access`, undefined parents, and cycles are rejected.",
    },
    {
      key: "permissions.<name>.workspace_roots",
      type: "table",
      description:
        "Profile-defined workspace roots that receive `:workspace_roots` filesystem rules alongside the session's runtime workspace roots.",
    },
    {
      key: "permissions.<name>.workspace_roots.<path>",
      type: "boolean",
      description:
        "Opt a path into the profile's workspace root set when `true`. Disabled entries remain inactive.",
    },
    {
      key: "permissions.<name>.filesystem",
      type: "table",
      description:
        "Named filesystem permission profile. Each key is an absolute path or special token such as `:minimal` or `:workspace_roots`.",
    },
    {
      key: "permissions.<name>.filesystem.glob_scan_max_depth",
      type: "number",
      description:
        "Maximum depth for expanding deny-read glob patterns on platforms that snapshot matches before sandbox startup. Must be at least `1` when set.",
    },
    {
      key: "permissions.<name>.filesystem.<path-or-glob>",
      type: '"read" | "write" | "deny" | table',
      description:
        'Grant direct access for a path, glob pattern, or special token, or scope nested entries under that root. Use `"deny"` to deny reads for matching paths.',
    },
    {
      key: 'permissions.<name>.filesystem.":workspace_roots".<subpath-or-glob>',
      type: '"read" | "write" | "deny"',
      description:
        'Scoped filesystem access relative to each effective workspace root. Use `"."` for the root itself; glob subpaths such as `"**/*.env"` can deny reads with `"deny"`.',
    },
    {
      key: "permissions.<name>.network.enabled",
      type: "boolean",
      description:
        "Enable network access for this named permissions profile. This changes the sandbox network policy; it does not start the network proxy by itself.",
    },
    {
      key: "permissions.<name>.network.proxy_url",
      type: "string",
      description:
        "HTTP listener URL used when this permissions profile enables sandboxed networking.",
    },
    {
      key: "permissions.<name>.network.enable_socks5",
      type: "boolean",
      description:
        "Expose SOCKS5 support when this permissions profile enables sandboxed networking.",
    },
    {
      key: "permissions.<name>.network.socks_url",
      type: "string",
      description: "SOCKS5 proxy endpoint used by this permissions profile.",
    },
    {
      key: "permissions.<name>.network.enable_socks5_udp",
      type: "boolean",
      description: "Allow UDP over the SOCKS5 listener when enabled.",
    },
    {
      key: "permissions.<name>.network.allow_upstream_proxy",
      type: "boolean",
      description:
        "Allow sandboxed networking to chain through another upstream proxy.",
    },
    {
      key: "permissions.<name>.network.dangerously_allow_non_loopback_proxy",
      type: "boolean",
      description:
        "Permit non-loopback bind addresses for sandboxed networking listeners. Enabling it can expose listeners beyond localhost.",
    },
    {
      key: "permissions.<name>.network.dangerously_allow_all_unix_sockets",
      type: "boolean",
      description:
        "Allow arbitrary Unix socket destinations instead of the default restricted set. Use only in tightly controlled environments.",
    },
    {
      key: "permissions.<name>.network.mode",
      type: "limited | full",
      description: "Network proxy mode used for subprocess traffic.",
    },
    {
      key: "permissions.<name>.network.domains",
      type: "table",
      description:
        "Domain rules for sandboxed networking. Supports exact hosts, `*.example.com` for subdomains only, `**.example.com` for apex plus subdomains, and global `*` allow rules. `deny` wins on conflicts.",
    },
    {
      key: "permissions.<name>.network.domains.<pattern>",
      type: "allow | deny",
      description:
        "Allow or deny an exact host or scoped wildcard pattern such as `*.example.com` or `**.example.com`.",
    },
    {
      key: "permissions.<name>.network.unix_sockets",
      type: "table",
      description:
        "Unix socket allowlist overrides for sandboxed networking. Use socket paths as keys; `allow` adds a path, and `deny` rejects it.",
    },
    {
      key: "permissions.<name>.network.unix_sockets.<path>",
      type: "allow | deny",
      description:
        "Add an absolute Unix socket path to the effective allowlist with `allow`, or reject it with `deny`. Denied entries are omitted from the effective allowlist.",
    },
    {
      key: "permissions.<name>.network.allow_local_binding",
      type: "boolean",
      description:
        "Permit broader local/private-network access through sandboxed networking. Exact local IP literal or `localhost` allow rules can still permit specific local targets when this stays `false`.",
    },
    {
      key: "projects.<path>.trust_level",
      type: "string",
      description:
        'Mark a project or worktree as trusted or untrusted (`"trusted"` | `"untrusted"`). Untrusted projects skip project-scoped `.codex/` layers, including project-local config, hooks, and rules.',
    },
    {
      key: "notice.hide_full_access_warning",
      type: "boolean",
      description: "Track acknowledgement of the full access warning prompt.",
    },
    {
      key: "notice.hide_world_writable_warning",
      type: "boolean",
      description:
        "Track acknowledgement of the Windows world-writable directories warning.",
    },
    {
      key: "notice.hide_rate_limit_model_nudge",
      type: "boolean",
      description: "Track opt-out of the rate limit model switch reminder.",
    },
    {
      key: "notice.hide_gpt5_1_migration_prompt",
      type: "boolean",
      description: "Track acknowledgement of the GPT-5.1 migration prompt.",
    },
    {
      key: "notice.hide_gpt-5.1-codex-max_migration_prompt",
      type: "boolean",
      description:
        "Track acknowledgement of the gpt-5.1-codex-max migration prompt.",
    },
    {
      key: "notice.model_migrations",
      type: "map<string,string>",
      description: "Track acknowledged model migrations as old->new mappings.",
    },
    {
      key: "forced_login_method",
      type: "chatgpt | api",
      description: "Restrict Codex to a specific authentication method.",
    },
    {
      key: "forced_chatgpt_workspace_id",
      type: "string (uuid)",
      description: "Limit ChatGPT logins to a specific workspace identifier.",
    },
  ]}
  client:load
/>

You can find the latest JSON schema for `config.toml` [here](https://developers.openai.com/codex/config-schema.json).

To get autocompletion and diagnostics when editing `config.toml` in VS Code or Cursor, you can install the [Even Better TOML](https://marketplace.visualstudio.com/items?itemName=tamasfe.even-better-toml) extension and add this line to the top of your `config.toml`:

```toml
#:schema https://developers.openai.com/codex/config-schema.json
```

Note: Rename `experimental_instructions_file` to `model_instructions_file`. Codex deprecates the old key; update existing configs to the new name.

## `requirements.toml`

`requirements.toml` is an admin-enforced configuration file that constrains security-sensitive settings users can't override. For details, locations, and examples, see [Admin-enforced requirements](https://developers.openai.com/codex/enterprise/managed-configuration#admin-enforced-requirements-requirementstoml).

For ChatGPT Business and Enterprise users, Codex can also apply cloud-fetched
requirements. See the security page for precedence details.

Use `[features]` in `requirements.toml` to pin feature flags by the same
canonical keys that `config.toml` uses. Omitted keys remain unconstrained.

<ConfigTable
  options={[
    {
      key: "allowed_approval_policies",
      type: "array<string>",
      description:
        "Allowed values for `approval_policy` (for example `untrusted`, `on-request`, `never`, and `granular`).",
    },
    {
      key: "allowed_approvals_reviewers",
      type: "array<string>",
      description:
        "Allowed values for `approvals_reviewer`, such as `user` and `auto_review`.",
    },
    {
      key: "guardian_policy_config",
      type: "string",
      description:
        "Managed Markdown policy instructions for automatic review. This takes precedence over local `[auto_review].policy`. Blank values are ignored.",
    },
    {
      key: "allowed_sandbox_modes",
      type: "array<string>",
      description: "Allowed values for `sandbox_mode`.",
    },
    {
      key: "remote_sandbox_config",
      type: "array<table>",
      description:
        "Host-specific sandbox requirements. The first entry whose `hostname_patterns` match the resolved host name overrides top-level `allowed_sandbox_modes` for that requirements source. Host-specific entries currently override sandbox modes only.",
    },
    {
      key: "remote_sandbox_config[].hostname_patterns",
      type: "array<string>",
      description:
        "Case-insensitive host name patterns. Supports `*` for any sequence of characters and `?` for one character.",
    },
    {
      key: "remote_sandbox_config[].allowed_sandbox_modes",
      type: "array<string>",
      description:
        "Allowed sandbox modes to apply when this host-specific entry matches.",
    },
    {
      key: "allowed_web_search_modes",
      type: "array<string>",
      description:
        "Allowed values for `web_search` (`disabled`, `cached`, `live`). `disabled` is always allowed; an empty list effectively allows only `disabled`.",
    },
    {
      key: "allow_managed_hooks_only",
      type: "boolean",
      description:
        "When `true`, Codex skips user, project, session, and plugin hooks while still allowing managed hooks from `requirements.toml` and other managed config layers.",
    },
    {
      key: "plugin_sharing",
      type: "boolean",
      description:
        "Set to `false` in cloud-managed `requirements.toml` to disable workspace sharing for locally built plugins.",
    },
    {
      key: "features",
      type: "table",
      description:
        "Pinned feature values keyed by the canonical names from `config.toml`'s `[features]` table.",
    },
    {
      key: "features.<name>",
      type: "boolean",
      description:
        "Require a specific canonical feature key to stay enabled or disabled.",
    },
    {
      key: "features.in_app_browser",
      type: "boolean",
      description:
        "Set to `false` in `requirements.toml` to disable the in-app browser pane.",
    },
    {
      key: "features.browser_use",
      type: "boolean",
      description:
        "Set to `false` in `requirements.toml` to disable Browser Use and Browser Agent availability.",
    },
    {
      key: "features.computer_use",
      type: "boolean",
      description:
        "Set to `false` in `requirements.toml` to disable Computer Use availability and related install or enablement flows.",
    },
    {
      key: "experimental_network",
      type: "table",
      description:
        "Network access requirements enforced from `requirements.toml`. These constraints are separate from `features.network_proxy` and can configure sandboxed networking without the user feature flag.",
    },
    {
      key: "experimental_network.enabled",
      type: "boolean",
      description:
        "Enable sandboxed networking requirements. This does not grant network access when the active sandbox keeps command networking off.",
    },
    {
      key: "experimental_network.http_port",
      type: "integer",
      description:
        "Loopback HTTP listener port to use for `[experimental_network]` requirements.",
    },
    {
      key: "experimental_network.socks_port",
      type: "integer",
      description:
        "Loopback SOCKS5 listener port to use for `[experimental_network]` requirements.",
    },
    {
      key: "experimental_network.allow_upstream_proxy",
      type: "boolean",
      description:
        "Allow sandboxed networking to chain through an upstream proxy from the environment.",
    },
    {
      key: "experimental_network.dangerously_allow_non_loopback_proxy",
      type: "boolean",
      description:
        "Permit non-loopback listener addresses for `[experimental_network]` requirements. Enabling it can expose listeners beyond localhost.",
    },
    {
      key: "experimental_network.dangerously_allow_all_unix_sockets",
      type: "boolean",
      description:
        "Permit arbitrary Unix socket destinations instead of allowlist-only access. Use only in tightly controlled environments.",
    },
    {
      key: "experimental_network.domains",
      type: "map<string, allow | deny>",
      description:
        "Map-shaped administrator domain policy for sandboxed networking. Supports exact hosts, `*.example.com` for subdomains only, `**.example.com` for apex plus subdomains, and global `*` allow rules; prefer scoped rules because `*` broadly opens public outbound access. `deny` wins on conflicts. Do not combine this with `experimental_network.allowed_domains` or `experimental_network.denied_domains`.",
    },
    {
      key: "experimental_network.allowed_domains",
      type: "array<string>",
      description:
        "List-shaped administrator allow rules for sandboxed networking. Do not combine this with `experimental_network.domains`.",
    },
    {
      key: "experimental_network.denied_domains",
      type: "array<string>",
      description:
        "List-shaped administrator deny rules for sandboxed networking. Do not combine this with `experimental_network.domains`.",
    },
    {
      key: "experimental_network.managed_allowed_domains_only",
      type: "boolean",
      description:
        "When `true`, only administrator-managed allow rules remain effective while sandboxed networking requirements are active; user allowlist additions are ignored. Without managed allow rules, user-added domain allow rules do not remain effective.",
    },
    {
      key: "experimental_network.unix_sockets",
      type: "map<string, allow | deny>",
      description:
        "Administrator-managed Unix socket policy for sandboxed networking.",
    },
    {
      key: "experimental_network.allow_local_binding",
      type: "boolean",
      description:
        "Permit broader local/private-network access for sandboxed networking. Exact local IP literal or `localhost` allow rules can still permit specific local targets when this stays `false`.",
    },
    {
      key: "hooks",
      type: "table",
      description:
        "Admin-enforced managed lifecycle hooks. Requires a managed hook directory and uses the same event schema as inline `[hooks]` in `config.toml`.",
    },
    {
      key: "hooks.managed_dir",
      type: "string (absolute path)",
      description:
        "Directory containing managed hook scripts on macOS and Linux. Codex validates that it is absolute and exists before loading managed hooks.",
    },
    {
      key: "hooks.windows_managed_dir",
      type: "string (absolute path)",
      description:
        "Directory containing managed hook scripts on Windows. Codex validates that it is absolute and exists before loading managed hooks.",
    },
    {
      key: "hooks.<Event>",
      type: "array<table>",
      description:
        "Matcher groups for a hook event such as `PreToolUse`, `PermissionRequest`, `PostToolUse`, `PreCompact`, `PostCompact`, `SessionStart`, `SubagentStart`, `SubagentStop`, `UserPromptSubmit`, or `Stop`.",
    },
    {
      key: "hooks.<Event>[].hooks",
      type: "array<table>",
      description:
        "Hook handlers for a matcher group. Command hooks are currently supported; prompt and agent hook handlers are parsed but skipped.",
    },
    {
      key: "hooks.<Event>[].hooks[].commandWindows",
      type: "string",
      description:
        "Windows-only command override for command hooks. The TOML alias `command_windows` is also accepted.",
    },
    {
      key: "permissions.filesystem.deny_read",
      type: "array<string>",
      description:
        "Admin-enforced filesystem read denials. Entries can be paths or glob patterns, and users cannot weaken them with local config.",
    },
    {
      key: "mcp_servers",
      type: "table",
      description:
        "Allowlist of MCP servers that may be enabled. Both the server name (`<id>`) and its identity must match for the MCP server to be enabled. Any configured MCP server not in the allowlist (or with a mismatched identity) is disabled.",
    },
    {
      key: "mcp_servers.<id>.identity",
      type: "table",
      description:
        "Identity rule for a single MCP server. Set either `command` (stdio) or `url` (streamable HTTP).",
    },
    {
      key: "mcp_servers.<id>.identity.command",
      type: "string",
      description:
        "Allow an MCP stdio server when its `mcp_servers.<id>.command` matches this command.",
    },
    {
      key: "mcp_servers.<id>.identity.url",
      type: "string",
      description:
        "Allow an MCP streamable HTTP server when its `mcp_servers.<id>.url` matches this URL.",
    },
    {
      key: "rules",
      type: "table",
      description:
        "Admin-enforced command rules merged with `.rules` files. Requirements rules must be restrictive.",
    },
    {
      key: "rules.prefix_rules",
      type: "array<table>",
      description:
        "List of enforced prefix rules. Each rule must include `pattern` and `decision`.",
    },
    {
      key: "rules.prefix_rules[].pattern",
      type: "array<table>",
      description:
        "Command prefix expressed as pattern tokens. Each token sets either `token` or `any_of`.",
    },
    {
      key: "rules.prefix_rules[].pattern[].token",
      type: "string",
      description: "A single literal token at this position.",
    },
    {
      key: "rules.prefix_rules[].pattern[].any_of",
      type: "array<string>",
      description: "A list of allowed alternative tokens at this position.",
    },
    {
      key: "rules.prefix_rules[].decision",
      type: "prompt | forbidden",
      description:
        "Required. Requirements rules can only prompt or forbid (not allow).",
    },
    {
      key: "rules.prefix_rules[].justification",
      type: "string",
      description:
        "Optional non-empty rationale surfaced in approval prompts or rejection messages.",
    },
  ]}
  client:load
/>


---


# Environment variables

Codex uses `config.toml` for durable settings. Use environment variables for
shell-scoped overrides, automation secrets, installer behavior, or diagnostics.

This page lists stable public environment variables that Codex reads directly.
It does not list internal development variables, test variables, or
provider-specific secret names you choose yourself with
[`env_key`](https://developers.openai.com/codex/config-advanced#custom-model-providers).

## Core locations

| Variable            | Used by                                    | Default      | Description                                                                                                                                                      |
| ------------------- | ------------------------------------------ | ------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `CODEX_HOME`        | CLI, IDE extension, app-server, installers | `~/.codex`   | Sets the root for Codex state, including config, auth, logs, sessions, skills, and standalone package metadata. If you set it, the directory must already exist. |
| `CODEX_SQLITE_HOME` | CLI and app-server state                   | `CODEX_HOME` | Sets where SQLite-backed state is stored. The `sqlite_home` config option takes precedence. Relative paths resolve from the current working directory.           |

For more about the files stored under `CODEX_HOME`, see
[Config and state locations](https://developers.openai.com/codex/config-advanced#config-and-state-locations).

## Installer variables

These variables apply to the standalone install scripts served from
`https://chatgpt.com/codex/install.sh` and
`https://chatgpt.com/codex/install.ps1`.

| Variable                | Default                                                                              | Description                                                                                                                                                     |
| ----------------------- | ------------------------------------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `CODEX_NON_INTERACTIVE` | `false`                                                                              | Set to `1`, `true`, or `yes` to skip installer prompts. Prompts use their default response, so use this for scripted installs and updates, not first-run setup. |
| `CODEX_INSTALL_DIR`     | `~/.local/bin` on macOS/Linux; `%LOCALAPPDATA%\Programs\OpenAI\Codex\bin` on Windows | Changes where the visible `codex` command is installed. The standalone package cache still lives under `CODEX_HOME/packages/standalone`.                        |

For unattended installs, set `CODEX_NON_INTERACTIVE=1` on the shell that runs
the downloaded installer:

```bash
curl -fsSL https://chatgpt.com/codex/install.sh | CODEX_NON_INTERACTIVE=1 sh
```

```powershell
$env:CODEX_NON_INTERACTIVE=1; irm https://chatgpt.com/codex/install.ps1 | iex
```

## Authentication and network

| Variable               | Used by                             | Description                                                                                                                                                               |
| ---------------------- | ----------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `CODEX_API_KEY`        | `codex exec`                        | Provides an API key for a single non-interactive run. This is only supported in `codex exec`; set it inline rather than job-wide when running repository-controlled code. |
| `CODEX_ACCESS_TOKEN`   | CLI, app-server, trusted automation | Provides a ChatGPT or Codex access token for trusted automation. For persisted login, pipe it to `codex login --with-access-token`.                                       |
| `CODEX_CA_CERTIFICATE` | HTTPS, login, and WebSocket clients | Points to a PEM CA bundle for environments with corporate TLS interception or private root CAs. Takes precedence over `SSL_CERT_FILE`.                                    |
| `SSL_CERT_FILE`        | HTTPS, login, and WebSocket clients | Fallback PEM CA bundle path when `CODEX_CA_CERTIFICATE` is unset.                                                                                                         |

For provider API keys, set
[`env_key`](https://developers.openai.com/codex/config-advanced#custom-model-providers) in the model provider
configuration. Codex reads the variable named by that config, so the variable
name itself is not a fixed Codex environment variable.

For automation secret handling, see
[Use API key auth](https://developers.openai.com/codex/noninteractive#use-api-key-auth).
For access token setup, see [Access tokens](https://developers.openai.com/codex/enterprise/access-tokens).

## Diagnostics

| Variable   | Used by            | Description                                                                                                             |
| ---------- | ------------------ | ----------------------------------------------------------------------------------------------------------------------- |
| `RUST_LOG` | CLI and app-server | Controls Rust log filtering and verbosity. `codex exec` defaults to `error` output unless you set a more verbose value. |

`RUST_LOG` accepts values such as `error`, `warn`, `info`, `debug`, and
`trace`. It also accepts more targeted Rust logging filters, such as
`codex_core=debug,codex_tui=debug`.

The interactive CLI records diagnostics in bounded local stores by default, but
the plaintext `codex-tui.log` file is opt-in. Set `log_dir` explicitly when you
need a plaintext log for troubleshooting:

```bash
RUST_LOG=debug codex -c log_dir=./.codex-log
tail -F ./.codex-log/codex-tui.log
```

In non-interactive mode, `codex exec` prints messages inline instead of writing
to a separate TUI log file.


---


# Sample Configuration

Use this example configuration as a starting point. It includes most keys Codex reads from `config.toml`, along with default behaviors, recommended values where helpful, and short notes.

For explanations and guidance, see:

- [Config basics](https://developers.openai.com/codex/config-basic)
- [Advanced Config](https://developers.openai.com/codex/config-advanced)
- [Config Reference](https://developers.openai.com/codex/config-reference)
- [Sandbox and approvals](https://developers.openai.com/codex/agent-approvals-security#sandbox-and-approvals)
- [Managed configuration](https://developers.openai.com/codex/enterprise/managed-configuration)

Use the snippet below as a reference. Copy only the keys and sections you need into `~/.codex/config.toml` (or into a project-scoped `.codex/config.toml`), then adjust values for your setup.

```toml
# Codex example configuration (config.toml)
#
# This file lists the main keys Codex reads from config.toml, along with default
# behaviors, recommended examples, and concise explanations. Adjust as needed.
#
# Notes
# - Root keys must appear before tables in TOML.
# - Optional keys that default to "unset" are shown commented out with notes.
# - MCP servers, profile files, and model providers are examples; remove or edit.

################################################################################

# Core Model Selection

################################################################################

# Primary model used by Codex. Recommended example for most users: "gpt-5.5".

model = "gpt-5.5"

# Communication style for supported models. Allowed values: none | friendly | pragmatic

# personality = "pragmatic"

# Optional model override for /review. Default: unset (uses current session model).

# review_model = "gpt-5.5"

# Provider id selected from [model_providers]. Default: "openai".

model_provider = "openai"

# Default OSS provider for --oss sessions. When unset, Codex prompts. Default: unset.

# oss_provider = "ollama"

# Preferred service tier. Built-in examples: fast | flex; model catalogs can add more.

# service_tier = "flex"

# Optional manual model metadata. When unset, Codex uses model or preset defaults.

# model_context_window = 128000 # tokens; default: auto for model

# model_auto_compact_token_limit = 64000 # tokens; unset uses model defaults

# tool_output_token_limit = 12000 # tokens stored per tool output

# model_catalog_json = "/absolute/path/to/models.json" # optional startup-only model catalog override

# background_terminal_max_timeout = 300000 # ms; max empty write_stdin poll window (default 5m)

# log_dir = "/absolute/path/to/codex-logs" # log directory; setting explicitly enables codex-tui.log; default: "$CODEX_HOME/log"

# sqlite_home = "/absolute/path/to/codex-state" # optional SQLite-backed runtime state directory

################################################################################

# Reasoning & Verbosity (Responses API capable models)

################################################################################

# Reasoning effort: minimal | low | medium | high | xhigh

# model_reasoning_effort = "medium"

# Optional override used when Codex runs in plan mode: none | minimal | low | medium | high | xhigh

# plan_mode_reasoning_effort = "high"

# Reasoning summary: auto | concise | detailed | none

# model_reasoning_summary = "auto"

# Text verbosity for GPT-5 family (Responses API): low | medium | high

# model_verbosity = "medium"

# Force enable or disable reasoning summaries for current model.

# model_supports_reasoning_summaries = true

################################################################################

# Instruction Overrides

################################################################################

# Additional user instructions are injected before AGENTS.md. Default: unset.

# developer_instructions = ""

# Inline override for the history compaction prompt. Default: unset.

# compact_prompt = ""

# Override the default commit co-author trailer. This only takes effect when

# [features].codex_git_commit is enabled. When enabled and unset, Codex uses

# "Codex <noreply@openai.com>". Set to "" to disable it.

# commit_attribution = "Jane Doe <jane@example.com>"

# Override built-in base instructions with a file path. Default: unset.

# model_instructions_file = "/absolute/or/relative/path/to/instructions.txt"

# Load the compact prompt override from a file. Default: unset.

# experimental_compact_prompt_file = "/absolute/or/relative/path/to/compact_prompt.txt"

################################################################################

# Notifications

################################################################################

# External notifier program (argv array). When unset: disabled.

# notify = ["notify-send", "Codex"]

################################################################################

# Approval & Sandbox

################################################################################

# When to ask for command approval:

# - untrusted: only known-safe read-only commands auto-run; others prompt

# - on-request: model decides when to ask (default)

# - never: never prompt (risky)

# - { granular = { ... } }: allow or auto-reject selected prompt categories

approval_policy = "on-request"

# Who reviews eligible approval prompts: user (default) | auto_review

# approvals_reviewer = "user"

# Example granular policy:

# approval_policy = { granular = {

# sandbox_approval = true,

# rules = true,

# mcp_elicitations = true,

# request_permissions = false,

# skill_approval = false

# } }

# Allow login-shell semantics for shell-based tools when they request `login = true`.

# Default: true. Set false to force non-login shells and reject explicit login-shell requests.

allow_login_shell = true

# Filesystem/network sandbox policy for tool calls:

# - read-only (default)

# - workspace-write

# - danger-full-access (no sandbox; extremely risky)

sandbox_mode = "read-only"

# Named permissions profile to apply by default. Built-ins:

# :read-only | :workspace | :danger-full-access

# Use a custom name such as "workspace" only when you also define [permissions.workspace].

# default_permissions = ":workspace"

################################################################################

# Authentication & Login

################################################################################

# Where to persist CLI login credentials: file (default) | keyring | auto

cli_auth_credentials_store = "file"

# Base URL for ChatGPT auth flow (not OpenAI API).

chatgpt_base_url = "https://chatgpt.com/backend-api/"

# Optional base URL override for the built-in OpenAI provider.

# openai_base_url = "https://us.api.openai.com/v1"

# Restrict ChatGPT login to a specific workspace id. Default: unset.

# forced_chatgpt_workspace_id = "00000000-0000-0000-0000-000000000000"

# Force login mechanism when Codex would normally auto-select. Default: unset.

# Allowed values: chatgpt | api

# forced_login_method = "chatgpt"

# Preferred store for MCP OAuth credentials: auto (default) | file | keyring

mcp_oauth_credentials_store = "auto"

# Optional fixed port for MCP OAuth callback: 1-65535. Default: unset.

# mcp_oauth_callback_port = 4321

# Optional redirect URI override for MCP OAuth login (for example, remote devbox ingress).

# Custom callback paths are supported. `mcp_oauth_callback_port` still controls the listener port.

# mcp_oauth_callback_url = "https://devbox.example.internal/callback"

################################################################################

# Project Documentation Controls

################################################################################

# Max bytes from AGENTS.md to embed into first-turn instructions. Default: 32768

project_doc_max_bytes = 32768

# Ordered fallbacks when AGENTS.md is missing at a directory level. Default: []

project_doc_fallback_filenames = []

# Project root marker filenames used when searching parent directories. Default: [".git"]

# project_root_markers = [".git"]

################################################################################

# History & File Opener

################################################################################

# URI scheme for clickable citations: vscode (default) | vscode-insiders | windsurf | cursor | none

file_opener = "vscode"

################################################################################

# UI, Notifications, and Misc

################################################################################

# Suppress internal reasoning events from output. Default: false

hide_agent_reasoning = false

# Show raw reasoning content when available. Default: false

show_raw_agent_reasoning = false

# Disable burst-paste detection in the TUI. Default: false

disable_paste_burst = false

# Track Windows onboarding acknowledgement (Windows only). Default: false

windows_wsl_setup_acknowledged = false

# Check for updates on startup. Default: true

check_for_update_on_startup = true

################################################################################

# Web Search

################################################################################

# Web search mode: disabled | cached | live. Default: "cached"

# cached serves results from a web search cache (an OpenAI-maintained index).

# cached returns pre-indexed results; live fetches the most recent data.

# If you use --yolo or another full access sandbox setting, web search defaults to live.

web_search = "cached"

# Config profiles are separate files under CODEX_HOME.

# Example: ~/.codex/ci.config.toml, selected with codex --profile ci.

# Suppress the warning shown when under-development feature flags are enabled.

# suppress_unstable_features_warning = true

################################################################################

# Agents (multi-agent roles and limits)

################################################################################

[agents]

# Maximum concurrently open agent threads. Default: 6

# max_threads = 6

# Maximum nested spawn depth. Root session starts at depth 0. Default: 1

# max_depth = 1

# Default timeout per worker for spawn_agents_on_csv jobs. When unset, the tool defaults to 1800 seconds.

# job_max_runtime_seconds = 1800

# [agents.reviewer]

# description = "Find correctness, security, and test risks in code."

# config_file = "./agents/reviewer.toml" # relative to the config.toml that defines it

# nickname_candidates = ["Athena", "Ada"]

################################################################################

# Skills (per-skill overrides)

################################################################################

# Disable or re-enable a specific skill without deleting it.

[[skills.config]]

# path = "/path/to/skill/SKILL.md"

# enabled = false

################################################################################

# Sandbox settings (tables)

################################################################################

# Extra settings used only when sandbox_mode = "workspace-write".

[sandbox_workspace_write]

# Additional writable roots beyond the workspace (cwd). Default: []

writable_roots = []

# Allow outbound network access inside the sandbox. Default: false

network_access = false

# Exclude $TMPDIR from writable roots. Default: false

exclude_tmpdir_env_var = false

# Exclude /tmp from writable roots. Default: false

exclude_slash_tmp = false

################################################################################

# Shell Environment Policy for spawned processes (table)

################################################################################

[shell_environment_policy]

# inherit: all (default) | core | none

inherit = "all"

# Skip default excludes for names containing KEY/SECRET/TOKEN (case-insensitive). Default: false

ignore_default_excludes = false

# Case-insensitive glob patterns to remove (e.g., "AWS*\*", "AZURE*\*"). Default: []

exclude = []

# Explicit key/value overrides (always win). Default: {}

set = {}

# Whitelist; if non-empty, keep only matching vars. Default: []

include_only = []

# Experimental: run via user shell profile. Default: false

experimental_use_profile = false

################################################################################

# Sandboxed networking settings

################################################################################

# Enable the feature before configuring sandboxed networking rules.

# [features.network_proxy]

# enabled = true

# domains = { "api.openai.com" = "allow", "example.com" = "deny" }

#

# Exact hosts match only themselves.

# "\*.example.com" matches subdomains only; "\*\*.example.com" matches the apex plus subdomains.

# "\*" allows any public host that is not denied, so prefer scoped rules when possible.

# `allow_local_binding = false` blocks loopback and private destinations by default.

# Add an exact local IP literal or `localhost` allow rule for one target, or set it to true only when broader local access is required.

#

# Set `default_permissions = "workspace"` before enabling this profile.

# Example additional workspace roots that inherit this profile's

# `:workspace_roots` filesystem rules.

# [permissions.workspace.workspace_roots]

# "~/code/app" = true

# "~/code/shared-lib" = true

#

# Example filesystem profile. Use `"deny"` to deny reads for exact paths or

# glob patterns. On platforms that need pre-expanded glob matches, set

# glob_scan_max_depth when using unbounded patterns such as `\*\*`.

# [permissions.workspace.filesystem]

# glob_scan_max_depth = 3

# ":workspace_roots" = { "." = "write", "\*\*/\*.env" = "deny" }

# "/absolute/path/to/secrets" = "deny"

#

# [permissions.workspace.network]

# enabled = true

# proxy_url = "http://127.0.0.1:43128"

# admin_url = "http://127.0.0.1:43129"

# enable_socks5 = false

# socks_url = "http://127.0.0.1:43130"

# enable_socks5_udp = false

# allow_upstream_proxy = false

# dangerously_allow_non_loopback_proxy = false

# dangerously_allow_non_loopback_admin = false

# dangerously_allow_all_unix_sockets = false

# mode = "limited" # limited | full

# allow_local_binding = false

#

# [permissions.workspace.network.domains]

# "api.openai.com" = "allow"

# "example.com" = "deny"

#

# [permissions.workspace.network.unix_sockets]

# "/var/run/docker.sock" = "allow"

################################################################################

# History (table)

################################################################################

[history]

# save-all (default) | none

persistence = "save-all"

# Maximum bytes for history file; oldest entries are trimmed when exceeded. Example: 5242880

# max_bytes = 5242880

################################################################################

# UI, Notifications, and Misc (tables)

################################################################################

[tui]

# Desktop notifications from the TUI: boolean or filtered list. Default: true

# Examples: false | ["agent-turn-complete", "approval-requested"]

notifications = false

# Notification mechanism for terminal alerts: auto | osc9 | bel. Default: "auto"

# notification_method = "auto"

# When notifications fire: unfocused (default) | always

# notification_condition = "unfocused"

# Enables welcome/status/spinner animations. Default: true

animations = true

# Show onboarding tooltips in the welcome screen. Default: true

show_tooltips = true

# Control alternate screen usage (auto skips it in Zellij to preserve scrollback).

# alternate_screen = "auto"

# Ordered list of footer status-line item IDs. When unset, Codex uses:

# ["model-with-reasoning", "context-remaining", "current-dir"].

# Set to [] to hide the footer.

# status_line = ["model", "context-remaining", "git-branch"]

# Ordered list of terminal window/tab title item IDs. When unset, Codex uses:

# ["spinner", "project"]. Set to [] to clear the title.

# Available IDs include app-name, project, spinner, status, thread, git-branch, model,

# and task-progress.

# terminal_title = ["spinner", "project"]

# Syntax-highlighting theme (kebab-case). Use /theme in the TUI to preview and save.

# You can also add custom .tmTheme files under $CODEX_HOME/themes.

# theme = "catppuccin-mocha"

# Custom key bindings. Selected composer actions fall back to matching [tui.keymap.global] bindings.

# Use [] to unbind an action.

# [tui.keymap.global]

# open_transcript = "ctrl-t"

# open_external_editor = []

#

# [tui.keymap.composer]

# submit = ["enter", "ctrl-m"]

# [tui.keymap.chat]

# interrupt_turn = "f12"

# Internal tooltip state keyed by model slug. Usually managed by Codex.

# [tui.model_availability_nux]

# "gpt-5.4" = 1

# Enable or disable analytics for this machine. When unset, Codex uses its default behavior.

[analytics]
enabled = true

# Control whether users can submit feedback from `/feedback`. Default: true

[feedback]
enabled = true

# In-product notices (mostly set automatically by Codex).

[notice]

# hide_full_access_warning = true

# hide_world_writable_warning = true

# hide_rate_limit_model_nudge = true

# hide_gpt5_1_migration_prompt = true

# "hide_gpt-5.1-codex-max_migration_prompt" = true

# model_migrations = { "gpt-5.3-codex" = "gpt-5.4" }

################################################################################

# Centralized Feature Flags (preferred)

################################################################################

[features]

# Leave this table empty to accept defaults. Set explicit booleans to opt in/out.

# shell_tool = true

# apps = false

# hooks = false

# codex_git_commit = false

# unified_exec = true

# shell_snapshot = true

# multi_agent = true

# personality = true

# network_proxy = false

# fast_mode = true

# enable_request_compression = true

# skill_mcp_dependency_install = true

# prevent_idle_sleep = false

################################################################################

# Memories (table)

################################################################################

# Enable memories with [features].memories, then tune memory behavior here.

# [memories]

# generate_memories = true

# use_memories = true

# disable_on_external_context = false # legacy alias: no_memories_if_mcp_or_web_search

################################################################################

# Lifecycle hooks can be configured here inline or in a sibling hooks.json.

################################################################################

# [hooks]

# [[hooks.PreToolUse]]

# matcher = "^Bash$"

#

# [[hooks.PreToolUse.hooks]]

# type = "command"

# command = 'python3 "/absolute/path/to/pre_tool_use_policy.py"'

# timeout = 30

# statusMessage = "Checking Bash command"

################################################################################

# Define MCP servers under this table. Leave empty to disable.

################################################################################

[mcp_servers]

# --- Example: STDIO transport ---

# [mcp_servers.docs]

# enabled = true # optional; default true

# required = true # optional; fail startup/resume if this server cannot initialize

# command = "docs-server" # required

# args = ["--port", "4000"] # optional

# env = { "API_KEY" = "value" } # optional key/value pairs copied as-is

# env_vars = ["ANOTHER_SECRET"] # optional: forward local parent env vars

# env_vars = ["LOCAL_TOKEN", { name = "REMOTE_TOKEN", source = "remote" }]

# cwd = "/path/to/server" # optional working directory override

# experimental_environment = "remote" # experimental: run stdio via a remote executor

# startup_timeout_sec = 10.0 # optional; default 10.0 seconds

# # startup_timeout_ms = 10000 # optional alias for startup timeout (milliseconds)

# tool_timeout_sec = 60.0 # optional; default 60.0 seconds

# enabled_tools = ["search", "summarize"] # optional allow-list

# disabled_tools = ["slow-tool"] # optional deny-list (applied after allow-list)

# scopes = ["read:docs"] # optional OAuth scopes

# oauth_resource = "https://docs.example.com/" # optional OAuth resource

# --- Example: Streamable HTTP transport ---

# [mcp_servers.github]

# enabled = true # optional; default true

# required = true # optional; fail startup/resume if this server cannot initialize

# url = "https://github-mcp.example.com/mcp" # required

# bearer_token_env_var = "GITHUB_TOKEN" # optional; Authorization: Bearer <token>

# http_headers = { "X-Example" = "value" } # optional static headers

# env_http_headers = { "X-Auth" = "AUTH_ENV" } # optional headers populated from env vars

# startup_timeout_sec = 10.0 # optional

# tool_timeout_sec = 60.0 # optional

# enabled_tools = ["list_issues"] # optional allow-list

# disabled_tools = ["delete_issue"] # optional deny-list

# scopes = ["repo"] # optional OAuth scopes

################################################################################

# Model Providers

################################################################################

# Built-ins include:

# - openai

# - ollama

# - lmstudio

# - amazon-bedrock

# These IDs are reserved. Use a different ID for custom providers.

[model_providers]

# --- Example: built-in Amazon Bedrock provider options ---

# model_provider = "amazon-bedrock"

# model = "<bedrock-model-id>"

# [model_providers.amazon-bedrock.aws]

# profile = "default"

# region = "eu-central-1"

# --- Example: OpenAI data residency with explicit base URL or headers ---

# [model_providers.openaidr]

# name = "OpenAI Data Residency"

# base_url = "https://us.api.openai.com/v1" # example with 'us' domain prefix

# wire_api = "responses" # only supported value

# # requires_openai_auth = true # use only for providers backed by OpenAI auth

# # request_max_retries = 4 # default 4; max 100

# # stream_max_retries = 5 # default 5; max 100

# # stream_idle_timeout_ms = 300000 # default 300_000 (5m)

# # supports_websockets = true # optional

# # experimental_bearer_token = "sk-example" # optional dev-only direct bearer token

# # http_headers = { "X-Example" = "value" }

# # env_http_headers = { "OpenAI-Organization" = "OPENAI_ORGANIZATION", "OpenAI-Project" = "OPENAI_PROJECT" }

# --- Example: Azure/OpenAI-compatible provider ---

# [model_providers.azure]

# name = "Azure"

# base_url = "https://YOUR_PROJECT_NAME.openai.azure.com/openai"

# wire_api = "responses"

# query_params = { api-version = "2025-04-01-preview" }

# env_key = "AZURE_OPENAI_API_KEY"

# env_key_instructions = "Set AZURE_OPENAI_API_KEY in your environment"

# # supports_websockets = false

# --- Example: command-backed bearer token auth ---

# [model_providers.proxy]

# name = "OpenAI using LLM proxy"

# base_url = "https://proxy.example.com/v1"

# wire_api = "responses"

#

# [model_providers.proxy.auth]

# command = "/usr/local/bin/fetch-codex-token"

# args = ["--audience", "codex"]

# timeout_ms = 5000

# refresh_interval_ms = 300000

# --- Example: Local OSS (e.g., Ollama-compatible) ---

# [model_providers.local_ollama]

# name = "Ollama"

# base_url = "http://localhost:11434/v1"

# wire_api = "responses"

################################################################################

# Apps / Connectors

################################################################################

# Optional per-app controls.

[apps]

# [_default] applies to all apps unless overridden per app.

# [apps._default]

# enabled = true

# destructive_enabled = true

# open_world_enabled = true

#

# [apps.google_drive]

# enabled = false

# destructive_enabled = false # block destructive-hint tools for this app

# default_tools_enabled = true

# default_tools_approval_mode = "prompt" # auto | prompt | approve

#

# [apps.google_drive.tools."files/delete"]

# enabled = false

# approval_mode = "approve"

# Optional tool suggestion allowlist for connectors or plugins Codex can offer to install.

# [tool_suggest]

# discoverables = [

# { type = "connector", id = "gmail" },

# { type = "plugin", id = "figma@openai-curated" },

# ]

# disabled_tools = [

# { type = "plugin", id = "slack@openai-curated" },

# { type = "connector", id = "connector_googlecalendar" },

# ]

################################################################################

# Config Profiles (separate files)

################################################################################

# To create a config profile, put overrides in a separate profile file under $CODEX_HOME.

# Select it with codex --profile ci.

# For example, a CI profile could live at $CODEX_HOME/ci.config.toml:

# model = "gpt-5.4"

# approval_policy = "on-request"

# sandbox_mode = "read-only"

# service_tier = "flex" # or another supported service tier id

# oss_provider = "ollama"

# model_reasoning_effort = "medium"

# plan_mode_reasoning_effort = "high"

# model_reasoning_summary = "auto"

# model_verbosity = "medium"

# personality = "pragmatic" # or "friendly" or "none"

# chatgpt_base_url = "https://chatgpt.com/backend-api/"

# model_catalog_json = "./models.json"

# model_instructions_file = "/absolute/or/relative/path/to/instructions.txt"

# experimental_compact_prompt_file = "./compact_prompt.txt"

# tools_view_image = true

# features = { unified_exec = false }

################################################################################

# Projects (trust levels)

################################################################################

[projects]

# Mark specific worktrees as trusted or untrusted.

# [projects."/absolute/path/to/project"]

# trust_level = "trusted" # or "untrusted"

################################################################################

# Tools

################################################################################

[tools]

# view_image = true

################################################################################

# OpenTelemetry (OTEL) - disabled by default

################################################################################

[otel]

# Include user prompt text in logs. Default: false

log_user_prompt = false

# Environment label applied to telemetry. Default: "dev"

environment = "dev"

# Exporter: none (default) | otlp-http | otlp-grpc

exporter = "none"

# Trace exporter: none (default) | otlp-http | otlp-grpc

trace_exporter = "none"

# Metrics exporter: none | statsig | otlp-http | otlp-grpc

metrics_exporter = "statsig"

# Example OTLP/HTTP exporter configuration

# [otel.exporter."otlp-http"]

# endpoint = "https://otel.example.com/v1/logs"

# protocol = "binary" # "binary" | "json"

# [otel.exporter."otlp-http".headers]

# "x-otlp-api-key" = "${OTLP_TOKEN}"

# [otel.exporter."otlp-http".tls]

# ca-certificate = "certs/otel-ca.pem"

# client-certificate = "/etc/codex/certs/client.pem"

# client-private-key = "/etc/codex/certs/client-key.pem"

# Example OTLP/gRPC trace exporter configuration

# [otel.trace_exporter."otlp-grpc"]

# endpoint = "https://otel.example.com:4317"

# headers = { "x-otlp-meta" = "abc123" }

################################################################################

# Windows

################################################################################

[windows]

# Native Windows sandbox mode (Windows only): unelevated | elevated

sandbox = "unelevated"
```


---


# Permissions

Beta. Permission profiles are under active development and may change.

Permission profiles do not compose with the older sandbox settings. Configure
  either `default_permissions` and `[permissions]`, or `sandbox_mode` /
  `sandbox_workspace_write`, but not both. If `sandbox_mode` appears in any
  active config layer, you pass `--sandbox`, or a config profile sets
  `sandbox_mode`, Codex uses those older sandbox settings instead of
  `default_permissions`.

Permission profiles let you apply least-privilege boundaries to local commands
Codex runs on your behalf. A profile is a named policy that combines filesystem
rules, which define what commands can read or write, with network rules, which
define which destinations commands can reach.

Use profiles to give Codex enough access for the current task without granting
broad access to your machine or network. For example, a read-only profile can
let Codex inspect a project without editing it, while a write-capable profile
can limit edits to selected workspace roots.

Local permission profiles are supported on macOS, Linux, WSL, and native
Windows. Platform-specific enforcement details and caveats are covered in
[Security limitations](#security-limitations).

For Codex cloud network settings, see [Internet Access](https://developers.openai.com/codex/cloud/internet-access).

## Define and select a profile

Codex includes three built-in permission profiles:

- `:read-only` keeps local command execution read-only.
- `:workspace` allows writes inside the active workspace roots.
- `:danger-full-access` removes local sandbox restrictions and should be used
  only when that broad access is intentional.

Create a named profile under `[permissions.<name>]`, then set the top-level
`default_permissions` key to that profile name or to one of the built-ins above.
In this example, `project-edit` is a user-defined profile name, not a built-in
value.

Custom profiles use two related concepts:

- `[permissions.<name>.workspace_roots]` adds concrete directories that should
  count as workspace roots for that profile.
- `[permissions.<name>.filesystem.":workspace_roots"]` defines the filesystem
  rules Codex applies inside every effective workspace root: the current
  session's runtime workspace roots plus the profile-defined roots above.

Profiles also use the normal config-layer model. Higher-precedence layers can
add or replace entries under the same profile name without restating the whole
profile.

For example, an organization-level config and a user-level config can extend
the same profile independently:

```toml
# /etc/codex/config.toml
[permissions.server.workspace_roots]
"~/code/server" = true
```

```toml
# ~/.codex/config.toml
[permissions.server.workspace_roots]
"~/code/mobile-app" = true
```

When `server` is active, both workspace roots participate in the effective
profile.

```toml
default_permissions = "project-edit"

[permissions.project-edit.workspace_roots]
"~/code/app" = true
"~/code/shared-lib" = true

[permissions.project-edit.filesystem]
":minimal" = "read"

[permissions.project-edit.filesystem.":workspace_roots"]
"." = "write"
".devcontainer" = "read"
"**/*.env" = "deny"

[permissions.project-edit.network]
enabled = true

[permissions.project-edit.network.domains]
"api.openai.com" = "allow"
"objects.githubusercontent.com" = "allow"
"*.github.com" = "allow"
"tracking.example.com" = "deny"
```

This profile:

- Reads the minimal runtime paths common developer tools need.
- Applies the same workspace-root rules to the current session and the
  profile-defined roots.
- Keeps IDE-adjacent settings such as `.devcontainer/` read-only under each
  root.
- Denies matching environment files with a glob rule.
- Allows network access only through the configured domain policy.

Inside an active profile, narrower deny rules stay in force even when a broader
path is readable or writable. For example, a profile can make workspace roots
writable while still setting a matching `.env` path to `deny`.

## Extend a profile

Use `extends` when a profile is mostly the same as a built-in or another named
profile. Prefer extending a built-in profile over starting from scratch so
baseline protections carry forward. Extending `:workspace`, for example, keeps
the workspace root's `.codex` directory read-only unless you explicitly
override it. Set the parent once, then add or override only the rules that
differ.

```toml
default_permissions = "project-edit"

[permissions.project-edit]
description = "Project editing with OpenAI API access."
extends = ":workspace"

[permissions.project-edit.filesystem.":workspace_roots"]
"**/*.env" = "deny"

[permissions.project-edit.network]
enabled = true

[permissions.project-edit.network.domains]
"api.openai.com" = "allow"
```

This profile starts with `:workspace`, keeps matching `.env` files denied, and
allows requests to `api.openai.com`. A profile can extend `:read-only`,
`:workspace`, or another named profile. It cannot extend
`:danger-full-access`; Codex also rejects unknown parents and inheritance
cycles.

## Configuration spec

| Entry                                                             | Type / values              | Default                 | Details                                                                                                                                                                                                                                                                                        |
| ----------------------------------------------------------------- | -------------------------- | ----------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `default_permissions`                                             | String profile name        | None                    | Names the permissions profile Codex applies by default. The value must match a profile under `[permissions]` or a built-in profile such as `:workspace`. Required when permission profiles are active. If an older sandbox setting is active, Codex uses those older sandbox settings instead. |
| `[permissions.<name>]`                                            | Table                      | None                    | Defines a profile and its identifier. `default_permissions` selects one profile as the default; other permission-profile selectors also use the profile name.                                                                                                                                  |
| `permissions.<name>.description`                                  | String                     | None                    | Provides a human-readable description for the profile. A profile does not inherit its parent's description through `extends`.                                                                                                                                                                  |
| `permissions.<name>.extends`                                      | String profile name        | None                    | Starts this profile from another named profile or the built-in `:read-only` or `:workspace` profile. Codex rejects `:danger-full-access`, unknown parents, and inheritance cycles.                                                                                                             |
| `[permissions.<name>.workspace_roots]`                            | Table                      | None                    | Adds profile-defined workspace roots that receive `:workspace_roots` filesystem rules alongside the current session's runtime workspace roots.                                                                                                                                                 |
| `permissions.<name>.workspace_roots."<path>"`                     | Boolean                    | `false`                 | Adds the path to the profile's workspace root set when `true`. Entries set to `false` remain inactive.                                                                                                                                                                                         |
| `[permissions.<name>.filesystem]`                                 | Table                      | None                    | Maps filesystem paths to access values or scoped subpath maps. Missing or empty filesystem tables keep filesystem access restricted and emit a startup warning.                                                                                                                                |
| `permissions.<name>.filesystem.glob_scan_max_depth`               | Number                     | None                    | Limits deny-read glob expansion on Linux, WSL, and native Windows when Codex snapshots matches before sandbox startup. Larger values can increase startup scanning work. Use a value of at least `1` when an unbounded `**` pattern needs bounded pre-expansion.                               |
| `[permissions.<name>.filesystem]."<path>"`                        | `read`, `write`, or `deny` | None                    | Grants direct access for a supported path. `deny` denies access and wins over equally specific `write` or `read` entries. Codex rejects direct write rules that the active runtime cannot enforce.                                                                                             |
| `[permissions.<name>.filesystem."<path>"]."<subpath>"`            | `read`, `write`, or `deny` | None                    | Grants access to a descendant of `<path>`. Use `.` for the base path. Other subpaths must be relative descendants and cannot contain `.` or `..` components.                                                                                                                                   |
| `[permissions.<name>.network]`                                    | Table                      | None                    | Configures the network sandbox proxy and the sandbox network policy for the profile.                                                                                                                                                                                                           |
| `permissions.<name>.network.enabled`                              | Boolean                    | `false`                 | Enables network access for sandboxed commands in the profile. This changes the sandbox network policy; it does not start the network proxy by itself.                                                                                                                                          |
| `[permissions.<name>.network.domains]`                            | Table                      | None                    | Maps host patterns to `allow` or `deny`. If there are no `allow` entries, domain requests are blocked. Deny entries override allow entries.                                                                                                                                                    |
| `permissions.<name>.network.domains."<pattern>"`                  | `allow` or `deny`          | None                    | Supports exact hosts, `*.example.com` for subdomains, `**.example.com` for apex plus subdomains, and `*` as an allow-only global wildcard. Host patterns are normalized by trimming, lowercasing, stripping a trailing dot, and stripping simple ports or brackets.                            |
| `[permissions.<name>.network.unix_sockets]`                       | Table                      | None                    | Maps Unix socket allowlist overrides. Use only for local integrations such as Docker.                                                                                                                                                                                                          |
| `permissions.<name>.network.unix_sockets."<path>"`                | `allow` or `deny`          | None                    | Adds an absolute Unix socket path to the effective allowlist with `allow`, or rejects it with `deny`. Denied entries are omitted from the effective allowlist.                                                                                                                                 |
| `permissions.<name>.network.proxy_url`                            | URL string                 | `http://127.0.0.1:3128` | HTTP proxy listener used for `HTTP_PROXY`, `HTTPS_PROXY`, websocket proxy variables, and related tool proxy environment variables.                                                                                                                                                             |
| `permissions.<name>.network.enable_socks5`                        | Boolean                    | `true`                  | Enables the SOCKS5 listener used for `ALL_PROXY` and FTP proxy variables.                                                                                                                                                                                                                      |
| `permissions.<name>.network.socks_url`                            | URL string                 | `http://127.0.0.1:8081` | SOCKS5 listener address.                                                                                                                                                                                                                                                                       |
| `permissions.<name>.network.enable_socks5_udp`                    | Boolean                    | `true`                  | Enables SOCKS5 UDP support when the SOCKS5 listener is enabled.                                                                                                                                                                                                                                |
| `permissions.<name>.network.allow_upstream_proxy`                 | Boolean                    | `true`                  | Allows the network sandbox proxy to respect upstream `HTTP(S)_PROXY` and `ALL_PROXY` settings for outbound requests.                                                                                                                                                                           |
| `permissions.<name>.network.allow_local_binding`                  | Boolean                    | `false`                 | Disables the local/private-network guard when `true`. When `false`, exact local literals such as `localhost` or `127.0.0.1` must be explicitly allowlisted, and hostnames that resolve to local or private IPs remain blocked.                                                                 |
| `permissions.<name>.network.dangerously_allow_non_loopback_proxy` | Boolean                    | `false`                 | Allows proxy listeners to bind non-loopback addresses. Leave unset for ordinary local development.                                                                                                                                                                                             |
| `permissions.<name>.network.dangerously_allow_all_unix_sockets`   | Boolean                    | `false`                 | Bypasses the Unix socket allowlist where Unix socket proxying is supported. This is a broad local escape hatch.                                                                                                                                                                                |

## Filesystem permissions

Filesystem entries use `read`, `write`, or `deny`:

| Access  | Meaning                                                                                                                           |
| ------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `read`  | Allows commands to read files and list directories under the path. Commands cannot create, modify, rename, or delete files there. |
| `write` | Allows commands to read and modify files under the path, including creating, renaming, and deleting files when the OS allows it.  |
| `deny`  | Denies both reads and writes under the path. Use it to carve out a denied subpath from a broader `read` or `write` grant.         |

More specific entries override broader entries. When two entries target the
same path, `deny` takes precedence over `write`, and `write` takes precedence
over `read`.

This precedence lets a profile describe a broad working area first, then carve
out files or directories that should stay unreadable:

```toml
[permissions.project-edit.filesystem]
":minimal" = "read"

[permissions.project-edit.filesystem.":workspace_roots"]
"." = "write"
".devcontainer" = "read"
"**/*.env" = "deny"
```

In this example, the workspace root stays writable, `.devcontainer/` stays
readable without becoming writable, and matching environment files remain
unavailable to sandboxed commands.

A more specific path can also reopen a narrower subtree inside a broader deny:

```toml
[permissions.project-edit.filesystem]
"~/Documents" = "deny"
"~/Documents/codex" = "write"
```

Supported path forms:

| Path               | Meaning                                                                                     | Scoped subpaths |
| ------------------ | ------------------------------------------------------------------------------------------- | --------------- |
| `:root`            | The filesystem root                                                                         | `.` only        |
| `:minimal`         | Platform and runtime paths needed by common tools                                           | `.` only        |
| `:workspace_roots` | The current session's workspace roots plus any enabled profile-defined workspace roots      | Yes             |
| `:tmpdir`          | The `$TMPDIR` location, when one is available                                               | `.` only        |
| `/absolute/path`   | A platform absolute path, such as `/path` on macOS/Linux/WSL or `C:\path` on native Windows | Yes             |
| `~/path`           | A path under the current user's home directory                                              | Yes             |

On native Windows, home-relative paths can also use backslashes, such as
`~\work`.

Use `:root` only when a profile intentionally needs broad read coverage:

```toml
[permissions.audit.filesystem]
":root" = "read"
```

Use nested entries under `:workspace_roots` to scope access to workspace-root
relative subpaths:

```toml
[permissions.project-edit.filesystem.":workspace_roots"]
"." = "write"          # each workspace root
"docs" = "read"        # each workspace-root docs directory
"generated" = "deny"   # each workspace-root generated directory
```

Nested subpaths must stay inside their workspace root. Parent traversal such as
`../other-repo` is rejected.

### Deny reads with exact paths or globs

Use `deny` for files or subtrees that Codex should not read, even when a broader
profile rule grants access nearby. Exact paths work well for stable locations
such as `~/.ssh`. Glob patterns work better when a profile needs to cover a
family of sensitive files whose exact locations vary across repositories.

When a glob sits under `:workspace_roots`, Codex interprets it relative to each
effective workspace root. For example:

```toml
[permissions.project-edit.filesystem.":workspace_roots"]
"**/*.env" = "deny"
```

This rule denies reads for matching `.env` files found beneath each runtime or
profile-defined workspace root. Use it when you want to preserve normal
workspace writes while keeping environment files, generated secrets, or similar
credential-bearing files unreadable.

`deny` glob patterns are supported as deny-read rules. `read` or `write` globs
are less portable on Linux, WSL, and native Windows sandboxing, so prefer exact
paths or subtree rules such as `"docs/**" = "read"` when possible.

On Linux, WSL, and native Windows, an unbounded `**` deny-read pattern may need
bounded pre-expansion before the sandbox starts. Set `glob_scan_max_depth` when
you use an unbounded pattern such as `"**/*.env" = "deny"`:

```toml
[permissions.project-edit.filesystem]
glob_scan_max_depth = 3

[permissions.project-edit.filesystem.":workspace_roots"]
"**/*.env" = "deny"
```

`glob_scan_max_depth` must be at least `1`. Higher values scan deeper before
sandbox startup, which can add startup work on Linux, WSL, and native Windows.
If you prefer not to use bounded expansion, enumerate explicit depths such as
`*.env`, `*/*.env`, and `*/*/*.env`.

Add reusable workspace roots to the profile when the same rules should apply to
more than the current session root:

```toml
[permissions.project-edit.workspace_roots]
"~/code/app" = true
"~/code/shared-lib" = true
```

When this profile is active, Codex applies the `:workspace_roots` rules to the
current session's runtime workspace roots and to each enabled profile-defined
workspace root.

On native Windows, drive-letter paths such as `D:\work` and UNC paths such as
`\\server\share` are supported as absolute paths.

## Network permissions

Set `enabled = true` to allow network access for the selected profile:

```toml
[permissions.project-edit.network]
enabled = true
```

When network access is enabled, Codex uses full network behavior by default.
Most profiles should also define domain rules:

```toml
[permissions.project-edit.network.domains]
"example.com" = "allow"      # exact host
"*.example.com" = "allow"    # subdomains only
"**.example.com" = "allow"   # apex and subdomains
"ads.example.com" = "deny"   # deny wins over allow
```

The network sandbox proxy binds to local listeners by default:

```toml
[permissions.project-edit.network]
enabled = true
proxy_url = "http://127.0.0.1:3128"
enable_socks5 = true
socks_url = "http://127.0.0.1:8081"
enable_socks5_udp = true
```

Leave these listener settings at their defaults unless you are integrating with
a specific runtime. The `dangerously_*` network keys are escape hatches for
specialized environments and should not be used for ordinary local development.

### Local and private networks

Codex applies a local/private-network guard by default as a defense against DNS
rebinding and accidental access to local services. To intentionally allow a
literal local target, allowlist the exact host or IP literal:

```toml
[permissions.project-edit.network.domains]
"localhost" = "allow"
"127.0.0.1" = "allow"
```

Set `allow_local_binding = true` only when the profile must reach allowlisted
hostnames that resolve to local or private addresses:

```toml
[permissions.project-edit.network]
enabled = true
allow_local_binding = true

[permissions.project-edit.network.domains]
"localhost" = "allow"
```

### Unix sockets

Unix socket proxying is a local escape hatch for tools such as Docker. Use it
sparingly:

```toml
[permissions.project-edit.network.unix_sockets]
"/var/run/docker.sock" = "allow"
"/tmp/old.sock" = "deny"
```

Use `deny` to reject a socket path, including an inherited allow entry. Denied
socket paths are omitted from the effective allowlist.

When Unix sockets are enabled, keep proxy listeners bound to loopback addresses.

## Migrate from older sandbox settings

Permission profiles replace the older combination of `sandbox_mode` and
`sandbox_workspace_write` when you want one reusable profile to describe both
filesystem and network behavior. Use one system or the other for a session, not
both.

Suggested starting points:

- For a read-only workflow, use the built-in `:read-only` profile or define a
  custom profile with read access only where needed.
- For workspace editing, use the built-in `:workspace` profile or define a
  custom profile that writes through `:workspace_roots` and adds only the extra
  temp or cache paths the workflow needs.
- For unrestricted local execution, use `:danger-full-access` only when you
  intentionally want the broadest local access model.

Profiles describe the local default posture for a session. Organization-managed
requirements can still add restrictions that user configuration should not
broaden. See [Managed configuration](https://developers.openai.com/codex/enterprise/managed-configuration)
for admin-enforced filesystem and network constraints.

## Scope and enforcement

Permission profiles define the boundaries for local sandboxed command
execution. Use them together with approval policies and the separate controls
for other Codex surfaces.

### What profiles control

- **Local command execution:** Permission profiles govern sandboxed commands
  that run on your machine. App connectors, MCP servers, browser or
  computer-use surfaces, Codex cloud environment settings, and approved
  escalations use their own controls.
- **Filesystem writes:** A write-capable profile can create persistent changes.
  Treat writes to scripts, build steps, package manager hooks, shell startup
  files, and shared directories as sensitive because later tools or users can
  execute those files outside the original sandbox context.
- **Outbound destinations:** Network domain rules constrain where sandboxed
  command traffic can go through the network proxy. They do not determine
  whether an allowed destination is trustworthy, and wildcard allow rules stay
  broad.
- **Local services:** Local and private network targets are blocked by default.
  Allowlisting `localhost`, private IPs, Unix sockets, or setting
  `allow_local_binding = true` explicitly opens access to local services.

### How enforcement works

- On macOS, Codex uses Seatbelt sandbox profiles. If the selected policy cannot
  be enforced by the platform sandbox, Codex refuses to run the command instead
  of silently running it unsandboxed.
- On Linux and WSL, Codex uses [bubblewrap](https://github.com/containers/bubblewrap)
  and [seccomp](https://www.kernel.org/doc/html/latest/userspace-api/seccomp_filter.html),
  with Landlock available for compatibility fallback paths. The strongest
  enforcement path depends on user namespaces and kernel support; restricted
  container hosts can force compatibility paths, and unsupported split policies
  are refused.
- On native Windows, [`elevated` sandboxing](https://developers.openai.com/codex/windows#windows-sandbox)
  is strongest because it can use dedicated lower-privilege sandbox users,
  filesystem permission boundaries, and firewall rules. `unelevated`
  sandboxing is a fallback with weaker network isolation and cannot enforce
  every split read/write carveout, so unsupported policies are refused. Use WSL
  when you need the Linux sandbox model.

### Operational guidance

Choose the narrowest profile that still lets the task complete, especially when
you grant writes or outbound network access. Keep approval policy, secret
handling, and allow rules aligned with that access level.

## Common profiles

### Read-only with network allowlist

```toml
default_permissions = "readonly-net"

[permissions.readonly-net.filesystem]
":minimal" = "read"

[permissions.readonly-net.filesystem.":workspace_roots"]
"." = "read"

[permissions.readonly-net.network]
enabled = true

[permissions.readonly-net.network.domains]
"api.openai.com" = "allow"
```

### Workspace write without network

```toml
default_permissions = "project-edit"

[permissions.project-edit.filesystem]
":minimal" = "read"

[permissions.project-edit.filesystem.":workspace_roots"]
"." = "write"

[permissions.project-edit.network]
enabled = false
```

### Workspace write with public web access

```toml
default_permissions = "workspace-net"

[permissions.workspace-net.filesystem]
":minimal" = "read"

[permissions.workspace-net.filesystem.":workspace_roots"]
"." = "write"

[permissions.workspace-net.network]
enabled = true

[permissions.workspace-net.network.domains]
"*" = "allow"
```

Use the global `"*"` allow rule only when you intend to allow public network
access. Deny rules can narrow a broad allowlist.


---


# Speed

## Fast mode

Codex offers the ability to increase the speed of the model for increased
credit consumption.

Fast mode increases supported model speed by 1.5x and consumes credits at a
higher rate than Standard mode. It currently supports GPT-5.5 and GPT-5.4,
consuming credits at 2.5x the Standard rate for GPT-5.5 and 2x the Standard
rate for GPT-5.4.

Use `/fast on`, `/fast off`, or `/fast status` in the CLI to change or inspect
the current setting. You can also persist the default with `service_tier =
"fast"` plus `[features].fast_mode = true` in `config.toml`. Fast mode is
available in the Codex IDE extension, Codex CLI, and the Codex app when you
sign in with ChatGPT. With an API key, Codex uses standard API pricing instead
and you can't use Fast mode credits.

<VideoPlayer
  src="/videos/codex/fast-mode-demo.mp4"
  class="[&_video]:mx-auto [&_video]:max-h-[400px] [&_video]:max-w-full [&_video]:w-auto"
/>

## Codex-Spark

GPT-5.3-Codex-Spark is a separate fast, less-capable Codex model optimized for
near-instant, real-time coding iteration. Unlike fast mode, which speeds up a
supported model at a higher credit rate, Codex-Spark is its own model choice
and has its own usage limits.

During research preview Codex-Spark is only available for ChatGPT Pro subscribers.


---


# Rules

Use rules to control which commands Codex can run outside the sandbox.

Rules are experimental and may change.

## Create a rules file

1. Create a `.rules` file under a `rules/` folder next to an active config layer (for example, `~/.codex/rules/default.rules`).
2. Add a rule. This example prompts before allowing `gh pr view` to run outside the sandbox.

   ```python
   # Prompt before running commands with the prefix `gh pr view` outside the sandbox.
   prefix_rule(
       # The prefix to match.
       pattern = ["gh", "pr", "view"],

       # The action to take when Codex requests to run a matching command.
       decision = "prompt",

       # Optional rationale for why this rule exists.
       justification = "Viewing PRs is allowed with approval",

       # `match` and `not_match` are optional "inline unit tests" where you can
       # provide examples of commands that should (or should not) match this rule.
       match = [
           "gh pr view 7888",
           "gh pr view --repo openai/codex",
           "gh pr view 7888 --json title,body,comments",
       ],
       not_match = [
           # Does not match because the `pattern` must be an exact prefix.
           "gh pr --repo openai/codex view 7888",
       ],
   )
   ```

3. Restart Codex.

Codex scans `rules/` under every active config layer at startup, including [Team Config](https://developers.openai.com/codex/enterprise/admin-setup#team-config) locations and the user layer at `~/.codex/rules/`. Project-local rules under `<repo>/.codex/rules/` load only when the project `.codex/` layer is trusted.

When you add a command to the allow list in the TUI, Codex writes to the user layer at `~/.codex/rules/default.rules` so future runs can skip the prompt.

When Smart approvals are enabled (the default), Codex may propose a
`prefix_rule` for you during escalation requests. Review the suggested prefix
carefully before accepting it.

Admins can also enforce restrictive `prefix_rule` entries from
[`requirements.toml`](https://developers.openai.com/codex/enterprise/managed-configuration#admin-enforced-requirements-requirementstoml).

## Understand rule fields

`prefix_rule()` supports these fields:

- `pattern` **(required)**: A non-empty list that defines the command prefix to match. Each element is either:
  - A literal string (for example, `"pr"`).
  - A union of literals (for example, `["view", "list"]`) to match alternatives at that argument position.
- `decision` **(defaults to `"allow"`)**: The action to take when the rule matches. Codex applies the most restrictive decision when more than one rule matches (`forbidden` > `prompt` > `allow`).
  - `allow`: Run the command outside the sandbox without prompting.
  - `prompt`: Prompt before each matching invocation.
  - `forbidden`: Block the request without prompting.
- `justification` **(optional)**: A non-empty, human-readable reason for the rule. Codex may surface it in approval prompts or rejection messages. When you use `forbidden`, include a recommended alternative in the justification when appropriate (for example, `"Use \`rg\` instead of \`grep\`."`).
- `match` and `not_match` **(defaults to `[]`)**: Examples that Codex validates when it loads your rules. Use these to catch mistakes before a rule takes effect.

When Codex considers a command to run, it compares the command's argument list to `pattern`. Internally, Codex treats the command as a list of arguments (like what `execvp(3)` receives).

## Shell wrappers and compound commands

Some tools wrap several shell commands into a single invocation, for example:

```text
["bash", "-lc", "git add . && rm -rf /"]
```

Because this kind of command can hide multiple actions inside one string, Codex treats `bash -lc`, `bash -c`, and their `zsh` / `sh` equivalents specially.

### When Codex can safely split the script

If the shell script is a linear chain of commands made only of:

- plain words (no variable expansion, no `VAR=...`, `$FOO`, `*`, etc.)
- joined by safe operators (`&&`, `||`, `;`, or `|`)

then Codex parses it (using tree-sitter) and splits it into individual commands before applying your rules.

The script above is treated as two separate commands:

- `["git", "add", "."]`
- `["rm", "-rf", "/"]`

Codex then evaluates each command against your rules, and the most restrictive result wins.

Even if you allow `pattern=["git", "add"]`, Codex won't auto allow `git add . && rm -rf /`, because the `rm -rf /` portion is evaluated separately and prevents the whole invocation from being auto allowed.

This prevents dangerous commands from being smuggled in alongside safe ones.

### When Codex does not split the script

If the script uses more advanced shell features, such as:

- redirection (`>`, `>>`, `<`)
- substitutions (`$(...)`, `...`)
- environment variables (`FOO=bar`)
- wildcard patterns (`*`, `?`)
- control flow (`if`, `for`, `&&` with assignments, etc.)

then Codex doesn't try to interpret or split it.

In those cases, the entire invocation is treated as:

```text
["bash", "-lc", "<full script>"]
```

and your rules are applied to that **single** invocation.

With this handling, you get the security of per-command evaluation when it's safe to do so, and conservative behavior when it isn't.

## Test a rule file

Use `codex execpolicy check` to test how your rules apply to a command:

```shell
codex execpolicy check --pretty \
  --rules ~/.codex/rules/default.rules \
  -- gh pr view 7888 --json title,body,comments
```

The command emits JSON showing the strictest decision and any matching rules, including any `justification` values from matched rules. Use more than one `--rules` flag to combine files, and add `--pretty` to format the output.

## Understand the rules language

The `.rules` file format uses `Starlark` (see the [language spec](https://github.com/bazelbuild/starlark/blob/master/spec.md)). Its syntax is like Python, but it's designed to be safe to run: the rules engine can run it without side effects (for example, touching the filesystem).


---


# Hooks

Hooks are an extensibility framework for Codex. They allow
you to inject your own scripts into the agentic loop, enabling features such as:

- Send the conversation to a custom logging/analytics engine
- Scan your team's prompts to block accidentally pasting API keys
- Summarize conversations to create persistent memories automatically
- Run a custom validation check when a conversation turn stops, enforcing standards
- Customize prompting when in a certain directory

Hooks are enabled by default. If you need to turn them off in `config.toml`,
set:

```toml
[features]
hooks = false
```

Use `hooks` as the canonical feature key. `codex_hooks` still works as a
deprecated alias.

Admins can force hooks off the same way in `requirements.toml` with
`[features].hooks = false`.

Runtime behavior to keep in mind:

- Matching hooks from multiple files all run.
- Multiple matching command hooks for the same event are launched concurrently,
  so one hook cannot prevent another matching hook from starting.
- Non-managed command hooks must be reviewed and trusted before they run.
- `PreToolUse`, `PermissionRequest`, `PostToolUse`, `PreCompact`,
  `PostCompact`, `UserPromptSubmit`, `SubagentStop`, and `Stop` run at turn
  scope. `SessionStart` and `SubagentStart` run at thread or subagent-start
  scope.

## Where Codex looks for hooks

Codex discovers hooks next to active config layers in either of these forms:

- `hooks.json`
- inline `[hooks]` tables inside `config.toml`

Installed plugins can also bundle lifecycle config through their plugin
manifest or a default `hooks/hooks.json` file. See [Build
plugins](https://developers.openai.com/codex/plugins/build#bundled-mcp-servers-and-lifecycle-config) for the
plugin packaging rules.

In practice, the four most useful locations are:

- `~/.codex/hooks.json`
- `~/.codex/config.toml`
- `<repo>/.codex/hooks.json`
- `<repo>/.codex/config.toml`

If more than one hook source exists, Codex loads all matching hooks.
Higher-precedence config layers don't replace lower-precedence hooks.
If a single layer contains both `hooks.json` and inline `[hooks]`, Codex
merges them and warns at startup. Prefer one representation per layer.

Codex can also discover hooks bundled with enabled plugins. Plugin-bundled
hooks load alongside other hook sources and use the same trust-review flow as
other non-managed hooks.

Project-local hooks load only when the project `.codex/` layer is trusted. In
untrusted projects, Codex still loads user and system hooks from their own
active config layers.

## Review and trust hooks

Codex lists configured hooks before deciding which ones can run. Before a
non-managed command hook can run, Codex requires you to review and trust the
exact hook definition. Codex records trust against the hook's current hash, so
new or changed hooks are marked for review and skipped until trusted.

Use `/hooks` in the CLI to inspect hook sources, review new or changed hooks,
trust hooks, or disable individual non-managed hooks. If hooks need review at
startup, Codex prints a warning that tells you to open `/hooks`.

Managed hooks from system, MDM, cloud, or `requirements.toml` sources are marked
as managed, trusted by policy, and can't be disabled from the user hook browser.

For one-off automation that already vets hook sources outside Codex, pass
`--dangerously-bypass-hook-trust` to run enabled hooks without requiring
persisted hook trust for that invocation.

## Config shape

Hooks are organized in three levels:

- A hook event such as `PreToolUse`, `PostToolUse`, `PreCompact`,
  `SubagentStart`, or `Stop`
- A matcher group that decides when that event matches
- One or more hook handlers that run when the matcher group matches

```json
{
  "hooks": {
    "SessionStart": [
      {
        "matcher": "startup|resume",
        "hooks": [
          {
            "type": "command",
            "command": "python3 ~/.codex/hooks/session_start.py",
            "statusMessage": "Loading session notes"
          }
        ]
      }
    ],
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [
          {
            "type": "command",
            "command": "/usr/bin/python3 \"$(git rev-parse --show-toplevel)/.codex/hooks/pre_tool_use_policy.py\"",
            "statusMessage": "Checking Bash command"
          }
        ]
      }
    ],
    "PermissionRequest": [
      {
        "matcher": "Bash",
        "hooks": [
          {
            "type": "command",
            "command": "/usr/bin/python3 \"$(git rev-parse --show-toplevel)/.codex/hooks/permission_request.py\"",
            "statusMessage": "Checking approval request"
          }
        ]
      }
    ],
    "PostToolUse": [
      {
        "matcher": "Bash",
        "hooks": [
          {
            "type": "command",
            "command": "/usr/bin/python3 \"$(git rev-parse --show-toplevel)/.codex/hooks/post_tool_use_review.py\"",
            "statusMessage": "Reviewing Bash output"
          }
        ]
      }
    ],
    "UserPromptSubmit": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "/usr/bin/python3 \"$(git rev-parse --show-toplevel)/.codex/hooks/user_prompt_submit_data_flywheel.py\""
          }
        ]
      }
    ],
    "Stop": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "/usr/bin/python3 \"$(git rev-parse --show-toplevel)/.codex/hooks/stop_continue.py\"",
            "timeout": 30
          }
        ]
      }
    ]
  }
}
```

Notes:

- `timeout` is in seconds.
- If `timeout` is omitted, Codex uses `600` seconds.
- `statusMessage` is optional.
- `commandWindows` is an optional Windows-only command override. In TOML, use
  `command_windows` or `commandWindows`.
- `async` is parsed, but async command hooks aren't supported yet. Codex skips
  handlers with `async: true`.
- Only `type: "command"` handlers run today. `prompt` and `agent` handlers are
  parsed but skipped.
- Commands run with the session `cwd` as their working directory.
- For repo-local hooks, prefer resolving from the git root instead of using a
  relative path such as `.codex/hooks/...`. Codex may be started from a
  subdirectory, and a git-root-based path keeps the hook location stable.

Equivalent inline TOML in `config.toml`:

```toml
[[hooks.PreToolUse]]
matcher = "^Bash$"

[[hooks.PreToolUse.hooks]]
type = "command"
command = '/usr/bin/python3 "$(git rev-parse --show-toplevel)/.codex/hooks/pre_tool_use_policy.py"'
timeout = 30
statusMessage = "Checking Bash command"

[[hooks.PostToolUse]]
matcher = "^Bash$"

[[hooks.PostToolUse.hooks]]
type = "command"
command = '/usr/bin/python3 "$(git rev-parse --show-toplevel)/.codex/hooks/post_tool_use_review.py"'
timeout = 30
statusMessage = "Reviewing Bash output"
```

## Managed hooks from `requirements.toml`

Enterprise-managed requirements can also define hooks inline under `[hooks]`.
This is useful when admins want to enforce the hook configuration while
delivering the actual scripts through MDM or another device-management system.
To enforce managed hooks even for users who disabled hooks locally, pin
`[features].hooks = true` in `requirements.toml` alongside `[hooks]`. To ignore
user, project, session, and plugin hooks while still allowing administrator
managed hooks, set `allow_managed_hooks_only = true`.

```toml
allow_managed_hooks_only = true

[features]
hooks = true

[hooks]
managed_dir = "/enterprise/hooks"
windows_managed_dir = 'C:\enterprise\hooks'

[[hooks.PreToolUse]]
matcher = "^Bash$"

[[hooks.PreToolUse.hooks]]
type = "command"
command = "python3 /enterprise/hooks/pre_tool_use_policy.py"
command_windows = 'py -3 C:\enterprise\hooks\pre_tool_use_policy.py'
timeout = 30
statusMessage = "Checking managed Bash command"
```

Notes for managed hooks:

- `managed_dir` is used on macOS and Linux.
- `windows_managed_dir` is used on Windows.
- Codex doesn't distribute the scripts in `managed_dir`; your enterprise
  tooling must install and update them separately.
- Managed hook commands should use absolute script paths under the configured
  managed directory.
- `allow_managed_hooks_only = true` skips hooks from user, project, session, and
  plugin sources, but still loads managed hooks from `requirements.toml` and
  other managed config layers.

## Plugin-bundled hooks

When a plugin is enabled, Codex can load lifecycle hooks from that plugin
alongside user, project, and managed hooks.

By default, Codex looks for `hooks/hooks.json` inside the plugin root. A plugin
manifest can override that default with a `hooks` entry in
`.codex-plugin/plugin.json`. The manifest entry can be a `./`-prefixed path, an
array of `./`-prefixed paths, an inline hooks object, or an array of inline
hooks objects.

```json
{
  "name": "repo-policy",
  "hooks": "./hooks/hooks.json"
}
```

Manifest hook paths are resolved relative to the plugin root and must stay
inside that root. If a manifest defines `hooks`, Codex uses those manifest
entries instead of the default `hooks/hooks.json`.

Plugin hook commands receive these environment variables:

- `PLUGIN_ROOT` is a Codex-specific extension that points to the installed
  plugin root.
- `PLUGIN_DATA` is a Codex-specific extension that points to the plugin's
  writable data directory.
- Codex also sets `CLAUDE_PLUGIN_ROOT` and `CLAUDE_PLUGIN_DATA` for
  compatibility with existing plugin hooks.

Plugin hooks use the same event schema as other hooks. Installing or enabling a
plugin doesn't automatically trust its hooks; Codex skips plugin-bundled hooks
until you review and trust the current hook definition.

## Matcher patterns

The `matcher` field is a regex string that filters when hooks fire. Use `"*"`,
`""`, or omit `matcher` entirely to match every occurrence of a supported
event.

Only some current Codex events honor `matcher`:

| Event               | What `matcher` filters | Notes                                                        |
| ------------------- | ---------------------- | ------------------------------------------------------------ |
| `PermissionRequest` | tool name              | Support includes `Bash`, `apply_patch`\*, and MCP tool names |
| `PostToolUse`       | tool name              | Support includes `Bash`, `apply_patch`\*, and MCP tool names |
| `PostCompact`       | compaction trigger     | Values are `manual` or `auto`                                |
| `PreCompact`        | compaction trigger     | Values are `manual` or `auto`                                |
| `PreToolUse`        | tool name              | Support includes `Bash`, `apply_patch`\*, and MCP tool names |
| `SessionStart`      | start source           | Values are `startup`, `resume`, `clear`, and `compact`       |
| `SubagentStart`     | subagent type          | Values depend on the subagent that starts                    |
| `SubagentStop`      | subagent type          | Values depend on the subagent that stops                     |
| `UserPromptSubmit`  | not supported          | Any configured `matcher` is ignored for this event           |
| `Stop`              | not supported          | Any configured `matcher` is ignored for this event           |

\*For `apply_patch`, `matcher` values can also use `Edit` or `Write`.

Examples:

- `Bash`
- `^apply_patch$`
- `Edit|Write`
- `mcp__filesystem__read_file`
- `mcp__filesystem__.*`
- `startup|resume|clear|compact`
- `manual|auto`

## Common input fields

Every command hook receives one JSON object on `stdin`.

These are the shared fields you will usually use:

| Field             | Type             | Meaning                                                             |
| ----------------- | ---------------- | ------------------------------------------------------------------- |
| `session_id`      | `string`         | Current Codex session id. Subagent hooks use the parent session id. |
| `transcript_path` | `string \| null` | Path to the session transcript file, if any                         |
| `cwd`             | `string`         | Working directory for the session                                   |
| `hook_event_name` | `string`         | Current hook event name                                             |
| `model`           | `string`         | Codex-specific extension. Active model slug                         |

Turn-scoped hooks list `turn_id` as a Codex-specific extension in their
event-specific tables.

`SessionStart`, `PreToolUse`, `PermissionRequest`, `PostToolUse`,
`UserPromptSubmit`, `SubagentStart`, `SubagentStop`, and `Stop` also include
`permission_mode`, which describes the current permission mode as `default`,
`acceptEdits`, `plan`, `dontAsk`, or `bypassPermissions`.

`transcript_path` points to a conversation transcript for convenience, but the
transcript format is not a stable interface for hooks and may change over time.

If you need the full wire format, see [Schemas](#schemas).

## Common output fields

`SessionStart`, `PreCompact`, `PostCompact`, `UserPromptSubmit`,
`SubagentStop`, and `Stop` support these shared JSON fields. `SubagentStart`
accepts the same shape for `systemMessage` and hook-specific context, but
`continue: false` doesn't stop the subagent:

```json
{
  "continue": true,
  "stopReason": "optional",
  "systemMessage": "optional",
  "suppressOutput": false
}
```

| Field            | Effect                                          |
| ---------------- | ----------------------------------------------- |
| `continue`       | If `false`, marks that hook run as stopped      |
| `stopReason`     | Recorded as the reason for stopping             |
| `systemMessage`  | Surfaced as a warning in the UI or event stream |
| `suppressOutput` | Parsed today but not yet implemented            |

Exit `0` with no output is treated as success and Codex continues.

`PreToolUse` and `PermissionRequest` support `systemMessage`, but `continue`,
`stopReason`, and `suppressOutput` aren't currently supported for those events.
If a `PreToolUse` hook returns one of those unsupported fields, Codex marks
that hook run as failed, reports the error, and continues the tool call.

`PostToolUse` supports `systemMessage`, `continue: false`, and `stopReason`.
`suppressOutput` is parsed but not currently supported for that event.

## Hooks

### SessionStart

`matcher` is applied to `source` for this event.

Fields in addition to [Common input fields](#common-input-fields):

| Field    | Type     | Meaning                                                             |
| -------- | -------- | ------------------------------------------------------------------- |
| `source` | `string` | How the session started: `startup`, `resume`, `clear`, or `compact` |

Plain text on `stdout` is added as extra developer context.

JSON on `stdout` supports [Common output fields](#common-output-fields) and this
hook-specific shape:

```json
{
  "hookSpecificOutput": {
    "hookEventName": "SessionStart",
    "additionalContext": "Load the workspace conventions before editing."
  }
}
```

That `additionalContext` text is added as extra developer context.

### SubagentStart

`matcher` is applied to `agent_type` for this event.

Fields in addition to [Common input fields](#common-input-fields):

| Field             | Type     | Meaning                                        |
| ----------------- | -------- | ---------------------------------------------- |
| `turn_id`         | `string` | Codex-specific extension. Active Codex turn id |
| `agent_id`        | `string` | Identifier for the subagent                    |
| `agent_type`      | `string` | Subagent type or profile                       |
| `permission_mode` | `string` | Current permission mode                        |

Plain text on `stdout` is added as extra developer context for the subagent.

JSON on `stdout` supports `systemMessage` and this hook-specific shape:

```json
{
  "hookSpecificOutput": {
    "hookEventName": "SubagentStart",
    "additionalContext": "Review the repository test conventions first."
  }
}
```

That `additionalContext` text is added as extra developer context for the
subagent. `continue: false` is parsed for compatibility, but it doesn't stop the
subagent from starting.

### PreToolUse

`PreToolUse` can intercept Bash, file edits performed through `apply_patch`,
and MCP tool calls. It's still a guardrail rather than a complete enforcement
boundary because Codex can often perform equivalent work through another
supported tool path.

This doesn't intercept all shell calls yet, only the simple ones. The newer
  `unified_exec` mechanism allows richer streaming stdin/stdout handling of
  shell, but interception is incomplete. Similarly, this doesn't intercept
  `WebSearch` or other non-shell, non-MCP tool calls.

`matcher` is applied to `tool_name` and matcher aliases. For file edits through
`apply_patch`, `matcher` values can use `apply_patch`, `Edit`, or `Write`; hook input
still reports `tool_name: "apply_patch"`.

Fields in addition to [Common input fields](#common-input-fields):

| Field         | Type         | Meaning                                                                                                    |
| ------------- | ------------ | ---------------------------------------------------------------------------------------------------------- |
| `turn_id`     | `string`     | Codex-specific extension. Active Codex turn id                                                             |
| `tool_name`   | `string`     | Canonical hook tool name, such as `Bash`, `apply_patch`, or an MCP name like `mcp__fs__read`               |
| `tool_use_id` | `string`     | Tool-call id for this invocation                                                                           |
| `tool_input`  | `JSON value` | Tool-specific input. `Bash` and `apply_patch` use `tool_input.command` while MCP tools send all arguments. |

Plain text on `stdout` is ignored.

JSON on `stdout` can use `systemMessage`. To deny a supported tool call, return
this hook-specific shape:

```json
{
  "hookSpecificOutput": {
    "hookEventName": "PreToolUse",
    "permissionDecision": "deny",
    "permissionDecisionReason": "Destructive command blocked by hook."
  }
}
```

Codex also accepts this older block shape:

```json
{
  "decision": "block",
  "reason": "Destructive command blocked by hook."
}
```

You can also use exit code `2` and write the blocking reason to `stderr`.

To add model-visible context without blocking, return
`hookSpecificOutput.additionalContext`:

```json
{
  "hookSpecificOutput": {
    "hookEventName": "PreToolUse",
    "additionalContext": "The pending command touches generated files."
  }
}
```

To rewrite a supported tool call without blocking, return
`permissionDecision: "allow"` with `updatedInput`:

```json
{
  "hookSpecificOutput": {
    "hookEventName": "PreToolUse",
    "permissionDecision": "allow",
    "updatedInput": {
      "command": "echo rewritten"
    }
  }
}
```

For Bash commands and `apply_patch`, `updatedInput` must include a string
`command` field. For MCP tools, `updatedInput` is the replacement arguments
object. Return `updatedInput` only with `permissionDecision: "allow"`; other
`updatedInput` shapes are reported as errors.

`permissionDecision: "ask"`, legacy `decision: "approve"`, `continue: false`,
`stopReason`, and `suppressOutput` are parsed but not supported yet. Codex marks
the hook run as failed, reports the error, and continues the tool call.

### PermissionRequest

`PermissionRequest` runs when Codex is about to ask for approval, such as a
shell escalation or managed-network approval. It can allow the request, deny
the request, or decline to decide and let the normal approval prompt continue.
It doesn't run for commands that don't need approval.

`matcher` is applied to `tool_name` and matcher aliases. Current canonical
values include `Bash`, `apply_patch`, and MCP tool names such as
`mcp__server__tool`; `apply_patch` also matches `Edit` and `Write`.

Fields in addition to [Common input fields](#common-input-fields):

| Field                    | Type             | Meaning                                                                                                   |
| ------------------------ | ---------------- | --------------------------------------------------------------------------------------------------------- |
| `turn_id`                | `string`         | Codex-specific extension. Active Codex turn id                                                            |
| `tool_name`              | `string`         | Canonical hook tool name, such as `Bash`, `apply_patch`, or an MCP name like `mcp__fs__read`              |
| `tool_input`             | `JSON value`     | Tool-specific input. `Bash` and `apply_patch` use `tool_input.command` while MCP tools send all the args. |
| `tool_input.description` | `string \| null` | Human-readable approval reason, when Codex has one                                                        |

Plain text on `stdout` is ignored.

Some tool inputs may include a human-readable description, but don't rely on a
`tool_input.description` field for every tool.

To approve the request, return:

```json
{
  "hookSpecificOutput": {
    "hookEventName": "PermissionRequest",
    "decision": {
      "behavior": "allow"
    }
  }
}
```

To deny the request, return:

```json
{
  "hookSpecificOutput": {
    "hookEventName": "PermissionRequest",
    "decision": {
      "behavior": "deny",
      "message": "Blocked by repository policy."
    }
  }
}
```

If multiple matching hooks return decisions, any `deny` wins. Otherwise, an
`allow` lets the request proceed without surfacing the approval prompt. If no
matching hook decides, Codex uses the normal approval flow.

Don't return `updatedInput`, `updatedPermissions`, or `interrupt` for
`PermissionRequest`; those fields are reserved for future behavior and fail
closed today.

### PostToolUse

`PostToolUse` runs after supported tools produce output, including Bash,
`apply_patch`, and MCP tool calls. For Bash, it also runs after commands that
exit with a non-zero status. It can't undo side effects from the tool that
already ran.

This doesn't intercept all shell calls yet, only the simple ones. The newer
  `unified_exec` mechanism allows richer streaming stdin/stdout handling of
  shell, but interception is incomplete. Similarly, this doesn't intercept
  `WebSearch` or other non-shell, non-MCP tool calls.

`matcher` is applied to `tool_name` and matcher aliases. For file edits through
`apply_patch`, `matcher` values can use `apply_patch`, `Edit`, or `Write`; hook input
still reports `tool_name: "apply_patch"`.

Fields in addition to [Common input fields](#common-input-fields):

| Field           | Type         | Meaning                                                                                                    |
| --------------- | ------------ | ---------------------------------------------------------------------------------------------------------- |
| `turn_id`       | `string`     | Codex-specific extension. Active Codex turn id                                                             |
| `tool_name`     | `string`     | Canonical hook tool name, such as `Bash`, `apply_patch`, or an MCP name like `mcp__fs__read`               |
| `tool_use_id`   | `string`     | Tool-call id for this invocation                                                                           |
| `tool_input`    | `JSON value` | Tool-specific input. `Bash` and `apply_patch` use `tool_input.command` while MCP tools send all arguments. |
| `tool_response` | `JSON value` | Tool-specific output. For MCP tools, this is the MCP call result.                                          |

Plain text on `stdout` is ignored.

JSON on `stdout` can use `systemMessage` and this hook-specific shape:

```json
{
  "decision": "block",
  "reason": "The Bash output needs review before continuing.",
  "hookSpecificOutput": {
    "hookEventName": "PostToolUse",
    "additionalContext": "The command updated generated files."
  }
}
```

That `additionalContext` text is added as extra developer context.

For this event, `decision: "block"` doesn't undo the completed Bash command.
Instead, Codex records the feedback, replaces the tool result with that
feedback, and continues the model from the hook-provided message.

You can also use exit code `2` and write the feedback reason to `stderr`.

To stop normal processing of the original tool result after the command has
already run, return `continue: false`. Codex will replace the tool result with
your feedback or stop text and continue from there.

`updatedMCPToolOutput` and `suppressOutput` are parsed but not supported yet.
Codex marks the hook run as failed, reports the error, and continues normal
processing of the tool result.

### PreCompact

`PreCompact` runs before Codex compacts the conversation. `matcher` is applied
to `trigger`, whose values are `manual` and `auto`.

Fields in addition to [Common input fields](#common-input-fields):

| Field     | Type     | Meaning                                        |
| --------- | -------- | ---------------------------------------------- |
| `turn_id` | `string` | Codex-specific extension. Active Codex turn id |
| `trigger` | `string` | What triggered compaction: `manual` or `auto`  |

Plain text on `stdout` is ignored.

JSON on `stdout` supports [Common output fields](#common-output-fields). If a
matching `PreCompact` hook returns `continue: false`, Codex stops before
compacting.

### PostCompact

`PostCompact` runs after Codex compacts the conversation. `matcher` is applied
to `trigger`, whose values are `manual` and `auto`.

Fields in addition to [Common input fields](#common-input-fields):

| Field     | Type     | Meaning                                        |
| --------- | -------- | ---------------------------------------------- |
| `turn_id` | `string` | Codex-specific extension. Active Codex turn id |
| `trigger` | `string` | What triggered compaction: `manual` or `auto`  |

Plain text on `stdout` is ignored.

JSON on `stdout` supports [Common output fields](#common-output-fields). If a
matching `PostCompact` hook returns `continue: false`, Codex stops after
compacting.

### UserPromptSubmit

`matcher` isn't currently used for this event.

Fields in addition to [Common input fields](#common-input-fields):

| Field     | Type     | Meaning                                        |
| --------- | -------- | ---------------------------------------------- |
| `turn_id` | `string` | Codex-specific extension. Active Codex turn id |
| `prompt`  | `string` | User prompt that's about to be sent            |

Plain text on `stdout` is added as extra developer context.

JSON on `stdout` supports [Common output fields](#common-output-fields) and
this hook-specific shape:

```json
{
  "hookSpecificOutput": {
    "hookEventName": "UserPromptSubmit",
    "additionalContext": "Ask for a clearer reproduction before editing files."
  }
}
```

That `additionalContext` text is added as extra developer context.

To block the prompt, return:

```json
{
  "decision": "block",
  "reason": "Ask for confirmation before doing that."
}
```

You can also use exit code `2` and write the blocking reason to `stderr`.

### SubagentStop

`matcher` is applied to `agent_type` for this event.

Fields in addition to [Common input fields](#common-input-fields):

| Field                    | Type             | Meaning                                         |
| ------------------------ | ---------------- | ----------------------------------------------- |
| `turn_id`                | `string`         | Codex-specific extension. Active Codex turn id  |
| `agent_id`               | `string`         | Identifier for the subagent                     |
| `agent_type`             | `string`         | Subagent type or profile                        |
| `agent_transcript_path`  | `string \| null` | Path to the subagent transcript file, if any    |
| `stop_hook_active`       | `boolean`        | Whether this subagent was already continued     |
| `last_assistant_message` | `string \| null` | Latest subagent assistant message, if available |

`SubagentStop` expects JSON on `stdout` when it exits `0`. Plain text output is
invalid for this event.

JSON on `stdout` supports [Common output fields](#common-output-fields). To ask
Codex to continue the subagent flow, return:

```json
{
  "decision": "block",
  "reason": "Run one more focused pass inside the subagent."
}
```

You can also use exit code `2` and write the continuation reason to `stderr`.

If any matching `SubagentStop` hook returns `continue: false`, that takes
precedence over continuation decisions from other matching `SubagentStop`
hooks.

### Stop

`matcher` isn't currently used for this event.

Fields in addition to [Common input fields](#common-input-fields):

| Field                    | Type             | Meaning                                           |
| ------------------------ | ---------------- | ------------------------------------------------- |
| `turn_id`                | `string`         | Codex-specific extension. Active Codex turn id    |
| `stop_hook_active`       | `boolean`        | Whether this turn was already continued by `Stop` |
| `last_assistant_message` | `string \| null` | Latest assistant message text, if available       |

`Stop` expects JSON on `stdout` when it exits `0`. Plain text output is invalid
for this event.

JSON on `stdout` supports [Common output fields](#common-output-fields). To keep
Codex going, return:

```json
{
  "decision": "block",
  "reason": "Run one more pass over the failing tests."
}
```

You can also use exit code `2` and write the continuation reason to `stderr`.

For this event, `decision: "block"` doesn't reject the turn. Instead, it tells
Codex to continue and automatically creates a new continuation prompt that acts
as a new user prompt, using your `reason` as that prompt text.

If any matching `Stop` hook returns `continue: false`, that takes precedence
over continuation decisions from other matching `Stop` hooks.

## Schemas

The linked `main` branch schemas may include hook fields that are not in the
  current release. Use this page as the release behavior reference.

If you need the exact current wire format, see the generated schemas in the
[Codex GitHub repository](https://github.com/openai/codex/tree/main/codex-rs/hooks/schema/generated).


---


# Custom instructions with AGENTS.md

Codex reads `AGENTS.md` files before doing any work. By layering global guidance with project-specific overrides, you can start each task with consistent expectations, no matter which repository you open.

## How Codex discovers guidance

Codex builds an instruction chain when it starts (once per run; in the TUI this usually means once per launched session). Discovery follows this precedence order:

1. **Global scope:** In your Codex home directory (defaults to `~/.codex`, unless you set `CODEX_HOME`), Codex reads `AGENTS.override.md` if it exists. Otherwise, Codex reads `AGENTS.md`. Codex uses only the first non-empty file at this level.
2. **Project scope:** Starting at the project root (typically the Git root), Codex walks down to your current working directory. If Codex cannot find a project root, it only checks the current directory. In each directory along the path, it checks for `AGENTS.override.md`, then `AGENTS.md`, then any fallback names in `project_doc_fallback_filenames`. Codex includes at most one file per directory.
3. **Merge order:** Codex concatenates files from the root down, joining them with blank lines. Files closer to your current directory override earlier guidance because they appear later in the combined prompt.

Codex skips empty files and stops adding files once the combined size reaches the limit defined by `project_doc_max_bytes` (32 KiB by default). For details on these knobs, see [Project instructions discovery](https://developers.openai.com/codex/config-advanced#project-instructions-discovery). Raise the limit or split instructions across nested directories when you hit the cap.

## Create global guidance

Create persistent defaults in your Codex home directory so every repository inherits your working agreements.

1. Ensure the directory exists:

   ```bash
   mkdir -p ~/.codex
   ```

2. Create `~/.codex/AGENTS.md` with reusable preferences:

   ```md
   # ~/.codex/AGENTS.md

   ## Working agreements

   - Always run `npm test` after modifying JavaScript files.
   - Prefer `pnpm` when installing dependencies.
   - Ask for confirmation before adding new production dependencies.
   ```

3. Run Codex anywhere to confirm it loads the file:

   ```bash
   codex --ask-for-approval never "Summarize the current instructions."
   ```

   Expected: Codex quotes the items from `~/.codex/AGENTS.md` before proposing work.

Use `~/.codex/AGENTS.override.md` when you need a temporary global override without deleting the base file. Remove the override to restore the shared guidance.

## Layer project instructions

Repository-level files keep Codex aware of project norms while still inheriting your global defaults.

1. In your repository root, add an `AGENTS.md` that covers basic setup:

   ```md
   # AGENTS.md

   ## Repository expectations

   - Run `npm run lint` before opening a pull request.
   - Document public utilities in `docs/` when you change behavior.
   ```

2. Add overrides in nested directories when specific teams need different rules. For example, inside `services/payments/` create `AGENTS.override.md`:

   ```md
   # services/payments/AGENTS.override.md

   ## Payments service rules

   - Use `make test-payments` instead of `npm test`.
   - Never rotate API keys without notifying the security channel.
   ```

3. Start Codex from the payments directory:

   ```bash
   codex --cd services/payments --ask-for-approval never "List the instruction sources you loaded."
   ```

   Expected: Codex reports the global file first, the repository root `AGENTS.md` second, and the payments override last.

Codex stops searching once it reaches your current directory, so place overrides as close to specialized work as possible.

Here is a sample repository after you add a global file and a payments-specific override:

<FileTree
  class="mt-4"
  tree={[
    {
      name: "AGENTS.md",
      comment: "Repository expectations",
      highlight: true,
    },
    {
      name: "services/",
      open: true,
      children: [
        {
          name: "payments/",
          open: true,
          children: [
            {
              name: "AGENTS.md",
              comment: "Ignored because an override exists",
            },
            {
              name: "AGENTS.override.md",
              comment: "Payments service rules",
              highlight: true,
            },
            { name: "README.md" },
          ],
        },
        {
          name: "search/",
          children: [{ name: "AGENTS.md" }, { name: "…", placeholder: true }],
        },
      ],
    },
  ]}
/>

## Customize fallback filenames

If your repository already uses a different filename (for example `TEAM_GUIDE.md`), add it to the fallback list so Codex treats it like an instructions file.

1. Edit your Codex configuration:

   ```toml
   # ~/.codex/config.toml
   project_doc_fallback_filenames = ["TEAM_GUIDE.md", ".agents.md"]
   project_doc_max_bytes = 65536
   ```

2. Restart Codex or run a new command so the updated configuration loads.

Now Codex checks each directory in this order: `AGENTS.override.md`, `AGENTS.md`, `TEAM_GUIDE.md`, `.agents.md`. Filenames not on this list are ignored for instruction discovery. The larger byte limit allows more combined guidance before truncation.

With the fallback list in place, Codex treats the alternate files as instructions:

<FileTree
  class="mt-4"
  tree={[
    {
      name: "TEAM_GUIDE.md",
      comment: "Detected via fallback list",
      highlight: true,
    },
    {
      name: ".agents.md",
      comment: "Fallback file in root",
    },
    {
      name: "support/",
      open: true,
      children: [
        {
          name: "AGENTS.override.md",
          comment: "Overrides fallback guidance",
          highlight: true,
        },
        {
          name: "playbooks/",
          children: [{ name: "…", placeholder: true }],
        },
      ],
    },
  ]}
/>

Set the `CODEX_HOME` environment variable when you want a different profile, such as a project-specific automation user:

```bash
CODEX_HOME=$(pwd)/.codex codex exec "List active instruction sources"
```

Expected: The output lists files relative to the custom `.codex` directory.

## Verify your setup

- Run `codex --ask-for-approval never "Summarize the current instructions."` from a repository root. Codex should echo guidance from global and project files in precedence order.
- Use `codex --cd subdir --ask-for-approval never "Show which instruction files are active."` to confirm nested overrides replace broader rules.
- To audit which instruction files Codex loaded, opt into a plaintext TUI log with `codex -c log_dir=./.codex-log` and check `./.codex-log/codex-tui.log`, or inspect the most recent `session-*.jsonl` file if you enabled session logging.
- If instructions look stale, restart Codex in the target directory. Codex rebuilds the instruction chain on every run (and at the start of each TUI session), so there is no cache to clear manually.

## Troubleshoot discovery issues

- **Nothing loads:** Verify you are in the intended repository and that `codex status` reports the workspace root you expect. Ensure instruction files contain content; Codex ignores empty files.
- **Wrong guidance appears:** Look for an `AGENTS.override.md` higher in the directory tree or under your Codex home. Rename or remove the override to fall back to the regular file.
- **Codex ignores fallback names:** Confirm you listed the names in `project_doc_fallback_filenames` without typos, then restart Codex so the updated configuration takes effect.
- **Instructions truncated:** Raise `project_doc_max_bytes` or split large files across nested directories to keep critical guidance intact.
- **Profile confusion:** Run `echo $CODEX_HOME` before launching Codex. A non-default value points Codex at a different home directory than the one you edited.

## Next steps

- Visit the official [AGENTS.md](https://agents.md) website for more information.
- Review [Prompting Codex](https://developers.openai.com/codex/prompting) for conversational patterns that pair well with persistent guidance.


---


# Model Context Protocol

Model Context Protocol (MCP) connects models to tools and context. Use it to give Codex access to third-party documentation, or to let it interact with developer tools like your browser or Figma.

Codex supports MCP servers in both the CLI and the IDE extension.

## Supported MCP features

- **STDIO servers**: Servers that run as a local process (started by a command).
  - Environment variables
- **Streamable HTTP servers**: Servers that you access at an address.
  - Bearer token authentication
  - OAuth authentication (run `codex mcp login <server-name>` for servers that support OAuth)
- **Server instructions**: Codex reads the MCP `instructions` field returned during initialization and uses it as server-wide guidance alongside the server's tools.

If you build or maintain an MCP server for Codex, use `instructions` for cross-tool workflows, constraints, and rate limits that apply across the server. Keep the first 512 characters self-contained so the most important guidance is available when Codex is deciding how to use the server.

## Connect Codex to an MCP server

Codex stores MCP configuration in `config.toml` alongside other Codex configuration settings. By default this is `~/.codex/config.toml`, but you can also scope MCP servers to a project with `.codex/config.toml` (trusted projects only).

The CLI and the IDE extension share this configuration. Once you configure your MCP servers, you can switch between the two Codex clients without redoing setup.

To configure MCP servers, choose one option:

1. **Use the CLI**: Run `codex mcp` to add and manage servers.
2. **Edit `config.toml`**: Update `~/.codex/config.toml` (or a project-scoped `.codex/config.toml` in trusted projects) directly.

### Configure with the CLI

#### Add an MCP server

```bash
codex mcp add <server-name> --env VAR1=VALUE1 --env VAR2=VALUE2 -- <stdio server-command>
```

For example, to add Context7 (a free MCP server for developer documentation), you can run the following command:

```bash
codex mcp add context7 -- npx -y @upstash/context7-mcp
```

#### Other CLI commands

To see all available MCP commands, you can run `codex mcp --help`.

#### Terminal UI (TUI)

In the `codex` TUI, use `/mcp` to see your active MCP servers.

### Configure with config.toml

For more fine-grained control over MCP server options, edit `~/.codex/config.toml` (or a project-scoped `.codex/config.toml`). In the IDE extension, select **MCP settings** > **Open config.toml** from the gear menu.

Configure each MCP server with a `[mcp_servers.<server-name>]` table in the configuration file.

#### STDIO servers

- `command` (required): The command that starts the server.
- `args` (optional): Arguments to pass to the server.
- `env` (optional): Environment variables to set for the server.
- `env_vars` (optional): Environment variables to allow and forward.
- `cwd` (optional): Working directory to start the server from.
- `experimental_environment` (optional): Set to `remote` to start the stdio
  server through a remote executor environment when one is available.

`env_vars` can contain plain variable names or objects with a source:

```toml
env_vars = ["LOCAL_TOKEN", { name = "REMOTE_TOKEN", source = "remote" }]
```

String entries and `source = "local"` read from Codex's local environment.
`source = "remote"` reads from the remote executor environment and requires
remote MCP stdio.

#### Streamable HTTP servers

- `url` (required): The server address.
- `bearer_token_env_var` (optional): Environment variable name for a bearer token to send in `Authorization`.
- `http_headers` (optional): Map of header names to static values.
- `env_http_headers` (optional): Map of header names to environment variable names (values pulled from the environment).

#### Other configuration options

- `startup_timeout_sec` (optional): Timeout (seconds) for the server to start. Default: `10`.
- `tool_timeout_sec` (optional): Timeout (seconds) for the server to run a tool. Default: `60`.
- `enabled` (optional): Set `false` to disable a server without deleting it.
- `required` (optional): Set `true` to make startup fail if this enabled server can't initialize.
- `enabled_tools` (optional): Tool allow list.
- `disabled_tools` (optional): Tool deny list (applied after `enabled_tools`).
- `default_tools_approval_mode` (optional): Default approval behavior for
  tools from this server. Supported values are `auto`, `prompt`, and
  `approve`.
- `tools.<tool>.approval_mode` (optional): Per-tool approval behavior override.

If your OAuth provider requires a fixed callback port, set the top-level `mcp_oauth_callback_port` in `config.toml`. If unset, Codex binds to an ephemeral port.

If your MCP OAuth flow must use a specific callback URL (for example, a remote Devbox ingress URL or a custom callback path), set `mcp_oauth_callback_url`. Codex uses this value as the OAuth `redirect_uri` while still using `mcp_oauth_callback_port` for the callback listener port. Local callback URLs (for example `localhost`) bind on the local interface; non-local callback URLs bind on `0.0.0.0` so the callback can reach the host.

If the MCP server advertises `scopes_supported`, Codex prefers those
server-advertised scopes during OAuth login. Otherwise, Codex falls back to the
scopes configured in `config.toml`.

#### config.toml examples

```toml
[mcp_servers.context7]
command = "npx"
args = ["-y", "@upstash/context7-mcp"]
env_vars = ["LOCAL_TOKEN"]

[mcp_servers.context7.env]
MY_ENV_VAR = "MY_ENV_VALUE"
```

```toml
# Optional MCP OAuth callback overrides (used by `codex mcp login`)
mcp_oauth_callback_port = 5555
mcp_oauth_callback_url = "https://devbox.example.internal/callback"
```

```toml
[mcp_servers.figma]
url = "https://mcp.figma.com/mcp"
bearer_token_env_var = "FIGMA_OAUTH_TOKEN"
http_headers = { "X-Figma-Region" = "us-east-1" }
```

```toml
[mcp_servers.chrome_devtools]
url = "http://localhost:3000/mcp"
enabled_tools = ["open", "screenshot"]
disabled_tools = ["screenshot"] # applied after enabled_tools
default_tools_approval_mode = "prompt"
startup_timeout_sec = 20
tool_timeout_sec = 45
enabled = true

[mcp_servers.chrome_devtools.tools.open]
approval_mode = "approve"
```

### Plugin-provided MCP servers

Installed plugins can bundle MCP servers in their plugin manifest. Those
servers are launched from the plugin, so user config doesn't set their
transport command. User config can still control on/off state and tool policy
under `plugins.<plugin>.mcp_servers.<server>`.

```toml
[plugins."sample@test".mcp_servers.sample]
enabled = true
default_tools_approval_mode = "prompt"
enabled_tools = ["read", "search"]

[plugins."sample@test".mcp_servers.sample.tools.search]
approval_mode = "approve"
```

## Examples of useful MCP servers

The list of MCP servers keeps growing. Here are a few common ones:

- [OpenAI Docs MCP](https://developers.openai.com/learn/docs-mcp): Search and read OpenAI developer docs.
- [Context7](https://github.com/upstash/context7): Connect to up-to-date developer documentation.
- Figma [Local](https://developers.figma.com/docs/figma-mcp-server/local-server-installation/) and [Remote](https://developers.figma.com/docs/figma-mcp-server/remote-server-installation/): Access your Figma designs.
- [Playwright](https://www.npmjs.com/package/@playwright/mcp): Control and inspect a browser using Playwright.
- [Chrome Developer Tools](https://github.com/ChromeDevTools/chrome-devtools-mcp/): Control and inspect Chrome.
- [Sentry](https://docs.sentry.io/product/sentry-mcp/#codex): Access Sentry logs.
- [GitHub](https://github.com/github/github-mcp-server): Manage GitHub beyond what `git` supports (for example, pull requests and issues).


---


# Plugins

## Overview

Plugins bundle skills, app integrations, and MCP servers into reusable
workflows for Codex.

Extend what Codex can do, for example:

- Install the Codex Security plugin to scan authorized code and confirm
  plausible vulnerability findings.
- Install the Gmail plugin to let Codex read and manage Gmail.
- Install the Google Drive plugin to work across Drive, Docs, Sheets, and
  Slides.
- Install the Slack plugin to summarize channels or draft replies.
- Install [Sites](https://developers.openai.com/codex/sites) to create and deploy hosted websites,
  web apps, and games.

A plugin can contain:

- **Skills:** reusable instructions for specific kinds of work. Codex can load
  them when needed so it follows the right steps and uses the right references
  or helper scripts for a task.
- **Apps:** connections to tools like GitHub, Slack, or Google Drive, so
  Codex can read information from those tools and take actions in them.
- **MCP servers:** services that give Codex access to more tools or shared
  information, often from systems outside your local project.

You can share plugins by publishing them through a marketplace source, such as a
repo marketplace for a project or team. See [Build plugins](https://developers.openai.com/codex/plugins/build)
for marketplace setup, packaging, and distribution guidance.

## Use and install plugins

### Plugin Directory in the Codex app

Open **Plugins** in the Codex app to browse and install curated plugins.

<CodexScreenshot
  alt="Codex Plugins page"
  lightSrc="/images/codex/plugins/directory.webp"
  darkSrc="/images/codex/plugins/directory-dark.webp"
/>

The plugin directory groups plugins into categories:

- **Curated by OpenAI:** highlighted plugins available to all Codex users.
- **Shared with you:** plugins shared by other members of your ChatGPT
  workspace.
- **Created by you:** plugins you created or added to your own workspace.

### Plugin directory in the CLI

In Codex CLI, run the following command to open the plugins list:

```text
codex
/plugins
```

<CodexScreenshot
  alt="Plugins list in Codex CLI"
  lightSrc="/images/codex/plugins/cli_light.png"
  darkSrc="/images/codex/plugins/codex-plugin-cli.png"
/>

The CLI plugin browser groups plugins by marketplace. Use the marketplace tabs
to switch sources, open a plugin to inspect details, install or uninstall
marketplace entries, and press <kbd>Space</kbd> on an installed plugin to toggle
its enabled state.

### Install and use a plugin

Once you open the plugin directory:

<WorkflowSteps>

1. Search or browse for a plugin, then open its details.
2. Select the install button. In the app, select the plus button or
   **Add to Codex**. In the CLI, select `Install plugin`.
3. If the plugin needs an external app, connect it when prompted. Some plugins
   ask you to authenticate during install. Others wait until the first time you
   use them.
4. After installation, start a new thread and ask Codex to use the plugin.

</WorkflowSteps>

After you install a plugin, you can use it directly in the prompt window:

<CodexScreenshot
  alt="Codex Plugins page"
  lightSrc="/images/codex/plugins/plugin-github-invoke.png"
  darkSrc="/images/codex/plugins/plugin-github-invoke-dark.png"
/>

<div class="not-prose mt-4 grid gap-4 md:grid-cols-2">
  <div class="rounded-xl border border-subtle bg-surface px-5 py-4">
    <p class="text-sm font-semibold text-default">Describe the task directly</p>
    <p class="mt-2 text-sm text-secondary">
      Ask for the outcome you want, such as "Summarize unread Gmail threads
      from today" or "Pull the latest launch notes from Google Drive."
    </p>
    <p class="mt-3 text-sm text-secondary">
      Use this when you want Codex to choose the right installed tools for the
      task.
    </p>
  </div>

  <div class="rounded-xl border border-subtle bg-surface px-5 py-4">
    <p class="text-sm font-semibold text-default">Choose a specific plugin</p>
    <p class="mt-2 text-sm text-secondary">
      Type <code>@</code> to invoke the plugin or one of its bundled skills
      explicitly.
    </p>
    <p class="mt-3 text-sm text-secondary">
      Use this when you want to be specific about which plugin or skill Codex
      should use. See <a href="/codex/app/commands">Codex app commands</a> and 
      <a href="/codex/skills">Skills</a>.
    </p>
  </div>
</div>

### How permissions and data sharing work

Installing a plugin makes its workflows available in Codex, but your existing
[approval settings](https://developers.openai.com/codex/agent-approvals-security) still apply. Any
connected external services remain subject to their own authentication,
privacy, and data-sharing policies.

- Bundled skills are available as soon as you install the plugin.
- If a plugin includes apps, Codex may prompt you to install or sign in to
  those apps in ChatGPT during setup or the first time you use them.
- If a plugin includes MCP servers, they may require extra setup or
  authentication before you can use them.
- When Codex sends data through a bundled app, that app's terms and privacy
  policy apply.

### Remove or turn off a plugin

To remove a plugin, reopen it from the plugin browser and select
**Uninstall plugin**.

Uninstalling a plugin removes the plugin bundle from Codex, but bundled apps
stay installed until you manage them in ChatGPT.

If you want to keep a plugin installed but turn it off, set its entry in
`~/.codex/config.toml` to `enabled = false`, then restart Codex:

```toml
[plugins."gmail@openai-curated"]
enabled = false
```

## Build your own plugin

If you want to create, test, or distribute your own plugin, see
[Build plugins](https://developers.openai.com/codex/plugins/build). That page covers local scaffolding,
manual marketplace setup, workspace sharing, plugin manifests, and packaging
guidance.

## Plugin guides

- [Codex Security plugin](https://developers.openai.com/codex/security/plugin): Scan authorized code,
  confirm findings, and prepare reviewed fixes.


---


# Build plugins

This page is for plugin authors. If you want to browse, install, and use
plugins in Codex, see [Plugins](https://developers.openai.com/codex/plugins). If you are still iterating on
one repo or one personal workflow, start with a local skill. Build a plugin
when you want to share that workflow across teams, bundle app integrations or
MCP config, package lifecycle hooks, or publish a stable package.

## Create a plugin with `@plugin-creator`

For the fastest setup, use the built-in `@plugin-creator` skill.

<CodexScreenshot
  alt="plugin-creator skill in Codex"
  lightSrc="/images/codex/plugins/plugin-creator.png"
  darkSrc="/images/codex/plugins/plugin-creator-dark.png"
/>

It scaffolds the required `.codex-plugin/plugin.json` manifest and can also
generate a local marketplace entry for testing. If you already have a plugin
folder, you can still use `@plugin-creator` to wire it into a local
marketplace.

<CodexScreenshot
  alt="how to invoke the plugin-creator skill"
  lightSrc="/images/codex/plugins/plugin-creator-invoke.png"
  darkSrc="/images/codex/plugins/plugin-creator-invoke-dark.png"
/>

### Build your own curated plugin list

A marketplace is a JSON catalog of plugins. `@plugin-creator` can generate one
for a single plugin, and you can keep adding entries to that same marketplace
to build your own curated list for a repo, team, or personal workflow.

In Codex, each marketplace appears as a selectable source in the plugin
directory. Use `$REPO_ROOT/.agents/plugins/marketplace.json` for a repo-scoped
list or `~/.agents/plugins/marketplace.json` for a personal list. Add one
entry per plugin under `plugins[]`, point each `source.path` at the plugin
folder with a `./`-prefixed path relative to the marketplace root, and set
`interface.displayName` to the label you want Codex to show in the marketplace
picker. Then restart Codex. After that, open the plugin directory, choose your
marketplace, and browse or install the plugins in that curated list.

You don't need a separate marketplace per plugin. One marketplace can expose a
single plugin while you are testing, then grow into a larger curated catalog as
you add more plugins.

<CodexScreenshot
  alt="custom local marketplace in the plugin directory"
  lightSrc="/images/codex/plugins/codex-local-plugin-light.png"
  darkSrc="/images/codex/plugins/codex-local-plugin.png"
/>

### Add a marketplace from the CLI

Use `codex plugin marketplace add` when you want Codex to install and track a
marketplace source for you instead of editing `config.toml` by hand.

```bash
codex plugin marketplace add owner/repo
codex plugin marketplace add owner/repo --ref main
codex plugin marketplace add https://github.com/example/plugins.git --sparse .agents/plugins
codex plugin marketplace add ./local-marketplace-root
```

Marketplace sources can be GitHub shorthand (`owner/repo` or
`owner/repo@ref`), HTTP or HTTPS Git URLs, SSH Git URLs, or local marketplace root
directories. Use `--ref` to pin a Git ref, and repeat `--sparse PATH` to use a
sparse checkout for Git-backed marketplace repos. `--sparse` is valid only for
Git marketplace sources.

To inspect, refresh, or remove configured marketplaces:

```bash
codex plugin marketplace list
codex plugin marketplace upgrade
codex plugin marketplace upgrade marketplace-name
codex plugin marketplace remove marketplace-name
```

`codex plugin marketplace list` prints each marketplace Codex is considering
and the root path it resolves from, including local default marketplaces and
configured marketplace snapshots.

### Create a plugin manually

Start with a minimal plugin that packages one skill.

1. Create a plugin folder with a manifest at `.codex-plugin/plugin.json`.

```bash
mkdir -p my-first-plugin/.codex-plugin
```

`my-first-plugin/.codex-plugin/plugin.json`

```json
{
  "name": "my-first-plugin",
  "version": "1.0.0",
  "description": "Reusable greeting workflow",
  "skills": "./skills/"
}
```

Use a stable plugin `name` in kebab-case. Codex uses it as the plugin
identifier and component namespace.

2. Add a skill under `skills/<skill-name>/SKILL.md`.

```bash
mkdir -p my-first-plugin/skills/hello
```

`my-first-plugin/skills/hello/SKILL.md`

```md
---
name: hello
description: Greet the user with a friendly message.
---

Greet the user warmly and ask how you can help.
```

3. Add the plugin to a marketplace. Use `@plugin-creator` to generate one, or
   follow [Build your own curated plugin list](#build-your-own-curated-plugin-list)
   to wire the plugin into Codex manually.

From there, you can add MCP config, app integrations, or marketplace metadata
as needed.

### Install a local plugin manually

Use a repo marketplace or a personal marketplace, depending on who should be
able to access the plugin or curated list.

<Tabs
  id="codex-plugins-local-install"
  param="install-scope"
  defaultTab="workspace"
  tabs={[
    {
      id: "workspace",
      label: "Repo",
    },
    {
      id: "global",
      label: "Personal",
    },
  ]}
>
  <div slot="workspace">
    Add a marketplace file at `$REPO_ROOT/.agents/plugins/marketplace.json`
    and store your plugins under `$REPO_ROOT/plugins/`.

    **Repo marketplace example**

    Step 1: Copy the plugin folder into `$REPO_ROOT/plugins/my-plugin`.

```bash
mkdir -p ./plugins
cp -R /absolute/path/to/my-plugin ./plugins/my-plugin
```

    Step 2: Add or update `$REPO_ROOT/.agents/plugins/marketplace.json` so
    that `source.path` points to that plugin directory with a `./`-prefixed
    relative path:

```json
{
  "name": "local-repo",
  "plugins": [
    {
      "name": "my-plugin",
      "source": {
        "source": "local",
        "path": "./plugins/my-plugin"
      },
      "policy": {
        "installation": "AVAILABLE",
        "authentication": "ON_INSTALL"
      },
      "category": "Productivity"
    }
  ]
}
```

    Step 3: Restart Codex and verify that the plugin appears.

  </div>

  <div slot="global">
    Add a marketplace file at `~/.agents/plugins/marketplace.json` and store
    your plugins under `~/.codex/plugins/`.

    **Personal marketplace example**

    Step 1: Copy the plugin folder into `~/.codex/plugins/my-plugin`.

```bash
mkdir -p ~/.codex/plugins
cp -R /absolute/path/to/my-plugin ~/.codex/plugins/my-plugin
```

    Step 2: Add or update `~/.agents/plugins/marketplace.json` so that the
    plugin entry's `source.path` points to that directory.

    Step 3: Restart Codex and verify that the plugin appears.

  </div>
</Tabs>

The marketplace file points to the plugin location, so those directories are
examples rather than fixed requirements. Codex resolves `source.path` relative
to the marketplace root, not relative to the `.agents/plugins/` folder. See
[Marketplace metadata](#marketplace-metadata) for the file format.

After you change the plugin, update the plugin directory that your marketplace
entry points to and restart Codex so the local install picks up the new files.

### Share a local plugin with your workspace

After you create a plugin and add it to Codex, you can share it with other
members of your ChatGPT workspace from the Codex app.

1. Open **Plugins** in the Codex app.
2. Go to **Created by you** and open the plugin details page.
3. Select **Share**.
4. Add workspace members or copy a share link.
5. Choose who has access, then send the invitation or link.

People you share with can find the plugin under **Shared with you** in the
plugin directory. Sharing a local plugin with your workspace doesn't publish
it to the public Plugin Directory. Shared plugins stay within your workspace
and organization boundary; accounts that aren't signed in to that workspace
can't access them. Use a marketplace when you want repo or CLI distribution,
and use workspace sharing when you want selected teammates to install a plugin
from the Codex app.

Workspace admins can disable plugin sharing from cloud-managed requirements by
adding `plugin_sharing = false` to `requirements.toml`:

```toml
plugin_sharing = false
```

### Marketplace metadata

If you maintain a repo marketplace, define it in
`$REPO_ROOT/.agents/plugins/marketplace.json`. For a personal marketplace, use
`~/.agents/plugins/marketplace.json`. A marketplace file controls plugin
ordering and install policies in Codex-facing catalogs. It can represent one
plugin while you are testing or a curated list of plugins that you want Codex
to show together under one marketplace name. Before you add a plugin to a
marketplace, make sure its `version`, publisher metadata, and install-surface
copy are ready for other developers to see.

```json
{
  "name": "local-example-plugins",
  "interface": {
    "displayName": "Local Example Plugins"
  },
  "plugins": [
    {
      "name": "my-plugin",
      "source": {
        "source": "local",
        "path": "./plugins/my-plugin"
      },
      "policy": {
        "installation": "AVAILABLE",
        "authentication": "ON_INSTALL"
      },
      "category": "Productivity"
    },
    {
      "name": "research-helper",
      "source": {
        "source": "local",
        "path": "./plugins/research-helper"
      },
      "policy": {
        "installation": "AVAILABLE",
        "authentication": "ON_INSTALL"
      },
      "category": "Productivity"
    }
  ]
}
```

- Use top-level `name` to identify the marketplace.
- Use `interface.displayName` for the marketplace title shown in Codex.
- Add one object per plugin under `plugins` to build a curated list that Codex
  shows under that marketplace title.
- Point each plugin entry's `source.path` at the plugin directory you want
  Codex to load. For repo installs, that often lives under `./plugins/`. For
  personal installs, a common pattern is `./.codex/plugins/<plugin-name>`.
- Keep `source.path` relative to the marketplace root, start it with `./`, and
  keep it inside that root.
- For local entries, `source` can also be a plain string path such as
  `"./plugins/my-plugin"`.
- Always include `policy.installation`, `policy.authentication`, and
  `category` on each plugin entry.
- Use `policy.installation` values such as `AVAILABLE`,
  `INSTALLED_BY_DEFAULT`, or `NOT_AVAILABLE`.
- Use `policy.authentication` to decide whether auth happens on install or
  first use.

The marketplace controls where Codex loads the plugin from. A local
`source.path` can point somewhere else if your plugin lives outside those
example directories. A marketplace file can live in the repo where you are
developing the plugin or in a separate marketplace repo, and one marketplace
file can point to one plugin or many.

Marketplace entries can also point at Git-backed plugin sources. Use
`"source": "url"` when the plugin lives at the repository root, or
`"source": "git-subdir"` when the plugin lives in a subdirectory:

```json
{
  "name": "remote-helper",
  "source": {
    "source": "git-subdir",
    "url": "https://github.com/example/codex-plugins.git",
    "path": "./plugins/remote-helper",
    "ref": "main"
  },
  "policy": {
    "installation": "AVAILABLE",
    "authentication": "ON_INSTALL"
  },
  "category": "Productivity"
}
```

Git-backed entries may use `ref` or `sha` selectors. If Codex can't resolve a
marketplace entry's source, it skips that plugin entry instead of failing the
whole marketplace.

### How Codex uses marketplaces

A plugin marketplace is a JSON catalog of plugins that Codex can read and
install.

Codex can read marketplace files from:

- the curated marketplace that powers the official Plugin Directory
- a repo marketplace at `$REPO_ROOT/.agents/plugins/marketplace.json`
- a legacy-compatible marketplace at `$REPO_ROOT/.claude-plugin/marketplace.json`
- a personal marketplace at `~/.agents/plugins/marketplace.json`

You can install any plugin exposed through a marketplace. Codex installs
plugins into
`~/.codex/plugins/cache/$MARKETPLACE_NAME/$PLUGIN_NAME/$VERSION/`. For local
plugins, `$VERSION` is `local`, and Codex loads the installed copy from that
cache path rather than directly from the marketplace entry.

You can enable or disable each plugin individually. Codex stores each plugin's
on or off state in `~/.codex/config.toml`.

## Package and distribute plugins

### Plugin structure

Every plugin has a manifest at `.codex-plugin/plugin.json`. It can also include
a `skills/` directory, a `hooks/` directory for lifecycle hooks, an `.app.json`
file that points at one or more apps or connectors, an `.mcp.json` file that
configures MCP servers, and assets used to present the plugin across supported
surfaces.

<FileTree
  class="mt-4"
  tree={[
    {
      name: "my-plugin/",
      open: true,
      children: [
        {
          name: ".codex-plugin/",
          open: true,
          children: [
            {
              name: "plugin.json",
              comment: "Required: plugin manifest",
            },
          ],
        },
        {
          name: "skills/",
          open: true,
          children: [
            {
              name: "my-skill/",
              open: true,
              children: [
                {
                  name: "SKILL.md",
                  comment: "Optional: skill instructions",
                },
              ],
            },
          ],
        },
        {
          name: "hooks/",
          open: true,
          children: [
            {
              name: "hooks.json",
              comment: "Optional: lifecycle hooks",
            },
          ],
        },
        {
          name: ".app.json",
          comment: "Optional: app or connector mappings",
        },
        {
          name: ".mcp.json",
          comment: "Optional: MCP server configuration",
        },
        {
          name: "assets/",
          comment: "Optional: icons, logos, screenshots",
        },
      ],
    },
  ]}
/>

Only `plugin.json` belongs in `.codex-plugin/`. Keep `skills/`, `hooks/`,
`assets/`, `.mcp.json`, and `.app.json` at the plugin root.

Published plugins typically use a richer manifest than the minimal example that
appears in quick-start scaffolds. The manifest has three jobs:

- Identify the plugin.
- Point to bundled components such as skills, apps, MCP servers, or hooks.
- Provide install-surface metadata such as descriptions, icons, and legal
  links.

Here's a complete manifest example:

```json
{
  "name": "my-plugin",
  "version": "0.1.0",
  "description": "Bundle reusable skills and app integrations.",
  "author": {
    "name": "Your team",
    "email": "team@example.com",
    "url": "https://example.com"
  },
  "homepage": "https://example.com/plugins/my-plugin",
  "repository": "https://github.com/example/my-plugin",
  "license": "MIT",
  "keywords": ["research", "crm"],
  "skills": "./skills/",
  "mcpServers": "./.mcp.json",
  "apps": "./.app.json",
  "hooks": "./hooks/hooks.json",
  "interface": {
    "displayName": "My Plugin",
    "shortDescription": "Reusable skills and apps",
    "longDescription": "Distribute skills and app integrations together.",
    "developerName": "Your team",
    "category": "Productivity",
    "capabilities": ["Read", "Write"],
    "websiteURL": "https://example.com",
    "privacyPolicyURL": "https://example.com/privacy",
    "termsOfServiceURL": "https://example.com/terms",
    "defaultPrompt": [
      "Use My Plugin to summarize new CRM notes.",
      "Use My Plugin to triage new customer follow-ups."
    ],
    "brandColor": "#10A37F",
    "composerIcon": "./assets/icon.png",
    "logo": "./assets/logo.png",
    "screenshots": ["./assets/screenshot-1.png"]
  }
}
```

`.codex-plugin/plugin.json` is the required entry point. The other manifest
fields are optional, but published plugins commonly use them.

### Manifest fields

Use the top-level fields to define package metadata and point to bundled
components:

- `name`, `version`, and `description` identify the plugin.
- `author`, `homepage`, `repository`, `license`, and `keywords` provide
  publisher and discovery metadata.
- `skills`, `mcpServers`, `apps`, and `hooks` point to bundled components
  relative to the plugin root.
- `interface` controls how install surfaces present the plugin.

Use the `interface` object for install-surface metadata:

- `displayName`, `shortDescription`, and `longDescription` control the title
  and descriptive copy.
- `developerName`, `category`, and `capabilities` add publisher and capability
  metadata.
- `websiteURL`, `privacyPolicyURL`, and `termsOfServiceURL` provide external
  links.
- `defaultPrompt`, `brandColor`, `composerIcon`, `logo`, and `screenshots`
  control starter prompts and visual presentation.

### Path rules

- Keep manifest paths relative to the plugin root and start them with `./`.
- Store visual assets such as `composerIcon`, `logo`, and `screenshots` under
  `./assets/` when possible.
- Use `skills` for bundled skill folders, `apps` for `.app.json`,
  `mcpServers` for `.mcp.json`, and `hooks` for lifecycle hooks.
- Enabled plugins can include lifecycle hooks alongside skills, MCP servers, and
  apps.
- If your plugin stores hooks at `./hooks/hooks.json`, you do not need a
  `hooks` entry in `.codex-plugin/plugin.json`; Codex checks that default file
  automatically.

### Bundled MCP servers and lifecycle hooks

`mcpServers` can point to an `.mcp.json` file that contains either a direct
server map or a wrapped `mcp_servers` object.

Direct server map:

```json
{
  "docs": {
    "command": "docs-mcp",
    "args": ["--stdio"]
  }
}
```

Wrapped server map:

```json
{
  "mcp_servers": {
    "docs": {
      "command": "docs-mcp",
      "args": ["--stdio"]
    }
  }
}
```

After installation, users can enable or disable a bundled MCP server and tune
tool approval policy from their Codex config without editing the plugin. Use
`plugins.<plugin>.mcp_servers.<server>` for plugin-scoped MCP server policy:

```toml
[plugins."my-plugin".mcp_servers.docs]
enabled = true
default_tools_approval_mode = "prompt"
enabled_tools = ["search"]

[plugins."my-plugin".mcp_servers.docs.tools.search]
approval_mode = "approve"
```

When your plugin is enabled, Codex can load lifecycle hooks from your plugin
alongside user, project, and managed hooks.

Installing or enabling a plugin doesn't automatically trust its hooks.
Plugin-bundled hooks are non-managed hooks, so Codex skips them until the user
reviews and trusts the current hook definition.

The default plugin hook file is `hooks/hooks.json`:

```json
{
  "hooks": {
    "SessionStart": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "python3 ${PLUGIN_ROOT}/hooks/session_start.py",
            "statusMessage": "Loading plugin context"
          }
        ]
      }
    ]
  }
}
```

If you define `hooks` in `.codex-plugin/plugin.json`, Codex uses that manifest
entry instead of the default `hooks/hooks.json`. The manifest field can be a
single path, an array of paths, an inline hooks object, or an array of inline
hooks objects.

```json
{
  "name": "repo-policy",
  "hooks": ["./hooks/session.json", "./hooks/tools.json"]
}
```

Hook paths follow the same manifest path rules as `skills`, `apps`, and
`mcpServers`: start with `./`, resolve relative to the plugin root, and stay
inside the plugin root.

Plugin hook commands receive the Codex-specific environment variables
`PLUGIN_ROOT` and `PLUGIN_DATA`. `PLUGIN_ROOT` points to the installed plugin
root, and `PLUGIN_DATA` points to the plugin's writable data directory. Codex
also sets `CLAUDE_PLUGIN_ROOT` and `CLAUDE_PLUGIN_DATA` for compatibility with
existing plugin hooks.

Plugin hooks use the same event schema as regular hooks. See
[Hooks](https://developers.openai.com/codex/hooks) for supported events, inputs, outputs, trust review, and
current limitations.

### Publish official public plugins

Adding plugins to the official Plugin Directory is coming soon.

Self-serve plugin publishing and management are coming soon.


---


# Sites

Sites lets Codex create, save, deploy, and inspect websites, web apps, and
games hosted by OpenAI. Use the **Sites** plugin when you want to turn a prompt
or a compatible existing project into a hosted site without setting up a
separate deployment workflow.

Every Sites deployment URL is a production deployment. If you want to review a
  build before it becomes live, ask Codex to save a version without deploying
  it.

## Get started with Sites

Sites is in preview and currently available for ChatGPT Business and Enterprise
workspaces, with more plans rolling out later. For ChatGPT Enterprise
workspaces, an admin must turn it on through role-based access control (RBAC)
before members can use it. Compare support by plan in
[Feature availability](https://developers.openai.com/codex/pricing#feature-availability).

<WorkflowSteps variant="headings">
1. Enable Sites for an Enterprise workspace

    If you use ChatGPT Enterprise, ask your workspace admin to open the RBAC
    controls in [ChatGPT admin settings](https://chatgpt.com/admin/settings) and
    turn on Sites for the appropriate role. ChatGPT Business workspaces can skip
    this step because Sites is enabled by default.

2. Add the Sites plugin

   If **Sites** isn't already available, open **Plugins** in the Codex app, find
   **Sites**, and add it to Codex. Start a new thread after installing a plugin.

3. Start a Sites task

   In a thread, describe the site you want to create or publish. You can name
   the plugin explicitly with `@Sites`, especially when your task should end in
   a hosted deployment.

   <CodexScreenshot
     alt="Codex app composer with the Sites plugin and connected apps mentioned in a prompt"
     lightSrc="/images/codex/sites/prompt-input-light.jpg"
     darkSrc="/images/codex/sites/prompt-input-dark.jpg"
     variant="no-wallpaper"
   />

4. Review whether to save or deploy

   Ask Codex to validate the site's build. Then tell it either to save a
   deployable version for review or to deploy the approved saved version.

5. Return to deployed sites

   Open **Sites** in the app sidebar to return to your Sites projects. You can
   also ask Codex to inspect saved versions, check deployment status, or change
   who can access a deployed site.

   <CodexScreenshot
     alt="Sites project list in the Codex app"
     lightSrc="/images/codex/sites/sites-list-light.jpg"
     darkSrc="/images/codex/sites/sites-list-dark.jpg"
     variant="no-wallpaper"
   />

</WorkflowSteps>

## Prompt Sites for common tasks

For a new website, dashboard, or internal tool, include the audience, core
experience, and required data:

```text
@Sites Build a project request dashboard for my operations team. Let team
members submit requests, see who owns each one, update the status, and filter
the list. Require people to sign in with their workspace account, and keep the
request data saved between visits.
```

For an existing project, ask Sites to prepare and publish the current app:

```text
@Sites Deploy this project. Check whether it is compatible with Sites, make any
required changes, and give me the deployment URL.
```

When a site needs durable application data or uploaded files, say so in the
request:

```text
@Sites Add persistent player scores and avatar uploads to this game. Use
the appropriate Sites storage and deploy the updated game.
```

Browse the [Sites showcase](https://developers.openai.com/showcase/sites) for deployed internal apps and
  the full prompts used to create them.

## Understand projects, versions, and deployments

A Sites project links a local source project to hosting managed through Sites.
Codex stores that linkage and optional storage binding names in
`.openai/hosting.json`. A newly created local starter can begin without a
`project_id`; Sites adds one after it provisions the hosted project.

For example, a provisioned site that uses a relational database binding and no
file storage can contain:

```json
{
  "project_id": "<project-id>",
  "d1": "DB",
  "r2": null
}
```

Sites publishing has two separate stages:

1. **Save a version.** Codex builds the deployable site and associates that
   version with the source Git commit used for the build. Use this stage when
   you want a reviewable deployment candidate.
2. **Deploy a version.** Codex publishes a saved version and reports the
   production URL when deployment succeeds. Use this only when you intend for
   the selected audience to access the site.

Ask Codex to list or inspect saved versions when you need to identify a
previous deployment candidate.

## Choose a supported site shape

Sites hosts projects that build Cloudflare Worker-compatible output as ES
modules. For new projects, the Sites workflow can start with its recommended
site starter. For an existing site, ask Codex to confirm that the project's
build can produce compatible deployment artifacts before you request a
deployment.

Tell Codex about the product behavior you need so it can select the appropriate
site shape:

| Site need                                                      | What to ask Sites for                                                         |
| -------------------------------------------------------------- | ----------------------------------------------------------------------------- |
| Content-led website or landing page                            | A site with no persistent application state unless the experience requires it |
| Saved records, user progress, or game scores                   | D1, a relational database for durable structured data                         |
| Images, documents, audio, video, or other uploads              | R2, object storage for files                                                  |
| Uploaded files with searchable metadata                        | D1 for metadata and R2 for file contents                                      |
| Internal site that needs the current workspace user's identity | Workspace-authenticated user identity                                         |
| Public sign-in or an external identity provider                | An authentication-enabled Sites project                                       |

Don't request durable storage for temporary presentation state, such as a
theme choice or a dismissed banner. Do request it for product data that people
expect the hosted site to remember.

## Control access and secrets

Set the audience before you share a deployed URL. For a new site, keep access
limited to the owner and workspace admins until you have reviewed the content,
data handling, and expected audience.

You can ask Sites to apply one of these access modes:

| Access mode                      | Who can access the site                                                                       |
| -------------------------------- | --------------------------------------------------------------------------------------------- |
| Owner and admins (`admins_only`) | The site owner and workspace admins                                                           |
| Workspace (`workspace_all`)      | All active users in the workspace                                                             |
| Custom (`custom`)                | Specific active users or workspace groups that you choose; Sites continues to allow the owner |

For example:

```text
@Sites Change this deployed site's access to everyone in my workspace after
showing me the current site and confirming the deployment URL.
```

### Configure runtime environment values

Open **Sites** in the app sidebar and select a project to add, update, or remove
hosted environment variables and secrets in the Sites panel. Don't store these
values in `.openai/hosting.json`. Keep local `.env` and `.env.example` files
aligned with the keys needed for local development, and don't commit secret
values.

When you add, update, or remove hosted environment values, ask Codex to
redeploy the approved saved version so the next deployment uses the updated
configuration.

## Review before you share

Before you deploy or widen access:

- Review the source changes and any database migrations in the Codex
  [review pane](https://developers.openai.com/codex/app/review).
- Confirm that the build succeeded and that the selected saved version is the
  version you intend to publish.
- Check that only the intended audience can access the site.
- Confirm that you configured runtime secret values through Sites and didn't
  commit them in source files.
- After deployment, ask Codex to confirm deployment status and the production
  URL before you share it.

## Related documentation

- [Plugins](https://developers.openai.com/codex/plugins) explains how to install and invoke Codex plugins.
- [Codex app](https://developers.openai.com/codex/app) introduces app navigation and project threads.
- [Review and ship changes](https://developers.openai.com/codex/app/review) explains how to inspect source
  changes before publishing them.


---


# Agent Skills

Use agent skills to extend Codex with task-specific capabilities. A skill packages instructions, resources, and optional scripts so Codex can follow a workflow reliably. Skills build on the [open agent skills standard](https://agentskills.io).

Skills are the authoring format for reusable workflows. Plugins are the installable distribution unit for reusable skills and apps in Codex. Use skills to design the workflow itself, then package it as a [plugin](https://developers.openai.com/codex/plugins/build) when you want other developers to install it.

Skills are available in the Codex CLI, IDE extension, and Codex app.

Skills use **progressive disclosure** to manage context efficiently: Codex starts with each skill's name, description, and file path. Codex loads the full `SKILL.md` instructions only when it decides to use a skill.

Codex includes an initial list of available skills in context so it can choose the right skill for a task. To avoid crowding out the rest of the prompt, this list is capped at roughly 2% of the model’s context window, or 8,000 characters when the context window is unknown. If many skills are installed, Codex shortens skill descriptions first. For very large skill sets, some skills may be omitted from the initial list, and Codex will show a warning.

This budget applies only to the initial skills list. When Codex selects a skill, it still reads the full SKILL.md instructions for that skill.

A skill is a directory with a `SKILL.md` file plus optional scripts and references. The `SKILL.md` file must include `name` and `description`.

<FileTree
  class="mt-4"
  tree={[
    {
      name: "my-skill/",
      open: true,
      children: [
        {
          name: "SKILL.md",
          comment: "Required: instructions + metadata",
        },
        {
          name: "scripts/",
          comment: "Optional: executable code",
        },
        {
          name: "references/",
          comment: "Optional: documentation",
        },
        {
          name: "assets/",
          comment: "Optional: templates, resources",
        },
        {
          name: "agents/",
          open: true,
          children: [
            {
              name: "openai.yaml",
              comment: "Optional: appearance and dependencies",
            },
          ],
        },
      ],
    },

]}
/>

## How Codex uses skills

Codex can activate skills in two ways:

1. **Explicit invocation:** Include the skill directly in your prompt. In CLI/IDE, run `/skills` or type `$` to mention a skill.
2. **Implicit invocation:** Codex can choose a skill when your task matches the skill `description`.

Because implicit matching depends on `description`, write concise descriptions with clear scope and boundaries. Front-load the key use case and trigger words so Codex can still match the skill if descriptions are shortened.

## Create a skill

Use the built-in creator first:

```text
$skill-creator
```

The creator asks what the skill does, when it should trigger, and whether it should stay instruction-only or include scripts. Instruction-only is the default.

You can also create a skill manually by creating a folder with a `SKILL.md` file:

```md
---
name: skill-name
description: Explain exactly when this skill should and should not trigger.
---

Skill instructions for Codex to follow.
```

Codex detects skill changes automatically. If an update doesn't appear, restart Codex.

## Where to save skills

Codex reads skills from repository, user, admin, and system locations. For repositories, Codex scans `.agents/skills` in every directory from your current working directory up to the repository root. If two skills share the same `name`, Codex doesn't merge them; both can appear in skill selectors.

| Skill Scope | Location                                                                                                  | Suggested use                                                                                                                                                                                        |
| :---------- | :-------------------------------------------------------------------------------------------------------- | :--------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `REPO`      | `$CWD/.agents/skills` <br /> Current working directory: where you launch Codex.                           | If you're in a repository or code environment, teams can check in skills relevant to a working folder. For example, skills only relevant to a microservice or a module.                              |
| `REPO`      | `$CWD/../.agents/skills` <br /> A folder above CWD when you launch Codex inside a Git repository.         | If you're in a repository with nested folders, organizations can check in skills relevant to a shared area in a parent folder.                                                                       |
| `REPO`      | `$REPO_ROOT/.agents/skills` <br /> The topmost root folder when you launch Codex inside a Git repository. | If you're in a repository with nested folders, organizations can check in skills relevant to everyone using the repository. These serve as root skills available to any subfolder in the repository. |
| `USER`      | `$HOME/.agents/skills` <br /> Any skills checked into the user's personal folder.                         | Use to curate skills relevant to a user that apply to any repository the user may work in.                                                                                                           |
| `ADMIN`     | `/etc/codex/skills` <br /> Any skills checked into the machine or container in a shared, system location. | Use for SDK scripts, automation, and for checking in default admin skills available to each user on the machine.                                                                                     |
| `SYSTEM`    | Bundled with Codex by OpenAI.                                                                             | Useful skills relevant to a broad audience such as the skill-creator and plan skills. Available to everyone when they start Codex.                                                                   |

Codex supports symlinked skill folders and follows the symlink target when scanning these locations.

These locations are for authoring and local discovery. When you want to
distribute reusable skills beyond a single repo, or optionally bundle them with
app integrations, use [plugins](https://developers.openai.com/codex/plugins/build).

## Distribute skills with plugins

Direct skill folders are best for local authoring and repo-scoped workflows. If
you want to distribute a reusable skill, bundle two or more skills together, or
ship a skill alongside an app integration, package them as a
[plugin](https://developers.openai.com/codex/plugins/build).

Plugins can include one or more skills. They can also optionally bundle app
mappings, MCP server configuration, and presentation assets in a single
package.

## Install curated skills for local use

To add curated skills beyond the built-ins for your own local Codex setup, use `$skill-installer`. For example, to install the `$linear` skill:

```bash
$skill-installer linear
```

You can also prompt the installer to download skills from other repositories.
Codex detects newly installed skills automatically; if one doesn't appear,
restart Codex.

Use this for local setup and experimentation. For reusable distribution of your
own skills, prefer plugins.

## Enable or disable skills

Use `[[skills.config]]` entries in `~/.codex/config.toml` to disable a skill without deleting it:

```toml
[[skills.config]]
path = "/path/to/skill/SKILL.md"
enabled = false
```

Restart Codex after changing `~/.codex/config.toml`.

## Optional metadata

Add `agents/openai.yaml` to configure UI metadata in the [Codex app](https://developers.openai.com/codex/app), to set invocation policy, and to declare tool dependencies for a more seamless experience with using the skill.

```yaml
interface:
  display_name: "Optional user-facing name"
  short_description: "Optional user-facing description"
  icon_small: "./assets/small-logo.svg"
  icon_large: "./assets/large-logo.png"
  brand_color: "#3B82F6"
  default_prompt: "Optional surrounding prompt to use the skill with"

policy:
  allow_implicit_invocation: false

dependencies:
  tools:
    - type: "mcp"
      value: "openaiDeveloperDocs"
      description: "OpenAI Docs MCP server"
      transport: "streamable_http"
      url: "https://developers.openai.com/mcp"
```

`allow_implicit_invocation` (default: `true`): When `false`, Codex won't implicitly invoke the skill based on user prompt; explicit `$skill` invocation still works.

## Best practices

- Keep each skill focused on one job.
- Prefer instructions over scripts unless you need deterministic behavior or external tooling.
- Write imperative steps with explicit inputs and outputs.
- Test prompts against the skill description to confirm the right trigger behavior.

For more examples, see [github.com/openai/skills](https://github.com/openai/skills) and [the agent skills specification](https://agentskills.io/specification).


---


# Subagents

Codex can run subagent workflows by spawning specialized agents in parallel and then collecting their results in one response. This can be particularly helpful for complex tasks that are highly parallel, such as codebase exploration or implementing a multi-step feature plan.

With subagent workflows, you can also define your own custom agents with different model configurations and instructions depending on the task.

For the concepts and tradeoffs behind subagent workflows, including context pollution, context rot, and model-selection guidance, see [Subagent concepts](https://developers.openai.com/codex/concepts/subagents).

## Availability

Current Codex releases enable subagent workflows by default.

Subagent activity is currently surfaced in the Codex app and CLI. Visibility
  in the IDE Extension is coming soon.

Codex only spawns subagents when you explicitly ask it to. Because each
subagent does its own model and tool work, subagent workflows consume more
tokens than comparable single-agent runs.

## Typical workflow

Codex handles orchestration across agents, including spawning new subagents,
routing follow-up instructions, waiting for results, and closing agent
threads.

When many agents are running, Codex waits until all requested results are
available, then returns a consolidated response.

Codex only spawns a new agent when you explicitly ask it to do so.

To see it in action, try the following prompt on your project:

```text
I would like to review the following points on the current PR (this branch vs main). Spawn one agent per point, wait for all of them, and summarize the result for each point.
1. Security issue
2. Code quality
3. Bugs
4. Race
5. Test flakiness
6. Maintainability of the code
```

## Managing subagents

- Use `/agent` in the CLI to switch between active agent threads and inspect the ongoing thread.
- Ask Codex directly to steer a running subagent, stop it, or close completed agent threads.

## Approvals and sandbox controls

Subagents inherit your current sandbox policy.

In interactive CLI sessions, approval requests can surface from inactive agent
threads even while you are looking at the main thread. The approval overlay
shows the source thread label, and you can press `o` to open that thread before
you approve, reject, or answer the request.

In non-interactive flows, or whenever a run can't surface a fresh approval, an
action that needs new approval fails and Codex surfaces the error back to the
parent workflow.

Codex also reapplies the parent turn's live runtime overrides when it spawns a
child. That includes sandbox and approval choices you set interactively during
the session, such as `/permissions` changes or `--yolo`, even if the selected
custom agent file sets different defaults.

You can also override the sandbox configuration for individual [custom agents](#custom-agents), such as explicitly marking one to work in read-only mode.

## Custom agents

Codex ships with built-in agents:

- `default`: general-purpose fallback agent.
- `worker`: execution-focused agent for implementation and fixes.
- `explorer`: read-heavy codebase exploration agent.

To define your own custom agents, add standalone TOML files under
`~/.codex/agents/` for personal agents or `.codex/agents/` for project-scoped
agents.

Each file defines one custom agent. Codex loads these files as configuration
layers for spawned sessions, so custom agents can override the same settings as
a normal Codex session config. That can feel heavier than a dedicated agent
manifest, and the format may evolve as authoring and sharing mature.

Every standalone custom agent file must define:

- `name`
- `description`
- `developer_instructions`

Optional fields such as `nickname_candidates`, `model`,
`model_reasoning_effort`, `sandbox_mode`, `mcp_servers`, and `skills.config`
inherit from the parent session when you omit them.

### Global settings

Global subagent settings still live under `[agents]` in your [configuration](https://developers.openai.com/codex/config-basic#configuration-precedence).

| Field                            | Type   | Required | Purpose                                                    |
| -------------------------------- | ------ | :------: | ---------------------------------------------------------- |
| `agents.max_threads`             | number |    No    | Concurrent open agent thread cap.                          |
| `agents.max_depth`               | number |    No    | Spawned agent nesting depth (root session starts at 0).    |
| `agents.job_max_runtime_seconds` | number |    No    | Default timeout per worker for `spawn_agents_on_csv` jobs. |

**Notes:**

- `agents.max_threads` defaults to `6` when you leave it unset.
- `agents.max_depth` defaults to `1`, which allows a direct child agent to spawn but prevents deeper nesting. Keep the default unless you specifically need recursive delegation. Raising this value can turn broad delegation instructions into repeated fan-out, which increases token usage, latency, and local resource consumption. `agents.max_threads` still caps concurrent open threads, but it doesn't remove the cost and predictability risks of deeper recursion.
- `agents.job_max_runtime_seconds` is optional. When you leave it unset, `spawn_agents_on_csv` falls back to its per-call default timeout of 1800 seconds per worker.
- If a custom agent name matches a built-in agent such as `explorer`, your custom agent takes precedence.

### Custom agent file schema

| Field                    | Type     | Required | Purpose                                                         |
| ------------------------ | -------- | :------: | --------------------------------------------------------------- |
| `name`                   | string   |   Yes    | Agent name Codex uses when spawning or referring to this agent. |
| `description`            | string   |   Yes    | Human-facing guidance for when Codex should use this agent.     |
| `developer_instructions` | string   |   Yes    | Core instructions that define the agent's behavior.             |
| `nickname_candidates`    | string[] |    No    | Optional pool of display nicknames for spawned agents.          |

You can also include other supported `config.toml` keys in a custom agent file, such as `model`, `model_reasoning_effort`, `sandbox_mode`, `mcp_servers`, and `skills.config`.

Codex identifies the custom agent by its `name` field. Matching the filename to
the agent name is the simplest convention, but the `name` field is the source
of truth.

### Display nicknames

Use `nickname_candidates` when you want Codex to assign more readable display
names to spawned agents. This is especially helpful when you run many
instances of the same custom agent and want the UI to show distinct labels
instead of repeating the same agent name.

Nicknames are presentation-only. Codex still identifies and spawns the agent by
its `name`.

Nickname candidates must be a non-empty list of unique names. Each nickname can
use ASCII letters, digits, spaces, hyphens, and underscores.

Example:

```toml
name = "reviewer"
description = "PR reviewer focused on correctness, security, and missing tests."
developer_instructions = """
Review code like an owner.
Prioritize correctness, security, behavior regressions, and missing test coverage.
"""
nickname_candidates = ["Atlas", "Delta", "Echo"]
```

In practice, the Codex app and CLI can show the nicknames where agent activity
appears, while the underlying agent type stays
`reviewer`.

### Example custom agents

The best custom agents are narrow and opinionated. Give each one clear job, a
tool surface that matches that job, and instructions that keep it from
drifting into adjacent work.

#### Example 1: PR review

This pattern splits review across three focused custom agents:

- `pr_explorer` maps the codebase and gathers evidence.
- `reviewer` looks for correctness, security, and test risks.
- `docs_researcher` checks framework or API documentation through a dedicated MCP server.

Project config (`.codex/config.toml`):

```toml
[agents]
max_threads = 6
max_depth = 1
```

`.codex/agents/pr-explorer.toml`:

```toml
name = "pr_explorer"
description = "Read-only codebase explorer for gathering evidence before changes are proposed."
model = "gpt-5.3-codex-spark"
model_reasoning_effort = "medium"
sandbox_mode = "read-only"
developer_instructions = """
Stay in exploration mode.
Trace the real execution path, cite files and symbols, and avoid proposing fixes unless the parent agent asks for them.
Prefer fast search and targeted file reads over broad scans.
"""
```

`.codex/agents/reviewer.toml`:

```toml
name = "reviewer"
description = "PR reviewer focused on correctness, security, and missing tests."
model = "gpt-5.4"
model_reasoning_effort = "high"
sandbox_mode = "read-only"
developer_instructions = """
Review code like an owner.
Prioritize correctness, security, behavior regressions, and missing test coverage.
Lead with concrete findings, include reproduction steps when possible, and avoid style-only comments unless they hide a real bug.
"""
```

`.codex/agents/docs-researcher.toml`:

```toml
name = "docs_researcher"
description = "Documentation specialist that uses the docs MCP server to verify APIs and framework behavior."
model = "gpt-5.4-mini"
model_reasoning_effort = "medium"
sandbox_mode = "read-only"
developer_instructions = """
Use the docs MCP server to confirm APIs, options, and version-specific behavior.
Return concise answers with links or exact references when available.
Do not make code changes.
"""

[mcp_servers.openaiDeveloperDocs]
url = "https://developers.openai.com/mcp"
```

This setup works well for prompts like:

```text
Review this branch against main. Have pr_explorer map the affected code paths, reviewer find real risks, and docs_researcher verify the framework APIs that the patch relies on.
```

## Process CSV batches with subagents (experimental)

This workflow is experimental and may change as subagent support evolves.
Use `spawn_agents_on_csv` when you have many similar tasks that map to one row per work item. Codex reads the CSV, spawns one worker subagent per row, waits for the full batch to finish, and exports the combined results to CSV.

This works well for repeated audits such as:

- reviewing one file, package, or service per row
- checking a list of incidents, PRs, or migration targets
- generating structured summaries for many similar inputs

The tool accepts:

- `csv_path` for the source CSV
- `instruction` for the worker prompt template, using `{column_name}` placeholders
- `id_column` when you want stable item ids from a specific column
- `output_schema` when each worker should return a JSON object with a fixed shape
- `output_csv_path`, `max_concurrency`, and `max_runtime_seconds` for job control

Each worker must call `report_agent_job_result` exactly once. If a worker exits without reporting a result, Codex marks that row with an error in the exported CSV.

Example prompt:

```text
Create /tmp/components.csv with columns path,owner and one row per frontend component.

Then call spawn_agents_on_csv with:
- csv_path: /tmp/components.csv
- id_column: path
- instruction: "Review {path} owned by {owner}. Return JSON with keys path, risk, summary, and follow_up via report_agent_job_result."
- output_csv_path: /tmp/components-review.csv
- output_schema: an object with required string fields path, risk, summary, and follow_up
```

When you run this through `codex exec`, Codex shows a single-line progress update on `stderr` while the batch is running. The exported CSV includes the original row data plus metadata such as `job_id`, `item_id`, `status`, `last_error`, and `result_json`.

Related runtime settings:

- `agents.max_threads` caps how many agent threads can stay open concurrently.
- `agents.job_max_runtime_seconds` sets the default per-worker timeout for CSV fan-out jobs. A per-call `max_runtime_seconds` override takes precedence.
- `sqlite_home` controls where Codex stores the SQLite-backed state used for agent jobs and their exported results.

#### Example 2: Frontend integration debugging

This pattern is useful for UI regressions, flaky browser flows, or integration bugs that cross application code and the running product.

Project config (`.codex/config.toml`):

```toml
[agents]
max_threads = 6
max_depth = 1
```

`.codex/agents/code-mapper.toml`:

```toml
name = "code_mapper"
description = "Read-only codebase explorer for locating the relevant frontend and backend code paths."
model = "gpt-5.4-mini"
model_reasoning_effort = "medium"
sandbox_mode = "read-only"
developer_instructions = """
Map the code that owns the failing UI flow.
Identify entry points, state transitions, and likely files before the worker starts editing.
"""
```

`.codex/agents/browser-debugger.toml`:

```toml
name = "browser_debugger"
description = "UI debugger that uses browser tooling to reproduce issues and capture evidence."
model = "gpt-5.4"
model_reasoning_effort = "high"
sandbox_mode = "workspace-write"
developer_instructions = """
Reproduce the issue in the browser, capture exact steps, and report what the UI actually does.
Use browser tooling for screenshots, console output, and network evidence.
Do not edit application code.
"""

[mcp_servers.chrome_devtools]
url = "http://localhost:3000/mcp"
startup_timeout_sec = 20
```

`.codex/agents/ui-fixer.toml`:

```toml
name = "ui_fixer"
description = "Implementation-focused agent for small, targeted fixes after the issue is understood."
model = "gpt-5.3-codex-spark"
model_reasoning_effort = "medium"
developer_instructions = """
Own the fix once the issue is reproduced.
Make the smallest defensible change, keep unrelated files untouched, and validate only the behavior you changed.
"""

[[skills.config]]
path = "/Users/me/.agents/skills/docs-editor/SKILL.md"
enabled = false
```

This setup works well for prompts like:

```text
Investigate why the settings modal fails to save. Have browser_debugger reproduce it, code_mapper trace the responsible code path, and ui_fixer implement the smallest fix once the failure mode is clear.
```


---


# Authentication

## OpenAI authentication

Codex supports two ways to sign in when using OpenAI models:

- Sign in with ChatGPT for subscription access
- Sign in with an API key for usage-based access

Codex cloud requires signing in with ChatGPT. The Codex CLI and IDE extension support both sign-in methods.

Your sign-in method also determines which admin controls and data-handling policies apply.

- With sign in with ChatGPT, Codex usage follows your ChatGPT workspace permissions, RBAC, and ChatGPT Enterprise retention and residency settings
- With an API key, usage follows your API organization's retention and data-sharing settings instead

For the CLI, Sign in with ChatGPT is the default authentication path when no valid session is available.

### Sign in with ChatGPT

When you sign in with ChatGPT from the Codex app, CLI, or IDE Extension, Codex opens a browser window for you to complete the login flow. After you sign in, the browser returns an access token to the CLI or IDE extension.

If your environment already provides a ChatGPT access token, the CLI can read
it from stdin:

```shell
printenv CODEX_ACCESS_TOKEN | codex login --with-access-token
```

### Sign in with an API key

You can also sign in to the Codex app, CLI, or IDE Extension with an API key. Get your API key from the [OpenAI dashboard](https://platform.openai.com/api-keys).

OpenAI bills API key usage through your OpenAI Platform account at standard API rates. See the [API pricing page](https://openai.com/api/pricing/).

API key authentication supports local Codex workflows, but some features that
rely on ChatGPT workspace access or cloud services are limited or unavailable.
Compare support by plan in
[Feature availability](https://developers.openai.com/codex/pricing#feature-availability).

When you sign in with an API key, Codex uses standard API pricing instead of
included ChatGPT plan credits.

We recommend API key authentication for programmatic Codex CLI workflows, such
as CI/CD jobs. Don't expose Codex execution in untrusted or public environments.

### Use Codex access tokens for enterprise automation

In ChatGPT Enterprise workspaces, admins can allow permitted members to create
Codex access tokens for trusted, non-interactive Codex local workflows. Use an
access token when automation needs ChatGPT workspace access, ChatGPT-managed
Codex entitlements, or enterprise workspace controls without a browser sign-in.

Access tokens are intended for trusted scripts, schedulers, and private CI
runners. For general OpenAI API calls, continue to use Platform API keys.

For setup steps, permissions, rotation, and revocation guidance, see
[Access tokens](https://developers.openai.com/codex/enterprise/access-tokens).

## Secure your Codex cloud account

Codex cloud interacts directly with your codebase, so it needs stronger security than many other ChatGPT features. Enable multi-factor authentication (MFA).

If you use a social login provider (Google, Microsoft, Apple), you aren't required to enable MFA on your ChatGPT account, but you can set it up with your social login provider.

For setup instructions, see:

- [Google](https://support.google.com/accounts/answer/185839)
- [Microsoft](https://support.microsoft.com/en-us/topic/what-is-multifactor-authentication-e5e39437-121c-be60-d123-eda06bddf661)
- [Apple](https://support.apple.com/en-us/102660)

If you access ChatGPT through single sign-on (SSO), your organization's SSO administrator should enforce MFA for all users.

If you log in using an email and password, you must set up MFA on your account before accessing Codex cloud.

If your account supports more than one login method and one of them is email and password, you must set up MFA before accessing Codex, even if you sign in another way.

## Login caching

When you sign in to the Codex app, CLI, or IDE Extension using either ChatGPT or an API key, Codex caches your login details and reuses them the next time you start the CLI or extension. The CLI and extension share the same cached login details. If you log out from either one, you'll need to sign in again the next time you start the CLI or extension.

Codex caches login details locally in a plaintext file at `~/.codex/auth.json` or in your OS-specific credential store.

For sign in with ChatGPT sessions, Codex refreshes tokens automatically during use before they expire, so active sessions usually continue without requiring another browser login.

## Credential storage

Use `cli_auth_credentials_store` to control where the Codex CLI stores cached credentials:

```toml
# file | keyring | auto
cli_auth_credentials_store = "keyring"
```

- `file` stores credentials in `auth.json` under `CODEX_HOME` (defaults to `~/.codex`).
- `keyring` stores credentials in your operating system credential store.
- `auto` uses the OS credential store when available, otherwise falls back to `auth.json`.

If you use file-based storage, treat `~/.codex/auth.json` like a password: it
  contains access tokens. Don't commit it, paste it into tickets, or share it in
  chat.

## Enforce a login method or workspace

In managed environments, admins may restrict how users are allowed to authenticate:

```toml
# Only allow ChatGPT login or only allow API key login.
forced_login_method = "chatgpt" # or "api"

# When using ChatGPT login, restrict users to a specific workspace.
forced_chatgpt_workspace_id = "00000000-0000-0000-0000-000000000000"
```

If the active credentials don't match the configured restrictions, Codex logs the user out and exits.

These settings are commonly applied via managed configuration rather than per-user setup. See [Managed configuration](https://developers.openai.com/codex/enterprise/managed-configuration).

## Login diagnostics

Direct `codex login` runs write a dedicated `codex-login.log` file under
your configured log directory. Use it when you need to debug browser-login or
device-code failures, or when support asks for login-specific logs.

## Custom CA bundles

If your network uses a corporate TLS proxy or private root CA, set
`CODEX_CA_CERTIFICATE` to a PEM bundle before logging in. When
`CODEX_CA_CERTIFICATE` is unset, Codex falls back to `SSL_CERT_FILE`. The same
custom CA settings apply to login, normal HTTPS requests, and secure WebSocket
connections.

```shell
export CODEX_CA_CERTIFICATE=/path/to/corporate-root-ca.pem
codex login
```

## Login on headless devices

If you are signing in to ChatGPT with the Codex CLI, there are some situations where the browser-based login UI may not work:

- You're running the CLI in a remote or headless environment.
- Your local networking configuration blocks the localhost callback Codex uses to return the OAuth token to the CLI after you sign in.

In these situations, prefer device code authentication (beta). In the interactive login UI, choose **Sign in with Device Code**, or run `codex login --device-auth` directly. If device code authentication doesn't work in your environment, use one of the fallback methods.

### Preferred: Device code authentication (beta)

1. Enable device code login in your ChatGPT security settings (personal account) or ChatGPT workspace permissions (workspace admin).
2. In the terminal where you're running Codex, choose one of these options:
   - In the interactive login UI, select **Sign in with Device Code**.
   - Run `codex login --device-auth`.
3. Open the link in your browser, sign in, then enter the one-time code.

If device code login isn't enabled by the server, Codex falls back to the standard browser-based login flow.

### Fallback: Authenticate locally and copy your auth cache

If you can complete the login flow on a machine with a browser, you can copy your cached credentials to the headless machine.

1. On a machine where you can use the browser-based login flow, run `codex login`.
2. Confirm the login cache exists at `~/.codex/auth.json`.
3. Copy `~/.codex/auth.json` to `~/.codex/auth.json` on the headless machine.

Treat `~/.codex/auth.json` like a password: it contains access tokens. Don't commit it, paste it into tickets, or share it in chat.

If your OS stores credentials in a credential store instead of `~/.codex/auth.json`, this method may not apply. See
[Credential storage](#credential-storage) for how to configure file-based storage.

Copy to a remote machine over SSH:

```shell
ssh user@remote 'mkdir -p ~/.codex'
scp ~/.codex/auth.json user@remote:~/.codex/auth.json
```

Or use a one-liner that avoids `scp`:

```shell
ssh user@remote 'mkdir -p ~/.codex && cat > ~/.codex/auth.json' < ~/.codex/auth.json
```

Copy into a Docker container:

```shell
# Replace MY_CONTAINER with the name or ID of your container.
CONTAINER_HOME=$(docker exec MY_CONTAINER printenv HOME)
docker exec MY_CONTAINER mkdir -p "$CONTAINER_HOME/.codex"
docker cp ~/.codex/auth.json MY_CONTAINER:"$CONTAINER_HOME/.codex/auth.json"
```

For a more advanced version of this same pattern on trusted CI/CD runners, see
[Maintain Codex account auth in CI/CD (advanced)](https://developers.openai.com/codex/auth/ci-cd-auth).
That guide explains how to let Codex refresh `auth.json` during normal runs and
then keep the updated file for the next job. API keys are still the recommended
default for automation.

### Fallback: Forward the localhost callback over SSH

If you can forward ports between your local machine and the remote host, you can use the standard browser-based flow by tunneling Codex's local callback server (default `localhost:1455`).

1. From your local machine, start port forwarding:

```shell
ssh -L 1455:localhost:1455 user@remote
```

2. In that SSH session, run `codex login` and follow the printed address on your local machine.

## Alternative model providers

When you define a [custom model provider](https://developers.openai.com/codex/config-advanced#custom-model-providers) in your configuration file, you can choose one of these authentication methods:

- **OpenAI authentication**: Set `requires_openai_auth = true` to use OpenAI authentication. You can then sign in with ChatGPT or an API key. This is useful when you access OpenAI models through an LLM proxy server. When `requires_openai_auth = true`, Codex ignores `env_key`.
- **Environment variable authentication**: Set `env_key = "<ENV_VARIABLE_NAME>"` to use a provider-specific API key from the local environment variable named `<ENV_VARIABLE_NAME>`.
- **No authentication**: If you don't set `requires_openai_auth` (or set it to `false`) and you don't set `env_key`, Codex assumes the provider doesn't require authentication. This is useful for local models.


---


# Agent approvals & security

Codex helps protect your code and data and reduces the risk of misuse.

This page covers how to operate Codex safely, including sandboxing, approvals,
  and network access. If you are looking for Codex Security, the product for
  scanning connected GitHub repositories, see [Codex Security](https://developers.openai.com/codex/security).

By default, the agent runs with network access turned off. Locally, Codex uses an OS-enforced sandbox that limits what it can touch (typically to the current workspace), plus an approval policy that controls when it must stop and ask you before acting.

For a high-level explanation of how sandboxing works across the Codex app, IDE
extension, and CLI, see [sandboxing](https://developers.openai.com/codex/concepts/sandboxing).
For a broader enterprise security overview, see the [Codex security white paper](https://trust.openai.com/?itemUid=382f924d-54f3-43a8-a9df-c39e6c959958&source=click).

## Sandbox and approvals

Codex security controls come from two layers that work together:

- **Sandbox mode**: What Codex can do technically (for example, where it can write and whether it can reach the network) when it executes model-generated commands.
- **Approval policy**: When Codex must ask you before it executes an action (for example, leaving the sandbox, using the network, or running commands outside a trusted set).

Codex uses different sandbox modes depending on where you run it:

- **Codex cloud**: Runs in isolated OpenAI-managed containers, preventing access to your host system or unrelated data. Uses a two-phase runtime model: setup runs before the agent phase and can access the network to install specified dependencies, then the agent phase runs offline by default unless you enable internet access for that environment. Secrets configured for cloud environments are available only during setup and are removed before the agent phase starts.
- **Codex CLI / IDE extension**: OS-level mechanisms enforce sandbox policies. Defaults include no network access and write permissions limited to the active workspace. You can configure the sandbox, approval policy, and network settings based on your risk tolerance.

In the `Auto` preset (for example, `--sandbox workspace-write --ask-for-approval on-request`), Codex can read files, make edits, and run commands in the working directory automatically.

Codex asks for approval to edit files outside the workspace or to run commands that require network access. If you want to chat or plan without making changes, switch to `read-only` mode with the `/permissions` command.

Codex can also elicit approval for app (connector) tool calls that advertise side effects, even when the action isn't a shell command or file change. Destructive app/MCP tool calls always require approval when the tool advertises a destructive annotation, even if it also advertises other hints (for example, read-only hints).

## Network access <ElevatedRiskBadge class="ml-2" />

For Codex cloud, see [agent internet access](https://developers.openai.com/codex/cloud/internet-access) to enable full internet access or a domain allow list.

For the Codex app, CLI, or IDE Extension, the default `workspace-write` sandbox mode keeps network access turned off unless you enable it in your configuration:

```toml
[sandbox_workspace_write]
network_access = true
```

### Network isolation

Network access is controlled through destination rules that apply to scripts,
programs, and subprocesses spawned by commands. When command network access is
already enabled, turn on the `network_proxy` feature to constrain that traffic
to the network policy you configure.

```toml
[features.network_proxy]
enabled = true
domains = { "api.openai.com" = "allow", "example.com" = "deny" }
```

For a one-off CLI session, use the boolean shorthand when you only need the
toggle, and the table form when you also set policy options:

```bash
codex \
  -c 'features.network_proxy=true' \
  -c 'sandbox_workspace_write.network_access=true'

codex \
  -c 'features.network_proxy.enabled=true' \
  -c 'features.network_proxy.domains={ "api.openai.com" = "allow", "example.com" = "deny" }' \
  -c 'sandbox_workspace_write.network_access=true'
```

The feature changes how enabled network access is enforced; it does not grant
network access by itself. Use `sandbox_workspace_write.network_access` with
`workspace-write` config to decide whether commands have network access at all:

- Network off + `network_proxy` on: network stays off, and the feature does nothing.
- Network on + `network_proxy` off: network stays on with unrestricted direct
  outbound access.
- Network on + `network_proxy` on: network stays on, and outbound traffic is
  constrained by the configured network policy.

Admin-managed `experimental_network` requirements are separate from the user
feature toggle. They can configure and start sandboxed networking without
`features.network_proxy`, but they do not turn on network access when the active
sandbox keeps it off. See [Managed configuration](https://developers.openai.com/codex/enterprise/managed-configuration#configure-network-access-requirements)
for the administrator-side `requirements.toml` shape.

#### Network policy

Domain rules are allowlist-first:

- Exact hosts match only themselves.
- `*.example.com` matches subdomains such as `api.example.com`, but not
  `example.com`.
- `**.example.com` matches both the apex and subdomains.
- A global `*` allow rule matches any public host that is not denied. Treat `*`
  as broad network access and prefer scoped rules when you can.
- `deny` always wins over `allow`, and global `*` is only valid for allow rules.

#### Local and private destinations

By default, `allow_local_binding = false` blocks loopback, link-local, and
private destinations:

- Specific exceptions: add an exact local IP literal or `localhost` allow rule
  when a command needs one local target.
- Broader access: set `allow_local_binding = true` only when you intentionally
  want wider local/private reach.
- Wildcards: wildcard rules do not count as explicit local exceptions.
- Resolved addresses: hostnames that resolve to local/private IPs stay blocked
  even if they match the allowlist.

#### DNS rebinding protections

Before allowing a hostname, Codex performs a best-effort DNS and IP
classification check:

- Lookups that fail or time out are blocked.
- Hostnames that resolve to non-public addresses are blocked.
- The check reduces DNS rebinding risk, but it does not eliminate it. Preventing
  rebinding completely would require pinning resolved IPs through the transport
  layer.

If hostile DNS is in scope, enforce egress controls at a lower layer too.

#### Dangerous settings

Two settings deliberately widen the trust boundary:

- `dangerously_allow_non_loopback_proxy = true` can expose proxy listeners beyond
  loopback.
- `dangerously_allow_all_unix_sockets = true` bypasses the Unix socket allowlist.

Use them only in tightly controlled environments. When Unix socket proxying is
enabled, listeners stay loopback-only even if non-loopback binding was requested,
so sandboxed networking does not become a remote bridge into local daemons.

`network_proxy` is off by default. When you enable it:

| Setting                                | Default | Behavior                                                                                                                                                                              |
| -------------------------------------- | ------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `enabled`                              | `false` | Starts sandboxed networking only when command network access is already on.                                                                                                           |
| `domains`                              | unset   | Uses allowlist behavior, so no external destinations are allowed until you add `allow` rules. Supports exact hosts, scoped wildcards, and global `*` allow rules; `deny` always wins. |
| `unix_sockets`                         | unset   | No Unix socket destinations are allowed until you add explicit `allow` rules.                                                                                                         |
| `allow_local_binding`                  | `false` | Blocks local and private-network destinations unless you add an exact local IP literal or `localhost` allow rule, or explicitly opt into broader local/private access.                |
| `enable_socks5`                        | `true`  | Exposes SOCKS5 support when policy allows it.                                                                                                                                         |
| `enable_socks5_udp`                    | `true`  | Allows UDP over SOCKS5 when SOCKS5 is available.                                                                                                                                      |
| `allow_upstream_proxy`                 | `true`  | Lets sandboxed networking honor an upstream proxy from the environment.                                                                                                               |
| `dangerously_allow_non_loopback_proxy` | `false` | Keeps listener endpoints on loopback unless you deliberately expose them beyond localhost.                                                                                            |
| `dangerously_allow_all_unix_sockets`   | `false` | Keeps Unix socket access allowlist-based unless you deliberately bypass that protection.                                                                                              |

You can also control the [web search tool](https://platform.openai.com/docs/guides/tools-web-search) without granting full network access to spawned commands. Codex defaults to using a web search cache to access results. The cache is an OpenAI-maintained index of web results, so cached mode returns pre-indexed results instead of fetching live pages. This reduces exposure to prompt injection from arbitrary live content, but you should still treat web results as untrusted. If you are using `--yolo` or another [full access sandbox setting](#common-sandbox-and-approval-combinations), web search defaults to live results. Use `--search` or set `web_search = "live"` to allow live browsing, or set it to `"disabled"` to turn the tool off:

```toml
web_search = "cached"  # default
# web_search = "disabled"
# web_search = "live"  # same as --search
```

Use caution when enabling network access or web search in Codex. Prompt injection can cause the agent to fetch and follow untrusted instructions.

## Defaults and recommendations

- On launch, Codex detects whether the folder is version-controlled and recommends:
  - Version-controlled folders: `Auto` (workspace write + on-request approvals)
  - Non-version-controlled folders: `read-only`
- Depending on your setup, Codex may also start in `read-only` until you explicitly trust the working directory (for example, via an onboarding prompt or `/permissions`).
- The workspace includes the current directory and temporary directories like `/tmp`. Use the `/status` command to see which directories are in the workspace.
- To accept the defaults, run `codex`.
- You can set these explicitly:
  - `codex --sandbox workspace-write --ask-for-approval on-request`
  - `codex --sandbox read-only --ask-for-approval on-request`

### Protected paths in writable roots

In the default `workspace-write` sandbox policy, writable roots still include protected paths:

- `<writable_root>/.git` is protected as read-only whether it appears as a directory or file.
- If `<writable_root>/.git` is a pointer file (`gitdir: ...`), the resolved Git directory path is also protected as read-only.
- `<writable_root>/.agents` is protected as read-only when it exists as a directory.
- `<writable_root>/.codex` is protected as read-only when it exists as a directory.
- Protection is recursive, so everything under those paths is read-only.

### Run without approval prompts

You can disable approval prompts with `--ask-for-approval never` or `-a never` (shorthand).

This option works with all `--sandbox` modes, so you still control Codex's level of autonomy. Codex makes a best effort within the constraints you set.

If you need Codex to read files, make edits, and run commands with network access without approval prompts, use `--sandbox danger-full-access` (or the `--dangerously-bypass-approvals-and-sandbox` flag). Use caution before doing so.

For a middle ground, `approval_policy = { granular = { ... } }` lets you keep specific approval prompt categories interactive while automatically rejecting others. The granular policy covers sandbox approvals, execpolicy-rule prompts, MCP prompts, `request_permissions` prompts, and skill-script approvals.

### Automatic approval reviews

By default, approval requests route to you:

```toml
approvals_reviewer = "user"
```

Automatic approval reviews apply when approvals are interactive, such as
`approval_policy = "on-request"` or a granular approval policy. Set
`approvals_reviewer = "auto_review"` to route eligible approval requests
through a reviewer agent before Codex runs the request:

```toml
approval_policy = "on-request"
approvals_reviewer = "auto_review"
```

For the full reviewer lifecycle, trigger conditions, configuration precedence,
and failure behavior, see
[Auto-review](https://developers.openai.com/codex/concepts/sandboxing/auto-review).

The reviewer evaluates only actions that already need approval, such as sandbox
escalations, blocked network requests, `request_permissions` prompts, or
side-effecting app and MCP tool calls. Actions that stay inside the sandbox
continue without an extra review step.

The reviewer policy checks for data exfiltration, credential probing, persistent
security weakening, and destructive actions. Low-risk and medium-risk actions
can proceed when policy allows them. The policy denies critical-risk actions.
High-risk actions require enough user authorization and no matching deny rule.
Prompt-build, review-session, and parse failures fail closed. Timeouts are
surfaced separately, but the action still does not run.

The [default reviewer policy](https://github.com/openai/codex/blob/main/codex-rs/core/src/guardian/policy.md)
is in the open-source Codex repository. Enterprises can replace its
tenant-specific section with `guardian_policy_config` in managed requirements.
Local `[auto_review].policy` text is also supported, but managed requirements
take precedence. For setup details, see
[Managed configuration](https://developers.openai.com/codex/enterprise/managed-configuration#configure-automatic-review-policy).

In the Codex app, these reviews appear as automatic review items with a status
such as Reviewing, Approved, Denied, Aborted, or Timed out. They can also
include a risk level and user-authorization assessment for the reviewed
request.

Automatic review uses extra model calls, so it can add to Codex usage. Admins
can constrain it with `allowed_approvals_reviewers`.

### Common sandbox and approval combinations

| Intent                                                            | Flags / config                                                                                                                      | Effect                                                                                                                                           |
| ----------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------ |
| Auto (preset)                                                     | _no flags needed_ or `--sandbox workspace-write --ask-for-approval on-request`                                                      | Codex can read files, make edits, and run commands in the workspace. Codex requires approval to edit outside the workspace or to access network. |
| Safe read-only browsing                                           | `--sandbox read-only --ask-for-approval on-request`                                                                                 | Codex can read files and answer questions. Codex requires approval to make edits, run commands, or access network.                               |
| Read-only non-interactive (CI)                                    | `--sandbox read-only --ask-for-approval never`                                                                                      | Codex can only read files; never asks for approval.                                                                                              |
| Automatically edit but ask for approval to run untrusted commands | `--sandbox workspace-write --ask-for-approval untrusted`                                                                            | Codex can read and edit files but asks for approval before running untrusted commands.                                                           |
| Auto-review mode                                                  | `--sandbox workspace-write --ask-for-approval on-request -c approvals_reviewer=auto_review` or `approvals_reviewer = "auto_review"` | Same sandbox boundary as standard on-request mode, but eligible approval requests are reviewed by Auto-review instead of surfacing to the user.  |
| Dangerous full access                                             | `--dangerously-bypass-approvals-and-sandbox` (alias: `--yolo`)                                                                      | <ElevatedRiskBadge /> No sandbox; no approvals _(not recommended)_                                                                               |

For non-interactive runs, use `codex exec --sandbox workspace-write`; Codex keeps older `codex exec --full-auto` invocations as a deprecated compatibility path and prints a warning.

With `--ask-for-approval untrusted`, Codex runs only known-safe read operations automatically. Commands that can mutate state or trigger external execution paths (for example, destructive Git operations or Git output/config-override flags) require approval.

#### Configuration in `config.toml`

For the broader configuration workflow, see [Config basics](https://developers.openai.com/codex/config-basic), [Advanced Config](https://developers.openai.com/codex/config-advanced#approval-policies-and-sandbox-modes), and the [Configuration Reference](https://developers.openai.com/codex/config-reference).

```toml
# Always ask for approval mode
approval_policy = "untrusted"
sandbox_mode    = "read-only"
allow_login_shell = false # optional hardening: disallow login shells for shell-based tools

# Optional: Allow network in workspace-write mode
[sandbox_workspace_write]
network_access = true

# Optional: granular approval policy
# approval_policy = { granular = {
#   sandbox_approval = true,
#   rules = true,
#   mcp_elicitations = true,
#   request_permissions = false,
#   skill_approval = false
# } }
```

You can also save presets as [profile files](https://developers.openai.com/codex/config-advanced#profiles), then select them with `codex --profile profile-name`:

```toml
# ~/.codex/full_auto.config.toml
approval_policy = "on-request"
sandbox_mode    = "workspace-write"
```

```toml
# ~/.codex/readonly_quiet.config.toml
approval_policy = "never"
sandbox_mode    = "read-only"
```

### Test the sandbox locally

To see what happens when a command runs under the Codex sandbox, use these Codex CLI commands:

```bash
# macOS
codex sandbox macos [--permissions-profile <name>] [--log-denials] [COMMAND]...
# Linux
codex sandbox linux [--permissions-profile <name>] [COMMAND]...
# Windows
codex sandbox windows [--permissions-profile <name>] [COMMAND]...
```

The `sandbox` command is also available as `codex debug`, and the platform helpers have aliases (for example `codex sandbox seatbelt` and `codex sandbox landlock`).

## OS-level sandbox

Codex enforces the sandbox differently depending on your OS:

- **macOS** uses Seatbelt policies and runs commands using `sandbox-exec` with a profile (`-p`) that corresponds to the `--sandbox` mode you selected. When restricted read access enables platform defaults, Codex appends a curated macOS platform policy (instead of broadly allowing `/System`) to preserve common tool compatibility.
- **Linux** uses `bwrap` plus `seccomp` by default.
- **Windows** uses the Linux sandbox implementation when running in [Windows Subsystem for Linux 2 (WSL2)](https://developers.openai.com/codex/windows#windows-subsystem-for-linux). WSL1 was supported through Codex `0.114`; starting in `0.115`, the Linux sandbox moved to `bwrap`, so WSL1 is no longer supported. When running natively on Windows, Codex uses a [Windows sandbox](https://developers.openai.com/codex/windows#windows-sandbox) implementation.

If you use the Codex IDE extension on Windows, it supports WSL2 directly. Set the following in your VS Code settings to keep the agent inside WSL2 whenever it's available:

```json
{
  "chatgpt.runCodexInWindowsSubsystemForLinux": true
}
```

This ensures the IDE extension inherits Linux sandbox semantics for commands, approvals, and filesystem access even when the host OS is Windows. Learn more in the [Windows setup guide](https://developers.openai.com/codex/windows).

When running natively on Windows, configure the native sandbox mode in `config.toml`:

```toml
[windows]
sandbox = "unelevated" # or "elevated"
# sandbox_private_desktop = true  # default; set false only for compatibility
```

See the [Windows setup guide](https://developers.openai.com/codex/windows#windows-sandbox) for details.

When you run Linux in a containerized environment such as Docker, the sandbox may not work if the host or container configuration blocks the namespace, setuid `bwrap`, or `seccomp` operations that Codex needs.

In that case, configure your Docker container to provide the isolation you need, then run `codex` with `--sandbox danger-full-access` (or the `--dangerously-bypass-approvals-and-sandbox` flag) inside the container.

### Run Codex in Dev Containers

If your host cannot run the Linux sandbox directly, or if your organization already standardizes on containerized development, run Codex with Dev Containers and let Docker provide the outer isolation boundary. This works with Visual Studio Code Dev Containers and compatible tools.

Use the [Codex secure devcontainer example](https://github.com/openai/codex/tree/main/.devcontainer) as a reference implementation. The example installs Codex, common development tools, `bubblewrap`, and firewall-based outbound controls.

Devcontainers provide substantial protection, but they do not prevent every
  attack. If you run Codex with `--sandbox danger-full-access` or
  `--dangerously-bypass-approvals-and-sandbox` inside the container, a malicious
  project can exfiltrate anything available inside the devcontainer, including
  Codex credentials. Use this pattern only with trusted repositories, and
  monitor Codex activity as you would in any other elevated environment.

The reference implementation includes:

- an Ubuntu 24.04 base image with Codex and common development tools installed;
- an allowlist-driven firewall profile for outbound access;
- VS Code settings and extension recommendations for reopening the workspace in a container;
- persistent mounts for command history and Codex configuration;
- `bubblewrap`, so Codex can still use its Linux sandbox when the container grants the needed capabilities.

To try it:

1. Install Visual Studio Code and the [Dev Containers extension](https://marketplace.visualstudio.com/items?itemName=ms-vscode-remote.remote-containers).
2. Copy the Codex example `.devcontainer` setup into your repository, or start from the Codex repository directly.
3. In VS Code, run **Dev Containers: Open Folder in Container...** and select `.devcontainer/devcontainer.secure.json`.
4. After the container starts, open a terminal and run `codex`.

You can also start the container from the CLI:

```bash
devcontainer up --workspace-folder . --config .devcontainer/devcontainer.secure.json
```

The example has three main pieces:

- `.devcontainer/devcontainer.secure.json` controls container settings, capabilities, mounts, environment variables, and VS Code extensions.
- `.devcontainer/Dockerfile.secure` defines the Ubuntu-based image and installed tools.
- `.devcontainer/init-firewall.sh` applies the outbound network policy.

The reference firewall is intentionally a starting point. If you depend on domain allowlisting for isolation, implement DNS rebinding and DNS refresh protections that fit your environment, such as TTL-aware refreshes or a DNS-aware firewall.

Inside the container, choose one of these modes:

- Keep Codex's Linux sandbox enabled if the Dev Container profile grants the capabilities needed for `bwrap` to create the inner sandbox.
- If the container is your intended security boundary, run Codex with `--sandbox danger-full-access` inside the container so Codex does not try to create a second sandbox layer.

## Version control

Codex works best with a version control workflow:

- Work on a feature branch and keep `git status` clean before delegating. This keeps Codex patches easier to isolate and revert.
- Prefer patch-based workflows (for example, `git diff`/`git apply`) over editing tracked files directly. Commit frequently so you can roll back in small increments.
- Treat Codex suggestions like any other PR: run targeted verification, review diffs, and document decisions in commit messages for auditing.

## Monitoring and telemetry

Codex supports opt-in monitoring via OpenTelemetry (OTel) to help teams audit usage, investigate issues, and meet compliance requirements without weakening local security defaults. Telemetry is off by default; enable it explicitly in your configuration.

### Overview

- Codex turns off OTel export by default to keep local runs self-contained.
- When enabled, Codex emits structured log events covering conversations, API requests, SSE/WebSocket stream activity, user prompts (redacted by default), tool approval decisions, and tool results.
- Codex tags exported events with `service.name` (originator), CLI version, and an environment label to separate dev/staging/prod traffic.

### Enable OTel (opt-in)

Add an `[otel]` block to your Codex configuration (typically `~/.codex/config.toml`), choosing an exporter and whether to log prompt text.

```toml
[otel]
environment = "staging"   # dev | staging | prod
exporter = "none"          # none | otlp-http | otlp-grpc
log_user_prompt = false     # redact prompt text unless policy allows
```

- `exporter = "none"` leaves instrumentation active but doesn't send data anywhere.
- To send events to your own collector, pick one of:

```toml
[otel]
exporter = { otlp-http = {
  endpoint = "https://otel.example.com/v1/logs",
  protocol = "binary",
  headers = { "x-otlp-api-key" = "${OTLP_TOKEN}" }
}}
```

```toml
[otel]
exporter = { otlp-grpc = {
  endpoint = "https://otel.example.com:4317",
  headers = { "x-otlp-meta" = "abc123" }
}}
```

Codex batches events and flushes them on shutdown. Codex exports only telemetry produced by its OTel module.

### Event categories

Representative event types include:

- `codex.conversation_starts` (model, reasoning settings, sandbox/approval policy)
- `codex.api_request` (attempt, status/success, duration, and error details)
- `codex.sse_event` (stream event kind, success/failure, duration, plus token counts on `response.completed`)
- `codex.websocket_request` and `codex.websocket_event` (request duration plus per-message kind/success/error)
- `codex.user_prompt` (length; content redacted unless explicitly enabled)
- `codex.tool_decision` (approved/denied, source: configuration vs. user)
- `codex.tool_result` (duration, success, output snippet)

Associated OTel metrics (counter plus duration histogram pairs) include `codex.api_request`, `codex.sse_event`, `codex.websocket.request`, `codex.websocket.event`, and `codex.tool.call` (with corresponding `.duration_ms` instruments).

For the full event catalog and configuration reference, see the [Codex configuration documentation on GitHub](https://github.com/openai/codex/blob/main/docs/config.md#otel).

### Security and privacy guidance

- Keep `log_user_prompt = false` unless policy explicitly permits storing prompt contents. Prompts can include source code and sensitive data.
- Route telemetry only to collectors you control; apply retention limits and access controls aligned with your compliance requirements.
- Treat tool arguments and outputs as sensitive. Favor redaction at the collector or SIEM when possible.
- Review local data retention settings (for example, `history.persistence` / `history.max_bytes`) if you don't want Codex to save session transcripts under `CODEX_HOME`. See [Advanced Config](https://developers.openai.com/codex/config-advanced#history-persistence) and [Configuration Reference](https://developers.openai.com/codex/config-reference).
- If you run the CLI with network access turned off, OTel export can't reach your collector. To export, allow network access in `workspace-write` mode for the OTel endpoint, or export from Codex cloud with the collector domain on your approved list.
- Review events periodically for approval/sandbox changes and unexpected tool executions.

OTel is optional and designed to complement, not replace, the sandbox and approval protections described above.

## Managed configuration

Enterprise admins can configure Codex security settings for their workspace in [Managed configuration](https://developers.openai.com/codex/enterprise/managed-configuration). See that page for setup and policy details.



---


# Remote connections

import {
  Desktop,
  Storage,
  Terminal,
} from "@components/react/oai/platform/ui/Icon.react";

Remote connections let you use Codex from another device or another machine.
Use Codex in the ChatGPT mobile app to work with Codex on a connected Mac or
Windows device, continue work from another supported Codex App device, or connect
the Codex App to projects on an SSH host.

Remote access uses the connected host's projects, threads, files, credentials,
permissions, plugins, Computer Use, browser setup, and local tools.

## What you can do remotely

- Start new threads in projects on the host, or continue existing ones.
- Send follow-up instructions, answer questions, and steer active work.
- Approve commands and other actions.
- Review outputs, diffs, test results, terminal output, and screenshots.
- Get notified when Codex completes a task or needs your attention.
- Switch between connected hosts and threads.

The next sections cover using Codex in the ChatGPT mobile app to control a Codex
App host. To connect Codex to a project on an SSH host, see
[connect to an SSH host](#connect-to-an-ssh-host).

<div class="not-prose my-6 max-w-4xl rounded-xl bg-[url('/images/codex/codex-wallpaper-1.webp')] bg-cover bg-center p-4 md:p-8">
  <CodexScreenshot
    alt="Codex mobile setup screen alongside the ChatGPT mobile Codex project list"
    lightSrc="/images/codex/app/mobile-setup-light.webp"
    darkSrc="/images/codex/app/mobile-setup-dark.webp"
    variant="no-wallpaper"
    maxHeight="none"
    maxWidth="420px"
  />
</div>

## Before you set up mobile access

Codex mobile setup supports Codex App hosts on macOS and Windows. You can
  control a Windows host from ChatGPT on iOS or Android, or from a Mac running
  Codex. Windows can't currently control another computer from the Codex App.

Make sure you have:

- Codex access in the ChatGPT account and workspace you want to use.
- The latest ChatGPT mobile app on an iOS or Android device. If you don't see
  Codex in the ChatGPT mobile app, update ChatGPT first.
- The latest Codex App for macOS or Windows running on a host that's awake,
  online, and signed in to the same account and workspace. Mobile setup starts
  from the Codex App; you can't set it up from the Codex CLI or IDE Extension.
- Any required multi-factor authentication, SSO, or passkey configuration for
  that account or workspace.

If you use Codex through a ChatGPT workspace, your admin may need to enable
Remote Control access before you can connect from your phone.

## Set up mobile access

Start in the Codex App on the host you want to connect. The setup flow enables
remote access for that host, then shows a QR code you can scan from your phone.

<WorkflowSteps variant="headings">

1. Start Codex mobile setup.

   Open Codex on the host and select **Set up Codex mobile** in the
   sidebar.

2. Scan the QR code.

   Use your phone to scan the QR code shown by Codex. The code opens ChatGPT so
   you can finish connecting the mobile app to the host.

3. Finish setup in ChatGPT.

   ChatGPT opens the Codex mobile setup flow. Confirm the same ChatGPT account
   and workspace, then complete any required multi-factor authentication, SSO,
   or passkey steps. After setup succeeds, the host appears in Codex on your
   phone.

4. Review host settings.

   In Codex on the host, use **Settings > Connections** to manage connected
   devices. You can also choose whether to keep the computer awake, enable
   Computer Use, or install the Chrome extension.

</WorkflowSteps>

<div class="not-prose my-6 max-w-4xl">
  <CodexScreenshot
    alt="Connections settings showing devices that can control this host and remote access settings"
    lightSrc="/images/codex/app/mobile-control-this-mac-framed-light.webp"
    darkSrc="/images/codex/app/mobile-control-this-mac-framed-dark.webp"
    maxHeight="480px"
    class="p-3 sm:p-4"
    imageClass="rounded-xl"
  />
</div>

## Choose what to connect

Start with the laptop or desktop where you already use Codex. Add an always-on
computer or SSH host when you need continuous access or a different environment.

### <span class="not-prose inline-flex items-center gap-3 align-middle"><span class="inline-flex h-7 w-7 shrink-0 items-center justify-center rounded-md bg-surface-secondary text-secondary"><Desktop width={17} height={17} /></span><span>Your laptop or desktop</span></span>

Connect the Mac or Windows PC where you already run Codex day to day. This gives
remote access to the same projects, threads, credentials, plugins, and local
setup you already use.

If that computer sleeps, loses network access, or closes Codex, remote access
stops until it's available again. If you use this computer as your host device,
keep it plugged in and use the host's connection settings to keep it awake where
available.

On a Mac laptop, remote access can stay available with the lid open and power
connected. With the lid closed, connect an external display as well. Choosing
**Sleep** still stops remote access.

On a Windows host, keep the session unlocked and available for tasks that use
[Computer Use](https://developers.openai.com/codex/app/computer-use). Computer use on Windows runs in the
foreground, so remote control is best for starting or checking work while you
dedicate the host desktop to the task.

### <span class="not-prose inline-flex items-center gap-3 align-middle"><span class="inline-flex h-7 w-7 shrink-0 items-center justify-center rounded-md bg-surface-secondary text-secondary"><Storage width={17} height={17} /></span><span>A dedicated always-on computer</span></span>

Use a dedicated always-on Mac or Windows PC when you want Codex to stay
reachable for longer-running work.

Install the projects, credentials, plugins, MCP servers, and tools Codex should
use on that machine.

### <span class="not-prose inline-flex items-center gap-3 align-middle"><span class="inline-flex h-7 w-7 shrink-0 items-center justify-center rounded-md bg-surface-secondary text-secondary"><Terminal width={17} height={17} /></span><span>A remote development environment</span></span>

Use an SSH host or managed remote development environment when the project
already lives in a remote environment. Connect the Codex App host to that
environment first; your phone still connects to the Codex App host, and Codex
works in the remote environment with its dependencies, security policies, and
compute resources.

For SSH setup details, see [connect to an SSH host](#connect-to-an-ssh-host).

For browser or desktop tasks on an always-on computer or remote host, enable
  Computer Use and install the Chrome extension on that host.

## What comes from the connected host

Your phone sends prompts, approvals, and follow-up messages to Codex. The
connected host provides the environment Codex uses.

That means:

- Repository files and local documents come from the connected host.
- Shell commands run on that host or remote environment.
- Any plugin installed on that host is available when you use Codex remotely.
- MCP servers, skills, browser access, and Computer Use come from that host's
  configuration.
- Signed-in websites and desktop apps are available only when the host can
  access them.
- The sandboxing settings, security controls, and action approvals still apply
  to the connected session.

Codex uses a secure relay layer to keep trusted machines reachable across your
authorized ChatGPT devices without exposing them directly to the public
internet.

## Pick up work from another device

You can continue work from another signed-in Codex App device that supports
remote control. For example, if your laptop is unavailable, you can start
a thread from your phone on an always-on host, then later open Codex on your
laptop and continue that same thread there.

In Codex on a Mac, use **Settings > Connections > Control other devices** to add
the other host. A device can allow remote access and control another device at
the same time. You can control Windows hosts from a Mac or from ChatGPT on iOS
or Android, but you can't use Windows to control another computer. For example,
you can control a Windows device from your Mac or phone, but you can't use a
Windows device to control another Windows device.

<div class="not-prose my-6 max-w-4xl">
  <CodexScreenshot
    alt="Connections settings showing another device available under Control other devices"
    lightSrc="/images/codex/app/mobile-control-other-devices-framed-light.webp"
    darkSrc="/images/codex/app/mobile-control-other-devices-framed-dark.webp"
    maxHeight="360px"
    class="p-3 sm:p-4"
    imageClass="rounded-xl"
  />
</div>

## Connect to an SSH host

In the Codex App, add remote projects from an SSH host and run threads against
the remote filesystem and shell. Remote project threads run commands, read
files, and write changes on the remote host.

Keep the remote host configured with the same security expectations you use for
normal SSH access: trusted keys, least-privilege accounts, and no
unauthenticated public listeners.

<WorkflowSteps variant="headings">

1. Add the host to your SSH config so Codex can auto-discover it.

   ```text
   Host devbox
     HostName devbox.example.com
     User you
     IdentityFile ~/.ssh/id_ed25519
   ```

   Codex reads concrete host aliases from `~/.ssh/config`, resolves them with
   OpenSSH, and ignores pattern-only hosts.

2. Confirm you can SSH to the host from the machine running the Codex App.

   ```bash
   ssh devbox
   ```

3. Install and authenticate Codex on the remote host.

   The app starts the remote Codex app server through SSH, using the remote
   user's login shell. Make sure the `codex` command is available on the
   remote host's `PATH` in that shell.

4. In the Codex App, open **Settings > Connections**, add or enable the SSH
   host, then choose a remote project folder.

</WorkflowSteps>

<CodexScreenshot
  alt="Codex app settings showing SSH remote connections"
  lightSrc="/images/codex/app/remote-connections-light.webp"
  darkSrc="/images/codex/app/remote-connections-dark.webp"
  maxHeight="420px"
  class="p-3 sm:p-4"
  imageClass="rounded-xl"
/>

## Authentication and network exposure

Remote connections use SSH to start and manage the remote Codex app server.
Don't expose app-server transports directly on a shared or public network.

If you need to reach a remote machine outside your current network, use a VPN
or mesh networking tool instead of exposing the app server directly to the
internet.

## Troubleshooting

### You don't see the host on your phone

Confirm that the Codex App is running on the host, you've enabled **Allow other
devices to connect**, and both devices use the same ChatGPT account and
workspace.

### The approval request doesn't appear

Open Codex in the ChatGPT mobile app. Confirm that the phone and host use the
same ChatGPT account and workspace, then scan the QR code again or restart setup
from the host. If you use a ChatGPT workspace, ask your admin to confirm that
they've enabled Remote Control access.

### The remote session disconnects

Check whether the host went to sleep, lost network access, or closed Codex.
Keep the host awake and connected while Codex works.

### Authentication blocks setup

Complete the account or workspace authentication prompt shown during setup. If
your organization requires SSO, multi-factor authentication, or a passkey,
finish that flow before trying again. If setup still fails, ask your workspace
admin to confirm that they've enabled Remote Control access.

## See also

- [Codex App](https://developers.openai.com/codex/app)
- [Codex App features](https://developers.openai.com/codex/app/features)
- [Codex App settings](https://developers.openai.com/codex/app/settings)
- [Computer Use](https://developers.openai.com/codex/app/computer-use)
- [Chrome extension](https://developers.openai.com/codex/app/chrome-extension)
- [Command line options](https://developers.openai.com/codex/cli/reference)
- [Authentication](https://developers.openai.com/codex/auth)



---



# Use Codex with Amazon Bedrock

Configure Codex to use OpenAI models available through Amazon Bedrock. In this
setup, Codex runs locally and sends model requests to Bedrock using
AWS-managed authentication and access controls.

## How it works

When you configure Codex with Amazon Bedrock as the model provider, the
OpenAI-hosted Responses API isn't in the request path. Codex sends model
requests to Amazon Bedrock, and Bedrock provides an OpenAI-compatible Responses
API implementation for supported OpenAI models.

Authentication is AWS-native. Users authenticate with a Bedrock API key or AWS
  IAM credentials. They do not use ChatGPT sign-in or `OPENAI_API_KEY` for this
  provider.

## Before you start

Make sure you have:

- Access to supported OpenAI models in Amazon Bedrock.
- An AWS Region where the selected model is available.
- Authentication for the Amazon Bedrock Mantle path configured for the AWS
  account.

## Configure Codex

Add the `amazon-bedrock` model provider for the Amazon Bedrock Mantle path to
`~/.codex/config.toml`. Supplying a model is optional. Select a supported model
explicitly when needed.

```toml
model_provider = "amazon-bedrock"
```

This guide covers the Amazon Bedrock Mantle path in supported commercial AWS
  Regions. Codex doesn't support Bedrock Mantle endpoints in AWS GovCloud
  Regions.

## Authentication options

Codex supports two Bedrock authentication paths. It checks them in this order:

1. Bedrock API key.
2. AWS SDK credential chain.

### Option 1: Bedrock API key

Set the Bedrock API key in the environment Codex reads. You must specify a
Region when using API-key authentication.

```shell
export AWS_BEARER_TOKEN_BEDROCK=<your-bedrock-api-key>
export AWS_REGION=us-east-2
```

### Option 2: AWS SDK credentials

Use this path when your organization manages Bedrock access through the AWS SDK
credential chain. Codex can use these standard AWS SDK credential sources:

1. Shared AWS `config` and `credentials` files.

   ```shell
   aws configure
   ```

2. Environment variables.

   ```shell
   export AWS_ACCESS_KEY_ID=<your-access-key-id>
   export AWS_SECRET_ACCESS_KEY=<your-secret-access-key>
   export AWS_SESSION_TOKEN=<your-session-token>
   ```

3. AWS Management Console credentials.

   ```shell
   aws login
   ```

4. AWS SSO or a named profile.

   ```shell
   aws sso login --profile codex-bedrock
   export AWS_PROFILE=codex-bedrock
   ```

5. Federated identity configured with `credential_process`. For corporate SSO or
   OIDC federation, configure the AWS profile outside Codex and let the AWS SDK
   resolve credentials. Put browser login, token exchange, caching, and refresh
   in your AWS profile's `credential_process` helper.

## Desktop app and VS Code extension

Desktop apps and IDE extensions may not inherit environment variables from the
shell. Put required values in `~/.codex/.env`, then restart the app or
extension.

```shell
export AWS_BEARER_TOKEN_BEDROCK=<your-bedrock-api-key>
export AWS_REGION=us-east-2
```

## Verify setup

- In Codex CLI, open `/status` and confirm Codex is using the
  `amazon-bedrock` model provider.
- In the desktop app or VS Code extension, start a new session after restarting
  the app.
- Confirm the selected model is available in the configured AWS Region and that
  the AWS identity has permission to access it.

## Supported models

Use exact model IDs:

```text
openai.gpt-5.5
openai.gpt-5.4
```

Model availability varies by AWS Region. Before selecting a model, see [model
support by AWS
Region](https://docs.aws.amazon.com/bedrock/latest/userguide/models-region-compatibility.html).

## Feature availability

This configuration supports local Codex workflows. Some features that depend on
OpenAI-hosted cloud services, hosted tools, or cloud-managed discovery aren't
currently available.

Fast Mode isn't available with Amazon Bedrock. Fast Mode uses priority
  processing, and the initial Amazon Bedrock offering supports on-demand
  inference only.

<ToggleSection title="Detailed feature availability">
  <CodexPlanFeatureMatrix
    client:load
    data={{
      plans: [
        {
          id: "bedrock",
          shortLabel: "Amazon Bedrock",
          label: "Amazon Bedrock",
        },
      ],
      sections: [
        {
          title: "Access and surfaces",
          features: [
            {
              name: "Codex web",
              href: "/codex/cloud",
              availability: {
                bedrock: "unavailable",
              },
            },
            {
              name: "Codex app for local tasks",
              href: "/codex/app",
              availability: {
                bedrock: "available",
              },
            },
            {
              name: "Codex CLI",
              href: "/codex/cli",
              availability: {
                bedrock: "available",
              },
            },
            {
              name: "IDE extension",
              href: "/codex/ide",
              availability: {
                bedrock: "available",
              },
            },
            {
              name: "Codex SDK, `codex exec`, and scriptable workflows",
              shortName: "Codex SDK and scripting",
              href: "/codex/sdk",
              availability: {
                bedrock: "available",
              },
            },
          ],
        },
        {
          title: "Models and multimodal",
          features: [
            {
              name: "Bedrock-backed inference with supported OpenAI models",
              shortName: "Bedrock-backed inference",
              href: "/codex/amazon-bedrock",
              availability: {
                bedrock: "available",
              },
            },
            {
              name: "Fast mode",
              href: "/codex/speed",
              availability: {
                bedrock: "unavailable",
              },
            },
            {
              name: "Image generation and editing",
              href: "/codex/app/features#image-generation",
              availability: {
                bedrock: "unavailable",
              },
            },
            {
              name: "Voice dictation",
              href: "/codex/app/features#voice-dictation",
              availability: {
                bedrock: "unavailable",
              },
            },
            {
              name: "Web search",
              href: "/codex/app/features#web-search",
              availability: {
                bedrock: "unavailable",
              },
            },
          ],
        },
        {
          title: "Local features",
          features: [
            {
              name: "Local code review with `/review`",
              shortName: "Local code review",
              href: "/codex/workflows#do-a-local-code-review",
              availability: {
                bedrock: "available",
              },
            },
            {
              name: "Auto-review for approval requests",
              href: "/codex/concepts/sandboxing/auto-review",
              availability: {
                bedrock: "available",
              },
            },
            {
              name: "Sandboxing and permission controls",
              href: "/codex/permissions",
              availability: {
                bedrock: "available",
              },
            },
            {
              name: "Project and standalone app automations",
              shortName: "App automations",
              href: "/codex/app/automations",
              availability: {
                bedrock: "available",
              },
            },
            {
              name: "Automations",
              href: "/codex/app/automations",
              availability: {
                bedrock: "available",
              },
            },
            {
              name: "Worktrees and built-in Git tools",
              shortName: "Built-in Git tools",
              href: "/codex/app/worktrees",
              availability: {
                bedrock: "available",
              },
            },
            {
              name: "Local environments and repeatable actions",
              shortName: "Repeatable actions",
              href: "/codex/app/local-environments",
              availability: {
                bedrock: "available",
              },
            },
            {
              name: "Appshots",
              href: "/codex/appshots",
              availability: {
                bedrock: "available",
              },
            },
          ],
        },
        {
          title: "Browser and remote control",
          features: [
            {
              name: "In-app browser previews and comments",
              shortName: "In-app browser",
              href: "/codex/app/browser",
              availability: {
                bedrock: "available",
              },
            },
            {
              name: "Browser Use automation",
              href: "/codex/app/browser#browser-use",
              availability: {
                bedrock: "limited",
              },
            },
            {
              name: "Chrome extension browser control",
              shortName: "Chrome browser control",
              href: "/codex/app/chrome-extension",
              availability: {
                bedrock: "limited",
              },
            },
            {
              name: "Computer Use",
              href: "/codex/app/computer-use",
              availability: {
                bedrock: "limited",
              },
            },
            {
              name: "SSH remote connections",
              shortName: "SSH remote",
              href: "/codex/remote-connections#connect-to-an-ssh-host",
              availability: {
                bedrock: "available",
              },
            },
            {
              name: "Mobile remote control",
              href: "/codex/remote-connections",
              availability: {
                bedrock: "unavailable",
              },
            },
          ],
        },
        {
          title: "Customization and extensions",
          features: [
            {
              name: "Custom instructions with `AGENTS.md`",
              shortName: "Custom instructions",
              href: "/codex/guides/agents-md",
              availability: {
                bedrock: "available",
              },
            },
            {
              name: "Skills",
              href: "/codex/skills",
              availability: {
                bedrock: "available",
              },
            },
            {
              name: "Plugins",
              href: "/codex/plugins",
              availability: {
                bedrock: "limited",
              },
              limitedFootnote: "plugins",
            },
            {
              name: "Plugin sharing",
              href: "/codex/plugins/build#share-a-local-plugin-with-your-workspace",
              availability: {
                bedrock: "unavailable",
              },
            },
            {
              name: "App connectors",
              href: "/codex/plugins",
              availability: {
                bedrock: "unavailable",
              },
            },
            {
              name: "MCP",
              href: "/codex/mcp",
              availability: {
                bedrock: "available",
              },
            },
            {
              name: "Subagents and custom agents",
              shortName: "Subagents",
              href: "/codex/subagents",
              availability: {
                bedrock: "available",
              },
            },
            {
              name: "Memories",
              href: "/codex/memories",
              availability: {
                bedrock: "limited",
              },
            },
            {
              name: "Chronicle",
              href: "/codex/memories/chronicle",
              availability: {
                bedrock: "unavailable",
              },
            },
          ],
        },
        {
          title: "Cloud and integrations",
          features: [
            {
              name: "Codex cloud tasks",
              shortName: "Cloud tasks",
              href: "/codex/cloud",
              availability: {
                bedrock: "unavailable",
              },
            },
            {
              name: "Sites",
              href: "/codex/sites",
              availability: {
                bedrock: "unavailable",
              },
            },
            {
              name: "GitHub issue and PR delegation with `@codex`",
              shortName: "GitHub delegation",
              href: "/codex/integrations/github#give-codex-other-tasks",
              availability: {
                bedrock: "unavailable",
              },
            },
            {
              name: "GitHub code review and automatic PR reviews",
              shortName: "GitHub PR reviews",
              href: "/codex/integrations/github",
              availability: {
                bedrock: "unavailable",
              },
            },
            {
              name: "Slack cloud integration",
              shortName: "Slack integration",
              href: "/codex/integrations/slack",
              availability: {
                bedrock: "unavailable",
              },
            },
            {
              name: "Linear cloud integration",
              shortName: "Linear integration",
              href: "/codex/integrations/linear",
              availability: {
                bedrock: "unavailable",
              },
            },
          ],
        },
        {
          title: "Admin, security, and analytics",
          features: [
            {
              name: "SAML SSO, MFA, and workspace user management",
              shortName: "Workspace management",
              href: "/codex/enterprise/admin-setup",
              availability: {
                bedrock: "unavailable",
              },
            },
            {
              name: "`requirements.toml` managed config",
              shortName: "`requirements.toml` config",
              href: "/codex/enterprise/managed-configuration",
              availability: {
                bedrock: "available",
              },
            },
            {
              name: "Cloud-managed config policies",
              shortName: "Cloud-managed policies",
              href: "/codex/enterprise/managed-configuration#cloud-managed-requirements",
              availability: {
                bedrock: "unavailable",
              },
            },
            {
              name: "Codex RBAC and custom roles",
              shortName: "RBAC and roles",
              href: "/codex/enterprise/admin-setup#step-2-set-up-custom-roles-rbac",
              availability: {
                bedrock: "unavailable",
              },
            },
            {
              name: "SCIM, EKM, and domain verification",
              shortName: "SCIM, EKM, and domains",
              href: "/codex/enterprise/admin-setup#enterprise-grade-security-and-privacy",
              availability: {
                bedrock: "unavailable",
              },
            },
            {
              name: "Enterprise retention and residency controls",
              shortName: "Retention and residency",
              href: "/codex/enterprise/admin-setup#enterprise-grade-security-and-privacy",
              availability: {
                bedrock: "unavailable",
              },
            },
            {
              name: "No training on API or business data by default",
              shortName: "No default training",
              href: "https://openai.com/business-data/",
              availability: {
                bedrock: "available",
              },
            },
            {
              name: "Analytics dashboard",
              href: "/codex/enterprise/governance#analytics-dashboard",
              availability: {
                bedrock: "unavailable",
              },
            },
            {
              name: "Analytics API",
              href: "/codex/enterprise/governance#analytics-api",
              availability: {
                bedrock: "unavailable",
              },
            },
            {
              name: "Compliance API and audit logs",
              shortName: "Compliance and audit logs",
              href: "/codex/enterprise/governance#compliance-api",
              availability: {
                bedrock: "unavailable",
              },
            },
            {
              name: "Codex Security for connected GitHub repositories",
              shortName: "Codex Security",
              href: "/codex/security",
              availability: {
                bedrock: "unavailable",
              },
            },
          ],
        },
      ],
    }}
  />

  <div
    id="codex-plan-region-limits"
    className="not-prose mt-3 text-sm text-secondary"
  >
    <sup>*</sup> Feature is currently limited to only specific regions. Check
    the individual feature documentation to learn more about geo restrictions.
  </div>
  <div
    id="codex-plan-plugin-limits"
    className="not-prose mt-1 text-sm text-secondary"
  >
    <sup>†</sup> Some first party plugins are not available.
  </div>
</ToggleSection>

## Troubleshooting

If setup fails, check the following:

- The model ID exactly matches a supported model.
- You specify an AWS Region where the model is available.
- The Bedrock API key or AWS credentials are valid and not expired.
- The AWS identity has permission to access the selected Bedrock model.
- `AWS_BEARER_TOKEN_BEDROCK` isn't set to an expired or unintended key.
- For desktop app or VS Code extension usage, required environment variables
  are present in `~/.codex/.env`.

## Support boundaries

OpenAI Support can help with Codex client setup, configuration, local CLI
behavior, desktop app behavior, IDE extension behavior, and the local Codex
product experience.

For AWS credentials, IAM permissions, Bedrock model access, quotas, billing,
regional availability, Bedrock request failures, AWS service logs, or Bedrock
service behavior, contact the customer's AWS administrator or AWS Support.


---


# Non-interactive mode

Non-interactive mode lets you run Codex from scripts (for example, continuous integration (CI) jobs) without opening the interactive TUI.
You invoke it with `codex exec`.

For flag-level details, see [`codex exec`](https://developers.openai.com/codex/cli/reference#codex-exec).

## When to use `codex exec`

Use `codex exec` when you want Codex to:

- Run as part of a pipeline (CI, pre-merge checks, scheduled jobs).
- Produce output you can pipe into other tools (for example, to generate release notes or summaries).
- Fit naturally into CLI workflows that chain command output into Codex and pass Codex output to other tools.
- Run with explicit, pre-set sandbox and approval settings.

## Basic usage

Pass a task prompt as a single argument:

```bash
codex exec "summarize the repository structure and list the top 5 risky areas"
```

While `codex exec` runs, Codex streams progress to `stderr` and prints only the final agent message to `stdout`. This makes it straightforward to redirect or pipe the final result:

```bash
codex exec "generate release notes for the last 10 commits" | tee release-notes.md
```

Use `--ephemeral` when you don't want to persist session rollout files to disk:

```bash
codex exec --ephemeral "triage this repository and suggest next steps"
```

If stdin is piped and you also provide a prompt argument, Codex treats the prompt as the instruction and the piped content as additional context.

This makes it easy to generate input with one command and hand it directly to Codex:

```bash
curl -s https://jsonplaceholder.typicode.com/comments \
  | codex exec "format the top 20 items into a markdown table" \
  > table.md
```

For more advanced stdin piping patterns, see [Advanced stdin piping](#advanced-stdin-piping).

## Permissions and safety

By default, `codex exec` runs in a read-only sandbox. In automation, set the least permissions needed for the workflow:

- Allow edits: `codex exec --sandbox workspace-write "<task>"`
- Allow broader access: `codex exec --sandbox danger-full-access "<task>"`

Use `danger-full-access` only in a controlled environment (for example, an isolated CI runner or container).

Codex keeps `codex exec --full-auto` as a deprecated compatibility flag and prints a warning. Prefer the explicit `--sandbox workspace-write` flag in new scripts.

Use `--ignore-user-config` when you need a run that doesn't load `$CODEX_HOME/config.toml`, and `--ignore-rules` when you need to skip user and project execpolicy `.rules` files for a controlled automation environment.

If you configure an enabled MCP server with `required = true` and it fails to initialize, `codex exec` exits with an error instead of continuing without that server.

## Make output machine-readable

To consume Codex output in scripts, use JSON Lines output:

```bash
codex exec --json "summarize the repo structure" | jq
```

When you enable `--json`, `stdout` becomes a JSON Lines (JSONL) stream so you can capture every event Codex emits while it's running. Event types include `thread.started`, `turn.started`, `turn.completed`, `turn.failed`, `item.*`, and `error`.

Item types include agent messages, reasoning, command executions, file changes, MCP tool calls, web searches, and plan updates.

Sample JSON stream (each line is a JSON object):

```jsonl
{"type":"thread.started","thread_id":"0199a213-81c0-7800-8aa1-bbab2a035a53"}
{"type":"turn.started"}
{"type":"item.started","item":{"id":"item_1","type":"command_execution","command":"bash -lc ls","status":"in_progress"}}
{"type":"item.completed","item":{"id":"item_3","type":"agent_message","text":"Repo contains docs, sdk, and examples directories."}}
{"type":"turn.completed","usage":{"input_tokens":24763,"cached_input_tokens":24448,"output_tokens":122,"reasoning_output_tokens":0}}
```

If you only need the final message, write it to a file with `-o <path>`/`--output-last-message <path>`. This writes the final message to the file and still prints it to `stdout` (see [`codex exec`](https://developers.openai.com/codex/cli/reference#codex-exec) for details).

## Create structured outputs with a schema

If you need structured data for downstream steps, use `--output-schema` to request a final response that conforms to a JSON Schema.
This is useful for automated workflows that need stable fields (for example, job summaries, risk reports, or release metadata).

`schema.json`

```json
{
  "type": "object",
  "properties": {
    "project_name": { "type": "string" },
    "programming_languages": {
      "type": "array",
      "items": { "type": "string" }
    }
  },
  "required": ["project_name", "programming_languages"],
  "additionalProperties": false
}
```

Run Codex with the schema and write the final JSON response to disk:

```bash
codex exec "Extract project metadata" \
  --output-schema ./schema.json \
  -o ./project-metadata.json
```

Example final output (stdout):

```json
{
  "project_name": "Codex CLI",
  "programming_languages": ["Rust", "TypeScript", "Shell"]
}
```

## Authenticate in automation

`codex exec` reuses saved CLI authentication by default. In CI, it's common to provide credentials explicitly:

### Use API key auth

For GitHub Actions, use the [Codex GitHub Action](https://developers.openai.com/codex/github-action) instead of installing and authenticating the CLI yourself. The action is designed to reduce API key exposure by installing Codex, starting a Responses API proxy, and running Codex with a configurable safety strategy.

Do not set `OPENAI_API_KEY` or `CODEX_API_KEY` as a job-level environment variable in workflows that check out or run repository-controlled code. Build scripts, tests, dependency lifecycle hooks, or a compromised action in the same job can read those environment variables.

For other automation environments, set `CODEX_API_KEY` only for the single `codex exec` invocation and make sure no untrusted code runs in the same process environment.

To use a different API key for a single run, set `CODEX_API_KEY` inline:

```bash
CODEX_API_KEY=<api-key> codex exec --json "triage open bug reports"
```

`CODEX_API_KEY` is only supported in `codex exec`.

<ToggleSection title="Use ChatGPT-managed auth in CI/CD (advanced)">
Read this if you need to run CI/CD jobs with a Codex user account instead of an
API key, such as enterprise teams using ChatGPT-managed Codex access on trusted
runners or users who need ChatGPT/Codex rate limits instead of API key usage.

API keys are the right default for automation because they are simpler to
provision and rotate. Use this path only if you specifically need to run as
your Codex account.

Treat `~/.codex/auth.json` like a password: it contains access tokens. Don't
commit it, paste it into tickets, or share it in chat.

Do not use this workflow for public or open-source repositories. If `codex login`
is not an option on the runner, seed `auth.json` through secure storage, run
Codex on the runner so Codex refreshes it in place, and persist the updated file
between runs.

See [Maintain Codex account auth in CI/CD (advanced)](https://developers.openai.com/codex/auth/ci-cd-auth).

</ToggleSection>

## Resume a non-interactive session

If you need to continue a previous run (for example, a two-stage pipeline), use the `resume` subcommand:

```bash
codex exec "review the change for race conditions"
codex exec resume --last "fix the race conditions you found"
```

You can also target a specific session ID with `codex exec resume <SESSION_ID>`.

## Git repository required

Codex requires commands to run inside a Git repository to prevent destructive changes. Override this check with `codex exec --skip-git-repo-check` if you're sure the environment is safe.

## Common automation patterns

### Example: Autofix CI failures in GitHub Actions

For GitHub Actions workflows, use [`openai/codex-action`](https://github.com/openai/codex-action) instead of installing Codex and passing the API key to a shell step. The action starts a secure proxy for the OpenAI API key.

You can use Codex to automatically propose fixes when a CI workflow fails. The pattern is:

1. Trigger a follow-up workflow when your main CI workflow completes with an error.
2. Check out the failing commit with repository read permissions only.
3. Run setup commands before Codex, without exposing your OpenAI API key to those steps.
4. Run the Codex GitHub Action.
5. Save Codex's local changes as a patch artifact.
6. In a separate job, apply the patch and open a pull request.

The Codex job below has only `contents: read`. After Codex runs, it only serializes the diff as an artifact. The `open_pr` job receives repository write permissions, but it does not receive `OPENAI_API_KEY`.

The example assumes a Node.js project. Adjust the setup and test commands to match your stack.

For a deeper security checklist, see the [Codex GitHub Action security guidance](https://github.com/openai/codex-action/blob/main/docs/security.md).

```yaml
name: Codex auto-fix on CI failure

on:
  workflow_run:
    workflows: ["CI"]
    types: [completed]

jobs:
  generate_fix:
    if: ${{ github.event.workflow_run.conclusion == 'failure' }}
    runs-on: ubuntu-latest
    permissions:
      contents: read
    outputs:
      has_patch: ${{ steps.diff.outputs.has_patch }}
    steps:
      - uses: actions/checkout@v5
        with:
          ref: ${{ github.event.workflow_run.head_sha }}
          fetch-depth: 0
          persist-credentials: false

      - uses: actions/setup-node@v4
        with:
          node-version: "20"

      - name: Install dependencies
        run: |
          if [ -f package-lock.json ]; then npm ci; fi

      - name: Run Codex
        uses: openai/codex-action@v1
        with:
          openai-api-key: ${{ secrets.OPENAI_API_KEY }}
          prompt: |
            The CI workflow "${{ github.event.workflow_run.name }}" failed for commit
            ${{ github.event.workflow_run.head_sha }}.

            Run `npm test --silent` to reproduce the failure. Identify the minimal
            change needed to make the tests pass, implement only that change, and
            run `npm test --silent` again.

            Do not refactor unrelated files.

      - name: Create patch artifact
        id: diff
        run: |
          git add -N .
          git diff --binary HEAD > codex.patch
          if [ -s codex.patch ]; then
            echo "has_patch=true" >> "$GITHUB_OUTPUT"
          else
            echo "has_patch=false" >> "$GITHUB_OUTPUT"
          fi

      - name: Upload patch artifact
        if: steps.diff.outputs.has_patch == 'true'
        uses: actions/upload-artifact@v4
        with:
          name: codex-fix-patch
          path: codex.patch
          if-no-files-found: error

  open_pr:
    runs-on: ubuntu-latest
    needs: generate_fix
    if: needs.generate_fix.outputs.has_patch == 'true'
    permissions:
      contents: write
      pull-requests: write
    steps:
      - uses: actions/checkout@v5
        with:
          ref: ${{ github.event.workflow_run.head_sha }}
          fetch-depth: 0

      - uses: actions/download-artifact@v4
        with:
          name: codex-fix-patch

      - name: Apply Codex patch
        run: git apply --index codex.patch

      - name: Open pull request
        env:
          GH_TOKEN: ${{ github.token }}
          FAILED_HEAD_BRANCH: ${{ github.event.workflow_run.head_branch }}
          FAILED_HEAD_SHA: ${{ github.event.workflow_run.head_sha }}
          RUN_ID: ${{ github.event.workflow_run.run_id }}
        run: |
          branch="codex/auto-fix-$RUN_ID"

          git config user.name "github-actions[bot]"
          git config user.email "41898282+github-actions[bot]@users.noreply.github.com"
          git switch -c "$branch"
          git commit -m "Auto-fix failing CI via Codex"
          git push origin "$branch"

          {
            echo "Codex generated this patch after CI failed for \`$FAILED_HEAD_SHA\`."
            echo
            echo "Review the changes before merging."
          } > pr-body.md

          gh pr create \
            --base "$FAILED_HEAD_BRANCH" \
            --head "$branch" \
            --title "Auto-fix failing CI via Codex" \
            --body-file pr-body.md
```

## Advanced stdin piping

When another command produces input for Codex, choose the stdin pattern based on where the instruction should come from. Use prompt-plus-stdin when you already know the instruction and want to pass piped output as context. Use `codex exec -` when stdin should become the full prompt.

### Use prompt-plus-stdin

Prompt-plus-stdin is useful when another command already produces the data you want Codex to inspect. In this mode, you write the instruction yourself and pipe in the output as context, which makes it a natural fit for CLI workflows built around command output, logs, and generated data.

```bash
npm test 2>&1 \
  | codex exec "summarize the failing tests and propose the smallest likely fix" \
  | tee test-summary.md
```

<ToggleSection title="More prompt-plus-stdin examples">

### Summarize logs

```bash
tail -n 200 app.log \
  | codex exec "identify the likely root cause, cite the most important errors, and suggest the next three debugging steps" \
  > log-triage.md
```

### Inspect TLS or HTTP issues

```bash
curl -vv https://api.example.com/health 2>&1 \
  | codex exec "explain the TLS or HTTP failure and suggest the most likely fix" \
  > tls-debug.md
```

### Prepare a Slack-ready update

```bash
gh run view 123456 --log \
  | codex exec "write a concise Slack-ready update on the CI failure, including the likely cause and next step" \
  | pbcopy
```

### Draft a pull request comment from CI logs

```bash
gh run view 123456 --log \
  | codex exec "summarize the failure in 5 bullets for the pull request thread" \
  | gh pr comment 789 --body-file -
```

</ToggleSection>

### Use `codex exec -` when stdin is the prompt

If you omit the prompt argument, Codex reads the prompt from stdin. Use `codex exec -` when you want to force that behavior explicitly.

The `-` sentinel is useful when another command or script is generating the entire prompt dynamically. This is a good fit when you store prompts in files, assemble prompts with shell scripts, or combine live command output with instructions before handing the whole prompt to Codex.

```bash
cat prompt.txt | codex exec -
```

```bash
printf "Summarize this error log in 3 bullets:\n\n%s\n" "$(tail -n 200 app.log)" \
  | codex exec -
```

```bash
generate_prompt.sh | codex exec - --json > result.jsonl
```



---



# Codex SDK

If you use Codex through the Codex CLI, the IDE extension, or Codex Web, you can also control it programmatically.

Use the SDK when you need to:

- Control Codex as part of your CI/CD pipeline
- Create your own agent that can engage with Codex to perform complex engineering tasks
- Build Codex into your own internal tools and workflows
- Integrate Codex within your own application

## TypeScript library

The TypeScript library provides a way to control Codex from within your application that's more comprehensive and flexible than non-interactive mode.

Use the library server-side; it requires Node.js 18 or later.

### Installation

To get started, install the Codex SDK using `npm`:

```bash
npm install @openai/codex-sdk
```

### Usage

Start a thread with Codex and run it with your prompt.

```ts


const codex = new Codex();
const thread = codex.startThread();
const result = await thread.run(
  "Make a plan to diagnose and fix the CI failures"
);

console.log(result);
```

Call `run()` again to continue on the same thread, or resume a past thread by providing a thread ID.

```ts
// running the same thread
const result = await thread.run("Implement the plan");

console.log(result);

// resuming past thread

const threadId = "<thread-id>";
const thread2 = codex.resumeThread(threadId);
const result2 = await thread2.run("Pick up where you left off");

console.log(result2);
```

For more details, check out the [TypeScript repo](https://github.com/openai/codex/tree/main/sdk/typescript).

## Python library

The Python SDK controls the local Codex app-server over JSON-RPC. It requires Python 3.10 or later. Published SDK builds include a pinned Codex CLI runtime dependency.

### Installation

To install the SDK run:

```bash
pip install openai-codex
```

Published SDK builds automatically use their pinned runtime. Pass `AppServerConfig(codex_bin=...)` only when you intentionally want to run against a specific local app-server binary.

### Usage

Start Codex, create a thread, and run a prompt:

```python
from openai_codex import Codex, Sandbox

with Codex() as codex:
    thread = codex.thread_start(
        model="gpt-5.4",
        sandbox=Sandbox.workspace_write,
    )
    result = thread.run("Make a plan to diagnose and fix the CI failures")
    print(result.final_response)
```

Use `AsyncCodex` when your application is already asynchronous:

```python
import asyncio

from openai_codex import AsyncCodex


async def main() -> None:
    async with AsyncCodex() as codex:
        thread = await codex.thread_start(model="gpt-5.4")
        result = await thread.run("Implement the plan")
        print(result.final_response)


asyncio.run(main())
```

### Sandbox presets

Use the same `Sandbox` presets when creating a thread or changing its filesystem
access for a later turn:

```python
from openai_codex import Codex, Sandbox

with Codex() as codex:
    thread = codex.thread_start(sandbox=Sandbox.workspace_write)
    thread.run("Make the requested change.")
    review = thread.run("Review the diff only.", sandbox=Sandbox.read_only)
```

Available presets:

- `Sandbox.read_only`: Read files without allowing writes.
- `Sandbox.workspace_write`: Read files and write inside the workspace and configured writable roots.
- `Sandbox.full_access`: Run without filesystem access restrictions.

When you omit `sandbox=`, app-server uses its configured default. A sandbox
passed to `run(...)` or `turn(...)` applies to that turn and later turns
on the thread.

For more details, check out the [Python repo](https://github.com/openai/codex/tree/main/sdk/python).


---



# Codex App Server

Codex app-server is the interface Codex uses to power rich clients (for example, the Codex VS Code extension). Use it when you want a deep integration inside your own product: authentication, conversation history, approvals, and streamed agent events. The app-server implementation is open source in the Codex GitHub repository ([openai/codex/codex-rs/app-server](https://github.com/openai/codex/tree/main/codex-rs/app-server)). See the [Open Source](https://developers.openai.com/codex/open-source) page for the full list of open-source Codex components.

If you are automating jobs or running Codex in CI, use the
  <a href="/codex/sdk">Codex SDK</a> instead.

## Protocol

Like [MCP](https://modelcontextprotocol.io/), `codex app-server` supports bidirectional communication using JSON-RPC 2.0 messages (with the `"jsonrpc":"2.0"` header omitted on the wire).

Supported transports:

- `stdio` (`--listen stdio://`, default): newline-delimited JSON (JSONL).
- `websocket` (`--listen ws://IP:PORT`, experimental and unsupported): one
  JSON-RPC message per WebSocket text frame.
- Unix socket (`--listen unix://` or `--listen unix://PATH`): WebSocket
  connections over Codex's default app-server control socket or a custom Unix
  socket path, using the standard HTTP Upgrade handshake.
- `off` (`--listen off`): don't expose a local transport.

When you run with `--listen ws://IP:PORT`, the same listener also serves basic
HTTP health probes:

- `GET /readyz` returns `200 OK` once the listener accepts new connections.
- `GET /healthz` returns `200 OK` when the request doesn't include an `Origin`
  header.
- Requests with an `Origin` header are rejected with `403 Forbidden`.

WebSocket transport is experimental and unsupported. Local listeners such as
`ws://127.0.0.1:PORT` are appropriate for localhost and SSH port-forwarding
workflows. Non-loopback WebSocket listeners currently allow unauthenticated
connections by default during rollout, so configure WebSocket auth before
exposing one remotely.

Supported WebSocket auth flags:

- `--ws-auth capability-token --ws-token-file /absolute/path`
- `--ws-auth capability-token --ws-token-sha256 HEX`
- `--ws-auth signed-bearer-token --ws-shared-secret-file /absolute/path`

For signed bearer tokens, you can also set `--ws-issuer`, `--ws-audience`, and
`--ws-max-clock-skew-seconds`. Clients present the credential as
`Authorization: Bearer <token>` during the WebSocket handshake, and app-server
enforces auth before JSON-RPC `initialize`.

Prefer `--ws-token-file` over passing raw bearer tokens on the command line. Use
`--ws-token-sha256` only when the client keeps the raw high-entropy token in a
separate local secret store; the hash is only a verifier, and clients still need
the original token.

In WebSocket mode, app-server uses bounded queues. When request ingress is full,
the server rejects new requests with JSON-RPC error code `-32001` and message
`"Server overloaded; retry later."` Clients should retry with an exponentially
increasing delay and jitter.

## Message schema

Requests include `method`, `params`, and `id`:

```json
{ "method": "thread/start", "id": 10, "params": { "model": "gpt-5.4" } }
```

Responses echo the `id` with either `result` or `error`:

```json
{ "id": 10, "result": { "thread": { "id": "thr_123" } } }
```

```json
{ "id": 10, "error": { "code": 123, "message": "Something went wrong" } }
```

Notifications omit `id` and use only `method` and `params`:

```json
{ "method": "turn/started", "params": { "turn": { "id": "turn_456" } } }
```

You can generate a TypeScript schema or a JSON Schema bundle from the CLI. Each output is specific to the Codex version you ran, so the generated artifacts match that version exactly:

```bash
codex app-server generate-ts --out ./schemas
codex app-server generate-json-schema --out ./schemas
```

## Getting started

1. Start the server with `codex app-server` (default stdio transport),
   `codex app-server --listen ws://127.0.0.1:4500` (TCP WebSocket), or
   `codex app-server --listen unix://` (default Unix socket).
2. Connect a client over the selected transport, then send `initialize` followed by the `initialized` notification.
3. Start a thread and a turn, then keep reading notifications from the active transport stream.

Example (Node.js / TypeScript):

```ts



const proc = spawn("codex", ["app-server"], {
  stdio: ["pipe", "pipe", "inherit"],
});
const rl = readline.createInterface({ input: proc.stdout });

const send = (message: unknown) => {
  proc.stdin.write(`${JSON.stringify(message)}\n`);
};

let threadId: string | null = null;

rl.on("line", (line) => {
  const msg = JSON.parse(line) as any;
  console.log("server:", msg);

  if (msg.id === 1 && msg.result?.thread?.id && !threadId) {
    threadId = msg.result.thread.id;
    send({
      method: "turn/start",
      id: 2,
      params: {
        threadId,
        input: [{ type: "text", text: "Summarize this repo." }],
      },
    });
  }
});

send({
  method: "initialize",
  id: 0,
  params: {
    clientInfo: {
      name: "my_product",
      title: "My Product",
      version: "0.1.0",
    },
  },
});
send({ method: "initialized", params: {} });
send({ method: "thread/start", id: 1, params: { model: "gpt-5.4" } });
```

## Core primitives

- **Thread**: A conversation between a user and the Codex agent. Threads contain turns.
- **Turn**: A single user request and the agent work that follows. Turns contain items and stream incremental updates.
- **Item**: A unit of input or output (user message, agent message, command runs, file change, tool call, and more).

Use the thread APIs to create, list, or archive conversations. Drive a conversation with turn APIs and stream progress via turn notifications.

## Lifecycle overview

- **Initialize once per connection**: Immediately after opening a transport connection, send an `initialize` request with your client metadata, then emit `initialized`. The server rejects any request on that connection before this handshake.
- **Start (or resume) a thread**: Call `thread/start` for a new conversation, `thread/resume` to continue an existing one, or `thread/fork` to branch history into a new thread id.
- **Begin a turn**: Call `turn/start` with the target `threadId` and user input. Optional fields override model, personality, `cwd`, sandbox policy, and more.
- **Steer an active turn**: Call `turn/steer` to append user input to the currently in-flight turn without creating a new turn.
- **Stream events**: After `turn/start`, keep reading notifications on stdout: `thread/archived`, `thread/unarchived`, `item/started`, `item/completed`, `item/agentMessage/delta`, tool progress, and other updates.
- **Finish the turn**: The server emits `turn/completed` with final status when the model finishes or after a `turn/interrupt` cancellation.

## Initialization

Clients must send a single `initialize` request per transport connection before invoking any other method on that connection, then acknowledge with an `initialized` notification. Requests sent before initialization receive a `Not initialized` error, and repeated `initialize` calls on the same connection return `Already initialized`.

The server returns the user agent string it will present to upstream services plus `platformFamily` and `platformOs` values that describe the runtime target. Set `clientInfo` to identify your integration.

`initialize.params.capabilities` also supports per-connection notification opt-out via `optOutNotificationMethods`, which is a list of exact method names to suppress for that connection. Matching is exact (no wildcards/prefixes). Unknown method names are accepted and ignored.

**Important**: Use `clientInfo.name` to identify your client for the OpenAI Compliance Logs Platform. If you are developing a new Codex integration intended for enterprise use, please contact OpenAI to get it added to a known clients list. For more context, see the [Codex logs reference](https://chatgpt.com/admin/api-reference#tag/Logs:-Codex).

Example (from the Codex VS Code extension):

```json
{
  "method": "initialize",
  "id": 0,
  "params": {
    "clientInfo": {
      "name": "codex_vscode",
      "title": "Codex VS Code Extension",
      "version": "0.1.0"
    }
  }
}
```

Example with notification opt-out:

```json
{
  "method": "initialize",
  "id": 1,
  "params": {
    "clientInfo": {
      "name": "my_client",
      "title": "My Client",
      "version": "0.1.0"
    },
    "capabilities": {
      "experimentalApi": true,
      "optOutNotificationMethods": ["thread/started", "item/agentMessage/delta"]
    }
  }
}
```

## Experimental API opt-in

Some app-server methods and fields are intentionally gated behind `experimentalApi` capability.

- Omit `capabilities` (or set `experimentalApi` to `false`) to stay on the stable API surface, and the server rejects experimental methods/fields.
- Set `capabilities.experimentalApi` to `true` to enable experimental methods and fields.

```json
{
  "method": "initialize",
  "id": 1,
  "params": {
    "clientInfo": {
      "name": "my_client",
      "title": "My Client",
      "version": "0.1.0"
    },
    "capabilities": {
      "experimentalApi": true
    }
  }
}
```

If a client sends an experimental method or field without opting in, app-server rejects it with:

`<descriptor> requires experimentalApi capability`

## API overview

- `thread/start` - create a new thread; emits `thread/started` and automatically subscribes you to turn/item events for that thread.
- `thread/resume` - reopen an existing thread by id so later `turn/start` calls append to it.
- `thread/fork` - fork a thread into a new thread id by copying stored history; emits `thread/started` for the new thread. Returned threads include `forkedFromId` when available.
- `thread/read` - read a stored thread by id without resuming it; set `includeTurns` to return full turn history. Returned `thread` objects include runtime `status`.
- `thread/list` - page through stored thread logs; supports cursor-based pagination plus `modelProviders`, `sourceKinds`, `archived`, `cwd`, and `searchTerm` filters. Returned `thread` objects include runtime `status`.
- `thread/turns/list` - page through a stored thread's turn history without resuming it. `itemsView` controls whether turn items are omitted, summarized, or fully loaded.
- `thread/turns/items/list` - reserved for paged turn-item loading; currently returns unsupported.
- `thread/loaded/list` - list the thread ids currently loaded in memory.
- `thread/name/set` - set or update a thread's user-facing name for a loaded thread or a persisted rollout; emits `thread/name/updated`.
- `thread/goal/set` - set the goal for a thread; emits `thread/goal/updated`.
- `thread/goal/get` - read the current goal for a thread.
- `thread/goal/clear` - clear the goal for a thread; emits `thread/goal/cleared`.
- `thread/metadata/update` - patch SQLite-backed stored thread metadata; currently supports persisted `gitInfo`.
- `thread/archive` - move a thread's log file into the archived directory; returns `{}` on success and emits `thread/archived`.
- `thread/unsubscribe` - unsubscribe this connection from thread turn/item events. If this was the last subscriber, the server unloads the thread after a no-subscriber inactivity grace period and emits `thread/closed`.
- `thread/unarchive` - restore an archived thread rollout back into the active sessions directory; returns the restored `thread` and emits `thread/unarchived`.
- `thread/status/changed` - notification emitted when a loaded thread's runtime `status` changes.
- `thread/compact/start` - trigger conversation history compaction for a thread; returns `{}` immediately while progress streams via `turn/*` and `item/*` notifications.
- `thread/shellCommand` - run a user-initiated shell command against a thread. This runs outside the sandbox with full access and doesn't inherit the thread sandbox policy.
- `thread/backgroundTerminals/clean` - stop all running background terminals for a thread (experimental; requires `capabilities.experimentalApi`).
- `thread/rollback` - drop the last N turns from the in-memory context and persist a rollback marker; returns the updated `thread`.
- `turn/start` - add user input to a thread and begin Codex generation; responds with the initial `turn` and streams events. For `collaborationMode`, `settings.developer_instructions: null` means "use built-in instructions for the selected mode."
- `thread/inject_items` - append raw Responses API items to a loaded thread's model-visible history without starting a user turn.
- `turn/steer` - append user input to the active in-flight turn for a thread; returns the accepted `turnId`.
- `turn/interrupt` - request cancellation of an in-flight turn; success is `{}` and the turn ends with `status: "interrupted"`.
- `review/start` - kick off the Codex reviewer for a thread; emits `enteredReviewMode` and `exitedReviewMode` items.
- `command/exec` - run a single command under the server sandbox without starting a thread/turn.
- `command/exec/write` - write `stdin` bytes to a running `command/exec` session or close `stdin`.
- `command/exec/resize` - resize a running PTY-backed `command/exec` session.
- `command/exec/terminate` - stop a running `command/exec` session.
- `command/exec/outputDelta` (notify) - emitted for base64-encoded stdout/stderr chunks from a streaming `command/exec` session.
- `process/spawn` - start an explicit process session outside Codex's sandbox (experimental; requires `capabilities.experimentalApi`).
- `process/writeStdin` - write stdin bytes to a running `process/spawn` session or close stdin (experimental).
- `process/resizePty` - resize a running PTY-backed process session (experimental).
- `process/kill` - terminate a running process session (experimental).
- `process/outputDelta` and `process/exited` (notify) - emitted for streaming process output and process exit status (experimental).
- `model/list` - list available models (set `includeHidden: true` to include entries with `hidden: true`) with effort options, optional `upgrade`, and `inputModalities`.
- `modelProvider/capabilities/read` - read provider capability bounds for model/provider combinations (experimental; requires `capabilities.experimentalApi`).
- `experimentalFeature/list` - list feature flags with lifecycle stage metadata and cursor pagination.
- `experimentalFeature/enablement/set` - patch in-memory runtime settings for supported feature keys such as `apps` and `plugins`.
- `collaborationMode/list` - list collaboration mode presets (experimental, no pagination).
- `skills/list` - list skills for one or more `cwd` values (supports `forceReload` and optional `perCwdExtraUserRoots`).
- `skills/changed` (notify) - emitted when watched local skill files change.
- `marketplace/add` - add a remote plugin marketplace and persist it into the user's marketplace config.
- `marketplace/upgrade` - refresh a configured Git marketplace, or all configured Git marketplaces when you omit the marketplace name.
- `plugin/list` - list discovered plugin marketplaces and plugin state, including install/auth policy metadata, marketplace load errors, featured plugin ids, and local, Git, or remote plugin source metadata.
- `plugin/read` - read one plugin by marketplace path or remote marketplace name and plugin name, including bundled skills, apps, and MCP server names when those details are available.
- `plugin/install` - install a plugin from a marketplace path or remote marketplace name.
- `plugin/uninstall` - uninstall an installed plugin.
- `app/list` - list available apps (connectors) with pagination plus accessibility/enabled metadata.
- `skills/config/write` - enable or disable skills by path.
- `mcpServer/oauth/login` - start an OAuth login for a configured MCP server; returns an authorization URL and emits `mcpServer/oauthLogin/completed` on completion.
- `tool/requestUserInput` - prompt the user with 1-3 short questions for a tool call (experimental); questions can set `isOther` for a free-form option.
- `config/mcpServer/reload` - reload MCP server configuration from disk and queue a refresh for loaded threads.
- `mcpServerStatus/list` - list MCP servers, tools, resources, and auth status (cursor + limit pagination). Use `detail: "full"` for full data or `detail: "toolsAndAuthOnly"` to omit resources.
- `mcpServer/resource/read` - read a single MCP resource through an initialized MCP server.
- `mcpServer/tool/call` - call a tool on a thread's configured MCP server.
- `mcpServer/startupStatus/updated` (notify) - emitted when a configured MCP server's startup status changes for a loaded thread.
- `windowsSandbox/setupStart` - start Windows sandbox setup for `elevated` or `unelevated` mode; returns quickly and later emits `windowsSandbox/setupCompleted`.
- `feedback/upload` - submit a feedback report (classification + optional reason/logs + conversation id, plus optional `extraLogFiles` attachments).
- `config/read` - fetch the effective configuration on disk after resolving configuration layering.
- `externalAgentConfig/detect` - detect external-agent artifacts that can be migrated with `includeHome` and optional `cwds`; each detected item includes `cwd` (`null` for home).
- `externalAgentConfig/import` - apply selected external-agent migration items by passing explicit `migrationItems` with `cwd` (`null` for home). Supported item types include config, skills, `AGENTS.md`, plugins, MCP server config, subagents, hooks, commands, and sessions; plugin imports emit `externalAgentConfig/import/completed`.
- `config/value/write` - write a single configuration key/value to the user's `config.toml` on disk.
- `config/batchWrite` - apply configuration edits atomically to the user's `config.toml` on disk.
- `configRequirements/read` - fetch requirements from `requirements.toml` and/or MDM, including allow-lists, pinned `featureRequirements`, and residency/network requirements (or `null` if you haven't set any up).
- `fs/readFile`, `fs/writeFile`, `fs/createDirectory`, `fs/getMetadata`, `fs/readDirectory`, `fs/remove`, `fs/copy`, `fs/watch`, `fs/unwatch`, and `fs/changed` (notify) - operate on absolute filesystem paths through the app-server v2 filesystem API.

Plugin summaries include a `source` union. Local plugins return
`{ "type": "local", "path": ... }`, Git-backed marketplace entries return
`{ "type": "git", "url": ..., "path": ..., "refName": ..., "sha": ... }`,
and remote catalog entries return `{ "type": "remote" }`. For remote-only
catalog entries, `PluginMarketplaceEntry.path` can be `null`; pass
`remoteMarketplaceName` instead of `marketplacePath` when reading or installing
those plugins.

## Models

### List models (`model/list`)

Call `model/list` to discover available models and their capabilities before rendering model or personality selectors.

```json
{ "method": "model/list", "id": 6, "params": { "limit": 20, "includeHidden": false } }
{ "id": 6, "result": {
  "data": [{
    "id": "gpt-5.4",
    "model": "gpt-5.4",
    "displayName": "GPT-5.4",
    "hidden": false,
    "defaultReasoningEffort": "medium",
    "supportedReasoningEfforts": [{
      "reasoningEffort": "low",
      "description": "Lower latency"
    }],
    "inputModalities": ["text", "image"],
    "supportsPersonality": true,
    "isDefault": true
  }],
  "nextCursor": null
} }
```

Each model entry can include:

- `supportedReasoningEfforts` - supported effort options for the model.
- `defaultReasoningEffort` - suggested default effort for clients.
- `upgrade` - optional recommended upgrade model id for migration prompts in clients.
- `upgradeInfo` - optional upgrade metadata for migration prompts in clients.
- `hidden` - whether the model is hidden from the default picker list.
- `inputModalities` - supported input types for the model (for example `text`, `image`).
- `supportsPersonality` - whether the model supports personality-specific instructions such as `/personality`.
- `isDefault` - whether the model is the recommended default.

By default, `model/list` returns picker-visible models only. Set `includeHidden: true` if you need the full list and want to filter on the client side using `hidden`.

When `inputModalities` is missing (older model catalogs), treat it as `["text", "image"]` for backward compatibility.

### List experimental features (`experimentalFeature/list`)

Use this endpoint to discover feature flags with metadata and lifecycle stage:

```json
{ "method": "experimentalFeature/list", "id": 7, "params": { "limit": 20 } }
{ "id": 7, "result": {
  "data": [{
    "name": "unified_exec",
    "stage": "beta",
    "displayName": "Unified exec",
    "description": "Use the unified PTY-backed execution tool.",
    "announcement": "Beta rollout for improved command execution reliability.",
    "enabled": false,
    "defaultEnabled": false
  }],
  "nextCursor": null
} }
```

`stage` can be `beta`, `underDevelopment`, `stable`, `deprecated`, or `removed`. For non-beta flags, `displayName`, `description`, and `announcement` may be `null`.

## Threads

- `thread/read` reads a stored thread without subscribing to it; set `includeTurns` to include turns.
- `thread/turns/list` pages through a stored thread's turn history without
  resuming it. Use `itemsView` to choose whether turn items are omitted,
  summarized, or fully loaded.
- `thread/list` supports cursor pagination plus `modelProviders`, `sourceKinds`, `archived`, `cwd`, and `searchTerm` filtering.
- `thread/loaded/list` returns the thread IDs currently in memory.
- `thread/archive` moves the thread's persisted JSONL log into the archived directory.
- `thread/metadata/update` patches stored thread metadata, currently including persisted `gitInfo`.
- `thread/unsubscribe` unsubscribes the current connection from a loaded thread and can trigger `thread/closed` after an inactivity grace period.
- `thread/unarchive` restores an archived thread rollout back into the active sessions directory.
- `thread/compact/start` triggers compaction and returns `{}` immediately.
- `thread/rollback` drops the last N turns from the in-memory context and records a rollback marker in the thread's persisted JSONL log.
- `thread/inject_items` appends raw Responses API items to a loaded thread's model-visible history without starting a user turn.

### Start or resume a thread

Start a fresh thread when you need a new Codex conversation.

```json
{ "method": "thread/start", "id": 10, "params": {
  "model": "gpt-5.4",
  "cwd": "/Users/me/project",
  "approvalPolicy": "never",
  "sandbox": "workspaceWrite",
  "personality": "friendly",
  "serviceName": "my_app_server_client"
} }
{ "id": 10, "result": {
  "thread": {
    "id": "thr_123",
    "sessionId": "thr_123",
    "preview": "",
    "ephemeral": false,
    "modelProvider": "openai",
    "createdAt": 1730910000
  }
} }
{ "method": "thread/started", "params": { "thread": { "id": "thr_123" } } }
```

`serviceName` is optional. Set it when you want app-server to tag thread-level metrics with your integration's service name.

`thread.sessionId` identifies the current live session tree root. Root threads
use their own thread id as the session id; forked threads keep the session id
of the root they came from. Clients should read the session id from
`thread.sessionId` instead of deriving it from the thread id.

To continue a stored session, call `thread/resume` with the `thread.id` you recorded earlier. The response shape matches `thread/start`. You can also pass the same configuration overrides supported by `thread/start`, such as `personality`:

```json
{ "method": "thread/resume", "id": 11, "params": {
  "threadId": "thr_123",
  "personality": "friendly"
} }
{ "id": 11, "result": { "thread": { "id": "thr_123", "name": "Bug bash notes", "ephemeral": false } } }
```

Resuming a thread doesn't update `thread.updatedAt` (or the rollout file's modified time) by itself. The timestamp updates when you start a turn.

If you mark an enabled MCP server as `required` in config and that server fails to initialize, `thread/start` and `thread/resume` fail instead of continuing without it.

`dynamicTools` on `thread/start` is an experimental field (requires `capabilities.experimentalApi = true`). Codex persists these dynamic tools in the thread rollout metadata and restores them on `thread/resume` when you don't supply new dynamic tools.

If you resume with a different model than the one recorded in the rollout, Codex emits a warning and applies a one-time model-switch instruction on the next turn.

### Manage a thread goal

Use `thread/goal/set`, `thread/goal/get`, and `thread/goal/clear` to manage the
same persisted goal state surfaced by `/goal` in the TUI.

```json
{ "method": "thread/goal/set", "id": 13, "params": {
  "threadId": "thr_123",
  "objective": "Finish the migration and keep tests green",
  "status": "active",
  "tokenBudget": 40000
} }
{ "id": 13, "result": { "goal": {
  "threadId": "thr_123",
  "objective": "Finish the migration and keep tests green",
  "status": "active",
  "tokenBudget": 40000,
  "tokensUsed": 0,
  "timeUsedSeconds": 0
} } }
{ "method": "thread/goal/updated", "params": {
  "threadId": "thr_123",
  "goal": {
    "threadId": "thr_123",
    "objective": "Finish the migration and keep tests green",
    "status": "active",
    "tokenBudget": 40000,
    "tokensUsed": 0,
    "timeUsedSeconds": 0
  }
} }
```

Goal objectives must be non-empty and at most 4,000 characters. Supplying a new
objective replaces the goal and resets usage accounting. Supplying the current
non-terminal objective, or omitting `objective`, updates status or token budget
while preserving usage history.

To branch from a stored session, call `thread/fork` with the `thread.id`. This creates a new thread id and emits a `thread/started` notification for it:

```json
{ "method": "thread/fork", "id": 12, "params": { "threadId": "thr_123" } }
{ "id": 12, "result": { "thread": { "id": "thr_456", "sessionId": "thr_123", "forkedFromId": "thr_123" } } }
{ "method": "thread/started", "params": { "thread": { "id": "thr_456" } } }
```

When a user-facing thread title has been set, app-server hydrates `thread.name` on `thread/list`, `thread/read`, `thread/resume`, `thread/unarchive`, and `thread/rollback` responses. `thread/start` and `thread/fork` may omit `name` (or return `null`) until a title is set later.

### Read a stored thread (without resuming)

Use `thread/read` when you want stored thread data but don't want to resume the thread or subscribe to its events.

- `includeTurns` - when `true`, the response includes the thread's turns; when `false` or omitted, you get the thread summary only.
- Returned `thread` objects include runtime `status` (`notLoaded`, `idle`, `systemError`, or `active` with `activeFlags`).

```json
{ "method": "thread/read", "id": 19, "params": { "threadId": "thr_123", "includeTurns": true } }
{ "id": 19, "result": { "thread": { "id": "thr_123", "name": "Bug bash notes", "ephemeral": false, "status": { "type": "notLoaded" }, "turns": [] } } }
```

Unlike `thread/resume`, `thread/read` doesn't load the thread into memory or emit `thread/started`.

### List thread turns

Use `thread/turns/list` to page a stored thread's turn history without resuming it. Results default to newest-first so clients can fetch older turns with `nextCursor`. The response also includes `backwardsCursor`; pass it as `cursor` with `sortDirection: "asc"` to fetch turns newer than the first item from the earlier page.

`itemsView` controls how much turn-item data the response includes:

- `notLoaded` omits items.
- `summary` returns summarized item data and is the default when omitted.
- `full` returns full item data.

```json
{ "method": "thread/turns/list", "id": 20, "params": {
  "threadId": "thr_123",
  "limit": 50,
  "sortDirection": "desc",
  "itemsView": "summary"
} }
{ "id": 20, "result": {
  "data": [],
  "nextCursor": "older-turns-cursor-or-null",
  "backwardsCursor": "newer-turns-cursor-or-null"
} }
```

`thread/turns/items/list` is reserved for paged turn-item loading, but the
current server returns an unsupported-method error.

### List threads (with pagination & filters)

`thread/list` lets you render a history UI. Results default to newest-first by `createdAt`. Filters apply before pagination. Pass any combination of:

- `cursor` - opaque string from a prior response; omit for the first page.
- `limit` - server defaults to a reasonable page size if unset.
- `sortKey` - `created_at` (default) or `updated_at`.
- `modelProviders` - restrict results to specific providers; unset, null, or an empty array includes all providers.
- `sourceKinds` - restrict results to specific thread sources. When omitted or `[]`, the server defaults to interactive sources only: `cli` and `vscode`.
- `archived` - when `true`, list archived threads only. When `false` or omitted, list non-archived threads (default).
- `cwd` - restrict results to threads whose session current working directory exactly matches this path.
- `searchTerm` - search stored thread summaries and metadata before pagination.

`sourceKinds` accepts the following values:

- `cli`
- `vscode`
- `exec`
- `appServer`
- `subAgent`
- `subAgentReview`
- `subAgentCompact`
- `subAgentThreadSpawn`
- `subAgentOther`
- `unknown`

Example:

```json
{ "method": "thread/list", "id": 20, "params": {
  "cursor": null,
  "limit": 25,
  "sortKey": "created_at"
} }
{ "id": 20, "result": {
  "data": [
    { "id": "thr_a", "preview": "Create a TUI", "ephemeral": false, "modelProvider": "openai", "createdAt": 1730831111, "updatedAt": 1730831111, "name": "TUI prototype", "status": { "type": "notLoaded" } },
    { "id": "thr_b", "preview": "Fix tests", "ephemeral": true, "modelProvider": "openai", "createdAt": 1730750000, "updatedAt": 1730750000, "status": { "type": "notLoaded" } }
  ],
  "nextCursor": "opaque-token-or-null"
} }
```

When `nextCursor` is `null`, you have reached the final page.

### Update stored thread metadata

Use `thread/metadata/update` to patch stored thread metadata without resuming the thread. Today this supports persisted `gitInfo`; omitted fields are left unchanged, and explicit `null` clears a stored value.

```json
{ "method": "thread/metadata/update", "id": 21, "params": {
  "threadId": "thr_123",
  "gitInfo": { "branch": "feature/sidebar-pr" }
} }
{ "id": 21, "result": {
  "thread": {
    "id": "thr_123",
    "gitInfo": { "sha": null, "branch": "feature/sidebar-pr", "originUrl": null }
  }
} }
```

### Track thread status changes

`thread/status/changed` is emitted whenever a loaded thread's runtime status changes. The payload includes `threadId` and the new `status`.

```json
{
  "method": "thread/status/changed",
  "params": {
    "threadId": "thr_123",
    "status": { "type": "active", "activeFlags": ["waitingOnApproval"] }
  }
}
```

### List loaded threads

`thread/loaded/list` returns thread IDs currently loaded in memory.

```json
{ "method": "thread/loaded/list", "id": 21 }
{ "id": 21, "result": { "data": ["thr_123", "thr_456"] } }
```

### Unsubscribe from a loaded thread

`thread/unsubscribe` removes the current connection's subscription to a thread. The response status is one of:

- `unsubscribed` when the connection was subscribed and is now removed.
- `notSubscribed` when the connection wasn't subscribed to that thread.
- `notLoaded` when the thread isn't loaded.

If this was the last subscriber, the server keeps the thread loaded until it has no subscribers and no thread activity for 30 minutes. When the grace period expires, app-server unloads the thread and emits a `thread/status/changed` transition to `notLoaded` plus `thread/closed`.

```json
{ "method": "thread/unsubscribe", "id": 22, "params": { "threadId": "thr_123" } }
{ "id": 22, "result": { "status": "unsubscribed" } }
```

If the thread later expires:

```json
{ "method": "thread/status/changed", "params": {
    "threadId": "thr_123",
    "status": { "type": "notLoaded" }
} }
{ "method": "thread/closed", "params": { "threadId": "thr_123" } }
```

### Archive a thread

Use `thread/archive` to move the persisted thread log (stored as a JSONL file on disk) into the archived sessions directory.

```json
{ "method": "thread/archive", "id": 22, "params": { "threadId": "thr_b" } }
{ "id": 22, "result": {} }
{ "method": "thread/archived", "params": { "threadId": "thr_b" } }
```

Archived threads won't appear in future calls to `thread/list` unless you pass `archived: true`.

### Unarchive a thread

Use `thread/unarchive` to move an archived thread rollout back into the active sessions directory.

```json
{ "method": "thread/unarchive", "id": 24, "params": { "threadId": "thr_b" } }
{ "id": 24, "result": { "thread": { "id": "thr_b", "name": "Bug bash notes" } } }
{ "method": "thread/unarchived", "params": { "threadId": "thr_b" } }
```

### Trigger thread compaction

Use `thread/compact/start` to trigger manual history compaction for a thread. The request returns immediately with `{}`.

App-server emits progress as standard `turn/*` and `item/*` notifications on the same `threadId`, including a `contextCompaction` item lifecycle (`item/started` then `item/completed`).

```json
{ "method": "thread/compact/start", "id": 25, "params": { "threadId": "thr_b" } }
{ "id": 25, "result": {} }
```

### Run a thread shell command

Use `thread/shellCommand` for user-initiated shell commands that belong to a thread. The request returns immediately with `{}` while progress streams through standard `turn/*` and `item/*` notifications.

This API runs outside the sandbox with full access and doesn't inherit the thread sandbox policy. Clients should expose it only for explicit user-initiated commands.

If the thread already has an active turn, the command runs as an auxiliary action on that turn and its formatted output is injected into the turn's message stream. If the thread is idle, app-server starts a standalone turn for the shell command.

```json
{ "method": "thread/shellCommand", "id": 26, "params": { "threadId": "thr_b", "command": "git status --short" } }
{ "id": 26, "result": {} }
```

### Clean background terminals

Use `thread/backgroundTerminals/clean` to stop all running background terminals associated with a thread. This method is experimental and requires `capabilities.experimentalApi = true`.

```json
{ "method": "thread/backgroundTerminals/clean", "id": 27, "params": { "threadId": "thr_b" } }
{ "id": 27, "result": {} }
```

### Roll back recent turns

Use `thread/rollback` to remove the last `numTurns` entries from the in-memory context and persist a rollback marker in the rollout log. The returned `thread` includes `turns` populated after the rollback.

```json
{ "method": "thread/rollback", "id": 28, "params": { "threadId": "thr_b", "numTurns": 1 } }
{ "id": 28, "result": { "thread": { "id": "thr_b", "name": "Bug bash notes", "ephemeral": false } } }
```

## Turns

The `input` field accepts a list of items:

- `{ "type": "text", "text": "Explain this diff" }`
- `{ "type": "image", "url": "https://.../design.png" }`
- `{ "type": "localImage", "path": "/tmp/screenshot.png" }`

You can override configuration settings per turn (model, effort, personality, `cwd`, sandbox policy, summary). When specified, these settings become the defaults for later turns on the same thread. `outputSchema` applies only to the current turn. For `sandboxPolicy.type = "externalSandbox"`, set `networkAccess` to `restricted` or `enabled`; for `workspaceWrite`, `networkAccess` remains a boolean.

For `turn/start.collaborationMode`, `settings.developer_instructions: null` means "use built-in instructions for the selected mode" rather than clearing mode instructions.

### Sandbox read access (`ReadOnlyAccess`)

`sandboxPolicy` supports explicit read-access controls:

- `readOnly`: optional `access` (`{ "type": "fullAccess" }` by default, or restricted roots).
- `workspaceWrite`: optional `readOnlyAccess` (`{ "type": "fullAccess" }` by default, or restricted roots).

Restricted read access shape:

```json
{
  "type": "restricted",
  "includePlatformDefaults": true,
  "readableRoots": ["/Users/me/shared-read-only"]
}
```

On macOS, `includePlatformDefaults: true` appends a curated platform-default Seatbelt policy for restricted-read sessions. This improves tool compatibility without broadly allowing all of `/System`.

Examples:

```json
{ "type": "readOnly", "access": { "type": "fullAccess" } }
```

```json
{
  "type": "workspaceWrite",
  "writableRoots": ["/Users/me/project"],
  "readOnlyAccess": {
    "type": "restricted",
    "includePlatformDefaults": true,
    "readableRoots": ["/Users/me/shared-read-only"]
  },
  "networkAccess": false
}
```

### Start a turn

```json
{ "method": "turn/start", "id": 30, "params": {
  "threadId": "thr_123",
  "input": [ { "type": "text", "text": "Run tests" } ],
  "cwd": "/Users/me/project",
  "approvalPolicy": "unlessTrusted",
  "sandboxPolicy": {
    "type": "workspaceWrite",
    "writableRoots": ["/Users/me/project"],
    "networkAccess": true
  },
  "model": "gpt-5.4",
  "effort": "medium",
  "summary": "concise",
  "personality": "friendly",
  "outputSchema": {
    "type": "object",
    "properties": { "answer": { "type": "string" } },
    "required": ["answer"],
    "additionalProperties": false
  }
} }
{ "id": 30, "result": { "turn": { "id": "turn_456", "status": "inProgress", "items": [], "error": null } } }
```

### Inject items into a thread

Use `thread/inject_items` to append prebuilt Responses API items to a loaded thread's prompt history without starting a user turn. These items are persisted to the rollout and included in subsequent model requests.

```json
{ "method": "thread/inject_items", "id": 31, "params": {
  "threadId": "thr_123",
  "items": [
    {
      "type": "message",
      "role": "assistant",
      "content": [{ "type": "output_text", "text": "Previously computed context." }]
    }
  ]
} }
{ "id": 31, "result": {} }
```

### Steer an active turn

Use `turn/steer` to append more user input to the active in-flight turn.

- Include `expectedTurnId`; it must match the active turn id.
- The request fails if there is no active turn on the thread.
- `turn/steer` doesn't emit a new `turn/started` notification.
- `turn/steer` doesn't accept turn-level overrides (`model`, `cwd`, `sandboxPolicy`, or `outputSchema`).

```json
{ "method": "turn/steer", "id": 32, "params": {
  "threadId": "thr_123",
  "input": [ { "type": "text", "text": "Actually focus on failing tests first." } ],
  "expectedTurnId": "turn_456"
} }
{ "id": 32, "result": { "turnId": "turn_456" } }
```

### Start a turn (invoke a skill)

Invoke a skill explicitly by including `$<skill-name>` in the text input and adding a `skill` input item alongside it.

```json
{ "method": "turn/start", "id": 33, "params": {
  "threadId": "thr_123",
  "input": [
    { "type": "text", "text": "$skill-creator Add a new skill for triaging flaky CI and include step-by-step usage." },
    { "type": "skill", "name": "skill-creator", "path": "/Users/me/.codex/skills/skill-creator/SKILL.md" }
  ]
} }
{ "id": 33, "result": { "turn": { "id": "turn_457", "status": "inProgress", "items": [], "error": null } } }
```

### Interrupt a turn

```json
{ "method": "turn/interrupt", "id": 31, "params": { "threadId": "thr_123", "turnId": "turn_456" } }
{ "id": 31, "result": {} }
```

On success, the turn finishes with `status: "interrupted"`.

## Review

`review/start` runs the Codex reviewer for a thread and streams review items. Targets include:

- `uncommittedChanges`
- `baseBranch` (diff against a branch)
- `commit` (review a specific commit)
- `custom` (free-form instructions)

Use `delivery: "inline"` (default) to run the review on the existing thread, or `delivery: "detached"` to fork a new review thread.

Example request/response:

```json
{ "method": "review/start", "id": 40, "params": {
  "threadId": "thr_123",
  "delivery": "inline",
  "target": { "type": "commit", "sha": "1234567deadbeef", "title": "Polish tui colors" }
} }
{ "id": 40, "result": {
  "turn": {
    "id": "turn_900",
    "status": "inProgress",
    "items": [
      { "type": "userMessage", "id": "turn_900", "content": [ { "type": "text", "text": "Review commit 1234567: Polish tui colors" } ] }
    ],
    "error": null
  },
  "reviewThreadId": "thr_123"
} }
```

For a detached review, use `"delivery": "detached"`. The response is the same shape, but `reviewThreadId` will be the id of the new review thread (different from the original `threadId`). The server also emits a `thread/started` notification for that new thread before streaming the review turn.

Codex streams the usual `turn/started` notification followed by an `item/started` with an `enteredReviewMode` item:

```json
{
  "method": "item/started",
  "params": {
    "item": {
      "type": "enteredReviewMode",
      "id": "turn_900",
      "review": "current changes"
    }
  }
}
```

When the reviewer finishes, the server emits `item/started` and `item/completed` containing an `exitedReviewMode` item with the final review text:

```json
{
  "method": "item/completed",
  "params": {
    "item": {
      "type": "exitedReviewMode",
      "id": "turn_900",
      "review": "Looks solid overall..."
    }
  }
}
```

Use this notification to render the reviewer output in your client.

## Process execution

`process/*` is an experimental, explicit process-control API. It requires
`capabilities.experimentalApi = true` and runs outside Codex's sandbox. Use it
only when your client intentionally exposes local process control without a
sandbox.

Start a process with `process/spawn` and provide a `processHandle`, then use
that handle for stdin, resize, and kill requests. Output streams through
`process/outputDelta` notifications and completion streams through
`process/exited`.

```json
{ "method": "process/spawn", "id": 48, "params": {
  "command": ["python3", "-m", "pytest", "-q"],
  "processHandle": "pytest-1",
  "cwd": "/Users/me/project",
  "tty": true
} }
{ "id": 48, "result": {} }
{ "method": "process/outputDelta", "params": {
  "processHandle": "pytest-1",
  "stream": "stdout",
  "deltaBase64": "Li4u"
} }
{ "method": "process/exited", "params": {
  "processHandle": "pytest-1",
  "exitCode": 0
} }
```

Use `process/writeStdin` with `deltaBase64`, `closeStdin`, or both to send
input. Use `process/resizePty` for PTY resize events and `process/kill` to
terminate a running process.

## Command execution

`command/exec` runs a single command (`argv` array) under the server sandbox without creating a thread.

```json
{ "method": "command/exec", "id": 50, "params": {
  "command": ["ls", "-la"],
  "cwd": "/Users/me/project",
  "sandboxPolicy": { "type": "workspaceWrite" },
  "timeoutMs": 10000
} }
{ "id": 50, "result": { "exitCode": 0, "stdout": "...", "stderr": "" } }
```

Use `sandboxPolicy.type = "externalSandbox"` if you already sandbox the server process and want Codex to skip its own sandbox enforcement. For external sandbox mode, set `networkAccess` to `restricted` (default) or `enabled`. For `readOnly` and `workspaceWrite`, use the same optional `access` / `readOnlyAccess` structure shown above.

Notes:

- The server rejects empty `command` arrays.
- `sandboxPolicy` accepts the same shape used by `turn/start` (for example, `dangerFullAccess`, `readOnly`, `workspaceWrite`, `externalSandbox`).
- When omitted, `timeoutMs` falls back to the server default.
- Set `tty: true` for PTY-backed sessions, and use `processId` when you plan to follow up with `command/exec/write`, `command/exec/resize`, or `command/exec/terminate`.
- Set `streamStdoutStderr: true` to receive `command/exec/outputDelta` notifications while the command is running.

### Read admin requirements (`configRequirements/read`)

Use `configRequirements/read` to inspect the effective admin requirements loaded from `requirements.toml` and/or MDM.

```json
{ "method": "configRequirements/read", "id": 52, "params": {} }
{ "id": 52, "result": {
  "requirements": {
    "allowedApprovalPolicies": ["onRequest", "unlessTrusted"],
    "allowedSandboxModes": ["readOnly", "workspaceWrite"],
    "featureRequirements": {
      "personality": true,
      "unified_exec": false
    },
    "network": {
      "enabled": true,
      "allowedDomains": ["api.openai.com"],
      "allowUnixSockets": ["/tmp/example.sock"],
      "dangerouslyAllowAllUnixSockets": false
    }
  }
} }
```

`result.requirements` is `null` when no requirements are configured. See the docs on [`requirements.toml`](https://developers.openai.com/codex/config-reference#requirementstoml) for details on supported keys and values.

### Windows sandbox setup (`windowsSandbox/setupStart`)

Custom Windows clients can trigger sandbox setup asynchronously instead of blocking on startup checks.

```json
{ "method": "windowsSandbox/setupStart", "id": 53, "params": { "mode": "elevated" } }
{ "id": 53, "result": { "started": true } }
```

App-server starts setup in the background and later emits a completion notification:

```json
{
  "method": "windowsSandbox/setupCompleted",
  "params": { "mode": "elevated", "success": true, "error": null }
}
```

Modes:

- `elevated` - run the elevated Windows sandbox setup path.
- `unelevated` - run the legacy setup/preflight path.

## Filesystem

The v2 filesystem APIs operate on absolute paths. Use `fs/watch` when a client needs to invalidate UI state after a file or directory changes.

```json
{ "method": "fs/watch", "id": 54, "params": {
  "watchId": "0195ec6b-1d6f-7c2e-8c7a-56f2c4a8b9d1",
  "path": "/Users/me/project/.git/HEAD"
} }
{ "id": 54, "result": { "path": "/Users/me/project/.git/HEAD" } }
{ "method": "fs/changed", "params": {
  "watchId": "0195ec6b-1d6f-7c2e-8c7a-56f2c4a8b9d1",
  "changedPaths": ["/Users/me/project/.git/HEAD"]
} }
{ "method": "fs/unwatch", "id": 55, "params": {
  "watchId": "0195ec6b-1d6f-7c2e-8c7a-56f2c4a8b9d1"
} }
{ "id": 55, "result": {} }
```

Watching a file emits `fs/changed` for that file path, including updates delivered by replace or rename operations.

## Events

Event notifications are the server-initiated stream for thread lifecycles, turn lifecycles, and the items within them. After you start or resume a thread, keep reading the active transport stream for `thread/started`, `thread/archived`, `thread/unarchived`, `thread/closed`, `thread/status/changed`, `turn/*`, `item/*`, and `serverRequest/resolved` notifications.

### Notification opt-out

Clients can suppress specific notifications per connection by sending exact method names in `initialize.params.capabilities.optOutNotificationMethods`.

- Exact-match only: `item/agentMessage/delta` suppresses only that method.
- Unknown method names are ignored.
- Applies to the current `thread/*`, `turn/*`, `item/*`, and related v2 notifications.
- Doesn't apply to requests, responses, or errors.

### Fuzzy file search events (experimental)

The fuzzy file search session API emits per-query notifications:

- `fuzzyFileSearch/sessionUpdated` - `{ sessionId, query, files }` with the current matches for the active query.
- `fuzzyFileSearch/sessionCompleted` - `{ sessionId }` once indexing and matching for that query completes.

### Windows sandbox setup events

- `windowsSandbox/setupCompleted` - `{ mode, success, error }` emitted after a `windowsSandbox/setupStart` request finishes.

### Turn events

- `turn/started` - `{ turn }` with the turn id, empty `items`, and `status: "inProgress"`.
- `turn/completed` - `{ turn }` where `turn.status` is `completed`, `interrupted`, or `failed`; failures carry `{ error: { message, codexErrorInfo?, additionalDetails? } }`.
- `turn/diff/updated` - `{ threadId, turnId, diff }` with the latest aggregated unified diff across every file change in the turn.
- `turn/plan/updated` - `{ turnId, explanation?, plan }` whenever the agent shares or changes its plan; each `plan` entry is `{ step, status }` with `status` in `pending`, `inProgress`, or `completed`.
- `thread/tokenUsage/updated` - usage updates for the active thread.

`turn/diff/updated` and `turn/plan/updated` currently include empty `items` arrays even when item events stream. Use `item/*` notifications as the source of truth for turn items.

### Items

`ThreadItem` is the tagged union carried in turn responses and `item/*` notifications. Common item types include:

- `userMessage` - `{id, content}` where `content` is a list of user inputs (`text`, `image`, or `localImage`).
- `agentMessage` - `{id, text, phase?}` containing the accumulated agent reply. When present, `phase` uses Responses API wire values (`commentary`, `final_answer`).
- `plan` - `{id, text}` containing proposed plan text in plan mode. Treat the final `plan` item from `item/completed` as authoritative.
- `reasoning` - `{id, summary, content}` where `summary` holds streamed reasoning summaries and `content` holds raw reasoning blocks.
- `commandExecution` - `{id, command, cwd, status, commandActions, aggregatedOutput?, exitCode?, durationMs?}`.
- `fileChange` - `{id, changes, status}` describing proposed edits; `changes` list `{path, kind, diff}`.
- `mcpToolCall` - `{id, server, tool, status, arguments, result?, error?}`.
- `dynamicToolCall` - `{id, tool, arguments, status, contentItems?, success?, durationMs?}` for client-executed dynamic tool invocations.
- `collabToolCall` - `{id, tool, status, senderThreadId, receiverThreadId?, newThreadId?, prompt?, agentStatus?}`.
- `webSearch` - `{id, query, action?}` for web search requests issued by the agent.
- `imageView` - `{id, path}` emitted when the agent invokes the image viewer tool.
- `enteredReviewMode` - `{id, review}` sent when the reviewer starts.
- `exitedReviewMode` - `{id, review}` emitted when the reviewer finishes.
- `contextCompaction` - `{id}` emitted when Codex compacts the conversation history.

For `webSearch.action`, the action `type` can be `search` (`query?`, `queries?`), `openPage` (`url?`), or `findInPage` (`url?`, `pattern?`).

The app server deprecates the legacy `thread/compacted` notification; use the `contextCompaction` item instead.

All items emit two shared lifecycle events:

- `item/started` - emits the full `item` when a new unit of work begins; the `item.id` matches the `itemId` used by deltas.
- `item/completed` - sends the final `item` once work finishes; treat this as the authoritative state.

### Item deltas

- `item/agentMessage/delta` - appends streamed text for the agent message.
- `item/plan/delta` - streams proposed plan text. The final `plan` item may not exactly equal the concatenated deltas.
- `item/reasoning/summaryTextDelta` - streams readable reasoning summaries; `summaryIndex` increments when a new summary section opens.
- `item/reasoning/summaryPartAdded` - marks a boundary between reasoning summary sections.
- `item/reasoning/textDelta` - streams raw reasoning text (when supported by the model).
- `item/commandExecution/outputDelta` - streams stdout/stderr for a command; append deltas in order.
- `item/fileChange/outputDelta` - deprecated compatibility notification for legacy `apply_patch` text output. Current app-server versions no longer emit it; use `fileChange` items and `turn/diff/updated` instead.

## Errors

If a turn fails, the server emits an `error` event with `{ error: { message, codexErrorInfo?, additionalDetails? } }` and then finishes the turn with `status: "failed"`. When an upstream HTTP status is available, it appears in `codexErrorInfo.httpStatusCode`.

Common `codexErrorInfo` values include:

- `ContextWindowExceeded`
- `UsageLimitExceeded`
- `HttpConnectionFailed` (4xx/5xx upstream errors)
- `ResponseStreamConnectionFailed`
- `ResponseStreamDisconnected`
- `ResponseTooManyFailedAttempts`
- `BadRequest`, `Unauthorized`, `SandboxError`, `InternalServerError`, `Other`

When an upstream HTTP status is available, the server forwards it in `httpStatusCode` on the relevant `codexErrorInfo` variant.

## Approvals

Depending on a user's Codex settings, command execution and file changes may require approval. The app-server sends a server-initiated JSON-RPC request to the client, and the client responds with a decision payload.

- Command execution decisions: `accept`, `acceptForSession`, `decline`, `cancel`, or `{ "acceptWithExecpolicyAmendment": { "execpolicy_amendment": ["cmd", "..."] } }`.
- File change decisions: `accept`, `acceptForSession`, `decline`, `cancel`.

- Requests include `threadId` and `turnId` - use them to scope UI state to the active conversation.
- The server resumes or declines the work and ends the item with `item/completed`.

### Command execution approvals

Order of messages:

1. `item/started` shows the pending `commandExecution` item with `command`, `cwd`, and other fields.
2. `item/commandExecution/requestApproval` includes `itemId`, `threadId`, `turnId`, optional `reason`, optional `command`, optional `cwd`, optional `commandActions`, optional `proposedExecpolicyAmendment`, optional `networkApprovalContext`, and optional `availableDecisions`. When `initialize.params.capabilities.experimentalApi = true`, the payload can also include experimental `additionalPermissions` describing requested per-command sandbox access. Any filesystem paths inside `additionalPermissions` are absolute on the wire.
3. Client responds with one of the command execution approval decisions above.
4. `serverRequest/resolved` confirms that the pending request has been answered or cleared.
5. `item/completed` returns the final `commandExecution` item with `status: completed | failed | declined`.

When `networkApprovalContext` is present, the prompt is for managed network access (not a general shell-command approval). The current v2 schema exposes the target `host` and `protocol`; clients should render a network-specific prompt and not rely on `command` being a user-meaningful shell command preview.

Codex groups concurrent network approval prompts by destination (`host`, protocol, and port). The app-server may therefore send one prompt that unblocks multiple queued requests to the same destination, while different ports on the same host are treated separately.

### File change approvals

Order of messages:

1. `item/started` emits a `fileChange` item with proposed `changes` and `status: "inProgress"`.
2. `item/fileChange/requestApproval` includes `itemId`, `threadId`, `turnId`, optional `reason`, and optional `grantRoot`.
3. Client responds with one of the file change approval decisions above.
4. `serverRequest/resolved` confirms that the pending request has been answered or cleared.
5. `item/completed` returns the final `fileChange` item with `status: completed | failed | declined`.

### `tool/requestUserInput`

When the client responds to `item/tool/requestUserInput`, app-server emits `serverRequest/resolved` with `{ threadId, requestId }`. If the pending request is cleared by turn start, turn completion, or turn interruption before the client answers, the server emits the same notification for that cleanup.

### Dynamic tool calls (experimental)

`dynamicTools` on `thread/start` and the corresponding `item/tool/call` request or response flow are experimental APIs.

Dynamic tool names and namespace names must follow Responses API naming
constraints. Avoid reserved namespace names used by built-in Codex tools.

When a dynamic tool is invoked during a turn, app-server emits:

1. `item/started` with `item.type = "dynamicToolCall"`, `status = "inProgress"`, plus `tool` and `arguments`.
2. `item/tool/call` as a server request to the client.
3. The client response payload with returned content items.
4. `item/completed` with `item.type = "dynamicToolCall"`, the final `status`, and any returned `contentItems` or `success` value.

### MCP tool-call approvals (apps)

App (connector) tool calls can also require approval. When an app tool call has side effects, the server may elicit approval with `tool/requestUserInput` and options such as **Accept**, **Decline**, and **Cancel**. Destructive tool annotations always trigger approval even when the tool also advertises less-privileged hints. If the user declines or cancels, the related `mcpToolCall` item completes with an error instead of running the tool.

## Skills

Invoke a skill by including `$<skill-name>` in the user text input. Add a `skill` input item (recommended) so the server injects full skill instructions instead of relying on the model to resolve the name.

```json
{
  "method": "turn/start",
  "id": 101,
  "params": {
    "threadId": "thread-1",
    "input": [
      {
        "type": "text",
        "text": "$skill-creator Add a new skill for triaging flaky CI."
      },
      {
        "type": "skill",
        "name": "skill-creator",
        "path": "/Users/me/.codex/skills/skill-creator/SKILL.md"
      }
    ]
  }
}
```

If you omit the `skill` item, the model will still parse the `$<skill-name>` marker and try to locate the skill, which can add latency.

Example:

```
$skill-creator Add a new skill for triaging flaky CI and include step-by-step usage.
```

Use `skills/list` to fetch available skills (optionally scoped by `cwds`, with `forceReload`). You can also include `perCwdExtraUserRoots` to scan extra absolute paths as `user` scope for specific `cwd` values. App-server ignores entries whose `cwd` isn't present in `cwds`. `skills/list` may reuse a cached result per `cwd`; set `forceReload: true` to refresh from disk. When present, the server reads `interface` and `dependencies` from `SKILL.json`.

```json
{ "method": "skills/list", "id": 25, "params": {
  "cwds": ["/Users/me/project", "/Users/me/other-project"],
  "forceReload": true,
  "perCwdExtraUserRoots": [
    {
      "cwd": "/Users/me/project",
      "extraUserRoots": ["/Users/me/shared-skills"]
    }
  ]
} }
{ "id": 25, "result": {
  "data": [{
    "cwd": "/Users/me/project",
    "skills": [
      {
        "name": "skill-creator",
        "description": "Create or update a Codex skill",
        "enabled": true,
        "interface": {
          "displayName": "Skill Creator",
          "shortDescription": "Create or update a Codex skill"
        },
        "dependencies": {
          "tools": [
            {
              "type": "env_var",
              "value": "GITHUB_TOKEN",
              "description": "GitHub API token"
            },
            {
              "type": "mcp",
              "value": "github",
              "transport": "streamable_http",
              "url": "https://example.com/mcp"
            }
          ]
        }
      }
    ],
    "errors": []
  }]
} }
```

The server also emits `skills/changed` notifications when watched local skill files change. Treat this as an invalidation signal and rerun `skills/list` with your current params when needed.

To enable or disable a skill by path:

```json
{
  "method": "skills/config/write",
  "id": 26,
  "params": {
    "path": "/Users/me/.codex/skills/skill-creator/SKILL.md",
    "enabled": false
  }
}
```

## Apps (connectors)

Use `app/list` to fetch available apps. In the CLI/TUI, `/apps` is the user-facing picker; in custom clients, call `app/list` directly. Each entry includes both `isAccessible` (available to the user) and `isEnabled` (enabled in `config.toml`) so clients can distinguish install/access from local enabled state. App entries can also include optional `branding`, `appMetadata`, and `labels` fields.

```json
{ "method": "app/list", "id": 50, "params": {
  "cursor": null,
  "limit": 50,
  "threadId": "thread-1",
  "forceRefetch": false
} }
{ "id": 50, "result": {
  "data": [
    {
      "id": "demo-app",
      "name": "Demo App",
      "description": "Example connector for documentation.",
      "logoUrl": "https://example.com/demo-app.png",
      "logoUrlDark": null,
      "distributionChannel": null,
      "branding": null,
      "appMetadata": null,
      "labels": null,
      "installUrl": "https://chatgpt.com/apps/demo-app/demo-app",
      "isAccessible": true,
      "isEnabled": true
    }
  ],
  "nextCursor": null
} }
```

If you provide `threadId`, app feature gating (`features.apps`) uses that thread's config snapshot. When omitted, app-server uses the latest global config.

`app/list` returns after both accessible apps and directory apps load. Set `forceRefetch: true` to bypass app caches and fetch fresh data. Cache entries are only replaced when refreshes succeed.

The server also emits `app/list/updated` notifications whenever either source (accessible apps or directory apps) finishes loading. Each notification includes the latest merged app list.

```json
{
  "method": "app/list/updated",
  "params": {
    "data": [
      {
        "id": "demo-app",
        "name": "Demo App",
        "description": "Example connector for documentation.",
        "logoUrl": "https://example.com/demo-app.png",
        "logoUrlDark": null,
        "distributionChannel": null,
        "branding": null,
        "appMetadata": null,
        "labels": null,
        "installUrl": "https://chatgpt.com/apps/demo-app/demo-app",
        "isAccessible": true,
        "isEnabled": true
      }
    ]
  }
}
```

Invoke an app by inserting `$<app-slug>` in the text input and adding a `mention` input item with the `app://<id>` path (recommended).

```json
{
  "method": "turn/start",
  "id": 51,
  "params": {
    "threadId": "thread-1",
    "input": [
      {
        "type": "text",
        "text": "$demo-app Pull the latest updates from the team."
      },
      {
        "type": "mention",
        "name": "Demo App",
        "path": "app://demo-app"
      }
    ]
  }
}
```

### Config RPC examples for app settings

Use `config/read`, `config/value/write`, and `config/batchWrite` to inspect or update app controls in `config.toml`.

Read the effective app config shape (including `_default` and per-tool overrides):

```json
{ "method": "config/read", "id": 60, "params": { "includeLayers": false } }
{ "id": 60, "result": {
  "config": {
    "apps": {
      "_default": {
        "enabled": true,
        "destructive_enabled": true,
        "open_world_enabled": true
      },
      "google_drive": {
        "enabled": true,
        "destructive_enabled": false,
        "default_tools_approval_mode": "prompt",
        "tools": {
          "files/delete": { "enabled": false, "approval_mode": "approve" }
        }
      }
    }
  }
} }
```

Update a single app setting:

```json
{
  "method": "config/value/write",
  "id": 61,
  "params": {
    "keyPath": "apps.google_drive.default_tools_approval_mode",
    "value": "prompt",
    "mergeStrategy": "replace"
  }
}
```

Apply multiple app edits atomically:

```json
{
  "method": "config/batchWrite",
  "id": 62,
  "params": {
    "edits": [
      {
        "keyPath": "apps._default.destructive_enabled",
        "value": false,
        "mergeStrategy": "upsert"
      },
      {
        "keyPath": "apps.google_drive.tools.files/delete.approval_mode",
        "value": "approve",
        "mergeStrategy": "upsert"
      }
    ]
  }
}
```

### Detect and import external agent config

Use `externalAgentConfig/detect` to discover external-agent artifacts that can be migrated, then pass the selected entries to `externalAgentConfig/import`.

Detection example:

```json
{ "method": "externalAgentConfig/detect", "id": 63, "params": {
  "includeHome": true,
  "cwds": ["/Users/me/project"]
} }
{ "id": 63, "result": {
  "items": [
    {
      "itemType": "AGENTS_MD",
      "description": "Import /Users/me/project/CLAUDE.md to /Users/me/project/AGENTS.md.",
      "cwd": "/Users/me/project"
    },
    {
      "itemType": "SKILLS",
      "description": "Copy skill folders from /Users/me/.claude/skills to /Users/me/.agents/skills.",
      "cwd": null
    }
  ]
} }
```

Import example:

```json
{ "method": "externalAgentConfig/import", "id": 64, "params": {
  "migrationItems": [
    {
      "itemType": "AGENTS_MD",
      "description": "Import /Users/me/project/CLAUDE.md to /Users/me/project/AGENTS.md.",
      "cwd": "/Users/me/project"
    }
  ]
} }
{ "id": 64, "result": {} }
```

When a request includes plugin imports, the server emits `externalAgentConfig/import/completed` after the import finishes. This notification may arrive immediately after the response or after background remote imports complete.

Supported `itemType` values are `AGENTS_MD`, `CONFIG`, `SKILLS`, `PLUGINS`,
and `MCP_SERVER_CONFIG`. For `PLUGINS` items, `details.plugins` lists each
`marketplaceName` and the `pluginNames` Codex can try to migrate. Detection
returns only items that still have work to do. For example, Codex skips AGENTS
migration when `AGENTS.md` already exists and is non-empty, and skill imports
don't overwrite existing skill directories.

When detecting plugins from `.claude/settings.json`, Codex reads configured
marketplace sources from `extraKnownMarketplaces`. If `enabledPlugins` contains
plugins from `claude-plugins-official` but the marketplace source is missing,
Codex infers `anthropics/claude-plugins-official` as the source.

## Auth endpoints

The JSON-RPC auth/account surface exposes request/response methods plus server-initiated notifications (no `id`). Use these to determine auth state, start or cancel logins, logout, inspect ChatGPT rate limits, and notify workspace owners about depleted credits or usage limits.

### Authentication modes

Codex supports these authentication modes. `account/updated.authMode` shows the active mode and includes the current ChatGPT `planType` when available. `account/read` also reports account and plan details.

- **API key (`apikey`)** - the caller supplies an OpenAI API key with `type: "apiKey"`, and Codex stores it for API requests.
- **ChatGPT managed (`chatgpt`)** - Codex owns the ChatGPT OAuth flow, persists tokens, and refreshes them automatically. Start with `type: "chatgpt"` for the browser flow or `type: "chatgptDeviceCode"` for the device-code flow.
- **ChatGPT external tokens (`chatgptAuthTokens`)** - experimental and intended for host apps that already own the user's ChatGPT auth lifecycle. The host app supplies an `accessToken`, `chatgptAccountId`, and optional `chatgptPlanType` directly, and must refresh the token when asked.

### API overview

- `account/read` - fetch current account info; optionally refresh tokens.
- `account/login/start` - begin login (`apiKey`, `chatgpt`, `chatgptDeviceCode`, or experimental `chatgptAuthTokens`).
- `account/login/completed` (notify) - emitted when a login attempt finishes (success or error).
- `account/login/cancel` - cancel a pending managed ChatGPT login by `loginId`.
- `account/logout` - sign out; triggers `account/updated`.
- `account/updated` (notify) - emitted whenever auth mode changes (`authMode`: `apikey`, `chatgpt`, `chatgptAuthTokens`, or `null`) and includes `planType` when available.
- `account/chatgptAuthTokens/refresh` (server request) - request fresh externally managed ChatGPT tokens after an authorization error.
- `account/rateLimits/read` - fetch ChatGPT rate limits.
- `account/rateLimits/updated` (notify) - emitted whenever a user's ChatGPT rate limits change.
- `account/sendAddCreditsNudgeEmail` - ask ChatGPT to email a workspace owner about depleted credits or a reached usage limit.
- `mcpServer/oauthLogin/completed` (notify) - emitted after a `mcpServer/oauth/login` flow finishes; payload includes `{ name, success, error? }`.
- `mcpServer/startupStatus/updated` (notify) - emitted when a configured MCP server's startup status changes for a loaded thread; payload includes `{ name, status, error }`.

### 1) Check auth state

Request:

```json
{ "method": "account/read", "id": 1, "params": { "refreshToken": false } }
```

Response examples:

```json
{ "id": 1, "result": { "account": null, "requiresOpenaiAuth": false } }
```

```json
{ "id": 1, "result": { "account": null, "requiresOpenaiAuth": true } }
```

```json
{
  "id": 1,
  "result": { "account": { "type": "apiKey" }, "requiresOpenaiAuth": true }
}
```

```json
{
  "id": 1,
  "result": {
    "account": {
      "type": "chatgpt",
      "email": "user@example.com",
      "planType": "pro"
    },
    "requiresOpenaiAuth": true
  }
}
```

Field notes:

- `refreshToken` (boolean): set `true` to force a token refresh in managed ChatGPT mode. In external token mode (`chatgptAuthTokens`), app-server ignores this flag.
- `requiresOpenaiAuth` reflects the active provider; when `false`, Codex can run without OpenAI credentials.

### 2) Log in with an API key

1. Send:

   ```json
   {
     "method": "account/login/start",
     "id": 2,
     "params": { "type": "apiKey", "apiKey": "sk-..." }
   }
   ```

2. Expect:

   ```json
   { "id": 2, "result": { "type": "apiKey" } }
   ```

3. Notifications:

   ```json
   {
     "method": "account/login/completed",
     "params": { "loginId": null, "success": true, "error": null }
   }
   ```

   ```json
   {
     "method": "account/updated",
     "params": { "authMode": "apikey", "planType": null }
   }
   ```

### 3) Log in with ChatGPT (browser flow)

1. Start:

   ```json
   { "method": "account/login/start", "id": 3, "params": { "type": "chatgpt" } }
   ```

   ```json
   {
     "id": 3,
     "result": {
       "type": "chatgpt",
       "loginId": "<uuid>",
       "authUrl": "https://chatgpt.com/...&redirect_uri=http%3A%2F%2Flocalhost%3A<port>%2Fauth%2Fcallback"
     }
   }
   ```

2. Open `authUrl` in a browser; the app-server hosts the local callback.
3. Wait for notifications:

   ```json
   {
     "method": "account/login/completed",
     "params": { "loginId": "<uuid>", "success": true, "error": null }
   }
   ```

   ```json
   {
     "method": "account/updated",
     "params": { "authMode": "chatgpt", "planType": "plus" }
   }
   ```

### 3b) Log in with ChatGPT (device-code flow)

Use this flow when your client owns the sign-in ceremony or when a browser callback is brittle.

1. Start:

   ```json
   {
     "method": "account/login/start",
     "id": 4,
     "params": { "type": "chatgptDeviceCode" }
   }
   ```

   ```json
   {
     "id": 4,
     "result": {
       "type": "chatgptDeviceCode",
       "loginId": "<uuid>",
       "verificationUrl": "https://auth.openai.com/codex/device",
       "userCode": "ABCD-1234"
     }
   }
   ```

2. Show `verificationUrl` and `userCode` to the user; the frontend owns the UX.
3. Wait for notifications:

   ```json
   {
     "method": "account/login/completed",
     "params": { "loginId": "<uuid>", "success": true, "error": null }
   }
   ```

   ```json
   {
     "method": "account/updated",
     "params": { "authMode": "chatgpt", "planType": "plus" }
   }
   ```

### 3c) Log in with externally managed ChatGPT tokens (`chatgptAuthTokens`)

Use this experimental mode only when a host application owns the user's ChatGPT auth lifecycle and supplies tokens directly. Clients must set `capabilities.experimentalApi = true` during `initialize` before using this login type.

1. Send:

   ```json
   {
     "method": "account/login/start",
     "id": 7,
     "params": {
       "type": "chatgptAuthTokens",
       "accessToken": "<jwt>",
       "chatgptAccountId": "org-123",
       "chatgptPlanType": "business"
     }
   }
   ```

2. Expect:

   ```json
   { "id": 7, "result": { "type": "chatgptAuthTokens" } }
   ```

3. Notifications:

   ```json
   {
     "method": "account/login/completed",
     "params": { "loginId": null, "success": true, "error": null }
   }
   ```

   ```json
   {
     "method": "account/updated",
     "params": { "authMode": "chatgptAuthTokens", "planType": "business" }
   }
   ```

When the server receives a `401 Unauthorized`, it may request refreshed tokens from the host app:

```json
{
  "method": "account/chatgptAuthTokens/refresh",
  "id": 8,
  "params": { "reason": "unauthorized", "previousAccountId": "org-123" }
}
{ "id": 8, "result": { "accessToken": "<jwt>", "chatgptAccountId": "org-123", "chatgptPlanType": "business" } }
```

The server retries the original request after a successful refresh response. Requests time out after about 10 seconds.

### 4) Cancel a ChatGPT login

```json
{ "method": "account/login/cancel", "id": 4, "params": { "loginId": "<uuid>" } }
{ "method": "account/login/completed", "params": { "loginId": "<uuid>", "success": false, "error": "..." } }
```

### 5) Logout

```json
{ "method": "account/logout", "id": 5 }
{ "id": 5, "result": {} }
{ "method": "account/updated", "params": { "authMode": null, "planType": null } }
```

### 6) Rate limits (ChatGPT)

```json
{ "method": "account/rateLimits/read", "id": 6 }
{ "id": 6, "result": {
  "rateLimits": {
    "limitId": "codex",
    "limitName": null,
    "primary": { "usedPercent": 25, "windowDurationMins": 15, "resetsAt": 1730947200 },
    "secondary": null,
    "rateLimitReachedType": null
  },
  "rateLimitsByLimitId": {
    "codex": {
      "limitId": "codex",
      "limitName": null,
      "primary": { "usedPercent": 25, "windowDurationMins": 15, "resetsAt": 1730947200 },
      "secondary": null,
      "rateLimitReachedType": null
    },
    "codex_other": {
      "limitId": "codex_other",
      "limitName": "codex_other",
      "primary": { "usedPercent": 42, "windowDurationMins": 60, "resetsAt": 1730950800 },
      "secondary": null,
      "rateLimitReachedType": null
    }
  }
} }
{ "method": "account/rateLimits/updated", "params": {
  "rateLimits": {
    "limitId": "codex",
    "primary": { "usedPercent": 31, "windowDurationMins": 15, "resetsAt": 1730948100 }
  }
} }
```

Field notes:

- `rateLimits` is the backward-compatible single-bucket view.
- `rateLimitsByLimitId` (when present) is the multi-bucket view keyed by metered `limit_id` (for example `codex`).
- `limitId` is the metered bucket identifier.
- `limitName` is an optional user-facing label for the bucket.
- `usedPercent` is current usage within the quota window.
- `windowDurationMins` is the quota window length.
- `resetsAt` is a Unix timestamp (seconds) for the next reset.
- `planType` is included when the server returns the ChatGPT plan associated with a bucket.
- `credits` is included when the server returns remaining workspace credit details.
- `rateLimitReachedType` identifies the server-classified limit state when one has been reached.

### 7) Notify a workspace owner about a limit

Use `account/sendAddCreditsNudgeEmail` to ask ChatGPT to email a workspace owner when credits are depleted or a usage limit has been reached.

```json
{ "method": "account/sendAddCreditsNudgeEmail", "id": 7, "params": { "creditType": "credits" } }
{ "id": 7, "result": { "status": "sent" } }
```

Use `creditType: "credits"` when workspace credits are depleted, or `creditType: "usage_limit"` when the workspace usage limit has been reached. If the owner was already notified recently, the response status is `cooldown_active`.


---



# Use Codex with the Agents SDK

# Running Codex as an MCP server

You can run Codex as an MCP server and connect it from other MCP clients (for example, an agent built with the [OpenAI Agents SDK MCP integration](https://developers.openai.com/api/docs/guides/agents/integrations-observability#mcp)).

To start Codex as an MCP server, you can use the following command:

```bash
codex mcp-server
```

You can launch a Codex MCP server with the [Model Context Protocol Inspector](https://modelcontextprotocol.io/legacy/tools/inspector):

```bash
npx @modelcontextprotocol/inspector codex mcp-server
```

Send a `tools/list` request to see two tools:

**`codex`**: Run a Codex session. Accepts configuration parameters that match the Codex `Config` struct. The `codex` tool takes these properties:

| Property                | Type      | Description                                                                                                |
| ----------------------- | --------- | ---------------------------------------------------------------------------------------------------------- |
| **`prompt`** (required) | `string`  | The initial user prompt to start the Codex conversation.                                                   |
| `approval-policy`       | `string`  | Approval policy for shell commands generated by the model: `untrusted`, `on-request`, and `never`.         |
| `base-instructions`     | `string`  | The set of instructions to use instead of the default ones.                                                |
| `config`                | `object`  | Individual configuration settings that override what's in `$CODEX_HOME/config.toml`.                       |
| `cwd`                   | `string`  | Working directory for the session. If relative, resolved against the server process's current directory.   |
| `include-plan-tool`     | `boolean` | Whether to include the plan tool in the conversation.                                                      |
| `model`                 | `string`  | Optional override for the model name (for example, `o3`, `o4-mini`).                                       |
| `profile`               | `string`  | Configuration profile name; Codex loads `$CODEX_HOME/profile-name.config.toml` to specify default options. |
| `sandbox`               | `string`  | Sandbox mode: `read-only`, `workspace-write`, or `danger-full-access`.                                     |

**`codex-reply`**: Continue a Codex session by providing the thread ID and prompt. The `codex-reply` tool takes these properties:

| Property                      | Type   | Description                                               |
| ----------------------------- | ------ | --------------------------------------------------------- |
| **`prompt`** (required)       | string | The next user prompt to continue the Codex conversation.  |
| **`threadId`** (required)     | string | The ID of the thread to continue.                         |
| `conversationId` (deprecated) | string | Deprecated alias for `threadId` (kept for compatibility). |

Use the `threadId` from `structuredContent.threadId` in the `tools/call` response. Approval prompts (exec/patch) also include `threadId` in their `params` payload.

Example response payload:

```json
{
  "structuredContent": {
    "threadId": "019bbb20-bff6-7130-83aa-bf45ab33250e",
    "content": "`ls -lah` (or `ls -alh`) — long listing, includes dotfiles, human-readable sizes."
  },
  "content": [
    {
      "type": "text",
      "text": "`ls -lah` (or `ls -alh`) — long listing, includes dotfiles, human-readable sizes."
    }
  ]
}
```

Note modern MCP clients generally report only `"structuredContent"` as the result of a tool call, if present, though the Codex MCP server also returns `"content"` for the benefit of older MCP clients.

# Creating multi-agent workflows

Codex CLI can do far more than run ad-hoc tasks. By exposing the CLI as a [Model Context Protocol](https://modelcontextprotocol.io/) (MCP) server and orchestrating it with the OpenAI Agents SDK, you can create deterministic, reviewable workflows that scale from a single agent to a complete software delivery pipeline.

This guide walks through the same workflow showcased in the [OpenAI Cookbook](https://github.com/openai/openai-cookbook/blob/main/examples/codex/codex_mcp_agents_sdk/building_consistent_workflows_codex_cli_agents_sdk.ipynb). You will:

- launch Codex CLI as a long-running MCP server,
- build a focused single-agent workflow that produces a playable browser game, and
- orchestrate a multi-agent team with hand-offs, guardrails, and full traces you can review afterwards.

Before starting, make sure you have:

- [Codex CLI](https://developers.openai.com/codex/cli) installed locally so the `codex` command is available.
- Python 3.10+ with `pip`.
- Node.js 18+ if you want to run the MCP Inspector example above.
- An OpenAI API key stored locally. You can create or manage keys in the [OpenAI dashboard](https://platform.openai.com/account/api-keys).

Create a working directory for the guide and add your API key to a `.env` file:

```bash
mkdir codex-workflows
cd codex-workflows
printf "OPENAI_API_KEY=sk-..." > .env
```

## Install dependencies

The Agents SDK handles orchestration across Codex, hand-offs, and traces. Install the latest SDK packages:

```bash
python -m venv .venv
source .venv/bin/activate
pip install --upgrade openai openai-agents python-dotenv
```

Activating a virtual environment keeps the SDK dependencies isolated from the
  rest of your system.

## Initialize Codex CLI as an MCP server

Start by turning Codex CLI into an MCP server that the Agents SDK can call. The server exposes two tools (`codex()` to start a conversation and `codex-reply()` to continue one) and keeps Codex alive across multiple agent turns.

Create a file called `codex_mcp.py` and add the following:

```python
import asyncio

from agents import Agent, Runner
from agents.mcp import MCPServerStdio


async def main() -> None:
    async with MCPServerStdio(
        name="Codex CLI",
        params={
            "command": "codex",
            "args": ["mcp-server"],
        },
        client_session_timeout_seconds=360000,
    ) as codex_mcp_server:
        print("Codex MCP server started.")
        # More logic coming in the next sections.
        return


if __name__ == "__main__":
    asyncio.run(main())
```

Run the script once to verify that Codex launches successfully:

```bash
python codex_mcp.py
```

The script exits after printing `Codex MCP server started.`. In the next sections you will reuse the same MCP server inside richer workflows.

## Build a single-agent workflow

Let’s start with a scoped example that uses Codex MCP to ship a small browser game. The workflow relies on two agents:

1. **Game Designer**: writes a brief for the game.
2. **Game Developer**: implements the game by calling Codex MCP.

Update `codex_mcp.py` with the following code. It keeps the MCP server setup from above and adds both agents.

```python
import asyncio
import os

from dotenv import load_dotenv

from agents import Agent, Runner, set_default_openai_api
from agents.mcp import MCPServerStdio

load_dotenv(override=True)
set_default_openai_api(os.getenv("OPENAI_API_KEY"))


async def main() -> None:
    async with MCPServerStdio(
        name="Codex CLI",
        params={
            "command": "codex",
            "args": ["mcp-server"],
        },
        client_session_timeout_seconds=360000,
    ) as codex_mcp_server:
        developer_agent = Agent(
            name="Game Developer",
            instructions=(
                "You are an expert in building simple games using basic html + css + javascript with no dependencies. "
                "Save your work in a file called index.html in the current directory. "
                "Always call codex with \"approval-policy\": \"never\" and \"sandbox\": \"workspace-write\"."
            ),
            mcp_servers=[codex_mcp_server],
        )

        designer_agent = Agent(
            name="Game Designer",
            instructions=(
                "You are an indie game connoisseur. Come up with an idea for a single page html + css + javascript game that a developer could build in about 50 lines of code. "
                "Format your request as a 3 sentence design brief for a game developer and call the Game Developer coder with your idea."
            ),
            model="gpt-5",
            handoffs=[developer_agent],
        )

        await Runner.run(designer_agent, "Implement a fun new game!")


if __name__ == "__main__":
    asyncio.run(main())
```

Execute the script:

```bash
python codex_mcp.py
```

Codex will read the designer's brief, create an `index.html` file, and write the full game to disk. Open the generated file in a browser to play the result. Every run produces a different design with unique play-style twists and polish.

## Expand to a multi-agent workflow

Now turn the single-agent setup into an orchestrated, traceable workflow. The system adds:

- **Project Manager**: creates shared requirements, coordinates hand-offs, and enforces guardrails.
- **Designer**, **Frontend Developer**, **Server Developer**, and **Tester**: each with scoped instructions and output folders.

Create a new file called `multi_agent_workflow.py`:

```python
import asyncio
import os

from dotenv import load_dotenv

from agents import (
    Agent,
    ModelSettings,
    Runner,
    WebSearchTool,
    set_default_openai_api,
)
from agents.extensions.handoff_prompt import RECOMMENDED_PROMPT_PREFIX
from agents.mcp import MCPServerStdio
from openai.types.shared import Reasoning

load_dotenv(override=True)
set_default_openai_api(os.getenv("OPENAI_API_KEY"))


async def main() -> None:
    async with MCPServerStdio(
        name="Codex CLI",
        params={"command": "codex", "args": ["mcp"]},
        client_session_timeout_seconds=360000,
    ) as codex_mcp_server:
        designer_agent = Agent(
            name="Designer",
            instructions=(
                f"""{RECOMMENDED_PROMPT_PREFIX}"""
                "You are the Designer.\n"
                "Your only source of truth is AGENT_TASKS.md and REQUIREMENTS.md from the Project Manager.\n"
                "Do not assume anything that is not written there.\n\n"
                "You may use the internet for additional guidance or research."
                "Deliverables (write to /design):\n"
                "- design_spec.md – a single page describing the UI/UX layout, main screens, and key visual notes as requested in AGENT_TASKS.md.\n"
                "- wireframe.md – a simple text or ASCII wireframe if specified.\n\n"
                "Keep the output short and implementation-friendly.\n"
                "When complete, handoff to the Project Manager with transfer_to_project_manager."
                "When creating files, call Codex MCP with {\"approval-policy\":\"never\",\"sandbox\":\"workspace-write\"}."
            ),
            model="gpt-5",
            tools=[WebSearchTool()],
            mcp_servers=[codex_mcp_server],
        )

        frontend_developer_agent = Agent(
            name="Frontend Developer",
            instructions=(
                f"""{RECOMMENDED_PROMPT_PREFIX}"""
                "You are the Frontend Developer.\n"
                "Read AGENT_TASKS.md and design_spec.md. Implement exactly what is described there.\n\n"
                "Deliverables (write to /frontend):\n"
                "- index.html – main page structure\n"
                "- styles.css or inline styles if specified\n"
                "- main.js or game.js if specified\n\n"
                "Follow the Designer’s DOM structure and any integration points given by the Project Manager.\n"
                "Do not add features or branding beyond the provided documents.\n\n"
                "When complete, handoff to the Project Manager with transfer_to_project_manager_agent."
                "When creating files, call Codex MCP with {\"approval-policy\":\"never\",\"sandbox\":\"workspace-write\"}."
            ),
            model="gpt-5",
            mcp_servers=[codex_mcp_server],
        )

        backend_developer_agent = Agent(
            name="Backend Developer",
            instructions=(
                f"""{RECOMMENDED_PROMPT_PREFIX}"""
                "You are the Backend Developer.\n"
                "Read AGENT_TASKS.md and REQUIREMENTS.md. Implement the backend endpoints described there.\n\n"
                "Deliverables (write to /backend):\n"
                "- package.json – include a start script if requested\n"
                "- server.js – implement the API endpoints and logic exactly as specified\n\n"
                "Keep the code as simple and readable as possible. No external database.\n\n"
                "When complete, handoff to the Project Manager with transfer_to_project_manager_agent."
                "When creating files, call Codex MCP with {\"approval-policy\":\"never\",\"sandbox\":\"workspace-write\"}."
            ),
            model="gpt-5",
            mcp_servers=[codex_mcp_server],
        )

        tester_agent = Agent(
            name="Tester",
            instructions=(
                f"""{RECOMMENDED_PROMPT_PREFIX}"""
                "You are the Tester.\n"
                "Read AGENT_TASKS.md and TEST.md. Verify that the outputs of the other roles meet the acceptance criteria.\n\n"
                "Deliverables (write to /tests):\n"
                "- TEST_PLAN.md – bullet list of manual checks or automated steps as requested\n"
                "- test.sh or a simple automated script if specified\n\n"
                "Keep it minimal and easy to run.\n\n"
                "When complete, handoff to the Project Manager with transfer_to_project_manager."
                "When creating files, call Codex MCP with {\"approval-policy\":\"never\",\"sandbox\":\"workspace-write\"}."
            ),
            model="gpt-5",
            mcp_servers=[codex_mcp_server],
        )

        project_manager_agent = Agent(
            name="Project Manager",
            instructions=(
                f"""{RECOMMENDED_PROMPT_PREFIX}"""
                """
                You are the Project Manager.

                Objective:
                Convert the input task list into three project-root files the team will execute against.

                Deliverables (write in project root):
                - REQUIREMENTS.md: concise summary of product goals, target users, key features, and constraints.
                - TEST.md: tasks with [Owner] tags (Designer, Frontend, Backend, Tester) and clear acceptance criteria.
                - AGENT_TASKS.md: one section per role containing:
                  - Project name
                  - Required deliverables (exact file names and purpose)
                  - Key technical notes and constraints

                Process:
                - Resolve ambiguities with minimal, reasonable assumptions. Be specific so each role can act without guessing.
                - Create files using Codex MCP with {"approval-policy":"never","sandbox":"workspace-write"}.
                - Do not create folders. Only create REQUIREMENTS.md, TEST.md, AGENT_TASKS.md.

                Handoffs (gated by required files):
                1) After the three files above are created, hand off to the Designer with transfer_to_designer_agent and include REQUIREMENTS.md and AGENT_TASKS.md.
                2) Wait for the Designer to produce /design/design_spec.md. Verify that file exists before proceeding.
                3) When design_spec.md exists, hand off in parallel to both:
                   - Frontend Developer with transfer_to_frontend_developer_agent (provide design_spec.md, REQUIREMENTS.md, AGENT_TASKS.md).
                   - Backend Developer with transfer_to_backend_developer_agent (provide REQUIREMENTS.md, AGENT_TASKS.md).
                4) Wait for Frontend to produce /frontend/index.html and Backend to produce /backend/server.js. Verify both files exist.
                5) When both exist, hand off to the Tester with transfer_to_tester_agent and provide all prior artifacts and outputs.
                6) Do not advance to the next handoff until the required files for that step are present. If something is missing, request the owning agent to supply it and re-check.

                PM Responsibilities:
                - Coordinate all roles, track file completion, and enforce the above gating checks.
                - Do NOT respond with status updates. Just handoff to the next agent until the project is complete.
                """
            ),
            model="gpt-5",
            model_settings=ModelSettings(
                reasoning=Reasoning(effort="medium"),
            ),
            handoffs=[designer_agent, frontend_developer_agent, backend_developer_agent, tester_agent],
            mcp_servers=[codex_mcp_server],
        )

        designer_agent.handoffs = [project_manager_agent]
        frontend_developer_agent.handoffs = [project_manager_agent]
        backend_developer_agent.handoffs = [project_manager_agent]
        tester_agent.handoffs = [project_manager_agent]

        task_list = """
Goal: Build a tiny browser game to showcase a multi-agent workflow.

High-level requirements:
- Single-screen game called "Bug Busters".
- Player clicks a moving bug to earn points.
- Game ends after 20 seconds and shows final score.
- Optional: submit score to a simple backend and display a top-10 leaderboard.

Roles:
- Designer: create a one-page UI/UX spec and basic wireframe.
- Frontend Developer: implement the page and game logic.
- Backend Developer: implement a minimal API (GET /health, GET/POST /scores).
- Tester: write a quick test plan and a simple script to verify core routes.

Constraints:
- No external database—memory storage is fine.
- Keep everything readable for beginners; no frameworks required.
- All outputs should be small files saved in clearly named folders.
"""

        result = await Runner.run(project_manager_agent, task_list, max_turns=30)
        print(result.final_output)


if __name__ == "__main__":
    asyncio.run(main())
```

Run the script and watch the generated files:

```bash
python multi_agent_workflow.py
ls -R
```

The project manager agent writes `REQUIREMENTS.md`, `TEST.md`, and `AGENT_TASKS.md`, then coordinates hand-offs across the designer, frontend, server, and tester agents. Each agent writes scoped artifacts in its own folder before handing control back to the project manager.

## Trace the workflow

Codex automatically records traces that capture every prompt, tool call, and hand-off. After the multi-agent run completes, open the [Traces dashboard](https://platform.openai.com/trace) to inspect the execution timeline.

The high-level trace highlights how the project manager verifies hand-offs before moving forward. Click into individual steps to see prompts, Codex MCP calls, files written, and execution durations. These details make it straightforward to audit every hand-off and understand how the workflow evolved turn by turn.
These traces make it straightforward to debug workflow hiccups, audit agent behavior, and measure performance over time without requiring extra instrumentation.


---


# Codex GitHub Action

Use the Codex GitHub Action (`openai/codex-action@v1`) to run Codex in CI/CD jobs, apply patches, or post reviews from a GitHub Actions workflow.
The action installs the Codex CLI, starts the Responses API proxy when you provide an API key, and runs `codex exec` under the permissions you specify.

Reach for the action when you want to:

- Automate Codex feedback on pull requests or releases without managing the CLI yourself.
- Gate changes on Codex-driven quality checks as part of your CI pipeline.
- Run repeatable Codex tasks (code review, release prep, migrations) from a workflow file.

For a CI example, see [Non-interactive mode](https://developers.openai.com/codex/noninteractive) and explore the source in the [openai/codex-action repository](https://github.com/openai/codex-action).

## Prerequisites

- Store your OpenAI key as a GitHub secret (for example `OPENAI_API_KEY`) and reference it in the workflow.
- Run the job on a Linux or macOS runner. For Windows, set `safety-strategy: unsafe`.
- Check out your code before invoking the action so Codex can read the repository contents.
- Decide which prompts you want to run. You can provide inline text via `prompt` or point to a file committed in the repo with `prompt-file`.

## Example workflow

The sample workflow below reviews new pull requests, captures Codex's response, and posts it back on the PR.

```yaml
name: Codex pull request review
on:
  pull_request:
    types: [opened, synchronize, reopened]

jobs:
  codex:
    runs-on: ubuntu-latest
    permissions:
      contents: read
    outputs:
      final_message: ${{ steps.run_codex.outputs.final-message }}
    steps:
      - uses: actions/checkout@v5
        with:
          ref: refs/pull/${{ github.event.pull_request.number }}/merge
          persist-credentials: false

      - name: Pre-fetch base and head refs
        env:
          PR_BASE_REF: ${{ github.event.pull_request.base.ref }}
          PR_NUMBER: ${{ github.event.pull_request.number }}
        run: |
          git fetch --no-tags origin \
            "$PR_BASE_REF" \
            "+refs/pull/$PR_NUMBER/head"

      - name: Run Codex
        id: run_codex
        uses: openai/codex-action@v1
        with:
          openai-api-key: ${{ secrets.OPENAI_API_KEY }}
          prompt-file: .github/codex/prompts/review.md
          output-file: codex-output.md

  post_feedback:
    runs-on: ubuntu-latest
    needs: codex
    if: needs.codex.outputs.final_message != ''
    permissions:
      issues: write
      pull-requests: write
    steps:
      - name: Post Codex feedback
        uses: actions/github-script@v7
        with:
          github-token: ${{ github.token }}
          script: |
            await github.rest.issues.createComment({
              owner: context.repo.owner,
              repo: context.repo.repo,
              issue_number: context.payload.pull_request.number,
              body: process.env.CODEX_FINAL_MESSAGE,
            });
        env:
          CODEX_FINAL_MESSAGE: ${{ needs.codex.outputs.final_message }}
```

Replace `.github/codex/prompts/review.md` with your own prompt file or use the `prompt` input for inline text. The example also writes the final Codex message to `codex-output.md` for later inspection or artifact upload.

## Configure `codex exec`

Fine-tune how Codex runs by setting the action inputs that map to `codex exec` options:

- `prompt` or `prompt-file` (choose one): Inline instructions or a repository path to Markdown or text with your task. Consider storing prompts in `.github/codex/prompts/`.
- `codex-args`: Extra CLI flags. Provide a JSON array (for example `["--ephemeral"]`) or a shell string (`--profile ci`) to configure sessions, profiles, or MCP settings.
- `model` and `effort`: Pick the Codex agent configuration you want; leave empty for defaults.
- `sandbox`: Match the sandbox mode (`workspace-write`, `read-only`, `danger-full-access`) to the permissions Codex needs during the run.
- `output-file`: Save the final Codex message to disk so later steps can upload or diff it.
- `codex-version`: Pin a specific CLI release. Leave blank to use the latest published version.
- `codex-home`: Point to a shared Codex home directory if you want to reuse configuration files or MCP setups across steps.

## Manage privileges

Codex has broad access on GitHub-hosted runners unless you restrict it. Use these inputs to control exposure:

- `safety-strategy` (default `drop-sudo`) removes `sudo` before running Codex. This is irreversible for the job and protects secrets in memory. On Windows you must set `safety-strategy: unsafe`.
- `unprivileged-user` pairs `safety-strategy: unprivileged-user` with `codex-user` to run Codex as a specific account. Ensure the user can read and write the repository checkout (see the [`unprivileged-user` example](https://github.com/openai/codex-action/blob/main/examples/unprivileged-user.yml) for an ownership fix).
- `read-only` keeps Codex from changing files or using the network, but it still runs with elevated privileges. Don't rely on `read-only` alone to protect secrets.
- `sandbox` limits filesystem and network access within Codex itself. Choose the narrowest option that still lets the task complete.
- `allow-users` and `allow-bots` restrict who can trigger the workflow. By default only users with write access can run the action; list extra trusted accounts explicitly or leave the field empty for the default behavior.

## Capture outputs

The action emits the last Codex message through the `final-message` output. Map it to a job output (as shown above) or handle it directly in later steps. Combine `output-file` with the uploaded artifacts feature if you prefer to collect the full transcript from the runner. When you need structured data, pass `--output-schema` through `codex-args` to enforce a JSON shape.

## Security checklist

- Limit who can start the workflow. Prefer trusted events or explicit approvals instead of allowing everyone to run Codex against your repository.
- Sanitize prompt inputs from pull requests, commit messages, or issue bodies to avoid prompt injection. Review HTML comments or hidden text before feeding it to Codex.
- Protect your `OPENAI_API_KEY` by keeping `safety-strategy` on `drop-sudo` or moving Codex to an unprivileged user. Never leave the action in `unsafe` mode on multi-tenant runners.
- Run Codex as the last step in a job so later steps don't inherit any unexpected state changes.
- Rotate keys immediately if you suspect the proxy logs or action output exposed secret material.

## Troubleshooting

- **You set both prompt and prompt-file**: Remove the duplicate input so you provide exactly one source.
- **responses-api-proxy didn't write server info**: Confirm the API key is present and valid; the proxy starts only when you provide `openai-api-key`.
- **Expected `sudo` removal, but `sudo` succeeded**: Ensure no earlier step restored `sudo` and that the runner OS is Linux or macOS. Re-run with a fresh job.
- **Permission errors after `drop-sudo`**: Grant write access before the action runs (for example with `chmod -R g+rwX "$GITHUB_WORKSPACE"` or by using the unprivileged-user pattern).
- **Unauthorized trigger blocked**: Adjust `allow-users` or `allow-bots` inputs if you need to permit service accounts beyond the default write collaborators.



---


# Best practices

If you’re new to Codex or coding agents in general, this guide will help you get better results faster. It covers the core habits that make Codex more effective across the [CLI](https://developers.openai.com/codex/cli), [IDE extension](https://developers.openai.com/codex/ide), and the [Codex app](https://developers.openai.com/codex/app), from prompting and planning to validation, MCP, skills, and automations.

Codex works best when you treat it less like a one-off assistant and more like a teammate you configure and improve over time.

A useful way to think about this: start with the right task context, use `AGENTS.md` for durable guidance, configure Codex to match your workflow, connect external systems with MCP, turn repeated work into skills, and automate stable workflows.

## Strong first use: Context and prompts

Codex is already strong enough to be useful even when your prompt isn't perfect. You can often hand it a hard problem with minimal setup and still get a strong result. Clear [prompting](https://developers.openai.com/codex/prompting) isn't required to get value, but it does make results more reliable, especially in larger codebases or higher-stakes tasks.

If you work in a large or complex repository, the biggest unlock is giving Codex the right task context and a clear structure for what you want done.

A good default is to include four things in your prompt:

- **Goal:** What are you trying to change or build?
- **Context:** Which files, folders, docs, examples, or errors matter for this task? You can @ mention certain files as context.
- **Constraints:** What standards, architecture, safety requirements, or conventions should Codex follow?
- **Done when:** What should be true before the task is complete, such as tests passing, behavior changing, or a bug no longer reproducing?

This helps Codex stay scoped, make fewer assumptions, and produce work that's easier to review.

Choose a reasoning level based on how hard the task is and test what works best for your workflow. Different users and tasks work best with different settings.

- Low for faster, well-scoped tasks
- Medium or High for more complex changes or debugging
- Extra High for long, agentic, reasoning-heavy tasks

To provide context faster, try using speech dictation inside the Codex app to
  dictate what you want Codex to do rather than typing it.

## Plan first for difficult tasks

If the task is complex, ambiguous, or hard to describe well, ask Codex to plan before it starts coding.

A few approaches work well:

**Use Plan mode:** For most users, this is the easiest and most effective option. Plan mode lets Codex gather context, ask clarifying questions, and build a stronger plan before implementation. Toggle with `/plan` or <kbd>Shift</kbd>+<kbd>Tab</kbd>.

**Ask Codex to interview you:** If you have a rough idea of what you want but aren't sure how to describe it well, ask Codex to question you first. Tell it to challenge your assumptions and turn the fuzzy idea into something concrete before writing code.

**Use a PLANS.md template:** For more advanced workflows, you can configure Codex to follow a `PLANS.md` or execution-plan template for longer-running or multi-step work. For more detail, see the [execution plans guide](https://developers.openai.com/cookbook/articles/codex_exec_plans).

## Make guidance reusable with `AGENTS.md`

Once a prompting pattern works, the next step is to stop repeating it manually. That's where [AGENTS.md](https://developers.openai.com/codex/guides/agents-md) comes in.

Think of `AGENTS.md` as an open-format README for agents. It loads into context automatically and is the best place to encode how you and your team want Codex to work in a repository.

A good `AGENTS.md` covers:

- repo layout and important directories
- How to run the project
- Build, test, and lint commands
- Engineering conventions and PR expectations
- Constraints and do-not rules
- What done means and how to verify work

The `/init` slash command in the CLI is the quick-start command to scaffold a starter `AGENTS.md` in the current directory. It's a great starting point, but you should edit the result to match how your team actually builds, tests, reviews, and ships code.

You can create `AGENTS.md` files at different levels: a global `AGENTS.md` for personal defaults that sits in `~/.codex`, a repo-level file for shared standards, and more specific files in subdirectories for local rules. If there’s a more specific file closer to your current directory, that guidance wins.

Keep it practical. A short, accurate `AGENTS.md` is more useful than a long file full of vague rules. Start with the basics, then add new rules only after you notice repeated mistakes.

If `AGENTS.md` starts getting too large, keep the main file concise and reference task-specific markdown files for things like planning, code review, or architecture.

When Codex makes the same mistake twice, ask it for a retrospective and update
  `AGENTS.md`. Guidance stays practical and based on real friction.

## Configure Codex for consistency

Configuration is one of the main ways to make Codex behave more consistently across sessions and surfaces. For example, you can set defaults for model choice, reasoning effort, sandbox mode, approval policy, profiles, and MCP setup.

A good starting pattern is:

- Keep personal defaults in `~/.codex/config.toml` (Settings → Configuration → Open config.toml from the Codex app)
- Keep repo-specific behavior in `.codex/config.toml`
- Use command-line overrides only for one-off situations (if you use the CLI)

[`config.toml`](https://developers.openai.com/codex/config-basic) is where you define durable preferences such as MCP servers, multi-agent setup, and feature flags. Profile-specific overrides live in separate `$CODEX_HOME/profile-name.config.toml` files.

Codex ships with operating level sandboxing and has two key knobs that you can control. Approval mode determines when Codex asks for your permission to run a command and sandbox mode determines if Codex can read or write in the directory and what files the agent can access.

If you're new to coding agents, start with the default permissions. Keep approval and sandboxing tight by default, then loosen permissions only for trusted repos or specific workflows once the need is clear.

Note that the CLI, IDE, and Codex app all share the same configuration layers. Learn more on the [sample configuration](https://developers.openai.com/codex/config-sample) page.

Configure Codex for your real environment early. Many quality issues are
  really setup issues, like the wrong working directory, missing write access,
  wrong model defaults, or missing tools and connectors.

## Improve reliability with testing and review

Don't stop at asking Codex to make a change. Ask it to create tests when needed, run the relevant checks, confirm the result, and review the work before you accept it.

Codex can do this loop for you, but only if it knows what “good” looks like. That guidance can come from either the prompt or `AGENTS.md`.

That can include:

- Writing or updating tests for the change
- Running the right test suites
- Checking lint, formatting, or type checks
- Confirming the final behavior matches the request
- Reviewing the diff for bugs, regressions, or risky patterns

Toggle the diff panel in the Codex app to directly [review
  changes](https://developers.openai.com/codex/app/review) locally. Click on a specific row to provide
  feedback that gets fed as context to the next Codex turn.

A useful option here is the slash command `/review`, which gives you a few ways to review code:

- Review against a base branch for PR-style review
- Review uncommitted changes
- Review a commit
- Use custom review instructions

If you and your team have a `code_review.md` file and reference it from `AGENTS.md`, Codex can follow that guidance during review as well. This is a strong pattern for teams that want review behavior to stay consistent across repositories and contributors.

Codex shouldn't just generate code. With the right instructions, it can also help **test it, check it, and review it**.

If you use GitHub Cloud, you can set up Codex to run [code reviews for your PRs](https://developers.openai.com/codex/integrations/github). At OpenAI, Codex reviews 100% of PRs. You can enable automatic reviews or have Codex reactively review when you @Codex.

## Use MCPs for external context

Use MCPs when the context Codex needs lives outside the repo. It lets Codex connect to the tools and systems you already use, so you don't have to keep copying and pasting live information into prompts.

[Model Context Protocol](https://developers.openai.com/codex/mcp), or MCP, is an open standard for connecting Codex to external tools and systems.

Use MCP when:

- The needed context lives outside the repo
- The data changes frequently
- You want Codex to use a tool rather than rely on pasted instructions
- You need a repeatable integration across users or projects

Codex supports both STDIO and Streamable HTTP servers with OAuth.

In the Codex App, head to Settings → MCP servers to see custom and recommended servers. Often, Codex can help you install the needed servers. All you need to do is ask. You can also use the `codex mcp add` command in the CLI to add your custom servers with a name, URL, and other details.

Add tools only when they unlock a real workflow. Do not start by wiring in
  every tool you use. Start with one or two tools that clearly remove a manual
  loop you already do often, then expand from there.

## Turn repeatable work into skills

Once a workflow becomes repeatable, stop relying on long prompts or repeated back-and-forth. Use a [Skill](https://developers.openai.com/codex/skills) to package the instructions in a SKILL.md file, context, and supporting logic Codex should apply consistently. Skills work across the CLI, IDE extension, and Codex app.

Keep each skill scoped to one job. Start with 2 to 3 concrete use cases, define clear inputs and outputs, and write the description so it says what the skill does and when to use it. Include the kinds of trigger phrases a user would actually say.

Don't try to cover every edge case up front. Start with one representative task, get it working well, then turn that workflow into a skill and improve from there. Include scripts or extra assets only when they improve reliability.

A good rule of thumb: if you keep reusing the same prompt or correcting the same workflow, it should probably become a skill.

Skills are especially useful for recurring jobs like:

- Log triage
- Release note drafting
- PR review against a checklist
- Migration planning
- Telemetry or incident summaries
- Standard debugging flows

The `$skill-creator` skill is the best place to start to scaffold the first version of a skill. Keep the first version local while you iterate. When it's ready to share broadly, package it as a [plugin](https://developers.openai.com/codex/plugins/build). One of the most important parts of a skill is the description. It should say what the skill does and when to use it.

Personal skills are stored in `$HOME/.agents/skills`, and shared team skills
  can be checked into `.agents/skills` inside a repository. This is especially
  helpful for onboarding new teammates.

## Use automations for repeated work

Once a workflow is stable, you can schedule Codex to run it in the background for you. In the Codex app, [automations](https://developers.openai.com/codex/app/automations) let you choose the project, prompt, cadence, and execution environment for a recurring task.

Once a task becomes repetitive for you, you can create an automation in the Automations tab on the Codex app. You can choose which project it runs in, the prompt it runs (you can invoke skills), and the cadence it will run. You can also choose whether the automation runs in a dedicated git worktree or in your local environment. Learn more about [git worktrees](https://developers.openai.com/codex/app/worktrees).

Good candidates include:

- Summarizing recent commits
- Scanning for likely bugs
- Drafting release notes
- Checking CI failures
- Producing standup summaries
- Running repeatable analysis workflows on a schedule

A useful rule is that skills define the method, automations define the schedule. If a workflow still needs a lot of steering, turn it into a skill first. Once it's predictable, automation becomes a force multiplier.

Use automations for reflection and maintenance, not just execution. Review
  recent sessions, summarize repeated friction, and improve prompts,
  instructions, or workflow setup over time.

## Organize long-running work with session controls

Codex sessions aren't just chat history. They're working threads that accumulate context, decisions, and actions over time, so managing them well has a big impact on quality.

The Codex app UI makes thread management easiest because you can pin threads and create worktrees. If you are using the CLI, these [slash commands](https://developers.openai.com/codex/cli/slash-commands) are especially useful:

- `/experimental` to toggle experimental features and add to your `config.toml`
- `/resume` to resume a saved conversation
- `/fork` to create a new thread while preserving the original transcript
- `/compact` when the thread is getting long and you want a summarized version of earlier context. Note that Codex does automatically compact conversations for you
- `/agent` when you are running parallel agents and want to switch between the active agent thread
- `/theme` to choose a syntax highlighting theme
- `/apps` to use ChatGPT apps directly in Codex
- `/status` to inspect the current session state

Keep one thread per coherent unit of work. If the work is still part of the same problem, staying in the same thread is often better because it preserves the reasoning trail. Fork only when the work truly branches.

Use Codex’s [subagent](https://developers.openai.com/codex/concepts/subagents) workflows to offload bounded
  work from the main thread. Keep the main agent focused on the core problem,
  and use subagents for tasks like exploration, tests, or triage.

## Common mistakes

A few common mistakes to avoid when first using Codex:

- Overloading the prompt with durable rules instead of moving them into `AGENTS.md` or a skill
- Not letting the agent see its work by not giving details on how to best run build and test commands
- Skipping planning on multi-step and complex tasks
- Giving Codex full permission to your computer before you understand the workflow
- Running live threads on the same files without using git worktrees
- Turning a recurring task into an automation before it's reliable manually
- Treating Codex like something you have to watch step by step instead of using it in parallel with your own work
- Using one thread per project instead of one thread per task. This leads to bloated context and worse results over time



---






---






---