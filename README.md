<div align="center">
  <img src="apps/gui/public/omnistream-mark.svg" alt="OmniStream" width="100" />
  <h1>OmniStream</h1>
  <p>A workspace for everyday AI assistance and recurring delegations.</p>
  <p>
    <img src="https://img.shields.io/badge/core-Rust-orange?logo=rust" alt="Rust Core" />
    <img src="https://img.shields.io/badge/UI-React%20%2B%20TypeScript-149eca?logo=react" alt="React and TypeScript" />
    <img src="https://img.shields.io/badge/native-Tauri%202-24c8d8?logo=tauri" alt="Tauri 2" />
    <img src="https://img.shields.io/badge/platforms-Web%20%7C%20Desktop%20%7C%20Android%20%7C%20iOS-347d39" alt="Web, desktop, Android and iOS" />
    <a href="https://github.com/omni-stream-ai/omni-stream/actions/workflows/release.yml"><img src="https://github.com/omni-stream-ai/omni-stream/actions/workflows/release.yml/badge.svg" alt="Release builds" /></a>
    <a href="https://github.com/omni-stream-ai/omni-stream/releases"><img src="https://img.shields.io/github/v/release/omni-stream-ai/omni-stream?include_prereleases" alt="GitHub Release" /></a>
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
- **Web, desktop, and mobile** — a React web app with PWA support and Tauri native apps for desktop, Android, and iOS. Desktop starts a local Rust Core; mobile connects to a remote Core.
- **System notifications** — task results, pending responses, and Slack inbox alerts; Android and browsers automatically register with the connected Core for [remote push](docs/zh/push.md) while the app or page is closed.

Native mobile projects are generated locally by the platform scripts below, with development and package build commands. See the [mobile guide](docs/zh/mobile.md) (Chinese) for setup, signing, and validation limits.

## Downloads and releases

[GitHub Releases](https://github.com/omni-stream-ai/omni-stream/releases) is the download destination for Web archives, Windows/Linux/macOS installers, standalone Core binaries, and signed Android APK/AAB packages. iOS IPA packaging is optional and requires Apple signing; Ad Hoc installs are limited to registered devices. Each release includes categorized commit notes, `manifest.json`, and `SHA256SUMS`.

The [Release workflow](.github/workflows/release.yml) builds on version changes, publishes stable versions from the default branch and prereleases from other branches, and supports manual builds without publishing. Configure Android signing secrets before the first automatic release. See [packaging and releases](docs/zh/releases.md) (Chinese) for the artifact matrix, version files, signing setup, and retry rules. Windows packages are unsigned; macOS packages use ad-hoc signing without notarization.

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

Configure a management model provider and an execution engine. For Codex collaboration with conversation recovery and interactive approvals:

```toml
[agents.codex]
command = "codex"
protocol = "app-server"
args = ["app-server"]
```

Codex App Server conversation delegations inherit the request's selected model and authentication configuration. See [model inheritance](docs/zh/configuration.md#委派模型继承) and the [adapter reference](docs/zh/adapters.md) (Chinese) for supported providers and execution modes.

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

For development with frontend hot reload and automatic rebuilds of the Tauri Rust code, run `pnpm --filter @omnistream/gui tauri dev`. Core changes require rebuilding the sidecar. NixOS users should enter the [project development environment](docs/zh/README.md#桌面端) first.

### Native Android / iOS apps

Android requires Android Studio, JDK 17+, the Android SDK, and NDK. iOS requires macOS, Xcode, CocoaPods, and an Apple development team. Follow the [mobile prerequisites](docs/zh/mobile.md#构建环境), then run:

```bash
pnpm --filter @omnistream/gui android:init
pnpm --filter @omnistream/gui android:dev
pnpm --filter @omnistream/gui android:build --apk

# Run these on macOS
pnpm --filter @omnistream/gui ios:init
pnpm --filter @omnistream/gui ios:dev
pnpm --filter @omnistream/gui ios:build
```

On first launch, enter the Core address of a computer or server; `127.0.0.1` on a phone refers to the phone itself. Mobile apps do not bundle Core, a shell, or Codex. See the [mobile guide](docs/zh/mobile.md) for APK/AAB and iOS signing, simulator builds, and output locations. Compilation and device validation are tracked separately.

**Network access:** Non-loopback listeners require client authorization. Approve the first device from the server terminal; existing authorized clients can approve later requests in an automatic dialog and revoke access. Use HTTPS for public endpoints and set `[access] require_auth = true` when exposing a loopback Core through a reverse proxy. See [client access](docs/zh/access.md) for the trust boundary and setup.

## Development

```bash
cargo test --workspace
pnpm --filter @omnistream/gui typecheck
pnpm --filter @omnistream/gui test:unit
pnpm gui:build
```

See the [test guide](docs/zh/testing.md) for browser tests and validation boundaries. The project uses documentation-driven development; read the [documentation index](docs/README.md) and [contribution rules](AGENTS.md) before making changes.
