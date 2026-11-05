import { test, expect } from '@playwright/test';
import { createServer } from 'node:http';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { spawn, type ChildProcess } from 'node:child_process';

let core: ChildProcess;
let directory: string;
let endpoint: string;
let address: string;
async function startCore() {
  core = spawn(resolve('../../target/debug/omnistream'), ['serve', address], { cwd: directory, env: { ...process.env, OMNISTREAM_HOME: directory }, stdio: 'ignore' });
  await expect.poll(() => fetch(endpoint + '/health').then(response => response.ok).catch(() => false)).toBe(true);
}
async function stopCore() {
  if (core && core.exitCode === null && core.signalCode === null) { await new Promise(done => { core.once('exit', done); core.kill(); }); }
}
test.beforeAll(async () => {
  directory = await mkdtemp(join(tmpdir(), 'plugin-setup-e2e-'));
  const listener = createServer();
  await new Promise<void>(done => listener.listen(0, '127.0.0.1', done));
  address = '127.0.0.1:' + (listener.address() as { port: number }).port;
  await new Promise<void>(done => listener.close(() => done()));
  endpoint = 'http://' + address;
});
test.afterEach(stopCore);
test.afterAll(async () => { await stopCore(); await rm(directory, { recursive: true, force: true }); });

test('starts assistance with safe Slack choices and returns to configuration after refresh and restart', async ({ page }, testInfo) => {
  await writeFile(join(directory, 'agent.sh'), 'printf \'%s\n\' "$2" >> "$1"\nprintf \'请完成登录，然后返回配置验证保存。\'\n');
  await writeFile(join(directory, 'config.toml'), '[agents.helper]\ncommand="sh"\nprotocol="cli"\nargs=[' + JSON.stringify(join(directory, 'agent.sh')) + ',' + JSON.stringify(join(directory, 'prompts.txt')) + ']\n');
  await startCore();
  await fetch(endpoint + '/plugins/slack/install', { method: 'POST' });
  await page.addInitScript(url => { localStorage.setItem('omnistream.url', url); localStorage.setItem('omnistream.locale', 'zh-CN'); }, endpoint);
  await page.goto('/chat/setup-entry');
  await page.getByRole('button', { name: '插件市场', exact: true }).click();
  const dialog = page.getByRole('dialog', { name: '插件市场' });
  await dialog.getByRole('article', { name: 'Slack', exact: true }).getByRole('button', { name: '配置与使用' }).click();
  const manual = dialog.locator('details.plugin-manual-setup');
  await expect(manual).not.toHaveAttribute('open');
  await dialog.getByLabel('Slack 连接方式').selectOption('slack_user');
  await dialog.getByLabel('持续接收新消息').check();
  await dialog.getByLabel('监听范围').selectOption('all');
  await dialog.getByLabel('允许写入此应用').check();
  await dialog.getByLabel('访问令牌', { exact: true }).fill('xoxp-unsent-private-draft');
  await dialog.getByLabel('App-level Token', { exact: true }).fill('xapp-unsent-private-draft');
  let setupBody: unknown;
  page.on('request', request => { if (request.url() === endpoint + '/plugins/slack/setup') setupBody = request.postDataJSON(); });
  await dialog.getByRole('button', { name: '让 Agent 协助获取令牌' }).click();
  await expect(dialog).toHaveCount(0);
  await expect(page).toHaveURL(/plugin-setup/);
  const card = page.getByRole('complementary', { name: 'Slack 配置协助' });
  await expect(card).toContainText('所有已加入的频道、私信和群聊');
  await expect(card).toContainText('申请写入权限');
  expect(setupBody).toEqual({ request_id: expect.any(String), options: { auth_mode: 'slack_user', listen: true, listen_scope: 'all', enabled: true, allow_writes: true, locale: 'zh-CN' } });
  expect(JSON.stringify(setupBody)).not.toContain('private-draft');
  await expect(page.locator('.agent-call')).toContainText('请完成登录，然后返回配置验证保存。');
  expect(await readFile(join(directory, 'prompts.txt'), 'utf8')).toContain('User Token Scopes');
  expect(await readFile(join(directory, 'prompts.txt'), 'utf8')).not.toContain('private-draft');
  await page.getByLabel('消息', { exact: true }).fill('已完成登录，请继续');
  await page.getByLabel('消息', { exact: true }).press('Enter');
  await expect(page.locator('.agent-call')).toHaveCount(2);
  await expect(page.locator('.agent-call').last()).toContainText('请完成登录，然后返回配置验证保存。');
  await stopCore(); await startCore(); await page.reload();
  await expect(card).toBeVisible();
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(card.getByRole('button', { name: '返回配置' })).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.screenshot({ path: testInfo.outputPath('plugin-setup-mobile.png') });
  await card.getByRole('button', { name: '返回配置' }).click();
  await expect(dialog.getByLabel('Slack 连接方式')).toHaveValue('slack_user');
  await expect(dialog.getByLabel('监听范围')).toHaveValue('all');
  await expect(dialog.getByLabel('允许写入此应用')).toBeChecked();
  await expect(dialog.getByLabel('访问令牌', { exact: true })).toHaveValue('');
  await expect(dialog.getByLabel('App-level Token', { exact: true })).toHaveValue('');
  await expect(manual).not.toHaveAttribute('open');
  await manual.getByText('手动配置说明', { exact: true }).click();
  await expect(manual.getByRole('link', { name: 'Socket Mode 配置说明' })).toBeVisible();
  expect(await dialog.evaluate(element => element.scrollWidth <= element.clientWidth)).toBe(true);
});

test('missing Agent leaves the form and manual instructions available without a fake conversation', async ({ page }) => {
  await writeFile(join(directory, 'config.toml'), '');
  await startCore();
  await fetch(endpoint + '/plugins/github/install', { method: 'POST' });
  await page.addInitScript(url => { localStorage.setItem('omnistream.url', url); localStorage.setItem('omnistream.locale', 'en-US'); }, endpoint);
  await page.goto('/chat/no-setup-agent');
  await page.getByRole('button', { name: 'Plugin marketplace', exact: true }).click();
  const dialog = page.getByRole('dialog', { name: 'Plugin marketplace' });
  await dialog.getByRole('article', { name: 'GitHub', exact: true }).getByRole('button', { name: 'Configure and use' }).click();
  await dialog.getByLabel('Access token', { exact: true }).fill('private-draft');
  await dialog.getByRole('button', { name: 'Get token with Agent assistance' }).click();
  await expect(dialog.getByRole('alert')).toContainText('config.toml');
  await expect(page).toHaveURL(/no-setup-agent$/);
  await expect(dialog.getByLabel('Access token', { exact: true })).toHaveValue('private-draft');
  await dialog.getByText('Manual setup instructions', { exact: true }).click();
  await expect(dialog.getByRole('link', { name: 'How to get a token' })).toBeVisible();
  await expect(dialog.getByRole('button', { name: 'Get token with Agent assistance' })).toBeEnabled();
});
