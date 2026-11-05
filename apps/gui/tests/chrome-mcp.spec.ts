import { test, expect } from '@playwright/test';
import { createServer } from 'node:http';
import { chmod, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { spawn, type ChildProcess } from 'node:child_process';

let directory: string;
let core: ChildProcess;
let endpoint: string;
let address: string;
async function startCore() {
  core = spawn(resolve('../../target/debug/omnistream'), ['serve', address], { cwd: directory, env: { ...process.env, OMNISTREAM_HOME: directory, OMNISTREAM_BROWSER_MCP_TEST_COMMAND: join(directory, 'mcp.py') }, stdio: 'ignore' });
  await expect.poll(() => fetch(endpoint + '/health').then(response => response.ok).catch(() => false)).toBe(true);
}
async function stopCore() {
  if (core && core.exitCode === null && core.signalCode === null) await new Promise(done => { core.once('exit', done); core.kill(); });
}
test.beforeAll(async () => {
  directory = await mkdtemp(join(tmpdir(), 'chrome-mcp-e2e-'));
  await writeFile(join(directory, 'mcp.py'), await readFile(resolve('../../crates/omnistream/tests/fixtures/browser_mcp.py')));
  await chmod(join(directory, 'mcp.py'), 0o755);
  await writeFile(join(directory, 'agent.py'), await readFile(resolve('../../crates/omnistream/tests/fixtures/browser_agent.py')));
  await writeFile(join(directory, 'config.toml'), '[agents.codex]\ncommand="python3"\nprotocol="app-server"\nargs=[' + JSON.stringify(join(directory, 'agent.py')) + ',"app-server"]\n');
  const listener = createServer();
  await new Promise<void>(done => listener.listen(0, '127.0.0.1', done));
  address = '127.0.0.1:' + (listener.address() as { port: number }).port;
  await new Promise<void>(done => listener.close(() => done()));
  endpoint = 'http://' + address;
  await startCore();
});
test.afterAll(async () => { await stopCore(); await rm(directory, { recursive: true, force: true }); });

test('installs Chrome, checks connection failures, equips the setup Agent, and persists disabling on mobile', async ({ page }, testInfo) => {
  test.setTimeout(60000);
  await page.addInitScript(url => { localStorage.setItem('omnistream.url', url); localStorage.setItem('omnistream.locale', 'zh-CN'); }, endpoint);
  await page.goto('/chat/chrome-entry');
  await page.getByRole('button', { name: '插件市场', exact: true }).click();
  const dialog = page.getByRole('dialog', { name: '插件市场' });
  const chrome = dialog.getByRole('article', { name: 'Chrome DevTools MCP' });
  await chrome.getByRole('button', { name: '下载', exact: true }).click();
  await chrome.getByRole('button', { name: '配置与使用' }).click();
  await expect(dialog.getByLabel('访问令牌', { exact: true })).toHaveCount(0);
  await expect(dialog.getByRole('button', { name: '检查连接' })).toBeDisabled();
  await dialog.getByLabel('允许 Agent 使用 Chrome 浏览器').check();
  await dialog.getByRole('button', { name: '保存', exact: true }).click();
  await expect(dialog.getByText('浏览器工具设置已保存')).toBeVisible();
  await writeFile(join(directory, 'browser-mode'), 'disconnected');
  await dialog.getByRole('button', { name: '检查连接' }).click();
  await expect(dialog.getByRole('alert')).toContainText('chrome://inspect');
  await expect(dialog).not.toContainText('private-page-fixture');
  await writeFile(join(directory, 'browser-mode'), 'ok');
  await dialog.getByRole('button', { name: '检查连接' }).click();
  await expect(dialog.getByText('Chrome 浏览器连接正常')).toBeVisible();
  await dialog.getByRole('button', { name: '返回插件市场' }).click();
  await dialog.getByRole('article', { name: 'Slack', exact: true }).getByRole('button', { name: '下载', exact: true }).click();
  await dialog.getByRole('article', { name: 'Slack', exact: true }).getByRole('button', { name: '配置与使用' }).click();
  await dialog.getByLabel('Slack 连接方式').selectOption('slack_user');
  await dialog.getByRole('button', { name: '让 Agent 协助获取令牌' }).click();
  await expect(dialog).toHaveCount(0);
  await expect(page.locator('.agent-call')).toContainText('Browser MCP enabled');
  expect(await readFile(join(directory, 'agent-args.jsonl'), 'utf8')).toContain('mcp_servers.omnistream_plugins=');
  expect(await readFile(join(directory, 'agent-turns.jsonl'), 'utf8')).toContain('list_pages');
  await page.getByRole('button', { name: '插件市场', exact: true }).click();
  await chrome.getByRole('button', { name: '配置与使用' }).click();
  await dialog.getByLabel('允许 Agent 使用 Chrome 浏览器').uncheck();
  await dialog.getByRole('button', { name: '保存', exact: true }).click();
  await expect(dialog.getByText('浏览器工具设置已保存')).toBeVisible();
  await stopCore(); await startCore(); await page.reload();
  await page.setViewportSize({ width: 390, height: 844 });
  await page.getByRole('button', { name: '切换侧栏' }).click();
  await page.getByRole('button', { name: '插件市场', exact: true }).click();
  await chrome.getByRole('button', { name: '配置与使用' }).click();
  await expect(dialog.getByLabel('允许 Agent 使用 Chrome 浏览器')).not.toBeChecked();
  await expect(dialog.getByRole('button', { name: '检查连接' })).toBeDisabled();
  expect(await dialog.evaluate(element => element.scrollWidth <= element.clientWidth)).toBe(true);
  await dialog.screenshot({ path: testInfo.outputPath('chrome-mcp-mobile.png') });
});
