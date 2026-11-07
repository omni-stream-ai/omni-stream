import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { translate } from '../i18n';
import type { NotificationSnapshot, SystemNotification } from './api';
import { isNotificationRead, notificationScope, receiptKey } from './notificationReceipts';

const shown = new Set<{ notification: Notification; server: string; instance: string; scope: string; id: number }>();

export type NotificationPermissionState = NotificationPermission | 'unsupported';

export async function notificationPermission(request = false): Promise<NotificationPermissionState> {
  if (isTauri()) return invoke<NotificationPermission>('system_notification_permission', { request });
  if (!window.isSecureContext || !('Notification' in window)) return 'unsupported';
  return request ? Notification.requestPermission() : Notification.permission;
}

export function notificationPath(item: SystemNotification) {
  return item.channel ? `/slack?channel=${encodeURIComponent(item.channel)}` : `/chat/${encodeURIComponent(item.conversation!)}`;
}

export async function showSystemNotification(item: SystemNotification, onOpen: (path: string) => void, server: string, instance = '') {
  if (isNotificationRead(server, instance, item)) return;
  const title = translate(`notifications.${item.kind}`);
  const body = item.title || translate('notifications.openDetails');
  const path = notificationPath(item);
  if (isTauri()) {
    await invoke('show_system_notification', { id: item.id % 2147483647 || 1, title, body, path, server, scope: notificationScope(item), instance, eventId: item.id });
    return;
  }
  if (Notification.permission !== 'granted') throw new Error(translate('notifications.permissionLost'));
  const registration = await navigator.serviceWorker?.getRegistration();
  if (isNotificationRead(server, instance, item)) return;
  if (registration?.active) {
    await registration.showNotification(title, { body, tag: `omnistream-${instance}-${item.id}`, icon: '/icons/omnistream-192.png', data: { path, server, scope: notificationScope(item), instance, id: item.id } });
    return;
  }
  const notification = new Notification(title, { body, tag: `omnistream-${item.id}`, icon: '/icons/omnistream-192.png' });
  const record = { notification, server, instance, scope: notificationScope(item), id: item.id };
  shown.add(record);
  notification.onclose = () => shown.delete(record);
  notification.onclick = () => { window.focus(); onOpen(path); notification.close(); };
}

export async function dismissSystemNotifications(server: string, snapshot: NotificationSnapshot) {
  if (isTauri()) {
    await invoke('dismiss_system_notifications', { server, ...snapshot });
    return;
  }
  for (const item of shown) {
    if (item.server === server && item.instance === snapshot.instance && item.scope === snapshot.scope && item.id <= snapshot.cursor) {
      item.notification.close(); shown.delete(item);
    }
  }
  const registration = await navigator.serviceWorker?.getRegistration();
  for (const notification of await registration?.getNotifications?.() ?? []) {
    const data = notification.data;
    if (data?.server === server && data.instance === snapshot.instance && data.scope === snapshot.scope && Number(data.id) <= snapshot.cursor) notification.close();
  }
}

export async function listenForNotificationClicks(onOpen: (path: string, server: string) => void) {
  const handle = ({ path, server }: { path?: unknown; server?: unknown }) => {
    if (typeof server === 'string' && typeof path === 'string' && (path.startsWith('/chat/') || path.startsWith('/slack?channel='))) onOpen(path, server);
  };
  if (isTauri()) {
    const stop = await listen<{ path: string; server: string }>('omnistream-notification', event => {
      handle(event.payload);
      // A live listener already routed this click; do not replay it on a reload.
      if (window.__OMNISTREAM_ANDROID__) void invoke('remote_push', { action: 'consumeClick', input: {} }).catch(() => {});
    });
    try {
      if (window.__OMNISTREAM_ANDROID__) {
        const pending = await invoke<{ path: string; server: string } | null>('remote_push', { action: 'consumeClick', input: {} });
        if (pending) handle(pending);
      }
    } catch (error) { stop(); throw error; }
    return stop;
  }
  if (window.location.hash.startsWith('#omnistream-notification=')) {
    try { handle(JSON.parse(decodeURIComponent(window.location.hash.slice('#omnistream-notification='.length)))); } catch { /* Ignore malformed launch data. */ }
    history.replaceState(null, '', window.location.pathname + window.location.search);
  }
  const worker = navigator.serviceWorker;
  const onMessage = (event: MessageEvent) => { if (event.data?.type === 'omnistream-notification') handle(event.data); };
  worker?.addEventListener('message', onMessage);
  return () => worker?.removeEventListener('message', onMessage);
}

// Serialize delivery between tabs when Web Locks are available. Persist only the
// cursor; task content and approval payloads never enter this client cache.
export async function deliverNotificationOnce(server: string, instance: string, item: SystemNotification, deliver: () => Promise<void | boolean>) {
  const key = `omnistream.notifications.cursor:${server}:${instance}`;
  const send = async () => {
    if (Number(localStorage.getItem(key) ?? '0') >= item.id) return;
    const unread = async () => {
      if (isNotificationRead(server, instance, item) || await deliver() !== false) localStorage.setItem(key, String(item.id));
    };
    if (navigator.locks) await navigator.locks.request('omnistream.notifications.read:' + receiptKey(server, notificationScope(item), instance), unread);
    else await unread();
  };
  if (navigator.locks) await navigator.locks.request(key, send);
  else await send();
}
