// Shared with notificationReceipts.ts: version 2 retains bindings and adds reads.
async function bindings(operation, store = 'bindings') {
  const db = await new Promise((resolve, reject) => {
    const request = indexedDB.open('omnistream-push', 2);
    request.onupgradeneeded = () => {
      if (!request.result.objectStoreNames.contains('bindings')) request.result.createObjectStore('bindings', { keyPath: 'server' });
      if (!request.result.objectStoreNames.contains('reads')) request.result.createObjectStore('reads', { keyPath: 'key' });
    };
    request.onerror = () => reject(request.error);
    request.onsuccess = () => resolve(request.result);
  });
  try {
    return await new Promise((resolve, reject) => {
      const tx = db.transaction(store, 'readwrite');
      const request = operation(tx.objectStore(store));
      tx.oncomplete = () => resolve(request.result);
      tx.onerror = () => reject(tx.error);
    });
  } finally { db.close(); }
}

const validPath = path => typeof path === 'string' && (path.startsWith('/chat/') || path.startsWith('/slack?channel='));
self.addEventListener('push', event => {
  event.waitUntil((async () => {
    let data;
    try { data = event.data?.json(); } catch { return; }
    if (!data || !validPath(data.path) || typeof data.server !== 'string' || typeof data.title !== 'string' || typeof data.body !== 'string' || typeof data.tag !== 'string') return;
    const binding = await bindings(store => store.get(data.server));
    if (!binding?.enabled || binding.id !== data.device) return;
    if (typeof data.scope !== 'string' || typeof data.instance !== 'string' || !Number.isSafeInteger(Number(data.id))) return;
    const key = JSON.stringify([data.server, data.scope, data.instance]);
    const display = async () => {
      const read = await bindings(store => store.get(key), 'reads');
      if (read && read.cursor >= Number(data.id)) return;
      await self.registration.showNotification(data.title, { body: data.body, tag: data.tag,
        icon: '/icons/omnistream-192.png', data: { path: data.path, server: data.server, scope: data.scope, instance: data.instance, id: Number(data.id) } });
    };
    if (self.navigator.locks) await self.navigator.locks.request('omnistream.notifications.read:' + key, display);
    else await display();
  })());
});

self.addEventListener('pushsubscriptionchange', event => {
  event.waitUntil((async () => {
    const active = (await bindings(store => store.getAll())).filter(binding => binding.enabled);
    if (!active.length) return;
    try {
      const subscription = event.newSubscription ?? await self.registration.pushManager.getSubscription()
        ?? await self.registration.pushManager.subscribe(event.oldSubscription.options);
      for (const binding of active) {
        const response = await fetch(binding.server + '/push/devices/register', { method: 'POST', headers: { 'content-type': 'application/json', ...(binding.clientToken ? { authorization: 'Bearer ' + binding.clientToken } : {}) },
          body: JSON.stringify({ id: binding.id, secret: binding.secret, server: binding.server, locale: binding.locale,
            transport: 'web', target: subscription.toJSON(), activate: false }), redirect: 'error', signal: AbortSignal.timeout(20000) });
        if (!response.ok) throw new Error('subscription_refresh_failed');
      }
    } catch {
      // No background retry loop. Settings can refresh the subscription explicitly.
      for (const binding of active) await bindings(store => store.put({ ...binding, refresh_error: true }));
    }
  })());
});

self.addEventListener('notificationclick', event => {
  const path = event.notification.data?.path;
  if (!validPath(path)) return;
  event.notification.close();
  event.waitUntil((async () => {
    // Cold launch waits until the app can verify the saved Core before routing.
    const target = new URL('/#omnistream-notification=' + encodeURIComponent(JSON.stringify({ path, server: event.notification.data?.server })), self.location.origin).href;
    const windows = await self.clients.matchAll({ type: 'window', includeUncontrolled: true });
    const client = windows.find(window => new URL(window.url).origin === self.location.origin);
    if (client) {
      client.postMessage({ type: 'omnistream-notification', path, server: event.notification.data?.server });
      await client.focus();
    }
    else await self.clients.openWindow(target);
  })());
});
