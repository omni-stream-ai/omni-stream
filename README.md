# OmniStream

OmniStream 是一个通用 AI 助手工作空间：在对话中处理查询、收集信息、分析和编程等即时请求，通过长期任务管理定时执行和周期检查的委托，并把每次结果交付到接收会话。

项目采用文档驱动开发，规范与模块说明见 [文档入口](docs/README.md)，协作流程见 [AGENTS.md](AGENTS.md)。

## 运行

在项目根目录启动后端 Core（需要 Rust 和 make）：

```bash
make run
```

直接执行 `make` 也会启动后端，默认监听 `127.0.0.1:4317`；可用 `make run ADDR=127.0.0.1:4320` 修改地址。该命令自动编译并执行 `cargo run -p omnistream -- serve <地址>`，保持前台运行，按 Ctrl+C 停止。

另开终端启动前端（需要 Node.js 和本机安装的 pnpm，项目不指定 pnpm 版本）：

```bash
pnpm install
pnpm dev
```

浏览器打开 `http://localhost:1420`，默认连接 `http://127.0.0.1:4317`。后端使用其他地址时，在界面连接设置中修改 Core 地址。完整模型对话和外部 Agent 执行需要下方的模型与 Agent 配置。

本项目也提供自然语言 CLI：

```bash
cargo run -p omnistream
```

`serve` 模式提供任务、Agent、事件和审批接口，供 GUI 或其他客户端使用。当前不维护独立 TUI，避免把终端渲染逻辑固化到 Core 协议中。

## 使用方式

默认在对话中提出请求，例如“查今天的 AI 新闻”“分析这份资料”或“修复这个 bug”。需要执行能力时调用配置的外部 Agent，结果返回当前会话，不创建用户任务。

需要对话结束后继续执行的事情才创建长期任务，例如“每天早上九点（UTC+8）收集 AI 新闻并给我日报”。对话生成的委托先保存为待确认草案，在侧栏“长期任务”面板确认启用后开始调度。也可以直接在面板填写内容、每日时间或固定间隔，并点击“创建并启用”。工作目录可选，不要求代码项目。

任务面板支持修改、暂停、恢复、取消、立即执行一次和查看执行记录。每次运行结束后，长期委托仍然保留；一次日报是一条执行记录，不是新任务。Core 需要持续运行才可按时执行。调度、重启恢复、时区和交付边界见[长期任务](docs/automations.md)。

结果保存在应用内接收会话，也可配置已授权插件动作自动投递。支持星期规则、带来源的日期增减、统计窗口和条件未满足时静默跳过；平台不硬编码日报、代码统计或国家假日日历。信息收集取决于 Agent 的数据源及联网能力；没有可用 Agent 或无法访问来源时明确记录失败。每日触发使用固定 UTC 偏移，条件监控通过周期检查实现。

## 当前状态

支持结构化模型对话、外部 Agent 协作、SQLite 会话存储、定时长期委托、独立执行历史、应用内结果交付和审批。外部 Agent 可通过统一插件网关使用已启用的 Slack、GitHub、Notion、Figma 和 Chrome 工具；接入方式与权限边界见[插件市场](docs/plugins.md)。会话规则见[会话](docs/sessions.md)，Agent 协议、恢复与配置边界见[适配器](docs/adapters.md)。

旧开发任务模块已移除，包含旧任务页面、`/tasks` 与 `/task-proposals` API、`task` CLI 命令、计划审批、服务控制和自动测试录屏。即时请求在会话中执行，定时委托使用长期任务。已有数据库记录保留但不再加载或执行旧开发任务，也不自动转换为长期任务；内部 Agent 执行记录继续持久化。验证方式见[测试说明](docs/testing.md)。

## 模型配置

支持 Codex 风格的 provider 配置，默认读取 `~/.omnistream/config.toml`；设置 `OMNISTREAM_HOME` 时从该目录读取全局配置。项目配置位于 `.omnistream/config.toml`，覆盖全局配置。

配置示例：

```toml
model_provider = "deepseek"
model = "deepseek-flash"
model_reasoning_effort = "low"
# 全局默认思考等级；provider 可用 reasoning_efforts 覆盖
model_reasoning_efforts = ["low", "medium", "high"]
max_concurrency = 4
auto_approve = ["read-only"]
# default | safe | all
approval_mode = "default"

[model_providers.deepseek]
name = "deepseek"
base_url = "https://api.deepseek.com"
wire_api = "responses"
models = [{ id = "deepseek-flash" }]
# 只有该 provider 有特殊能力时才覆盖全局默认值
# reasoning_efforts = ["low", "medium", "high", "xhigh"]

[model_providers.deepseek.auth]
command = "pass"
args = ["show", "codex/deepseek-api-key"]
timeout_ms = 15000
refresh_interval_ms = 0

[agents.codex]
command = "codex"
args = []
retries = 1

```

