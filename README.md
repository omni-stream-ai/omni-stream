<div align="center">
  <img src="apps/gui/public/omnistream-mark.svg" alt="OmniStream" width="100" />
  <h1>OmniStream</h1>
  <p>A workspace for everyday AI assistance and recurring delegations.</p>
  <p>
    <img src="https://img.shields.io/badge/core-Rust-orange?logo=rust" alt="Rust Core" />
    <img src="https://img.shields.io/badge/UI-React%20%2B%20TypeScript-149eca?logo=react" alt="React and TypeScript" />
    <img src="https://img.shields.io/badge/desktop-Tauri%202-24c8d8?logo=tauri" alt="Tauri 2" />
    <img src="https://img.shields.io/badge/platforms-Web%20%7C%20Desktop-347d39" alt="Web and desktop" />
  </p>
  <p><a href="docs/zh/README.md">中文文档</a> · <a href="docs/README.md">Documentation</a> · <a href="https://omnistreamai.com/">Website</a></p>
</div>

---

OmniStream brings chat, specialized Agents, connected apps, and scheduled work into one workspace. Ask for research, analysis, or coding help in a conversation, or delegate recurring work and receive each result in an inbox conversation.

## Features

- **Chat and Agent collaboration** — persistent conversations, streaming responses, execution progress, approvals, and cancellation.
- **Specialized assistants** — create named Agents with role instructions, call them with @mentions, and assign them to recurring tasks.
- **Recurring delegations** — daily or interval schedules, calendar rules, execution history, optional approval before each run, and authorized plugin deliveries.
- **Connected apps** — Slack, GitHub, Notion, Figma, and Chrome DevTools MCP, with explicit account setup and write permissions.
- **Web and desktop** — a React web app with PWA support and a Tauri desktop shell that starts a local Rust Core.

Web builds and desktop packaging are configured. Android and iOS have an entry point for remote Core access, but native projects and a complete packaging workflow are not yet provided.

## Quick start

Prerequisites: Rust, Node.js **20.19+ or 22.12+**, pnpm, and `make` for the shortcut below.

```bash
git clone https://github.com/omni-stream-ai/omni-stream.git
cd omni-stream
pnpm install
make run
```

In a second terminal:

```bash
pnpm dev
```

Open **http://localhost:1420**. Core listens on **http://127.0.0.1:4317** by default. Change the Core address in the connection settings when using another host or port.

Without `make`, run `cargo run -p omnistream -- serve 127.0.0.1:4317`. For the interactive CLI, run `cargo run -p omnistream`.

## Configuration

Model chat and external Agent execution require configuration. Core reads `~/.omnistream/config.toml` by default; `OMNISTREAM_HOME` changes the global directory, and `.omnistream/config.toml` in a project overrides global settings.

Configure your management model provider separately from external Agent credentials. For Codex collaboration with conversation recovery and interactive approvals:

```toml
[agents.codex]
command = "codex"
protocol = "app-server"
args = ["app-server"]
```

Codex uses its own model and login configuration. See the [configuration guide](docs/zh/configuration.md) and [adapter reference](docs/zh/adapters.md) (Chinese) for provider examples and protocol limits.

Recurring work created in chat starts as a draft: activate it in the recurring tasks panel. Core must remain running for scheduled work to execute.

## Build and deploy

### Web / PWA

```bash
pnpm gui:build
```

Deploy `apps/gui/dist` to a static host. For **Cloudflare Pages**, use the repository root, select **React (Vite)**, set the build command to `pnpm gui:build`, and the output directory to `apps/gui/dist`. The host must serve `index.html` for client-side routes such as `/chat/:conversationId`.

No application environment variables are required. Set the Core address in the UI, or optionally provide `VITE_OMNISTREAM_URL` at build time. The default `127.0.0.1` address refers to the visitor's computer. Cloudflare hosts the frontend; Rust Core runs separately. Use an HTTPS Core endpoint for remote connections from an HTTPS site.

### Desktop

Build `omnistream` for the target platform and put it in `apps/gui/src-tauri/binaries/` using Tauri's sidecar naming convention, `omnistream-<target-triple>` (with `.exe` on Windows). Install the platform's [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/), then run:

```bash
pnpm --filter @omnistream/gui tauri build
```

The desktop app starts its bundled Core. Connection settings offer opt-in LAN access and launch at login, both disabled by default. LAN changes take effect after restarting the app. Normal app exit stops the Core it started. See [desktop behavior and platform validation limits](docs/zh/configuration.md#桌面设置).

**Network access:** Core currently has no unified access authentication. Enable LAN access only on trusted networks and protect any remotely reachable Core endpoint; protecting the frontend alone does not protect Core.

## Development

```bash
cargo test --workspace
pnpm --filter @omnistream/gui typecheck
pnpm --filter @omnistream/gui test:unit
pnpm gui:build
```

See the [test guide](docs/zh/testing.md) for browser tests and validation boundaries. The project uses documentation-driven development; read the [documentation index](docs/README.md) and [contribution rules](AGENTS.md) before making changes.
