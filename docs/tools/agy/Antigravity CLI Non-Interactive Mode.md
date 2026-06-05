# **Comprehensive Analysis of Google Antigravity CLI (agy): Non-Interactive Capabilities, Architectural Policies, and Integration Mechanics**

## **1\. Architectural Evolution and the Headless Execution Paradigm**

The enterprise software development landscape is undergoing a profound paradigm shift with the introduction of Google Antigravity 2.0, an agent-first development platform designed to orchestrate autonomous coding tasks. A critical component of this ecosystem is the new Antigravity CLI, executed via the agy binary, which serves as a lightweight, Go-based Terminal User Interface (TUI).1 This binary officially supersedes the legacy gemini-cli tool, which is scheduled for a complete consumer deprecation on June 18, 2026, forcing a massive architectural migration for integration pipelines and agentic orchestration frameworks.3  
While the Antigravity CLI is fundamentally optimized for keyboard-centric developers executing multi-step reasoning, multi-file editing, and tool calling in interactive terminal sessions, its role in programmatic and non-interactive environments is equally critical.1 Non-interactive execution—often referred to as "headless" or "batch" mode—is the backbone of Continuous Integration (CI) systems, automated code review bots, and sophisticated multi-agent wrappers that pipe repository events through the Large Language Model (LLM) harness without human intervention.4  
However, the migration to agy has surfaced complex discrepancies in execution policies, configuration hierarchies, and security sandboxing. Headless workflows that previously relied on robust read-only permission boundaries and predictable JSON over standard I/O (via the Agent Client Protocol) are now encountering opaque session management and unprompted tool approvals.4 This exhaustive report dissects the complete operational profile of the Antigravity CLI in non-interactive environments, detailing its command surface, configuration schema, extensibility policies, and error diagnostics to provide systems architects with a definitive integration blueprint.

## **2\. Command Surface and Operational Semantics in Non-Interactive Mode**

The operational mechanics of the agy binary diverge significantly depending on the presence of a human operator. In standard TUI operations, users govern the agent via a scrollable conversation pane and interactive slash commands (such as /resume, /config, and /permissions).5 Because these slash commands are inherently bound to the terminal UI layer, they are entirely inert and unavailable during programmatic, headless execution.4 Instead, non-interactive governance relies strictly on command-line flags, standard streams, and pre-existing file-based configurations.

### **2.1 Complete List of Commands and Flags for Non-Interactive Operations**

The non-interactive command surface is explicitly invoked to perform single-shot, deterministic evaluations. The agent ingests the prompt, executes necessary tools autonomously, and returns a finalized text block before terminating the process. The following table provides an exhaustive categorization of the arguments and flags that dictate headless execution parameters.

| Command / Flag | Alias | Description and Operational Impact |
| :---- | :---- | :---- |
| \--print | \-p | The primary trigger for non-interactive execution. It accepts a string argument representing the prompt. The agent suppresses the TUI, runs its reasoning and tool-calling loop silently, and dumps the final response to standard output (stdout) before exiting.7 |
| \--print-timeout | N/A | Establishes a strict maximum duration for the evaluation loop (e.g., \--print-timeout 90s). If tool execution or reasoning exceeds this temporal boundary, the process is forcefully terminated to prevent resource deadlocks in CI pipelines.4 |
| \--sandbox | N/A | Activates native OS-level containment mechanisms (nsjail on Linux, sandbox-exec on macOS, and AppContainer on Windows). This restricts the agent's shell execution to the active workspace, minimizing the risk of systemic compromise.4 |
| \--dangerously-skip-permissions | N/A | Bypasses all standard user confirmation prompts. In headless contexts, this automatically approves the agent's requests to read files, run terminal commands, and modify the repository, ensuring the process does not hang awaiting standard input.4 |
| \--conversation | N/A | Accepts a specific UUID to direct the agent to append the current prompt to an existing, historical conversation transcript, enabling stateful multi-turn interactions across independent process invocations.11 |
| \--continue | \-c | Instructs the binary to blindly resume the most recently accessed conversation globally across the host machine. This is designed for single-user resumption but is highly dangerous in concurrent automated environments due to race conditions.11 |
| \--record-responses | N/A | Accepts a file path as an argument. Instructs the CLI to record the raw response outputs and evaluation telemetry into the specified file, serving as an audit trail for non-interactive jobs.12 |
| \--prompt-interactive | \-i | While technically initiating an interactive session, this flag allows orchestrators to pass an initial prompt programmatically. The prompt is processed immediately upon initialization, unlike \-p which evaluates entirely headlessly.12 |

### **2.2 Security Discrepancies and Policy Vulnerabilities in Headless Execution**

