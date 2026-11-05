# 适配器与 Core

本文维护适配器契约、Core 与前端边界，并记录当前协议支持情况、配置行为和验证方式。项目入口见 [文档索引](README.md)。

## 统一契约

Rust 运行时使用 `adapter::Adapter`、`Request`、`Host` 和 `Event`；`Configured` 根据配置选择 CLI、ACP、JSONL、App Server 或 Mock。早期 TypeScript `AgentAdapter` 属于独立脚手架，不是 Rust 运行时契约。

- 请求包含项目目录、提示词和可选的外部会话 ID；仅声明支持恢复的适配器使用已保存的 ID。
- 适配器通过事件报告会话、文本、进度、完成和失败；Core 负责持久化，可通过 `POST /conversation/detail` 的 calls/events 和 `/events` SSE 读取。
- 接入统一审批的适配器通过 `Host` 提交请求，Core 负责持久化、等待用户及返回响应；必须保留原协议的请求 ID 和载荷。`Host::approve` 支持批准、会话内批准、拒绝和取消；需要结构化输入的请求通过 `Host` 请求响应接口处理。
- 能力声明必须反映实际支持情况，实现统一接口不代表自动具备会话恢复、审批或流式能力。
- **执行时长**：Core 不为 Agent 设置整轮执行时限，适用于普通委派、配置协助、长期委托生成。审批和用户输入持续等待响应或主动取消，插件网关随执行存续，不按运行时长失效。旧 `[agents.<名称>].timeout_ms` 仅兼容读取，不再生效；网络、工具单次请求及认证命令仍使用各自独立的超时规则。

## 当前支持情况

| 适配器 | 会话恢复、统一人工审批、流式事件 | 限制 |
| --- | --- | --- |
| App Server | 已支持 | 协议覆盖范围见下文；仅在恢复明确占用时先分支，不重放已开始的工作 |
| CLI、ACP、JSONL | 基础执行已支持；插件工具按插件市场的通用网关接入 | 仍不具备 App Server 的全部会话恢复、统一人工审批或流式能力；自定义 Agent 需支持 MCP 或命令执行 |
| Mock | 提供测试实现 | 模拟能力，不启动外部 Agent |

JSONL 仍缺少执行期间的取消控制，不能由统一接口推断其具备此能力。

CLI 的长期委托与插件配置协助在成功时只将 stdout 作为 Agent 结果，避免进程诊断信息破坏 JSON 条件结果或进入外部投递内容；失败时仍返回包含退出状态、stdout 和 stderr 的诊断。其他 CLI 执行保留原有进程报告。

## Codex App Server

当前适配器直接通过 stdio JSON-RPC 与 Codex CLI 通信，仅需 Core 服务和 Codex CLI，无需 Python/Node 桥接。当前线程恢复与占用分支已在 `codex-cli 0.166.0` 验证；升级时需重新生成协议 schema 并运行真实 CLI 测试。

### 启动与配置

```toml
[agents.codex]
command = "codex"
protocol = "app-server"
args = ["app-server"]
```

`args` 为空时默认使用 `app-server`；非空时必须包含完整参数及子命令。Codex 使用自身的模型、provider 和登录配置，Core 服务的管理模型 provider 设置不影响它。

可在该 Agent 配置表中设置 `config_file = "~/.codex/wechat.config.toml"`，复用配置文件，无需 `--profile`。当前行为如下：

