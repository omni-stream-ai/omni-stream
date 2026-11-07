import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { deliverNotificationOnce, dismissSystemNotifications, listenForNotificationClicks, notificationPermission, showSystemNotification } from './systemNotifications';
import { notificationScope, saveReadReceipt } from './notificationReceipts';
import type { SystemNotification } from './api';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(), isTauri: vi.fn() }));
const requestPermission = vi.fn();
const shown: { title: string; options: NotificationOptions; onclick?: () => void; close: ReturnType<typeof vi.fn> }[] = [];
const item: SystemNotification = { id: 5, kind: 'task_completed', title: '项目检查', conversation: 'chat/id', channel: null };

beforeEach(() => {
  localStorage.clear();
  shown.length = 0;
  vi.mocked(isTauri).mockReturnValue(false);
  requestPermission.mockResolvedValue('granted');
  vi.stubGlobal('isSecureContext', true);
  vi.stubGlobal('Notification', class {
    static permission = 'granted';
    static requestPermission = requestPermission;
    close = vi.fn();
    constructor(public title: string, public options: NotificationOptions) { shown.push(this); }
  });
  Object.defineProperty(navigator, 'serviceWorker', { configurable: true, value: undefined });
  Object.defineProperty(navigator, 'locks', { configurable: true, value: undefined });
});
afterEach(() => { vi.unstubAllGlobals(); vi.restoreAllMocks(); vi.resetAllMocks(); });

it('requests permission only after an explicit action and reports insecure browsers', async () => {
  expect(await notificationPermission()).toBe('granted');
  expect(requestPermission).not.toHaveBeenCalled();
  await notificationPermission(true);
  expect(requestPermission).toHaveBeenCalledOnce();
  vi.stubGlobal('isSecureContext', false);
  expect(await notificationPermission()).toBe('unsupported');
});

it('opens the source conversation when a browser notification is clicked', async () => {
  const open = vi.fn();
  vi.spyOn(window, 'focus').mockImplementation(() => {});
  await showSystemNotification(item, open, 'core');
  expect(shown[0].title).toBe('任务执行完成');
  expect(shown[0].options.body).toBe('项目检查');
  shown[0].onclick?.();
  expect(open).toHaveBeenCalledWith('/chat/chat%2Fid');
  expect(shown[0].close).toHaveBeenCalledOnce();
});

it('uses the service worker when available and targets the inbox channel', async () => {
  const showNotification = vi.fn().mockResolvedValue(undefined);
  Object.defineProperty(navigator, 'serviceWorker', { configurable: true, value: { getRegistration: vi.fn().mockResolvedValue({ active: {}, showNotification }) } });
  await showSystemNotification({ ...item, kind: 'inbox_message', conversation: null, channel: 'D123' }, vi.fn(), 'core');
  expect(showNotification).toHaveBeenCalledWith('Slack 收件箱有新消息', expect.objectContaining({ data: expect.objectContaining({ path: '/slack?channel=D123', server: 'core' }) }));
  expect(shown).toHaveLength(0);
});

it.each(['Desktop', 'Android', 'iPhone', 'Macintosh'])('uses native permission and delivery commands with %s user agents', async userAgent => {
  vi.spyOn(navigator, 'userAgent', 'get').mockReturnValue(userAgent);
  vi.mocked(isTauri).mockReturnValue(true);
  vi.mocked(invoke).mockResolvedValue('granted');
  await notificationPermission(true);
  expect(invoke).toHaveBeenCalledWith('system_notification_permission', { request: true });
  await showSystemNotification(item, vi.fn(), 'core');
  expect(invoke).toHaveBeenCalledWith('show_system_notification', { id: 5, title: '任务执行完成', body: '项目检查', path: '/chat/chat%2Fid', server: 'core', instance: '', scope: notificationScope(item), eventId: 5 });
  expect(shown).toHaveLength(0);
});