The transition from gemini-cli to agy introduced a severe security regression concerning non-interactive tool permissions. Under the legacy system, architects utilized the \--approval-mode plan flag to enforce a rigorous read-only boundary during automated runs.4 This allowed the agent to verify code or read repository states using tools like grep or read\_file, while the CLI permissions layer strictly blocked side-effecting operations like write\_file or outbound network requests, regardless of the prompt's instructions.4  
Currently, agy version 1.0.0 completely lacks an equivalent read-only or plan-mode flag.4 The default policy for the non-interactive \-p mode assumes that, because there is no human present to manually approve or deny permission cards, all tool calls must be automatically approved.4 This design flaw creates a critical vulnerability for systems that process untrusted inputs, such as agent wrappers reviewing third-party pull requests. If an adversarial prompt injection is embedded in the untrusted code, instructing the model to execute a destructive shell command or overwrite critical system files, the non-interactive agy process will comply unconditionally.4  
Furthermore, attempts to mitigate this by combining the \--sandbox flag with \--dangerously-skip-permissions trigger a known architectural failure. While the sandbox is intended to act as an impermeable containment barrier, the \--dangerously-skip-permissions flag inadvertently auto-approves the agent's internal security prompt to bypass the sandbox (bypassSandbox: true).4 Consequently, an agent executing under these conditions can effortlessly break out of its designated workspace and write data to arbitrary paths on the host system.4 Until a strict \--no-write or \--read-only flag is integrated, enterprise workflows relying on agy \-p must enforce strict OS-level containerization, such as ephemeral Docker containers, to mitigate prompt injection risks.

## **3\. Session IDs: Policy, Persistence, and Orchestration Mechanics**

The ability to maintain contextual memory and stateful continuity across sequential, independent executions is foundational to complex agentic workflows. In the Antigravity architecture, this continuity is governed by Session IDs, which are internally generated as standard Universally Unique Identifiers (UUIDs). The policies surrounding how these identifiers are generated, stored, and exposed dictate the limits of programmatic orchestration.

### **3.1 The Policy of Opacity in Non-Interactive Output**

The fundamental policy governing Session IDs within agy's non-interactive \-p mode is strict opacity.11 When an orchestrating application fires a prompt via the command line, the runtime generates a new conversation thread and an associated UUID, processes the response, and outputs the final text. However, the resulting Session ID is intentionally suppressed; it is not surfaced in standard output (stdout), standard error (stderr), or any documented ephemeral payload file.11 The design philosophy behind this policy prioritizes clean, human-readable text output suitable for basic shell piping, explicitly scrubbing metadata from the output stream.11  
This policy creates profound orchestration deficits for third-party tools. If a parent application (such as a Discord bot, a custom IDE wrapper, or an autonomous worker framework like Crosstalk) spawns an agy \-p subprocess to initiate a task, it has no programmatic method to capture the conversation's UUID.11 Consequently, the orchestrator cannot use the \--conversation \<id\> flag for subsequent prompts, forcing every new interaction to begin with a completely blank context window (tabula rasa).11  
While developers can theoretically use the \-c or \--continue flag to resume the most recent global session, this is catastrophically unstable in concurrent environments. If a server attempts to process twenty pull requests in parallel using parallel subprocesses, utilizing \--continue will cause severe race conditions where different agents inject prompts into each other's historical transcripts, resulting in widespread hallucination and task failure.11

### **3.2 The Proposed Solution: Agent Client Protocol (ACP) Over Stdio**

To resolve the Session ID capture deficit and establish a stable programmatic interface, enterprise architects heavily rely on the Agent Client Protocol (ACP). The legacy gemini-cli supported an \--acp flag, which allowed the binary to daemonize and communicate via a JSON-RPC 2.0 layer over standard input and output streams.7  
Under the ACP architecture, a parent orchestrator sends an initialize request to the binary. The binary responds, and the orchestrator subsequently issues a session/new command. The CLI then generates the workspace and returns a structured JSON payload containing the explicit Session ID UUID.7 This allows the parent process to maintain a persistent dictionary of active sessions and route subsequent session/prompt events flawlessly.7 Furthermore, ACP supports session/update notifications, enabling the orchestrator to capture real-time streaming tokens (agent\_message\_chunk and agent\_thought\_chunk) rather than waiting for the entire non-interactive loop to resolve.7 As of the current release, agy lacks native \--acp support, forcing developers to rely on highly brittle workarounds, such as scraping the local SQLite or JSON history caches generated in the \~/.gemini/antigravity-cli/ directory to reverse-engineer session ownership.

### **3.3 Session Management within the Antigravity SDK**

In stark contrast to the CLI's opacity, the Antigravity Python SDK provides deterministic, programmatic control over Session IDs. Operating on the same underlying runtime harness, the SDK permits developers to explicitly capture and inject session state.13 When interacting with the SDK's Agent class, the runtime exposes the active context, allowing scripts to retrieve the ID and pass conversation\_id back into the LocalAgentConfig upon subsequent initializations.13 This divergence highlights that the limitation within agy is strictly an interface policy choice rather than a backend limitation.

## **4\. Repository Configuration: Hierarchies, Formats, and Locality**

