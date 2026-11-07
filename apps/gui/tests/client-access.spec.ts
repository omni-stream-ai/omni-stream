import { expect, test, type BrowserContext } from '@playwright/test';
import { spawn, execFileSync, type ChildProcess } from 'node:child_process';
import { mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { resolve } from 'node:path';
import { createServer } from 'node:net';

let core: ChildProcess;
let home: string;
let endpoint: string;
const binary = resolve('../../target/debug/omnistream');
test.beforeAll(async () => {
  home = mkdtempSync(resolve(tmpdir(), 'omnistream-access-e2e-'));
  writeFileSync(resolve(home, 'config.toml'), '[access]\nrequire_auth=true\n[push]\nenabled=false\n');
  const port = await new Promise<number>(done => {
    const listener = createServer();
    listener.listen(0, '127.0.0.1', () => { const address = listener.address() as { port: number }; listener.close(() => done(address.port)); });
  });
  endpoint = 'http://127.0.0.1:' + port;
  const env = { ...process.env, OMNISTREAM_HOME: home };
  delete env.OMNISTREAM_DESKTOP_TOKEN;
  core = spawn(binary, ['serve', '127.0.0.1:' + port], { cwd: home, env, stdio: 'ignore' });
  await expect.poll(async () => { try { return (await fetch(endpoint + '/health')).ok; } catch { return false; } }, { timeout: 10000 }).toBe(true);
});
test.afterAll(async () => {
  if (core?.exitCode === null) { const stopped = new Promise(done => core.once('exit', done)); core.kill(); await stopped; }
  if (home) rmSync(home, { recursive: true, force: true });
});

async function configure(context: BrowserContext, locale = 'zh-CN') {
  await context.addInitScript(({ endpoint, locale }) => { localStorage.setItem('omnistream.url', endpoint); localStorage.setItem('omnistream.locale', locale); }, { endpoint, locale });
}
async function noOverflow(page: import('@playwright/test').Page) {
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  const header = await page.locator('.header-tools').boundingBox();
  expect(header!.x + header!.width).toBeLessThanOrEqual(page.viewportSize()!.width);
}

test('first-device bootstrap, automatic cross-client approval, rejection, cancellation and revocation', async ({ browser }) => {
  test.setTimeout(90000);
  const first = await browser.newContext(); const second = await browser.newContext({ viewport: { width: 390, height: 844 } });
  const third = await browser.newContext();
  try {
    await configure(first); await configure(second, 'en-US'); await configure(third);
    const a = await first.newPage(); const b = await second.newPage(); const c = await third.newPage();
    await a.goto('/');
    await a.getByLabel('客户端名称', { exact: true }).fill('Desktop A');
    await a.getByRole('button', { name: '申请授权', exact: true }).click();
    await expect(a.getByLabel('核对码', { exact: true })).toBeVisible();
    const initial = JSON.parse(execFileSync(binary, ['client-auth', 'list'], { env: { ...process.env, OMNISTREAM_HOME: home }, encoding: 'utf8' })).pending[0];
    execFileSync(binary, ['client-auth', 'approve', initial.id, initial.code], { env: { ...process.env, OMNISTREAM_HOME: home } });
    await expect(a.getByRole('textbox', { name: '消息', exact: true })).toBeVisible({ timeout: 10000 });
    await b.goto('/');
    await noOverflow(b);
    await b.getByLabel('Client name', { exact: true }).fill('Phone B');
    await b.getByRole('button', { name: 'Request access', exact: true }).click();
    const code = await b.getByLabel('Verification code', { exact: true }).textContent();
    const modal = a.getByRole('dialog', { name: '客户端授权', exact: true });
    await expect(modal).toBeVisible({ timeout: 10000 });
    await expect(modal).toContainText('Phone B'); await expect(modal).toContainText(code!);
    await modal.getByRole('button', { name: '批准此客户端', exact: true }).click();
    await expect(b.getByRole('textbox', { name: 'Message', exact: true })).toBeVisible({ timeout: 10000 });
    await noOverflow(b);
    await b.reload();
    await expect(b.getByRole('textbox', { name: 'Message', exact: true })).toBeVisible();
    await b.getByLabel('Language', { exact: true }).selectOption('zh-CN');
    await noOverflow(b);
    await b.getByLabel('语言', { exact: true }).selectOption('en-US');
    await c.goto('/');
    await c.getByLabel('客户端名称', { exact: true }).fill('Tablet C');
    await c.getByRole('button', { name: '申请授权', exact: true }).click();
    await expect(modal).toContainText('Tablet C', { timeout: 10000 });
    const mobileModal = b.getByRole('dialog', { name: 'Client access', exact: true });
    await expect(mobileModal).toContainText('Tablet C', { timeout: 10000 });
    await noOverflow(b);
    await mobileModal.getByRole('button', { name: 'Reject request', exact: true }).click();
    await expect(c.getByRole('status')).toContainText('已被拒绝', { timeout: 10000 });
    await expect(modal).not.toBeVisible({ timeout: 10000 });
    await c.getByRole('button', { name: '申请授权', exact: true }).click();
    await expect(c.getByRole('button', { name: '取消申请', exact: true })).toBeVisible();
    await c.getByRole('button', { name: '取消申请', exact: true }).click();
    await expect(c.getByRole('status')).toContainText('已取消');
    await expect(mobileModal).not.toBeVisible({ timeout: 10000 });
    await a.getByRole('button', { name: '客户端授权', exact: true }).click();
    await modal.getByRole('button', { name: '已授权客户端', exact: true }).click();
    const phone = modal.locator('article').filter({ has: a.getByRole('heading', { name: 'Phone B', exact: true }) });
    await phone.getByRole('button', { name: '撤销授权', exact: true }).click();
    await phone.getByRole('button', { name: '确认撤销', exact: true }).click();
    await expect(b.getByRole('heading', { name: 'Client authorization required', exact: true })).toBeVisible({ timeout: 10000 });
    await expect(b.getByRole('textbox', { name: 'Message', exact: true })).not.toBeVisible();
  } finally { await first.close(); await second.close(); await third.close(); }
});
