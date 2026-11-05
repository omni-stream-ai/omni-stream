import { test, expect } from '@playwright/test';
import { createServer } from 'node:http';
import { mkdtemp, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { resolve, join } from 'node:path';
import { spawn, type ChildProcess } from 'node:child_process';

let core: ChildProcess;
let directory: string;
let endpoint: string;
let agentId: number;
test.beforeAll(async () => {
  directory = await mkdtemp(join(tmpdir(), 'agent-memory-e2e-'));
  await writeFile(join(directory, 'config.toml'), "[agents.default]\ncommand='fixture'\nprotocol='mock'\n");
  const listener = createServer();
  await new Promise<void>(done => listener.listen(0, '127.0.0.1', done));
  const address = listener.address() as { port: number };
  await new Promise<void>(done => listener.close(() => done()));
  endpoint = 'http://127.0.0.1:' + address.port;
  core = spawn(resolve('../../target/debug/omnistream'), ['serve', '127.0.0.1:' + address.port], { cwd: directory, env: { ...process.env, OMNISTREAM_HOME: directory }, stdio: 'pipe' });
  core.stderr?.on('data', data => process.stderr.write(data));
  await expect.poll(async () => fetch(endpoint + '/health').then(response => response.ok).catch(() => false)).toBe(true);
  const response = await fetch(endpoint + '/agents', { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ name: '工作助手', instructions: 'Check sources', engine: 'default', plugins: [] }) });
  expect(response.ok).toBe(true);
  agentId = (await response.json()).id;
  const saved = await fetch(`${endpoint}/agents/${agentId}/memory`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ operation: 'upsert', key: '日报格式'.repeat(20), content: '简洁并附来源'.repeat(300) }) });
  expect(saved.ok).toBe(true);
});
test.afterAll(async () => {
  if (core && core.exitCode === null) { core.kill(); await new Promise(done => core.once('exit', done)); }
  await rm(directory, { recursive: true, force: true });
});

test('inspect, refresh and forget persistent Agent memory on mobile', async ({ page }) => {
  await page.addInitScript(url => { localStorage.setItem('omnistream.url', url); localStorage.setItem('omnistream.locale', 'zh-CN'); }, endpoint);
  await page.goto('/chat/memory');
  await page.getByRole('button', { name: 'Agent 管理', exact: true }).click();
  const dialog = page.getByRole('dialog', { name: 'Agent 管理' });
  await dialog.getByText('长期记忆', { exact: true }).click();
  await expect(dialog.getByText('简洁并附来源'.repeat(300), { exact: true })).toBeVisible();
  await page.setViewportSize({ width: 390, height: 844 });
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await dialog.getByRole('button', { name: '刷新记忆', exact: true }).click();
  await dialog.getByRole('button', { name: '删除记忆 ' + '日报格式'.repeat(20), exact: true }).click();
  await expect(dialog.getByText('还没有长期记忆', { exact: true })).toBeVisible();
  await expect.poll(async () => fetch(`${endpoint}/agents/${agentId}/memory`).then(response => response.json())).toEqual([]);
  await page.reload();
  await page.getByRole('button', { name: '切换侧栏' }).click();
  await page.getByRole('button', { name: 'Agent 管理', exact: true }).click();
  await dialog.getByText('长期记忆', { exact: true }).click();
  await expect(dialog.getByText('还没有长期记忆', { exact: true })).toBeVisible();
});