Antigravity 2.0 eschews monolithic configurations in favor of a sophisticated, hierarchical settings architecture. This structure is designed to provide granular control over security boundaries, ensuring that sensitive enterprise repositories operate under strict containment, while personal projects enjoy elevated autonomy.14 Configurations are structurally split between host-wide global preferences and tightly isolated project-level (repository) parameters.

### **4.1 Global Configuration Formats and Locations**

Global settings dictate the default behavior for all agent invocations occurring on the host machine, unless explicitly overridden.

* **Format and Locality:** The primary global configuration is stored in a plain JSON format file located securely at \~/.gemini/antigravity-cli/settings.json.8 The file structure relies on simple key-value pairings and nested objects for advanced permission matrices.  
* **MCP Server Configurations:** Unlike legacy versions of the CLI that inlined external tool URLs directly into the main settings payload, the modern agy runtime mandates a distinct, isolated file for Model Context Protocol servers. This is structured as a JSON file and is globally located at \~/.gemini/antigravity/mcp\_config.json.15  
* **Global Sidecars and Hooks:** Background daemon processes known as sidecars are managed globally via definitions placed in \~/.gemini/config/sidecars/, where each sidecar subdirectory contains its own sidecar.json schema.17 Global event hooks are managed in \~/.gemini/config/hooks.json.18

### **4.2 Project-Level Configuration Formats and Locations**

To transition away from rigid workspace models, Antigravity introduces the concept of a "Project," which defines the operational boundaries, folder associations, and localized behaviors of an agent.19

* **Format and Locality:** Project-specific configurations are housed within a dedicated customization directory at the root of the active workspace or repository. Historically recognized as .gemini/, the modern runtime defaults to looking for the .agents/ directory.16  
* **Repository Injections:** If a project relies on specific shell execution limits or bespoke event triggers, a hooks.json file is placed directly inside .agents/hooks.json.18 Any repository containing an .agents/ folder will force the agy binary to evaluate these local settings, automatically overriding conflicting directives established in the global settings.json.8 This ensures that when a repository is cloned by a new developer, the agent's behavioral guardrails—such as mandatory testing linters or strict file access denials—are immediately enforced.19

## **5\. Exhaustive Variable Definitions within Configuration Files**

The Antigravity settings architecture utilizes an extensive taxonomy of variables to govern system behaviors. Understanding the exact variable names, their accepted primitive types, and their operational implications is critical for configuring headless orchestration. The following table provides an exhaustive map of configuration parameters supported by the Antigravity JSON schemas.

| Variable / JSON Key | Associated File / Scope | Type | Accepted Values and Operational Mechanics |
| :---- | :---- | :---- | :---- |
| enableTerminalSandbox | settings.json (Global) | Boolean | Accepts true or false. Defaults to false. When activated, this variable triggers the native execution containment barriers (nsjail, AppContainer), isolating all local agent shell processes from the broader host filesystem and network.9 |
| permissions | settings.json (Global/Project) | Object | Defines fine-grained autonomy levels for specific toolsets. It maps a tool identifier (e.g., read\_file, write\_file, command) to an autonomy state: request-review (always halt and prompt the user), always-proceed (execute without intervention), or strict (complete denial).9 |
| Terminal Execution Policy | Project Config UI / JSON | String | Accepts Always proceed, Ask (interactive mode fallback), or Deny. Dictates whether the agent is permitted to interface with the system shell to execute standard binaries or scripts.14 |
| Outside of Folder File Access Policy | Project Config UI / JSON | String | Accepts Always Allow, Always Ask, or Always Deny. A crucial security variable that governs lateral filesystem movement. If set to deny, the agent cannot manipulate files residing structurally above or outside the defined .agents/ project boundary.14 |
| AI Credit Overages | settings.json (Global) | Boolean | Accepts true or false. Determines whether the backend billing logic should automatically transition to consuming pre-purchased AI credits at consumption pricing once the baseline monthly subscription token quota is exhausted.23 |
| command | mcp\_config.json (Local MCP) | String | A discrete path to an executable binary (e.g., path/to/executable, python3, or /bin/bash) required to launch a local Model Context Protocol server. Mutually exclusive with serverUrl.15 |
| args | mcp\_config.json (Local MCP) | Array | A JSON array of string arguments passed directly into the command execution process upon server initialization (e.g., \["--arg1", "value1"\]).15 |
| env | mcp\_config.json / sidecar.json | Object | A map of environment variables specifically injected into the MCP or sidecar subprocess runtime (e.g., {"API\_KEY": "your-api-key"}).15 |
| serverUrl | mcp\_config.json (Remote MCP) | String | Defines the HTTP/SSE connection endpoint for a remotely hosted MCP server. This key officially replaces the legacy url and httpUrl parameters used in gemini-cli.16 |
| enabled | hooks.json (Project/Global) | Boolean | Accepts true or false. Defaults to true. Provides a mechanism to temporarily silence a defined hook without physically deleting its structural definition from the JSON file.18 |
| PreToolUse | hooks.json (Project/Global) | Array | An array of handler objects that execute scripts synchronously immediately *before* a tool is executed by the agent. Commonly used to validate arguments or inject environmental dependencies.18 |
| PostToolUse | hooks.json (Project/Global) | Array | An array of handler objects executed immediately *after* a tool completes. Primarily utilized for logging, auditing, or output sanitization before the result is returned to the model's context window.18 |
| PreInvocation | hooks.json (Project/Global) | Array | Handlers executed immediately *before* the agy runtime initiates an API call to the Gemini LLM. Used to append dynamic system instructions or verify network health.18 |
| PostInvocation | hooks.json (Project/Global) | Array | Handlers executed immediately *after* a tool call finishes and yields data back to the event loop.18 |
| Stop | hooks.json (Project/Global) | Array | Handlers that fire when the overall agent execution loop terminates. Useful for cleanup operations or sending final webhooks to orchestration frameworks.18 |
| matcher | hooks.json (Project/Global) | String | A regular expression target string utilized exclusively within PreToolUse and PostToolUse objects. The runtime evaluates this regex against the active tool name (e.g., run\_command) to determine if the associated hook should fire.18 |
| builtin | sidecar.json (Global/Plugin) | String | Currently only supports the value schedule. Defines a native background execution pattern. Mutually exclusive with command.17 |
| restart\_policy | sidecar.json (Global/Plugin) | String | Accepts always, on-failure, or never. Defaults to always. Dictates the daemon recovery mechanisms managed by the Antigravity sidecar lifecycle engine if the background process crashes.17 |