模型接口同时支持 `wire_api = "responses"` 和 OpenAI 兼容的
`wire_api = "chat_completions"`。兼容模式会请求
`<base_url>/chat/completions`，因此可以把 `base_url` 配置为服务根地址或
带 `/v1` 的地址；Core 服务不会自动追加或移除 `/v1`。

在 CLI 中使用 `model` 查看当前选择，使用 `model test` 验证认证命令。认证命令的 stdout 只在内存中使用，不写入任务日志。

自然语言输入进入持续对话。管理模型使用内置工具管理长期委托、项目和会话，需要文件操作、实时数据收集等能力时调用外部 Agent。管理模型和 Agent 使用各自配置；认证命令的 stdout 只在内存中使用，不写入任务日志。

`max_concurrency` 限制长期委托之间的运行并发（默认 4），同一委托不会重叠执行。Agent 执行时长和审批规则见[适配器](docs/adapters.md)。

项目仅是可选工作目录上下文，可通过对话登记已有目录并按名称引用。不指定工作目录的长期委托和 HTTP 即时对话使用全局配置及通用工作区，不需要先启动项目。内置管理规则见 [skill](skills/omnistream-project-management/SKILL.md)。

## 多 Agent 与插件

侧栏「Agent 管理」支持创建带角色指令的专属助手；输入框 @名称 选择调用，也可在长期任务表单中指定助手。插件权限默认沿用当前账号，也可单独缩小范围。配置与行为见[命名 Agent](docs/agents.md)。

当自然语言请求超出内置工具能力（例如收集实时信息、读取资料、分析源码）时，管理模型可返回 `delegate` 决策。本系统按可选工作目录和全局配置列出支持 CLI、ACP、JSONL 或 App Server 的 Agent，并根据任务选择执行；输入编号或配置名称执行，输入 `取消` 放弃。委派携带原始请求、项目路径和已有上下文，结果回到当前对话并持久化。交互 CLI 中的选择仅在当前进程有效；HTTP 对话自动选取第一个可用 Agent。单次 CLI 命令不能继续选择，请使用交互模式。没有可用 Agent 时会提示配置，认证、网络或格式错误不会自动触发委派。

需要连续对话与交互审批时推荐：

```toml
[agents.codex]
command = "codex"
protocol = "app-server"
args = ["app-server"]
config_file = "~/.codex/wechat.config.toml" # 可选：转换为 Codex -c 参数
```

执行期间用 `approvals` 查看请求，`approval approve <id>` 或 `approval reject <id>` 回复，`agent cancel` 取消委派。完成后用 `agent continue <内容>` 继续同一会话。此适配器直接使用 Rust 与 Codex CLI 通信，不需要 Python/Node SDK。Codex 使用自身的模型和登录配置。

本地模拟模型服务：`cargo run -p omnistream --bin omnistream-mock-model -- fixtures/models/replies.json`。完整测试和协议边界见 [适配器说明](docs/adapters.md)。

例如输入 `/data/code/shop-api 是什么项目`，出现选择菜单后输入 `1` 或 `codex`。列表来自 `[agents.<名称>]`，新增配置即可扩展列表。委派本身不会创建任务或自动运行项目测试。

Agent 默认执行 `codex <任务目标>`，也可以配置：

```toml
[agents.codex]
command = "codex"
args = []

```

