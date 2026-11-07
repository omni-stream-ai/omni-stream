import { expect, test } from '@playwright/test';

test('a real service worker handles push after the page closes with stable event tags', async ({ page, context, baseURL }) => {
  const origin = new URL(baseURL!).origin;
  await context.grantPermissions(['notifications'], { origin });
  // Use a same-origin document without the foreground app or its notification poller.
  await page.goto('/notification-worker.js');
  await page.evaluate(async () => {
    await navigator.serviceWorker.register('/notification-worker.js', { scope: '/' });
    await navigator.serviceWorker.ready;
    await new Promise<void>((resolve, reject) => {
      const request = indexedDB.open('omnistream-push', 1);
      request.onupgradeneeded = () => request.result.createObjectStore('bindings', { keyPath: 'server' });
      request.onerror = () => reject(request.error);
      request.onsuccess = () => {
        const db = request.result;
        const tx = db.transaction('bindings', 'readwrite');
        tx.objectStore('bindings').put({ server: 'https://core.example', id: 'test-device', enabled: true });
        tx.oncomplete = () => { db.close(); resolve(); };
      };
    });
  });
  const inspector = await context.newPage();
  await inspector.goto('/notification-worker.js');
  const session = await context.newCDPSession(inspector);
  let registrationId: string | undefined;
  session.on('ServiceWorker.workerRegistrationUpdated', ({ registrations }) => {
    registrationId = registrations.find(item => item.scopeURL === origin + '/')?.registrationId ?? registrationId;
  });
  await session.send('ServiceWorker.enable');
  await expect.poll(() => registrationId).toBeTruthy();
  const worker = context.serviceWorkers()[0];
  // CI may have no OS notification daemon. Observe the browser display API while
  // retaining the real worker, IndexedDB and browser-delivered PushEvent.
  await worker.evaluate(() => {
    const shown: unknown[] = [];
    Object.assign(self, { recordedPushNotifications: shown });
    self.registration.showNotification = async (title, options) => { shown.push({ title, ...options }); };
  });
  await page.close();
  const data = { title: '任务执行完成', body: 'Push smoke test', path: '/chat/test', server: 'https://core.example', device: 'test-device', tag: 'omnistream-test-1', scope: '["chat","test"]', instance: 'db', id: '1' };
  const deliver = (payload: typeof data) => session.send('ServiceWorker.deliverPushMessage', { origin, registrationId: registrationId!, data: JSON.stringify(payload) });
  const notifications = () => worker.evaluate('self.recordedPushNotifications');
  await deliver(data);
  await expect.poll(notifications).toEqual([{ title: data.title, body: data.body, data: { path: data.path, server: data.server, scope: data.scope, instance: data.instance, id: 1 }, tag: data.tag, icon: '/icons/omnistream-192.png' }]);
  await deliver({ ...data, body: 'Same event updated' });
  await expect.poll(notifications).toHaveLength(2);
  expect((await notifications() as { tag: string }[]).map(item => item.tag)).toEqual([data.tag, data.tag]);
  await inspector.evaluate(async data => {
    await new Promise<void>((resolve, reject) => {
      const request = indexedDB.open('omnistream-push', 2);
      request.onerror = () => reject(request.error);
      request.onsuccess = () => {
        const db = request.result; const tx = db.transaction('reads', 'readwrite');
        tx.objectStore('reads').put({ key: JSON.stringify([data.server, data.scope, data.instance]), cursor: 1 });
        tx.oncomplete = () => { db.close(); resolve(); };
        tx.onerror = () => reject(tx.error);
      };
    });
  }, data);
  await deliver({ ...data, body: 'Delayed already read notification' });
  await inspector.waitForTimeout(200);
  await expect.poll(notifications).toHaveLength(2);
  await deliver({ ...data, id: '2', tag: 'omnistream-test-2' });
  await expect.poll(notifications).toHaveLength(3);
});