Command-line overrides fundamentally alter these variables during execution. If an orchestrator fires agy \-p "prompt" \--sandbox, the runtime forces enableTerminalSandbox to true strictly for the lifecycle of that specific process memory block. The CLI visually indicates command-line overrides via its TUI status menus, though the persistent JSON file on the disk remains unmodified unless explicitly written to.8

## **6\. Extensibility Policies: Skills, MCPs, Agents, and Plugins**

Modern AI agents often suffer from "Context Saturation"—a severe degradation in reasoning latency and precision that occurs when massive token payloads containing every possible tool, codebase rule, and instruction are dumped blindly into the model's active memory.24 To combat this, Antigravity enforces an architectural policy of Progressive Disclosure across all of its extensibility mechanisms.24

### **6.1 Agent Skills**

Skills are modular, reusable packages of procedural knowledge written entirely in human-readable Markdown and YAML.21

* **Policy and Operation:** The Antigravity engine initially exposes only a highly compressed "menu" of metadata to the active LLM. If the semantic intent of the user's prompt (e.g., "Refactor the authentication middleware") matches the metadata of a specific skill, the agent dynamically fetches and loads the heavier instructional logic into its context window.24 This selective memory architecture ensures the model remains fast and highly focused.  
* **Configuration Settings:** Skills are housed either globally in \~/.gemini/antigravity/skills/\<skill-folder\>/ or at the project level in \<workspace-root\>/.agents/skills/\<skill-folder\>/.21 The structural requirement for a valid skill is the presence of a SKILL.md file within the folder. This file must begin with YAML frontmatter explicitly declaring a name: and description:, followed by standard markdown text containing the exact procedural logic, guidelines, and restrictions the agent must adhere to when the skill is active.21 While interactive modes convert these skills into executable slash commands, non-interactive mode relies exclusively on autonomous semantic triggering.16

### **6.2 Model Context Protocol (MCP) Servers**

The Model Context Protocol establishes a secure, standardized bridge between the isolated agent environment and external data planes, such as live databases, cloud logging platforms, and third-party SaaS APIs (e.g., GitHub, Notion, Linear).15

* **Policy and Operation:** MCP integration provides two critical capabilities: Context Resources (allowing the agent to passively fetch live data, such as reading an updated Supabase schema) and Custom Tools (allowing the agent to actively execute specific, safe operations, such as generating a Jira ticket).15 The platform policy dictates that all MCP connections must be authenticated through strict D-Bus keyring protocols, with OAuth tokens managed automatically via \~/.gemini/antigravity/mcp\_oauth\_tokens.json.15  
* **Configuration Settings:** As established, MCP definitions are strictly localized to the mcp\_config.json file. Administrators define the connection mechanism within the JSON hierarchy—either via direct binary execution (command, args, env for stdio transport) or via HTTP endpoints using the serverUrl parameter.15

### **6.3 Asynchronous Subagents**

The migration to the Go-based agy binary unlocked the capability for concurrent delegation via asynchronous subagents.1

