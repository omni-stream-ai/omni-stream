import { expect, test } from '@playwright/test';
import { spawn, type ChildProcess } from 'node:child_process';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { createServer, type AddressInfo } from 'node:net';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';

let core: ChildProcess | undefined;
let directory: string;
let endpoint: string;

test.beforeAll(async () => {
  directory = await mkdtemp(join(tmpdir(), 'session-list-e2e-'));
  await writeFile(join(directory, 'config.toml'), `[agents.default]\ncommand='unused'\nprotocol='mock'\n`);
  const reservation = createServer();
  await new Promise<void>(done => reservation.listen(0, '127.0.0.1', done));
  const port = (reservation.address() as AddressInfo).port;
  await new Promise<void>(done => reservation.close(() => done()));
  endpoint = `http://127.0.0.1:${port}`;
  core = spawn(resolve('../../target/debug/omnistream'), ['serve', `127.0.0.1:${port}`], { cwd: directory, env: { ...process.env, OMNISTREAM_HOME: directory }, stdio: 'pipe' });
  core.stderr?.on('data', data => process.stderr.write(data));
  await expect.poll(() => fetch(`${endpoint}/health`).then(response => response.ok).catch(() => false)).toBe(true);
  for (let index = 0; index < 45; index++) {
    const response = await fetch(`${endpoint}/conversation/settings`, { method: 'POST', body: JSON.stringify({ conversation: `session-${index}`, settings: { title: `Conversation ${index}`, approval_mode: 'default', archived: index === 25 } }) });
    expect(response.ok).toBe(true);
  }
  const response = await fetch(`${endpoint}/conversation/rename`, { method: 'POST', body: JSON.stringify({ conversation: 'session-0', title: 'Updated oldest' }) });
  expect(response.ok).toBe(true);
});
test.afterAll(async () => {
  if (core && core.exitCode === null) { core.kill(); await new Promise(done => core!.once('exit', done)); }
  await rm(directory, { recursive: true, force: true });
});

for (const width of [1280, 390]) {
  test(`sessions load on sidebar scroll at ${width}px and page failures require manual retry`, async ({ page }) => {
    await page.setViewportSize({ width, height: 844 });
    await page.addInitScript(url => localStorage.setItem('omnistream.url', url), endpoint);
    const cursors: unknown[] = [];
    const historyRequests: string[] = [];
    let failNext = true;
    await page.route(`${endpoint}/conversations/list`, async route => {
      const cursor = route.request().postDataJSON().cursor;
      cursors.push(cursor);
      if (cursor && failNext) {
        failNext = false;
        await route.fulfill({ status: 503, contentType: 'application/json', body: JSON.stringify({ error: 'List unavailable' }) });
      } else await route.continue();
    });
    page.on('request', request => {
      if (request.url().startsWith(`${endpoint}/conversation?`) || request.url() === `${endpoint}/conversation/settings/read`) historyRequests.push(request.url());
    });
    await page.goto('/');
    if (width < 640) await page.getByRole('button', { name: '切换侧栏', exact: true }).click();
    const rows = page.locator('.conversation-list-entry .conversation-item');
    const scroll = page.locator('.sidebar-scroll');
    await expect(rows).toHaveCount(20);
    await expect(rows.first()).toHaveText('Updated oldest');
    expect(cursors).toEqual([null]);
    expect(await scroll.evaluate(element => element.scrollHeight > element.clientHeight)).toBe(true);
    expect(await page.locator('.sidebar').evaluate(element => element.getBoundingClientRect().bottom <= innerHeight + 1)).toBe(true);
    await scroll.evaluate(element => { element.scrollTop = element.scrollHeight; });
    await expect(scroll.getByRole('alert')).toContainText('暂时无法获取会话列表');
    await expect(rows).toHaveCount(20);
    expect(cursors).toHaveLength(2);
    await scroll.getByRole('button', { name: '重试' }).click();
    await expect(rows).toHaveCount(40);
    expect(cursors[2]).toEqual(cursors[1]);
    await expect(scroll.getByRole('alert')).toHaveCount(0);
    await scroll.evaluate(element => { element.scrollTop = element.scrollHeight; });
    await expect(rows).toHaveCount(44);
    expect(cursors).toHaveLength(4);
    const titles = await rows.allTextContents();
    expect(new Set(titles).size).toBe(44);
    expect(titles).not.toContain('Conversation 25');
    expect(historyRequests).toEqual([]);
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    await page.screenshot({ path: `test-results/session-list-${width}.png`, fullPage: true });
  });
}

test('renaming an older session moves it to the top and survives reload', async ({ page }) => {
  await page.addInitScript(url => localStorage.setItem('omnistream.url', url), endpoint);
  await page.goto('/');
  const rows = page.locator('.conversation-list-entry .conversation-item');
  await expect(rows).toHaveCount(20);
  const scroll = page.locator('.sidebar-scroll');
  await scroll.evaluate(element => { element.scrollTop = element.scrollHeight; });
  await expect(rows).toHaveCount(40);
  const item = page.locator('.conversation-list-entry').filter({ has: page.getByRole('button', { name: 'Conversation 10', exact: true }) });
  await item.hover();
  await item.getByRole('button', { name: '会话操作 Conversation 10', exact: true }).click();
  await page.getByRole('menuitem', { name: '重命名', exact: true }).click();
  await page.getByRole('textbox', { name: '会话名称', exact: true }).fill('Moved conversation');
  await page.getByRole('button', { name: '保存名称', exact: true }).click();
  await expect(rows.first()).toHaveText('Moved conversation');
  await page.reload();
  await expect(rows).toHaveCount(20);
  await expect(rows.first()).toHaveText('Moved conversation');
});