- **参数转换**：Core 服务解析 TOML，将每个顶层设置转换为 `-c key=value`，插入 `app-server` 后。嵌套表转为 TOML 内联表，保留嵌套带引号的键、数组和标量，不经过 shell 求值。顶层键仅允许字母、数字、下划线和连字符，以适配 Codex CLI 的键解析规则。
- **覆盖顺序**：文件值覆盖 Codex 基础配置；顶层表整体替换，不递归合并。`args` 中位于 `app-server` 后的显式覆盖参数优先于文件值。这是 CLI 参数覆盖，不是原生 profile 层，不改变 `CODEX_HOME`、登录或历史记录。
- **路径与加载**：此选项仅适用于 App Server，每次运行重新读取；文件缺失或无效时在启动 Codex 前报错。文件路径相对于声明它的 Core 配置文件解析，`~/` 使用 HOME。导入文件内部的路径仍遵循 CLI 覆盖语义，`model_catalog_json` 等资源应使用绝对路径。更换配置文件路径会隔离已保存的会话。
- **凭据**：不要在导入文件中存放明文密钥，因为转换后的值会出现在进程参数中；使用 provider 认证命令或环境变量键引用。

服务及浏览器插件按[通用 Agent 接入](plugins.md#通用-agent-接入)向各协议提供 MCP 或命令工具。App Server 以进程参数加载统一网关，并保留上述 config_file 导入；Agent 的 plugin_transport 和 mcp_args 选项见该节。插件工具的服务写入审批由 Core 网关统一执行，不取决于适配器是否支持 Agent 自身的人工审批。

### 协议与审批

客户端依次执行 `initialize`、`initialized`、`thread/start` 或 `thread/resume`、`turn/start`，持续处理通知直到收到对应的 `turn/completed`，包括先于启动响应到达的通知。恢复时遇到下文定义的写锁占用，可在开始本轮工作前调用 `thread/fork`。

当前请求处理范围：

| 请求 | 处理方式 |
| --- | --- |
| 命令、文件变更、额外权限 | 转为 Core 审批 |
| 工具用户输入、MCP elicitation | 等待 `approval respond <id> <JSON>`，按 App Server schema 校验响应 |
| 动态工具调用 | Core 未注册对应工具时返回结构化工具失败 |
| 外部令牌刷新、attestation | 返回明确的协议错误，Core 服务不管理这些上游凭据 |
| 未知请求 | 返回 method-not-found，不隐式批准 |

这不是 App Server 全部端点的完整实现。[审批模式](sessions.md#审批模式)决定线程策略：`default` 不覆盖外部配置；`safe` 使用 `approvalPolicy = "on-request"`、`sandbox = "workspace-write"`；`all` 使用 `approvalPolicy = "never"`、`sandbox = "danger-full-access"`。不保证每条命令都会提示审批；`all` 模式仍将用户输入和 MCP elicitation 交给用户回答。

### 执行生命周期

App Server 委派在后台执行，期间可使用：

```text
approvals
approval approve 12
approval reject 12
approval respond 12 '{"answers":{"question_id":{"answers":["value"]}}}'
agent cancel
```

HTTP 客户端通过 `POST /agent-calls/<id>/cancel` 取消会话内的 Agent 执行，编号来自会话详情的 `calls[].task_id`；该接口不启动或重跑执行。审批使用 Core 审批接口；结构化输入对应 `POST /approvals/<id>/respond`，请求体为上述响应 JSON。

- **恢复与隔离**：外部会话 ID 持久化保存，直接委派按管理会话、项目、Agent 名称及 command/protocol/args/config_file、插件接入设置与工具授权配置身份隔离；长期委托按任务及运行作用域隔离。各作用域新建各自线程，同一作用域的连续对话恢复已保存线程，不能因复用工作目录而共用其他会话的线程。临时插件网关路径不影响同配置下的恢复。完成后可用 `agent continue <内容>` 继续委派对话。
- **占用与分支**：同一作用域仅允许一个执行，使用 Core 状态目录下的系统文件锁协调本进程及其他 Core 进程，冲突明确提示等待当前执行结束；正常结束或进程退出释放锁，保留锁文件避免删除与重建竞态。若外部终端已经打开已保存的 Codex 线程，且 `thread/resume` 明确返回 −32600 与该线程的 `already has an active writer` 错误，在发送 `turn/start` 前只尝试一次 `thread/fork`，复制当时已保存的历史到独立线程，并沿用本轮工作目录、审批和工具配置。不终止占用者、不改写原线程。分支的新 ID 在开始本轮工作前保存，后续对话恢复该 ID；执行事件记录原 ID、新 ID 和原因。空 ID、与原线程相同的 ID、分支失败、认证/网络及其他恢复错误均明确失败，不改用空白线程、不循环重试。本轮工作开始后的错误不触发分支或重放。
  线程分支隔离会话状态，工作目录仍按本轮配置使用；它不会创建独立代码工作树。
- **输出与事件**：文本在终端流式显示，结构化进度和输出持久化。委派事件使用任务 ID 0，并携带作用域键。
- **取消**：共享取消标记覆盖执行和审批等待。用户取消时，若已知 turn ID，先发送 `turn/interrupt`，再终止本地进程组；退出交互 shell 也会取消运行中的委派。取消结果明确报告为“已取消”。
- **崩溃残留**：进程异常崩溃可能留下待处理审批记录，后续 App Server 运行不能复用该记录。

## Core 与前端边界

Core 负责 SQLite、Agent 进程、会话、事件和审批状态，CLI 是当前调试前端。GUI 应通过 Core HTTP API、事件流或 `omnistream-core` 中适用于前端的审批与协议模型接入，不依赖终端渲染代码。

源码开发可通过项目根目录的 Makefile 启动 Core，见[运行说明](../README.md#运行)。安装后也可直接运行 `omnistream serve`：

```bash
cargo install --path crates/omnistream
omnistream serve 127.0.0.1:4317
```

GUI 的技术栈、启动及打包方式见 [项目说明](../README.md#core-与-gui)。

## 验证方式与覆盖范围

`MockAdapter` 在不使用外部可执行程序或模型的情况下验证 Core，测试可控制审批、失败、取消和会话恢复。配置 `protocol = "mock"` 选择基础回显实现；command 可用非空占位值，不会实际执行。

独立模拟模型基于 `tiny_http`，按脚本顺序返回 HTTP 响应，接受 Responses 和 Chat Completions 路径，由 fixture 提供相应响应体。它不检查凭据、不调用外部模型；请求记录在内存中（不含请求头），在服务停止时打印。fixture 不得包含密钥。

```bash
cargo run -p omnistream --bin omnistream-mock-model -- fixtures/models/replies.json 127.0.0.1:4320
```

脚本为 JSON 数组，每项可包含 `body`、`status`（默认 200）、`delay_ms`、`disconnect` 或用于 SSE 的 `events`。SSE 按脚本响应顺序发送，不模拟逐 token 时序；脚本耗尽后返回 HTTP 500。按 Enter 停止服务。

```bash
cargo test --workspace
cargo test -p omnistream --bin omnistream real_codex_with_mock_model_and_resume -- --ignored --nocapture
```

- 真实 CLI 测试需要安装 Codex CLI，可用 `OMNISTREAM_TEST_CODEX` 指定路径。测试使用隔离的临时 `CODEX_HOME` 和本地回环模型端点，不需要真实模型凭据，也不产生付费推理；验证 App Server 握手、两轮对话和历史保留。
- 独立模拟 App Server 测试覆盖审批回复、提前到达的事件、格式错误、RPC 错误、EOF、取消、占用时分支以及其他错误不触发分支。隔离 Core 验证分支 ID 持久化及后续恢复，系统锁测试验证同作用域互斥、不同作用域独立和进程退出释放。真实 Core 的回归用例将旧 Agent 超时配置为 30 ms，在超过该时长后验证审批仍待响应、会话可恢复，以及 App Server、CLI、ACP 的执行和插件网关仍可使用并可主动取消。真实 CLI 测试使用独立 CODEX_HOME 和回环模拟模型，验证恢复与原线程持锁时的分支历史保留；未覆盖所有工具和审批流程，不能据此宣称协议已完整支持。

协议参考：[App Server](https://developers.openai.com/codex/app-server/)、[Codex SDK](https://developers.openai.com/codex/sdk/)。
