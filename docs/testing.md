# 测试

Core 与前端的职责和适配器验证边界见[适配器说明](adapters.md)，会话与长期委托的行为分别见[会话](sessions.md)和[长期任务](automations.md)。

## 检查命令

```bash
cargo fmt --all -- --check
cargo test --workspace
pnpm --filter @omnistream/gui typecheck
pnpm --filter @omnistream/gui test:unit
pnpm --filter @omnistream/gui build
```

运行浏览器回归前先执行 `cargo build -p omnistream`，再运行 `pnpm --filter @omnistream/gui test:e2e`。浏览器可通过 `PLAYWRIGHT_CHROMIUM_EXECUTABLE` 指定，例如使用当前系统安装的 Chrome。Playwright 自动启动独立 Vite 服务；用例启动隔离的 Core 和模拟模型、Agent 或插件服务，不使用生产数据与真实账号。

## 验证范围

Rust 测试覆盖会话、Agent 执行与取消、审批、插件权限、长期委托及持久化；`legacy_removal` 验证旧开发接口和命令不可用、旧记录不恢复执行及新 Agent 执行不覆盖旧数据。

GUI 单元测试检查组件、失败重试、表单与多语言；浏览器回归检查会话输入、附件、模型设置、Agent 管理、长期任务与插件流程，包含窄屏布局。真实服务或实际 Agent 的验证应单独说明使用环境及未覆盖部分；模拟测试不能代表生产账号或所有外部协议已通过验收。

桌面设置组件回归运行 `pnpm --filter @omnistream/gui test:unit DesktopSettings.test.tsx`，覆盖浏览器隐藏设置、系统登录项状态读取、局域网保存与重启提示、保存失败保留原值和加载失败重试。桌面 Rust 使用 `cargo check --manifest-path apps/gui/src-tauri/Cargo.toml` 检查，需准备[桌面打包要求的 sidecar](../README.md#core-与-gui)及平台开发库。系统登录后自启、真实局域网连接和正常退出清理需在对应桌面系统验收；组件模拟不能替代这些检查。

旧开发模块的计划、执行准备、自动测试及录屏管线已移除，Core 不再管理 x11box 桌面或提供 `/recordings` 媒体接口。需要界面测试时，由执行 Agent 根据用户请求选择并运行项目自己的验证工具。
