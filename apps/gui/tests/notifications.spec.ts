import { expect, test } from '@playwright/test';
import { spawn, type ChildProcess } from 'node:child_process';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { createServer, type AddressInfo } from 'node:net';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';

let core: ChildProcess | undefined;
let directory: string;
let endpoint: string;

async function post(path: string, input: unknown) {
  const response = await fetch(endpoint + path, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(input) });
  expect(response.ok).toBe(true);
  return response.json();
}
async function approval(conversation: string) {
  await post('/conversation/settings', { conversation, settings: { title: conversation, approval_mode: 'default' } });
  return post('/approvals', { action: 'plugin.invoke', payload: { conversation, secret: 'private-approval-argument' } });
}

test.beforeAll(async () => {
  directory = await mkdtemp(join(tmpdir(), 'notifications-e2e-'));
  await writeFile(join(directory, 'config.toml'), ['[push]', 'enabled=false', '[agents.default]', 'command="fixture"', 'protocol="mock"'].join('\n'));
  const reservation = createServer();
  await new Promise<void>(done => reservation.listen(0, '127.0.0.1', done));
  const port = (reservation.address() as AddressInfo).port;
  await new Promise<void>(done => reservation.close(() => done()));
  endpoint = 'http://127.0.0.1:' + port;
  core = spawn(resolve('../../target/debug/omnistream'), ['serve', '127.0.0.1:' + port], { cwd: directory, env: { ...process.env, OMNISTREAM_HOME: directory }, stdio: 'pipe' });
  core.stderr?.on('data', data => process.stderr.write(data));
  await expect.poll(() => fetch(endpoint + '/health').then(response => response.ok).catch(() => false)).toBe(true);
});
test.afterAll(async () => {
  if (core && core.exitCode === null) { core.kill(); await new Promise(done => core!.once('exit', done)); }
  await rm(directory, { recursive: true, force: true });
});

test.beforeEach(async ({ page, context }) => {
  await context.grantPermissions(['notifications']);
  await page.addInitScript(url => {
    localStorage.setItem('omnistream.url', url);
    const original = window.Notification;
    const recorded: { title: string; options: NotificationOptions; onclick?: () => void }[] = [];
    Object.assign(window, { recordedNotifications: recorded });
    Object.defineProperty(window, 'Notification', { configurable: true, value: class {
      static get permission() { return original.permission; }
      static requestPermission() { return original.requestPermission(); }
      onclick?: () => void;
      onclose?: () => void;
      closed = false;
      constructor(public title: string, public options: NotificationOptions) { recorded.push(this); }
      close() { this.closed = true; this.onclose?.(); }
    } });
  }, endpoint);
});

test('viewing loaded content clears its notification while hidden content remains unread', async ({ page }) => {
  await page.goto('/chat/elsewhere');
  await page.getByRole('button', { name: '通知设置', exact: true }).click();
  await page.getByRole('checkbox', { name: '启用系统通知' }).click();
  await expect(page.getByRole('checkbox')).toBeChecked();
  await page.getByRole('dialog', { name: '通知设置' }).getByRole('button', { name: '关闭', exact: true }).click();
  const first = await approval('read-target');
  const records = () => page.evaluate(() => (window as unknown as { recordedNotifications: { closed: boolean }[] }).recordedNotifications.map(item => item.closed));
  await expect.poll(records).toEqual([false]);
  await page.evaluate(() => { history.pushState(null, '', '/chat/read-target'); window.dispatchEvent(new PopStateEvent('popstate')); });
  await expect.poll(records).toEqual([true]);
  const pending = await fetch(endpoint + '/approvals').then(response => response.json());
  expect(pending.find((item: { id: number }) => item.id === first.id)?.status).toBe('pending');
  await page.evaluate(() => { Object.defineProperty(document, 'visibilityState', { configurable: true, value: 'hidden' }); document.dispatchEvent(new Event('visibilitychange')); });
  await approval('read-target');
  await expect.poll(records).toEqual([true, false]);
  await page.evaluate(() => { Object.defineProperty(document, 'visibilityState', { configurable: true, value: 'visible' }); document.dispatchEvent(new Event('visibilitychange')); });
  await expect.poll(records).toEqual([true, true]);
});

test('opts in, skips history, delivers approval and task results once, and opens their conversations', async ({ page }) => {
  await approval('old-notification');
  await page.goto('/');
  await page.getByRole('button', { name: '通知设置', exact: true }).click();
  const toggle = page.getByRole('checkbox', { name: '启用系统通知' });
  await expect(toggle).not.toBeChecked();
  const baseline = page.waitForResponse(response => response.url() === endpoint + '/notifications/poll' && response.request().postDataJSON().after === null);
  await toggle.click();
  await expect(toggle).toBeChecked();
  await baseline;
  const count = () => page.evaluate(() => (window as unknown as { recordedNotifications: unknown[] }).recordedNotifications.length);
  expect(await count()).toBe(0);
  await approval('new-notification');
  await expect.poll(count).toBe(1);
  const notification = await page.evaluate(() => (window as unknown as { recordedNotifications: unknown[] }).recordedNotifications[0]);
  expect(JSON.stringify(notification)).not.toContain('private-approval-argument');
  await page.evaluate(() => (window as unknown as { recordedNotifications: { onclick?: () => void }[] }).recordedNotifications[0].onclick?.());
  await expect(page).toHaveURL(/chat\/new-notification$/);
  const task = await post('/automations', { title: 'Notification task', goal: 'Return a short result', schedule: { kind: 'interval', seconds: 86400 }, conversation: 'notification-result' });
  await post('/automations/' + task.id + '/run', {});
  await expect.poll(count).toBe(2);
  const result = await page.evaluate(() => (window as unknown as { recordedNotifications: { title: string }[] }).recordedNotifications[1]);
  expect(result.title).toBe('任务执行完成');
  await page.evaluate(() => (window as unknown as { recordedNotifications: { onclick?: () => void }[] }).recordedNotifications[1].onclick?.());
  await expect(page).toHaveURL(/chat\/notification-result$/);
  const reloaded = page.waitForResponse(response => response.url() === endpoint + '/notifications/poll');
  await page.reload();
  await reloaded;
  expect(await count()).toBe(0);
});

test('failed polling pauses until manual retry and settings fit on mobile', async ({ page }, testInfo) => {
  await page.setViewportSize({ width: 390, height: 844 });
  let requests = 0;
  await page.route(endpoint + '/notifications/poll', async route => {
    requests++;
    if (requests === 2) await route.fulfill({ status: 503, contentType: 'application/json', body: JSON.stringify({ error: 'Notifications unavailable' }) });
    else await route.continue();
  });
  await page.goto('/');
  await page.getByRole('button', { name: '通知设置', exact: true }).click();
  await page.getByRole('checkbox', { name: '启用系统通知' }).click();
  await expect(page.getByRole('checkbox', { name: '启用系统通知' })).toBeChecked();
  const error = page.getByRole('alert');
  await expect(error).toContainText('Notifications unavailable');
  const current = requests;
  await page.waitForTimeout(3500);
  expect(requests).toBe(current);
  await error.getByRole('button', { name: '重试', exact: true }).click();
  await expect(error).toHaveCount(0);
  await expect.poll(() => requests).toBe(current + 1);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.screenshot({ path: testInfo.outputPath('notifications-mobile.png'), fullPage: true });
  await page.getByRole('checkbox', { name: '启用系统通知' }).click();
  await expect(page.getByRole('checkbox', { name: '启用系统通知' })).not.toBeChecked();
  const stopped = requests;
  await page.waitForTimeout(3500);
  expect(requests).toBe(stopped);
});