ACP Agent 可设置 `protocol = "acp"`。自定义插件协议设置 `protocol = "jsonl"`，两者不可混用。
ACP 收到 `session/request_permission` 时，只有工具标题匹配 `auto_approve` 才会自动选择；其他请求会明确取消并使任务失败，不会隐式放行。
命令行单次运行会等待任务结束，交互模式支持在执行期间查询状态；执行时长与取消规则见[适配器](docs/adapters.md#统一契约)。

`approvals` 是 TOML 顶层列表，不能放在 `[agents.codex]` 表内。当前仅 `run_script` 会检查该列表；其他动作和 ACP 工具审批尚未接入，不能依赖这份列表约束 Agent 内部操作。

外部进程插件通过 JSON Lines 进程协议通信，消息类型包括 `initialize`、`invoke`、`result`、`event` 和 `error`。manifest 示例：

```json
{"id":"development","version":"0.1.0","name":"Development","runtime":"rust","capabilities":["agent.execute","test.run"],"permissions":["project.read","project.write"],"actions":["edit_source"]}
```

可用 `plugin validate manifest.json` 校验。

GUI 侧栏提供“插件市场”，可搜索并按需安装、卸载 Slack、GitHub、Notion、Figma 插件，使用访问令牌连接账号，在界面或对话中读取数据、发送消息、创建 Issue / Notion 子页面和发表评论。市场另提供 Chrome DevTools MCP，可显式启用浏览器网页操作，并检查与现有 Chrome 的连接。配置页可一键启动 Agent 协助获取令牌，长说明折叠在手动配置中；用户完成登录授权并将令牌填回配置表单。插件可独立停用，写入权限默认关闭，助手写入遵循会话审批模式；凭据、权限、操作和验证边界见[插件市场](docs/plugins.md)。

Slack 同时支持 Bot 和个人 User Token：保留机器人连接，也可切换为本人身份收发消息，无需邀请机器人到公共频道。个人账号可通过 Socket Mode 持续接收，选择相关消息或全部已加入会话，自动同步到 Slack 收件箱并直接回复。使用官方 API，无需安装 Slack CLI；令牌、事件订阅、权限和历史同步边界见[Slack 连接方式](docs/plugins.md#slack-连接方式)。

启动 HTTP 查询服务：

```bash
cargo run -p omnistream -- serve 127.0.0.1:4317
curl http://127.0.0.1:4317/health
curl http://127.0.0.1:4317/automations
curl -X POST -H 'Content-Type: application/json' \
  -d '{"title":"AI 日报","goal":"收集新闻并注明来源","schedule":{"kind":"daily","hour":9,"minute":0,"utc_offset_minutes":480},"conversation":"inbox"}' \
  http://127.0.0.1:4317/automations
curl -X POST http://127.0.0.1:4317/automations/1/pause
curl -X POST http://127.0.0.1:4317/automations/1/resume
curl -X POST http://127.0.0.1:4317/automations/1/run
curl -X POST http://127.0.0.1:4317/automations/1/cancel
```

## Core 与 GUI

网站元信息使用 `https://omnistreamai.com/` 作为正式网址，页面和 PWA 描述与本文件的项目介绍一致。Open Graph 和 Twitter 使用 `apps/gui/public/og-image.png` 分享图（1200×630）；favicon、PWA 和应用图标使用无文字标志。PWA manifest 和 service worker 由 Vite 插件在构建时生成。部署到其他正式域名时需同步更新 canonical 和分享资源的绝对地址。

GUI 接入 `omnistream serve` 提供的 Core 服务；职责划分与依赖约束见 [Core 与前端边界](docs/adapters.md#core-与前端边界)。

长期任务在侧栏面板统一管理，结果跳转到普通接收会话。GUI 使用浏览器路由标识当前工作区：`/chat/:conversationId` 为会话，`/slack` 为收件箱。根路径和已移除的旧任务地址会回到本地记忆的当前会话；会话链接支持刷新后恢复。

GUI 位于 `apps/gui`，使用 Tauri 2、React、TanStack React Query、`@ai-sdk/react` 和 Lucide React。桌面版将 `omnistream` 可执行文件作为 Tauri sidecar，并自动启动 `omnistream serve`。Android/iOS 版本不在本地运行 shell 或 Codex；构建时设置 `VITE_OMNISTREAM_URL`，或在顶部栏输入远程 Core 地址。

桌面版连接设置提供“允许局域网设备连接”和“开机自启”，均默认关闭。前者默认监听 `127.0.0.1:4317`，开启后监听 `0.0.0.0:4317`，保存到本机应用配置目录的 `desktop.json`，退出并重启桌面应用后生效；仅影响桌面应用启动的本机 Core，不修改远程 Core 或 CLI 默认地址。其他设备使用 `http://本机局域网IP:4317` 连接，需允许防火墙端口。当前 Core 无统一访问鉴权，同网络设备可操作助手，仅在可信网络开启，不应直接暴露到公网。“开机自启”通过系统当前用户登录项启动桌面应用及 Core，勾选或取消立即更新系统登录项；不作为系统后台服务，未登录时不启动。Web/PWA 和移动端不显示这些本机桌面设置。

直接对话使用 `@ai-sdk/react` 和 Core 的 `POST /chat/stream` 文本传输。任务和审批数据仍由 TanStack React Query 管理的 Core 业务资源负责，客户端 SDK 不直接执行这些操作。

正常退出桌面应用会终止由它启动的 Core 子进程；退出前应完成运行中的任务。不会终止手动启动或远程运行的 Core。应用崩溃或系统强制结束时不保证清理子进程。

```bash
pnpm install
pnpm --filter @omnistream/gui dev
pnpm --filter @omnistream/gui typecheck
pnpm --filter @omnistream/gui build
```

For a desktop bundle, build the `omnistream` binary for each target and place it under `apps/gui/src-tauri/binaries/omnistream-<target-triple>` before running `pnpm --filter @omnistream/gui tauri build`. The sidecar name must include the target triple as required by Tauri.
