<div align="center">
  <img src="../../apps/gui/public/omnistream-mark.svg" alt="OmniStream" width="100" />
  <h1>OmniStream</h1>
  <p>面向即时请求与长期委托的通用 AI 助手工作空间。</p>
  <p>
    <img src="https://img.shields.io/badge/core-Rust-orange?logo=rust" alt="Rust Core" />
    <img src="https://img.shields.io/badge/UI-React%20%2B%20TypeScript-149eca?logo=react" alt="React 与 TypeScript" />
    <img src="https://img.shields.io/badge/native-Tauri%202-24c8d8?logo=tauri" alt="Tauri 2" />
    <img src="https://img.shields.io/badge/platforms-Web%20%7C%20Desktop%20%7C%20Android%20%7C%20iOS-347d39" alt="Web、桌面、Android 与 iOS" />
    <a href="https://github.com/omni-stream-ai/omni-stream/actions/workflows/release.yml"><img src="https://github.com/omni-stream-ai/omni-stream/actions/workflows/release.yml/badge.svg" alt="打包发布" /></a>
    <a href="https://github.com/omni-stream-ai/omni-stream/releases"><img src="https://img.shields.io/github/v/release/omni-stream-ai/omni-stream?include_prereleases" alt="GitHub Release" /></a>
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
- **Web、桌面与移动端**：支持 PWA 的 React 网页，以及 Tauri 桌面、Android 和 iOS 原生应用；桌面自动启动本机 Rust Core，移动端连接远程 Core。
- **系统通知**：任务完成或失败、待响应和 Slack 收件箱新消息提醒；Android 与浏览器在授权后可自动启用[远程推送](push.md)，在页面或 App 关闭时接收。

原生移动工程通过平台脚本在本机生成，提供开发运行与安装包构建入口；环境准备、签名和验证边界见[移动端指南](mobile.md)。

## 下载与发布

[GitHub Releases](https://github.com/omni-stream-ai/omni-stream/releases) 统一提供 Web 静态包、Windows/Linux/macOS 安装包、独立 Core 和已签名 Android APK/AAB；iOS IPA 按需启用 Apple 签名，仅限 Ad Hoc 描述文件登记的设备安装。每版附分类更新日志、`manifest.json` 与 `SHA256SUMS`。

[Release 工作流](../../.github/workflows/release.yml) 在版本变化后构建，默认分支发布稳定版，其他分支发布预发布版；手动运行只构建。首次自动发布前需配置 Android 签名 secrets，完整产物、版本清单、签名配置与重试规则见[打包与发布](releases.md)。Windows 包未做代码签名，macOS 使用 ad-hoc 签名且未公证；暂不发布到包仓库和应用商店。

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

完整模型对话和外部 Agent 执行需要配置。默认读取 `~/.omnistream/config.toml`；设置 `OMNISTREAM_HOME` 可修改全局目录，项目中的 `.omnistream/config.toml` 覆盖全局配置。

需要连续对话与交互审批的 Codex 配置：

```toml
[agents.codex]
command = "codex"
protocol = "app-server"
args = ["app-server"]
```

Codex App Server 的会话委派可[继承当前请求的模型与认证配置](configuration.md#委派模型继承)。模型 provider 示例、CLI 与桌面设置见[配置指南](configuration.md)，协议支持及限制见[适配器](adapters.md)。

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

开发热更新使用 `pnpm --filter @omnistream/gui tauri dev`，会自动启动 Vite、原生窗口与 Core；前端修改即时更新，`src-tauri` 下的 Rust 修改后重新编译并重启。Core sidecar 需在 Core 源码变化后重新编译并复制，不能由 GUI 热更新代替。

NixOS / Nix 开发机使用[开发环境](../../flake.nix)，其中提供 Node.js、pnpm、rustup、pkg-config，以及 Linux 所需的 WebKitGTK 4.1、GTK 3、GLib、libsoup 3、librsvg、OpenSSL、D-Bus 开发库和 patchelf。首次新增 flake 文件时先让 Git 跟踪，随后进入环境启动：

```bash
git add flake.nix flake.lock  # 仅首次新增文件时需要，Nix Git flake 会忽略未跟踪文件
nix develop -c pnpm --filter @omnistream/gui tauri dev
```

也可先运行 `nix develop`，再在该 shell 中执行开发或打包命令；修改环境依赖后需退出并重新进入。环境不修改系统全局配置。`libdbus-sys` 报找不到 `dbus-1.pc` 时，使用 `nix develop -c pkg-config --modversion dbus-1` 检查；仅在普通终端安装了 pkg-config 并不意味着已加载项目依赖。Tauri 环境检查使用 `nix develop -c pnpm --filter @omnistream/gui tauri info`，编译检查使用 `nix develop -c cargo check --manifest-path apps/gui/src-tauri/Cargo.toml`。打包仍需先准备 sidecar，真实窗口与系统通知须在图形会话验收。

### Android / iOS 原生 App

Android 需要 Android Studio、JDK 17+、SDK 与 NDK；iOS 需要 macOS、Xcode、CocoaPods 和 Apple 开发团队配置。准备好[移动端构建环境](mobile.md#构建环境)后运行：

```bash
pnpm --filter @omnistream/gui android:init
pnpm --filter @omnistream/gui android:dev
pnpm --filter @omnistream/gui android:build --apk

# 以下命令在 macOS 上运行
pnpm --filter @omnistream/gui ios:init
pnpm --filter @omnistream/gui ios:dev
pnpm --filter @omnistream/gui ios:build
```

首次启动填写电脑或服务器的 Core 地址，手机上的 `127.0.0.1` 指向手机自身。App 不包含本机 Core、shell 或 Codex。Android APK/AAB 与 iOS 签名、模拟器及产物位置见[移动端指南](mobile.md)；编译通过与真机验收分别记录。

**网络访问边界**：Core 监听非回环地址时启用客户端授权；首个设备在服务端终端批准，后续申请可在已授权客户端弹窗中处理并随时撤销。公网使用 HTTPS；反向代理回环 Core 时必须显式强制授权。信任边界、配置和操作见[客户端访问授权](access.md)。

## 开发与文档

```bash
cargo test --workspace
pnpm --filter @omnistream/gui typecheck
pnpm --filter @omnistream/gui test:unit
pnpm gui:build
```

浏览器回归与验证边界见[测试](testing.md)。本项目采用文档驱动开发，修改前请阅读[文档目录](index.md)和[协作约定](../../AGENTS.md)。
