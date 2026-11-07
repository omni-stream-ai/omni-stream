# Documentation

[Project overview](../README.md) · [中文文档](zh/README.md) · [Contribution rules](../AGENTS.md)

OmniStream uses documentation-driven development. The detailed module specifications are currently maintained in Chinese under [`docs/zh`](zh/index.md).

## Conventions

- Confirmed behavior and constraints guide implementation. Descriptions of the current implementation record its capabilities and limits; proposals are not implementation requirements.
- Explicitly requested specification changes apply within the user's authorized scope and must be reflected in the documentation. Other changes follow the project contribution rules and any applicable module rules.
- Clarify unexplained conflicts between documents rather than choosing by location or age. Resolve code/documentation discrepancies according to the contribution rules.
- Organize documents for practical maintenance. Share common rules through links and keep this index current when documents are split or moved.

## Guides and specifications

| Document | Scope |
| --- | --- |
| [Chinese overview](zh/README.md) | Features, quick start, builds, and deployment |
| [Native mobile](zh/mobile.md) | Android/iOS setup, Firebase client setup, remote Core, development, builds, signing, and validation limits |
| [Packaging and releases](zh/releases.md) | GitHub Actions, version rules, downloads, signing secrets, changelogs, and release retries |
| [Configuration](zh/configuration.md) | Model providers, CLI, desktop settings, network access, and site metadata |
| [Client access](zh/access.md) | Remote Core authorization, approval from existing clients, first-device bootstrap, and revocation |
| [Recurring delegations](zh/automations.md) | Scheduling, calendars, per-run confirmation, delivery authorization, and execution records |
| [Sessions](zh/sessions.md) | Session list ordering and pagination, Agent collaboration, message presentation, session settings, and user input |
| [Named Agents](zh/agents.md) | Roles, @mentions, memory, context isolation, plugin scope, and recurring tasks |
| [Built-in skills](zh/builtin-skills.md) | Assistant, recurring task, and project management rules |
| [GUI](zh/gui.md) | Shared presentation, system notifications and read receipts, empty states, failures, retries, and localization |
| [Remote push](zh/push.md) | Android FCM, browser Web Push, bundled Core credentials, automatic device registration, delivery, and failures |
| [Plugins](zh/plugins.md) | App connectors, Chrome MCP, Agent tools, credentials, Slack inbox, and write approvals |
| [Adapters and Core](zh/adapters.md) | Adapter contracts, protocol support, limitations, and frontend boundaries |
| [Testing](zh/testing.md) | Core, GUI, mock services, browser regression, and desktop validation limits |

Not every module has a specification yet. Add confirmed behavior when working on an undocumented feature; do not invent a complete specification from assumptions.
