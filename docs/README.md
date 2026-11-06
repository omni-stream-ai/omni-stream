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
| [Configuration](zh/configuration.md) | Model providers, CLI, desktop settings, network access, and site metadata |
| [Recurring delegations](zh/automations.md) | Scheduling, calendars, per-run confirmation, delivery authorization, and execution records |
| [Sessions](zh/sessions.md) | Agent collaboration, message presentation, session settings, and user input |
| [Named Agents](zh/agents.md) | Roles, @mentions, memory, context isolation, plugin scope, and recurring tasks |
| [Built-in skills](zh/builtin-skills.md) | Assistant, recurring task, and project management rules |
| [GUI](zh/gui.md) | Shared presentation, empty states, failures, retries, and localization |
| [Plugins](zh/plugins.md) | App connectors, Chrome MCP, Agent tools, credentials, Slack inbox, and write approvals |
| [Adapters and Core](zh/adapters.md) | Adapter contracts, protocol support, limitations, and frontend boundaries |
| [Testing](zh/testing.md) | Core, GUI, mock services, browser regression, and desktop validation limits |

Not every module has a specification yet. Add confirmed behavior when working on an undocumented feature; do not invent a complete specification from assumptions.