it('deduplicates successful delivery and keeps failures retryable across Core instances', async () => {
  const deliver = vi.fn().mockRejectedValueOnce(new Error('unavailable')).mockResolvedValue(undefined);
  await expect(deliverNotificationOnce('core', 'first', item, deliver)).rejects.toThrow('unavailable');
  await deliverNotificationOnce('core', 'first', item, deliver);
  await deliverNotificationOnce('core', 'first', item, deliver);
  expect(deliver).toHaveBeenCalledTimes(2);
  await deliverNotificationOnce('core', 'replacement', item, deliver);
  await deliverNotificationOnce('another-core', 'first', item, deliver);
  expect(deliver).toHaveBeenCalledTimes(4);
});

it('serializes concurrent tab deliveries with Web Locks', async () => {
  const pending = new Map<string, Promise<unknown>>();
  Object.defineProperty(navigator, 'locks', { configurable: true, value: { request: vi.fn((key, callback) => { const next = (pending.get(key) ?? Promise.resolve()).then(callback); pending.set(key, next); return next; }) } });
  const deliver = vi.fn().mockResolvedValue(undefined);
  await Promise.all([deliverNotificationOnce('core', 'db', item, deliver), deliverNotificationOnce('core', 'db', item, deliver)]);
  expect(deliver).toHaveBeenCalledOnce();
});

it('suppresses read events without skipping newer events or notifications for another scope or Core', async () => {
  saveReadReceipt('core', { scope: notificationScope(item), instance: 'db', cursor: item.id });
  const deliver = vi.fn().mockResolvedValue(undefined);
  await deliverNotificationOnce('core', 'db', item, deliver);
  expect(deliver).not.toHaveBeenCalled();
  await deliverNotificationOnce('core', 'db', { ...item, id: 6 }, deliver);
  await deliverNotificationOnce('other-core', 'db', item, deliver);
  await deliverNotificationOnce('core', 'replaced-db', item, deliver);
  expect(deliver).toHaveBeenCalledTimes(3);
});

it('clears only already shown notifications covered by the read snapshot', async () => {
  await showSystemNotification(item, vi.fn(), 'core', 'db');
  await showSystemNotification({ ...item, id: 6 }, vi.fn(), 'core', 'db');
  await showSystemNotification(item, vi.fn(), 'other-core', 'db');
  await dismissSystemNotifications('core', { scope: notificationScope(item), instance: 'db', cursor: 5 });
  expect(shown[0].close).toHaveBeenCalledOnce();
  expect(shown[1].close).not.toHaveBeenCalled();
  expect(shown[2].close).not.toHaveBeenCalled();
});

it('leaves a cancelled delivery available for another active tab', async () => {
  const deliver = vi.fn().mockResolvedValueOnce(false).mockResolvedValue(undefined);
  await deliverNotificationOnce('core', 'db', item, deliver);
  await deliverNotificationOnce('core', 'db', item, deliver);
  expect(deliver).toHaveBeenCalledTimes(2);
});

it('handles service worker clicks through the router and removes its listener', async () => {
  const worker = new EventTarget();
  Object.defineProperty(navigator, 'serviceWorker', { configurable: true, value: worker });
  const open = vi.fn();
  const stop = await listenForNotificationClicks(open);
  worker.dispatchEvent(new MessageEvent('message', { data: { type: 'omnistream-notification', path: '/slack?channel=D1', server: 'core' } }));
  expect(open).toHaveBeenCalledWith('/slack?channel=D1', 'core');
  worker.dispatchEvent(new MessageEvent('message', { data: { type: 'omnistream-notification', path: 'https://example.com', server: 'core' } }));
  expect(open).toHaveBeenCalledOnce();
  stop();
  worker.dispatchEvent(new MessageEvent('message', { data: { type: 'omnistream-notification', path: '/chat/test', server: 'core' } }));
  expect(open).toHaveBeenCalledOnce();
});
