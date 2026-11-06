<div align="center">
  <img src="../../apps/gui/public/omnistream-mark.svg" alt="OmniStream" width="100" />
  <h1>OmniStream</h1>
  <p>面向即时请求与长期委托的通用 AI 助手工作空间。</p>
  <p>
    <img src="https://img.shields.io/badge/core-Rust-orange?logo=rust" alt="Rust Core" />
    <img src="https://img.shields.io/badge/UI-React%20%2B%20TypeScript-149eca?logo=react" alt="React 与 TypeScript" />
    <img src="https://img.shields.io/badge/desktop-Tauri%202-24c8d8?logo=tauri" alt="Tauri 2" />
    <img src="https://img.shields.io/badge/platforms-Web%20%7C%20Desktop-347d39" alt="Web 与桌面端" />
  </p>
  <p><a href="../../README.md">English</a> · <a href="index.md">文档目录</a> · <a href="https://omnistreamai.com/">项目网站</a></p>
</div>

---

OmniStream 将对话、专属 Agent、应用连接与定时执行整合到同一工作空间。你可以在会话中提出查询、研究、分析和编程请求，也可以创建长期委托，将每次执行结果交付到接收会话。

## 功能

- **对话与 Agent 协作**：持久化会话、流式回复、执行进度、审批和取消。
- **专属助手**：创建带角色指令的命名 Agent，通过 @ 调用，或指定为长期任务的执行助手。
- **长期委托**：每日或固定间隔调度、日历规则、执行记录、逐次确认与授权插件投递。
- **应用连接**：Slack、GitHub、Notion、Figma 和 Chrome DevTools MCP，账号连接和写入权限均显式配置。
- **Web 与桌面端**：支持 PWA 的 React 网页，以及自动启动本机 Rust Core 的 Tauri 桌面应用。

当前已配置 Web 构建与桌面打包。Android/iOS 保留远程 Core 接入入口，尚未提供原生工程和完整打包流程。

## 快速开始

需要 Rust、Node.js **20.19+ 或 22.12+**、pnpm；使用下面的快捷命令还需要 `make`。

```bash
git clone https://github.com/omni-stream-ai/omni-stream.git
cd omni-stream
pnpm install
make run
```

另开终端启动前端：

```bash
pnpm dev
```

打开 **http://localhost:1420**，默认连接 **http://127.0.0.1:4317**。使用其他 Core 地址时，在顶部连接设置中修改。

没有 `make` 时可用 `cargo run -p omnistream -- serve 127.0.0.1:4317`。自然语言交互 CLI 使用 `cargo run -p omnistream`。

## 配置与使用

完整模型对话和外部 Agent 执行需要配置。默认读取 `~/.omnistream/config.toml`；设置 `OMNISTREAM_HOME` 可修改全局目录，项目中的 `.omnistream/config.toml` 覆盖全局配置。管理模型与外部 Agent 使用各自的模型和凭据。

需要连续对话与交互审批的 Codex 配置：

```toml
[agents.codex]
command = "codex"
protocol = "app-server"
args = ["app-server"]
```

Codex 使用自身的模型与登录配置。模型 provider 示例、CLI 与桌面设置见[配置指南](configuration.md)，协议支持及限制见[适配器](adapters.md)。

即时请求在当前会话执行，不创建用户任务。在对话中生成的长期委托先保存为待确认草案，需在侧栏面板确认启用；Core 持续运行时才能按时执行。任务支持修改、暂停、恢复、取消、立即执行及查看记录，详见[长期任务](automations.md)。

## 构建与部署

### Web / PWA

```bash
pnpm gui:build
```

将 `apps/gui/dist` 部署到静态托管服务。Cloudflare Pages 使用仓库根目录，框架选 **React (Vite)**，构建命令填 `pnpm gui:build`，输出目录填 `apps/gui/dist`。静态托管需支持将 `/chat/:conversationId` 等前端路由回退到 `index.html`。

没有必填的应用环境变量。部署后可在界面中填写 Core 地址，也可构建时设置 `VITE_OMNISTREAM_URL`；默认 `127.0.0.1` 指访问者自己的电脑。Cloudflare 仅托管前端，Rust Core 需独立运行。HTTPS 网页连接远程 Core 时应使用 HTTPS 接口地址。

### 桌面端

先编译目标平台的 `omnistream`，按 Tauri sidecar 规则放入 `apps/gui/src-tauri/binaries/omnistream-<target-triple>`，Windows 文件需带 `.exe` 后缀。安装对应系统的 [Tauri 构建依赖](https://v2.tauri.app/start/prerequisites/)，再运行：

```bash
pnpm --filter @omnistream/gui tauri build
```

桌面版启动与退出时管理自身的 Core，支持默认关闭的局域网连接与开机自启选项。具体行为和平台验证范围见[桌面设置](configuration.md#桌面设置)。

**网络访问边界**：当前 Core 无统一访问鉴权。局域网访问仅在可信网络开启；远程接口需单独保护，仅保护前端不能保护 Core。

## 开发与文档

```bash
cargo test --workspace
pnpm --filter @omnistream/gui typecheck
pnpm --filter @omnistream/gui test:unit
pnpm gui:build
```

浏览器回归与验证边界见[测试](testing.md)。本项目采用文档驱动开发，修改前请阅读[文档目录](index.md)和[协作约定](../../AGENTS.md)。
