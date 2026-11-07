# 配置与运行参考

项目介绍、快速开始和构建命令见[中文首页](README.md)。本文维护模型配置、CLI 接入与桌面运行边界。

## Core 与前端

Core 负责 SQLite、Agent 进程、会话、事件和审批，GUI 通过 HTTP API 与事件流访问，职责划分见[Core 与前端边界](adapters.md#core-与前端边界)。当前不维护独立 TUI。

`make` 或 `make run` 在前台编译并启动 Core，默认地址为 `127.0.0.1:4317`；使用 `make run ADDR=127.0.0.1:4320` 修改地址，Ctrl+C 停止。也可直接执行：

```bash
cargo run -p omnistream -- serve 127.0.0.1:4317
curl http://127.0.0.1:4317/health
```

GUI 位于 `apps/gui`，使用 Tauri 2、React、TanStack React Query、`@ai-sdk/react` 和 Lucide React。直接对话通过 Core 的 `POST /chat/stream` 文本传输，任务和审批通过 Core 业务资源管理，客户端 SDK 不直接执行这些操作。

浏览器路由 `/chat/:conversationId` 表示会话，`/slack` 表示收件箱。根路径和已移除的旧任务地址回到本地记忆的当前会话，会话链接支持刷新后恢复。Android/iOS 使用原生 App 连接远程 Core，不在本地运行 shell 或 Codex；构建、签名和连接规则见[移动端指南](mobile.md)。

## 模型配置

默认读取 `~/.omnistream/config.toml`，设置 `OMNISTREAM_HOME` 时从该目录读取全局配置。项目中的 `.omnistream/config.toml` 覆盖全局配置。

以下是 provider 与 Agent 配置示例；模型 ID、服务地址和接口类型需与实际供应商支持情况一致：

```toml
model_provider = "deepseek"
model = "deepseek-flash"
model_reasoning_effort = "low"
model_reasoning_efforts = ["low", "medium", "high"]
max_concurrency = 4
auto_approve = ["read-only"]
approval_mode = "default" # default | safe | all

[model_providers.deepseek]
name = "deepseek"
base_url = "https://api.deepseek.com"
wire_api = "responses"
models = [{ id = "deepseek-flash" }]
# 仅在 provider 有特殊能力时覆盖全局默认值
# reasoning_efforts = ["low", "medium", "high", "xhigh"]

[model_providers.deepseek.auth]
command = "pass"
args = ["show", "codex/deepseek-api-key"]
timeout_ms = 15000
refresh_interval_ms = 0

[agents.codex]
command = "codex"
protocol = "app-server"
args = ["app-server"]
```

模型接口支持 `wire_api = "responses"` 和 OpenAI 兼容的 `wire_api = "chat_completions"`。兼容模式请求 `<base_url>/chat/completions`，Core 不自动追加或移除 `/v1`。

CLI 中使用 `model` 查看当前选择，`model test` 验证认证命令。认证命令的 stdout 只在内存中使用，不写入任务日志。Codex App Server 的会话委派遵循下述模型继承规则。

`max_concurrency` 限制长期委托之间的并发，默认 4，同一委托不重叠执行。项目只是可选工作目录上下文，可登记已有目录后按名称引用；未指定目录的长期委托和 HTTP 即时对话使用全局配置及通用工作区。执行时长和审批规则见[适配器](adapters.md)，内置管理规则见[内置 skill](builtin-skills.md)。

### 委派模型继承

普通会话委派、前置子任务及显式 @助手在使用 App Server 引擎时，继承发起请求时选定的 provider、模型、思考等级、服务地址和认证配置。界面选择仍是全局共享；每条请求固定自己的配置快照，子任务续接沿用该快照，执行中切换选择只影响后续请求。交互 CLI 使用当次请求的管理模型选择，`agent continue` 重新读取选择。

这些模型参数优先于 Agent 的 `config_file`、显式模型参数及 Codex 自身模型配置；思考等级未指定时使用 `medium`，不继承外部配置中的旧等级。工作目录、插件接入和审批仍按各自规则处理，不修改用户的 Codex 配置文件。

当前继承仅支持 App Server 的 `responses` provider。不兼容的接口、缺失模型/provider、无效服务地址、认证命令缺失或失败均明确报错，不自动换模型、provider 或使用 Codex 登录凭据。没有任何管理模型选择时仍可使用 Agent 自身配置；CLI、ACP、JSONL 等其他执行协议暂不注入模型配置。后台长期任务、逐次确认讨论和插件配置协助继续使用各自引擎配置，不随聊天中的模型切换而改变。

启动时按所选 provider 的认证命令与缓存规则获取密钥，仅通过本次子进程环境变量传入；生成的启动参数只包含环境变量名称，不携带密钥值，Core 不将密钥写入提示词、会话或执行日志。模型、provider、地址、思考等级或认证配置引用变化会隔离外部线程；同一配置可恢复原线程。密钥刷新不改变线程身份。

## 外部 Agent 与 CLI

侧栏可创建命名助手，通过 @ 调用或绑定长期任务，详见[命名 Agent](agents.md)。底层执行引擎来自 `[agents.<名称>]`，支持 CLI、ACP、JSONL 和 App Server，能力边界见[适配器](adapters.md)。

当自然语言请求超出内置工具能力时，管理模型可返回 `delegate` 决策。系统按可选工作目录与全局配置列出 Agent 并选择执行，携带原始请求、路径和已有上下文，结果回到当前会话并持久化，不创建用户任务或自动运行项目测试。认证、网络或格式错误不会自动触发委派；没有可用 Agent 时提示配置。

交互 CLI 可输入编号或配置名称选择 Agent，输入 `取消` 放弃，选择仅对当前进程有效。HTTP 对话自动选取第一个可用 Agent；单次 CLI 命令不能继续选择，需要选择时使用交互模式。

需要连续对话与交互审批时推荐 App Server 配置，见[启动与配置](adapters.md#启动与配置)。执行期间使用：

```text
approvals
approval approve <id>
approval reject <id>
agent cancel
agent continue <内容>
```

`agent continue` 用于完成后继续同一会话。此适配器直接使用 Rust 与 Codex CLI 通信，无需 Python/Node SDK。

默认 CLI Agent 执行 `codex <任务目标>`，可配置 `command = "codex"`、`args = []`。ACP 设置 `protocol = "acp"`，JSON Lines 自定义协议设置 `protocol = "jsonl"`，两者不可混用。ACP 收到 `session/request_permission` 时，仅工具标题匹配 `auto_approve` 才自动选择；其他请求明确取消并使执行失败。单次命令等待执行结束，交互模式支持执行期间查询状态。

`approvals` 是 TOML 顶层列表，不能放在 `[agents.codex]` 中；当前仅 `run_script` 检查该列表，不能依赖它约束其他动作或 Agent 内部操作。

外部进程插件通过 JSON Lines 通信，消息包含 `initialize`、`invoke`、`result`、`event` 和 `error`。manifest 由 `id`、`version`、`name`、`runtime`、`capabilities`、`permissions` 和 `actions` 描述，可用 `plugin validate manifest.json` 校验。市场插件、凭据、写入权限及 Agent 网关见[插件市场](plugins.md)。

## 桌面设置

Android 与浏览器远程通知使用可选的 `[push]` 配置，内置发送账号、运行时凭据覆盖、VAPID 密钥、设备注册与撤销见[远程推送](push.md)。

桌面应用将 `omnistream` 作为 Tauri sidecar，启动时自动运行 `omnistream serve`。正常退出时终止自身启动的 Core，不终止手动启动或远程 Core；退出前应完成运行中的任务，崩溃或系统强制结束时不保证子进程清理。

连接设置提供以下选项，均默认关闭，只影响当前电脑，不显示在 Web/PWA 或移动端：

| 选项 | 行为 |
| --- | --- |
| 允许局域网设备连接 | 默认监听 `127.0.0.1:4317`；开启后监听 `0.0.0.0:4317`，设置保存到本机应用配置目录的 `desktop.json`，退出并重启应用后生效。不改变远程 Core 或 CLI 默认地址。 |
| 开机自启 | 勾选或取消立即注册或移除当前用户登录启动项。登录后启动桌面应用及 Core，不是未登录时运行的系统后台服务。 |

其他设备使用 `http://本机局域网IP:4317` 连接，需放行防火墙端口并完成[客户端访问授权](access.md)。开启局域网后，本机桌面窗口通过运行期凭据访问自己启动的 Core，可批准其他设备的申请；移动端与远程网页不共享该凭据。公网使用 HTTPS；反向代理和 `[access] require_auth` 配置见上述文档。

自启通过 `tauri-plugin-autostart` 对接 Windows 当前用户 `Run` 注册表、macOS `~/Library/LaunchAgents/` 与 Linux `~/.config/autostart/`。Linux 依赖桌面环境支持 XDG Autostart。已完成 Linux 编译与组件模拟验证，三系统安装后登录自启、真实局域网访问和退出清理仍需各平台实测，不视为已通过验收。

## 网站元信息

正式网址为 `https://omnistreamai.com/`。Open Graph 与 Twitter 分享图使用 `apps/gui/public/og-image.png`（1200×630），favicon、PWA 和应用图标使用无文字标志。PWA manifest 与 service worker 由 Vite 插件在构建时生成。

页面与 PWA 描述应与项目定位保持一致。部署到其他正式域名时，同步更新 canonical 和分享资源的绝对地址。

### 应用图标

应用图标共用 `apps/gui/public/omnistream-mark.svg` 中的 `omnistream-symbol` 图形，通过 `pnpm --filter @omnistream/gui icons:generate` 重新生成。`scripts/icons.mjs` 统一居中和留白，产出桌面 PNG/ICO/ICNS、favicon、Apple touch、PWA 与原生移动端资源；界面内的标志布局保持独立。

- Android 自适应图标采用不透明纯色背景和独立透明前景；前景符号位于中央安全区，系统圆形、圆角方形等裁切后两侧仍有留白。不得把整张带底色图标当作放大的前景。传统启动图标沿用居中的标准图标。
- 桌面与普通 Web 图标保留圆角底色和透明外边距，符号居中并保留约 21% 的左右画布边距；桌面 PNG 使用 RGBA8，避免 Tauri 将 16 位 PNG 作为默认图标时启动失败。
- iOS 与 Apple touch 图标使用完整不透明底色，由系统完成外形裁切，避免图标周围出现白边。
- PWA 的 `any` 与 `maskable` 使用不同资源；后者为完整不透明底色，关键图形保留在中央安全区，manifest 不再对普通透明图标声明可裁切用途。

生成后的桌面与 Web 资源随代码提交。移动端资源目录忽略提交，平台脚本依据图形源和生成器的指纹自动重建；现有 Android 工程同步资源，已有 iOS AppIcon 资源目录同步对应图片。修改图形后执行上述命令并检查各平台预览，原生构建和验证范围见[移动端指南](mobile.md)与[测试指南](testing.md)。