* **Policy and Operation:** When a primary agent is faced with a massive, parallelizable task (such as comprehensive test suite generation or fleet-wide codebase refactoring), it utilizes the invoke\_subagent tool. The runtime spawns a new concurrent session.26 To prevent context pollution, the subagent initiates with a "clean slate" memory transcript, inheriting zero conversation history from the parent.26 However, it strictly inherits the parent's environmental configuration, tool access, and permission guardrails.27 During execution, subagents operate asynchronously in the background. The parent agent yields control, allowing the user (or orchestrator) to continue interacting while background polling mechanisms capture the subagent's eventual payload.20  
* **Configuration Settings:** Subagent orchestration is highly configurable. The parent can direct the subagent to operate directly within the existing workspace ("Local Mode"), or, to prevent destructive conflicts during massive refactors, the agent can be configured to execute in "New Worktree Mode." This mechanism clones the Git repository into an isolated temporary worktree, allowing the subagent to modify files freely before ultimately generating a secure diff payload for the parent.19

### **6.4 Plugin Architecture**

Plugins operate as comprehensive namespaces that bundle multiple extensions into a single deployable artifact, replacing the older concept of Gemini CLI Extensions.3

* **Policy and Configuration:** When a plugin is installed, the binary stages the files inside \~/.gemini/antigravity-cli/plugins/\<plugin\_name\>/.9 The absolute minimum requirement for a plugin to be parsed by the runtime is the presence of a plugin.json marker file at the root of the plugin directory.9 Once recognized, the plugin acts as a container. Within this directory structure, developers can optionally include an mcp\_config.json file to register external tools, a hooks.json file to manipulate the runtime lifecycle, a skills/ subdirectory to inject prompt-based instructions, an agents/ folder for pre-configured subagent roles, and a rules/ directory containing markdown files that impose strict systemic constraints on the agent's behavior.9

## **7\. Model Availability and Headless Assignment Dynamics**

The analytical intelligence governing the agy runtime is dynamically interchangeable, drawing on Google's proprietary Tensor Processing Unit (TPU) fleets. Selecting the correct model is a balance between raw deductive reasoning capacity, latency, and token cost.

### **7.1 Complete List of Active Models**

The platform curates a tier-based matrix of Large Language Models specifically co-trained for agentic tool-calling and extensive context window manipulation:

* **Gemini 3.5 Flash:** Designated as the default and highly recommended intelligence layer for standard local agent operations.29 It delivers state-of-the-art speed and possesses an expansive context window capacity. It is the primary engine used when spawning asynchronous subagents, as its low latency prevents background task bottlenecks.30  
* **Gemini 3.1 Pro:** The heavy-duty architectural model. It handles highly complex, multi-step deductive reasoning tasks that require profound abstract logic.23 While significantly slower and more expensive computationally than the Flash variant, it is essential for dense refactoring directives.  
* **Gemini 3 Pro:** The foundational agent-optimized model prominently highlighted during the platform's initial public preview release.31  
* **Third-Party Models:** The architecture natively supports routing to non-Google third-party LLMs (e.g., Claude 3.5 Sonnet), though this access is strictly walled off and available exclusively to users subscribed to the elite Google AI Ultra tier.23

### **7.2 The Mechanics of Model Selection**

The mechanisms for configuring and assigning the active model highlight a glaring functionality gap between the visual TUI and non-interactive execution.

