import { expect, test } from '@playwright/test';

for (const width of [390, 1024]) {
  test('native startup connects to a remote Core and restores it at ' + width + 'px', async ({ page }) => {
    await page.setViewportSize({ width, height: 844 });
    await page.addInitScript(() => {
      window.__OMNISTREAM_MOBILE__ = true;
      Object.defineProperty(crypto, 'randomUUID', { value: undefined });
    });
    // Keep this test isolated from any Core running on the developer's computer.
    await page.route('http://127.0.0.1:4317/**', route => route.abort());
    const endpoint = 'http://127.0.0.1:44455';
    let unavailable = true;
    let healthRequests = 0;
    await page.route(endpoint + '/**', async route => {
      const path = new URL(route.request().url()).pathname;
      if (path === '/health') {
        healthRequests++;
        if (unavailable) {
          await route.fulfill({ status: 503, json: { error: 'Core unavailable' } });
          return;
        }
      }
      const resources: Record<string, unknown> = {
        '/health': { ok: true },
        '/projects': [],
        '/agents': [],
        '/conversations/list': { items: [], next_cursor: null, total: 0 },
        '/conversation/detail': { exists: false, history: [], calls: [], settings: { approval_mode: 'default' } },
        '/settings/models': { providers: [], selection: { provider: null, model: null, reasoning: null } },
      };
      if (!(path in resources)) throw new Error('Unexpected mobile test request: ' + path);
      await route.fulfill({ json: resources[path] });
    });
    await page.goto('/');
    await page.addStyleTag({ content: ':root { --safe-top: 47px; --safe-bottom: 34px; }' });
    const connection = page.getByRole('dialog', { name: '连接设置', exact: true });
    await expect(connection).toBeVisible();
    await expect(connection).toContainText('127.0.0.1 指向手机自身');
    await expect(connection.getByLabel('开机自启')).toHaveCount(0);
    await connection.getByRole('textbox', { name: 'Core 地址', exact: true }).fill(endpoint);
    await connection.getByRole('button', { name: '连接', exact: true }).click();
    await expect(page.getByRole('alert')).toContainText('暂时无法连接');
    expect(healthRequests).toBe(1);
    unavailable = false;
    await page.getByRole('button', { name: '重试连接', exact: true }).click();
    await expect(page.getByRole('textbox', { name: '消息', exact: true })).toBeVisible();
    expect(healthRequests).toBe(2);
    expect(await page.evaluate(() => localStorage.getItem('omnistream.url'))).toBe(endpoint);
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    const panel = await page.locator('.chat-panel').boundingBox();
    expect(panel!.y + panel!.height).toBeLessThanOrEqual(844 - 34);
    await page.reload();
    await expect(connection).not.toBeVisible();
    await expect(page.getByRole('textbox', { name: '消息', exact: true })).toBeVisible();
    await page.getByRole('button', { name: '连接设置', exact: true }).click();
    await expect(connection.getByRole('textbox', { name: 'Core 地址', exact: true })).toHaveValue(endpoint);
  });
}

test('browser startup keeps the existing connection flow', async ({ page }) => {
  await page.route('http://127.0.0.1:4317/**', route => route.abort());
  await page.goto('/');
  await expect(page.getByRole('button', { name: '检查地址', exact: true })).toBeVisible();
  await expect(page.getByRole('dialog')).not.toBeVisible();
});
