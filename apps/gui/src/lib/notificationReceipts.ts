import type { NotificationSnapshot, SystemNotification } from './api';
import { translate } from '../i18n';

export const receiptKey = (server: string, scope: string, instance: string) => JSON.stringify([server, scope, instance]);
export const readStoragePrefix = 'omnistream.notifications.read:';
export const notificationScope = (item: SystemNotification) => item.scope ?? JSON.stringify(item.channel
  ? ['slack', item.team ?? '', item.owner ?? '', item.channel] : ['chat', item.conversation]);
export function isNotificationRead(server: string, instance: string, item: SystemNotification) {
  const cursor = localStorage.getItem(readStoragePrefix + receiptKey(server, notificationScope(item), instance));
  return cursor !== null && Number(cursor) >= item.id;
}
export function saveReadReceipt(server: string, snapshot: NotificationSnapshot): NotificationSnapshot {
  const key = readStoragePrefix + receiptKey(server, snapshot.scope, snapshot.instance);
  const cursor = Math.max(snapshot.cursor, Number(localStorage.getItem(key) ?? '-1'));
  localStorage.setItem(key, String(cursor));
  return { ...snapshot, cursor };
}

// Version 2 is also used by notification-worker.js; retain existing bindings.
export async function putPushRecord(store: 'bindings' | 'reads', value: object) {
  await new Promise<void>((resolve, reject) => {
    const request = indexedDB.open('omnistream-push', 2);
    request.onupgradeneeded = () => {
      if (!request.result.objectStoreNames.contains('bindings')) request.result.createObjectStore('bindings', { keyPath: 'server' });
      if (!request.result.objectStoreNames.contains('reads')) request.result.createObjectStore('reads', { keyPath: 'key' });
    };
    request.onerror = () => reject(new Error(translate('push.storageFailed')));
    request.onsuccess = () => {
      const db = request.result;
      const tx = db.transaction(store, 'readwrite');
      const target = tx.objectStore(store);
      if (store === 'reads') {
        const read = value as { key: string; cursor: number };
        const previous = target.get(read.key);
        previous.onsuccess = () => target.put({ ...value, cursor: Math.max(read.cursor, previous.result?.cursor ?? -1) });
      } else target.put(value);
      tx.oncomplete = () => { db.close(); resolve(); };
      tx.onerror = () => { db.close(); reject(new Error(translate('push.storageFailed'))); };
      tx.onabort = tx.onerror;
    };
  });
}