* **Configuration File and TUI Interaction:** In standard operations, a user enters the /model slash command to invoke a visual interface, selecting their preferred LLM from a dropdown list.9 The agy runtime immediately writes this selection to the \~/.gemini/antigravity-cli/settings.json file, ensuring the choice persists across all subsequent sessions.9  
* **Non-Interactive Assignment Constraints:** In non-interactive (-p) mode, orchestrators face a severe bottleneck: agy version 1.0.0 completely lacks a \--model command-line parameter (an issue highly documented and tracked as Feature Request \#35).4 Headless environments cannot override the model dynamically per-process. To switch from Gemini 3.5 Flash to Gemini 3.1 Pro for a specific CI job, the orchestration script must use external utilities (like jq or sed) to physically rewrite the JSON payload within \~/.gemini/antigravity-cli/settings.json mere milliseconds before spawning the agy subprocess.4 The runtime then evaluates the file state from the disk during initialization.

## **8\. Error Taxonomy and Diagnostics in Headless Contexts**

When operating interactively, agy gracefully handles connection failures and tool rejections by rendering formatted warning cards into the conversation pane, prompting the user for intervention or alternative instructions. In non-interactive \-p mode, fatal exceptions propagate immediately to standard output or standard error as raw text strings, structured JSON blocks, or Golang panic dumps. Parent orchestration frameworks must rely on complex regular expression parsing to determine the root cause of the crash.

### **8.1 Keyring Authentication and Subsystem Deadlocks**

The most pervasive error vector encountered during headless execution relates to authentication architecture. agy utilizes the go-keyring library, which depends fundamentally on native D-Bus and secret storage daemons, such as gnome-keyring on Linux and WSL2 environments, or the Keychain on macOS.32

* **Error Format and Sequence:** When invoked without a valid active session, the non-interactive process will dump a sequential string of degradation logs:  
  1. Print mode: not authenticated, trying silent auth  
  2. keyringAuth: timed out after 1s, skipping keyring auth  
  3. Print mode: silent auth failed, triggering OAuth  
  4. error getting token source: You are not logged into Antigravity. 10  
* **Operational Context:** Because the \-p flag intrinsically forbids opening an interactive web browser for OAuth challenges, the failure of the background silent fetch process triggers an immediate, unrecoverable exit.10 In headless WSL2 containers, architects must manually inject export $(dbus-launch) commands into their shell profiles to ensure the daemon is running, thereby preventing the persistent 1-second keyring timeout.33

### **8.2 Security Policy and Tool Execution Rejections**

When the agent attempts to execute a tool that explicitly violates the boundaries defined in settings.json permissions or project-level .agents/rules/ directives, the internal security engine intercepts the call.

* **Error Format:** The runtime injects a highly structured text string directly into the output stream indicating the block: Error executing tool \<tool\_name\>: Tool execution for "\<tool\_name\> (\<plugin\_name\> MCP Server)" denied by policy. 34  
* **Operational Context:** A known bug in the current release causes this error to trigger falsely. Even if an orchestrator explicitly passes the \--allowed-tools=\<tool\_name\> argument during a non-interactive run, the policy engine often ignores the override and yields the "denied by policy" exception, breaking automated toolchains.34

### **8.3 MCP Execution Isolation and $PATH Crashes**

The agy binary manages external tools by spawning background language\_server processes using sterile launch environments, heavily restricting system $PATH variables to base directories (e.g., /usr/bin:/bin under macOS launchd).35

* **Error Format:** If an MCP server relies on standard user-level runtimes installed in custom directories (such as Node's npx, bun, or Dart runtimes in \~/.nvm or \~/.pub-cache), the CLI will immediately crash, throwing a raw Golang execution error: executable file not found in $PATH.35  
* **Operational Context:** Because non-interactive daemons do not naturally inherit the rich, interactive shell profile (.bashrc or .zshrc), architects must explicitly hardcode absolute binary paths (e.g., /home/user/.nvm/versions/node/v20/bin/npx) inside the mcp\_config.json payload to prevent fatal startup crashes.35

### **8.4 Global API and Network Terminations**

Standard protocol errors resulting from malformed requests or backend connectivity issues are enveloped in a predictable JSON structure array, frequently prepended with a Unicode cross mark for terminal visibility.

* **Error Format:** A standard API rejection manifests identically to the following payload: ✕\].36

## **9\. Quota Metrology, Token Exhaustion, and High Traffic Denials**

To manage the immense computational overhead generated by autonomous agents performing multi-step reasoning loops and ingesting thousands of local repository files, Google enforces a rigid quota metrology tied directly to subscription tiers.

### **9.1 The Shared Pool Architecture and Rate Limits**

Under the legacy system, users maintained entirely distinct rate limits for Gemini Pro and Gemini Flash operations. Antigravity 2.0 abandons this model in favor of a single, unified token pool, dynamically drawn down based on standard API cost pricing ratios.37 If Gemini Flash costs 8x less than Gemini Pro, the engine mathematically normalizes the token burn rate, allowing users to consume any linear combination of models from their shared quota.37  
The replenishment mechanics vary drastically by tier. Non-paying users are constrained to a strict, highly limited weekly refresh cycle, replacing the generous 24-hour resets available in the older gemini-cli platform.38 Paid subscriptions (the $20/month AI Pro, $100/month AI Ultra, and $200/month AI Ultra plans) operate on aggressive 5-hour micro-refresh intervals, designed to smooth out datacenter loads globally.23

### **9.2 Exhaustion Signatures in Non-Interactive Mode**

When an agent executing under the headless \-p mode burns through its active token allocation, the failure is categorical. The pipeline immediately halts, providing no graceful fallback.

* **Standard Return Format:** The most frequently documented terminal string returned directly to stdout upon hitting capacity constraints is: "Our servers are experiencing high traffic." or the synonymous "Our servers are experiencing high demand.".36  
* **JSON API Formatting:** Depending on the exact nature of the failure (whether it is an aggregate infrastructure load failure or an explicit account boundary), the exhaustion may also manifest via the standard structured API Error array, surfacing either HTTP Code 429 (Resource Exhausted) or a derivative HTTP 403 (Permission Denied).36

To mitigate the catastrophic impact of a 5-hour micro-limit terminating an active CI/CD pipeline, enterprise users leverage the global "AI Credit Overages" variable within the settings.json file.23 When this boolean is set to true, the moment the baseline quota is exhausted, the backend billing logic intercepts the 429 error, automatically transitioning the session to consume pre-purchased AI credits at standard consumption pricing, ensuring the headless pipeline completes its task uninterrupted.23 Without this configuration, non-interactive wrappers are entirely reliant on basic string-matching heuristics (scanning for "high traffic") to trigger orchestration retry loops.

#### **Sources des citations**

1. Antigravity CLI Tutorial Series, consulté le mai 24, 2026, [https://medium.com/google-cloud/antigravity-cli-tutorial-series-12b46cfe3bf2](https://medium.com/google-cloud/antigravity-cli-tutorial-series-12b46cfe3bf2)  
2. Antigravity CLI Overview, consulté le mai 24, 2026, [https://antigravity.google/docs/cli-overview](https://antigravity.google/docs/cli-overview)  
3. An important update: Transitioning Gemini CLI to Antigravity CLI \- Google Developers Blog, consulté le mai 24, 2026, [https://developers.googleblog.com/an-important-update-transitioning-gemini-cli-to-antigravity-cli/](https://developers.googleblog.com/an-important-update-transitioning-gemini-cli-to-antigravity-cli/)  
4. Feature request: read-only / plan-mode equivalent for non ... \- GitHub, consulté le mai 24, 2026, [https://github.com/google-antigravity/antigravity-cli/issues/45](https://github.com/google-antigravity/antigravity-cli/issues/45)  
5. Antigravity CLI: A Hands-On Guide to Google's Terminal Coding Agent \- DEV Community, consulté le mai 24, 2026, [https://dev.to/arindam\_1729/antigravity-cli-a-hands-on-guide-to-googles-terminal-coding-agent-5bc7](https://dev.to/arindam_1729/antigravity-cli-a-hands-on-guide-to-googles-terminal-coding-agent-5bc7)  
6. Aider Deep Dive: The CLI Agentic Coding Tutorial 2026 \- Digital Applied, consulté le mai 24, 2026, [https://www.digitalapplied.com/blog/aider-deep-dive-cli-agentic-coding-tutorial-2026](https://www.digitalapplied.com/blog/aider-deep-dive-cli-agentic-coding-tutorial-2026)  
7. Feature request: add ACP (Agent Client Protocol) stdio JSON-RPC ..., consulté le mai 24, 2026, [https://github.com/google-antigravity/antigravity-cli/issues/31](https://github.com/google-antigravity/antigravity-cli/issues/31)  
8. Using AGY CLI \- Google Antigravity Documentation, consulté le mai 24, 2026, [https://antigravity.google/docs/cli-using](https://antigravity.google/docs/cli-using)  
9. Google Antigravity CLI features, consulté le mai 24, 2026, [https://antigravity.google/docs/cli-features](https://antigravity.google/docs/cli-features)  
10. macOS: agy repeatedly asks for OAuth because keyringAuth times out after 1s despite token existing in Keychain \#51 \- GitHub, consulté le mai 24, 2026, [https://github.com/google-antigravity/antigravity-cli/issues/51](https://github.com/google-antigravity/antigravity-cli/issues/51)  
11. feat(--print): emit per-conversation ID so headless callers can ..., consulté le mai 24, 2026, [https://github.com/google-antigravity/antigravity-cli/issues/7](https://github.com/google-antigravity/antigravity-cli/issues/7)  
12. Gemini CLI configuration, consulté le mai 24, 2026, [https://geminicli.com/docs/reference/configuration/](https://geminicli.com/docs/reference/configuration/)  
13. Google Antigravity SDK, consulté le mai 24, 2026, [https://antigravity.google/blog/introducing-google-antigravity-sdk](https://antigravity.google/blog/introducing-google-antigravity-sdk)  
14. Settings \- Google Antigravity Documentation, consulté le mai 24, 2026, [https://antigravity.google/docs/settings](https://antigravity.google/docs/settings)  
15. Antigravity Editor: MCP Integration, consulté le mai 24, 2026, [https://antigravity.google/docs/mcp](https://antigravity.google/docs/mcp)  
16. Migrating from Gemini CLI \- Google Antigravity, consulté le mai 24, 2026, [https://antigravity.google/docs/gcli-migration](https://antigravity.google/docs/gcli-migration)  
17. Sidecars \- Google Antigravity Documentation, consulté le mai 24, 2026, [https://antigravity.google/docs/sidecars](https://antigravity.google/docs/sidecars)  
18. Hooks \- Google Antigravity Documentation, consulté le mai 24, 2026, [https://antigravity.google/docs/hooks](https://antigravity.google/docs/hooks)  
19. Projects \- Google Antigravity Documentation, consulté le mai 24, 2026, [https://antigravity.google/docs/projects](https://antigravity.google/docs/projects)  
20. Subagents, Hooks, Scheduled Tasks, Agent Management, Voice, and Much More \- Google Antigravity, consulté le mai 24, 2026, [https://antigravity.google/blog/google-io-2026-feature-deep-dive?curius=5294](https://antigravity.google/blog/google-io-2026-feature-deep-dive?curius=5294)  
21. Agent Skills \- Google Antigravity Documentation, consulté le mai 24, 2026, [https://antigravity.google/docs/skills](https://antigravity.google/docs/skills)  
22. Getting Started with Google Antigravity, consulté le mai 24, 2026, [https://codelabs.developers.google.com/getting-started-google-antigravity](https://codelabs.developers.google.com/getting-started-google-antigravity)  
23. Plans \- Google Antigravity Documentation, consulté le mai 24, 2026, [https://antigravity.google/docs/plans](https://antigravity.google/docs/plans)  
24. Authoring Google Antigravity Skills, consulté le mai 24, 2026, [https://codelabs.developers.google.com/getting-started-with-antigravity-skills](https://codelabs.developers.google.com/getting-started-with-antigravity-skills)  
25. Plugins \- Google Antigravity Documentation, consulté le mai 24, 2026, [https://antigravity.google/docs/plugins](https://antigravity.google/docs/plugins)  
26. Asynchronous Subagents \- Google Antigravity Documentation, consulté le mai 24, 2026, [https://antigravity.google/docs/subagents](https://antigravity.google/docs/subagents)  
27. Subagents, Hooks, Scheduled Tasks, Agent Management, Voice, and Much More \- Google Antigravity, consulté le mai 24, 2026, [https://antigravity.google/blog/google-io-2026-feature-deep-dive](https://antigravity.google/blog/google-io-2026-feature-deep-dive)  
28. Google Antigravity Documentation, consulté le mai 24, 2026, [https://antigravity.google/docs](https://antigravity.google/docs)  
29. Google Antigravity Built an OS (and more), consulté le mai 24, 2026, [https://antigravity.google/blog/google-antigravity-built-an-os](https://antigravity.google/blog/google-antigravity-built-an-os)  
30. Google Antigravity Documentation, consulté le mai 24, 2026, [https://antigravity.google/docs/home](https://antigravity.google/docs/home)  
31. Introducing Google Antigravity, a New Era in AI-Assisted Software Development, consulté le mai 24, 2026, [https://antigravity.google/blog/introducing-google-antigravity](https://antigravity.google/blog/introducing-google-antigravity)  
32. antigravity cli doesn't remember auth : r/GeminiAI \- Reddit, consulté le mai 24, 2026, [https://www.reddit.com/r/GeminiAI/comments/1ti1xiq/antigravity\_cli\_doesnt\_remember\_auth/](https://www.reddit.com/r/GeminiAI/comments/1ti1xiq/antigravity_cli_doesnt_remember_auth/)  
33. \[Bug\] Antigravity CLI (agy) fails to persist authentication state in WSL 2 environment, consulté le mai 24, 2026, [https://discuss.ai.google.dev/t/bug-antigravity-cli-agy-fails-to-persist-authentication-state-in-wsl-2-environment/146059](https://discuss.ai.google.dev/t/bug-antigravity-cli-agy-fails-to-persist-authentication-state-in-wsl-2-environment/146059)  
34. Regression: \--allowed-tools fails in non-interactive mode (-p) with "denied by policy" · Issue \#16012 · google-gemini/gemini-cli \- GitHub, consulté le mai 24, 2026, [https://github.com/google-gemini/gemini-cli/issues/16012](https://github.com/google-gemini/gemini-cli/issues/16012)  
35. \[Bug\] MCP Servers crash with "executable file not found in $PATH" when Antigravity is launched via macOS GUI \- Google AI Developers Forum, consulté le mai 24, 2026, [https://discuss.ai.google.dev/t/bug-mcp-servers-crash-with-executable-file-not-found-in-path-when-antigravity-is-launched-via-macos-gui/138495](https://discuss.ai.google.dev/t/bug-mcp-servers-crash-with-executable-file-not-found-in-path-when-antigravity-is-launched-via-macos-gui/138495)  
36. Substitute to Antigravity ("Our servers are experiencing high demand"), consulté le mai 24, 2026, [https://discuss.ai.google.dev/t/substitute-to-antigravity-our-servers-are-experiencing-high-demand/140535](https://discuss.ai.google.dev/t/substitute-to-antigravity-our-servers-are-experiencing-high-demand/140535)  
37. Changes to Antigravity Plans, consulté le mai 24, 2026, [https://antigravity.google/blog/changes-to-antigravity-plans](https://antigravity.google/blog/changes-to-antigravity-plans)  
38. Please make the quota for free users at least like GEMINI CLI and resets every 24 hours, not 7 days. · Issue \#79 \- GitHub, consulté le mai 24, 2026, [https://github.com/google-antigravity/antigravity-cli/issues/79](https://github.com/google-antigravity/antigravity-cli/issues/79)  
39. Allow more flexible use of monthly quota (opt-out of rigid 5-Hour limits) \#93 \- GitHub, consulté le mai 24, 2026, [https://github.com/google-antigravity/antigravity-cli/issues/93](https://github.com/google-antigravity/antigravity-cli/issues/93)