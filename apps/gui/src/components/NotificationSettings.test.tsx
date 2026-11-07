import { act, cleanup, fireEvent, render, renderHook, screen } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { api, type NotificationPage } from '../lib/api';
import { useSystemNotifications } from '../lib/useSystemNotifications';
import { notificationPermission, showSystemNotification } from '../lib/systemNotifications';
import { NotificationSettings } from './NotificationSettings';

vi.mock('../lib/systemNotifications', () => ({
  notificationPermission: vi.fn(), showSystemNotification: vi.fn(),
  listenForNotificationClicks: vi.fn().mockResolvedValue(() => {}),
  deliverNotificationOnce: vi.fn((_server, _instance, _item, deliver) => deliver()),
}));
const baseline: NotificationPage = { instance: 'db', cursor: 10, items: [] };
const update: NotificationPage = { instance: 'db', cursor: 11, items: [{ id: 11, kind: 'approval_pending', title: '', conversation: 'chat', channel: null }] };

beforeEach(() => {
  vi.useFakeTimers(); localStorage.clear();
  vi.mocked(notificationPermission).mockResolvedValue('granted');
  vi.mocked(showSystemNotification).mockResolvedValue(undefined);
  vi.spyOn(api, 'notifications').mockResolvedValue(baseline);
});
afterEach(() => { cleanup(); vi.useRealTimers(); vi.restoreAllMocks(); vi.clearAllMocks(); });

function Settings() { return <NotificationSettings state={useSystemNotifications('core', true, vi.fn())} />; }
const settle = () => act(async () => {});
const tick = () => act(async () => { await vi.advanceTimersByTimeAsync(3000); });

it('defaults to off and only enables polling after the user grants permission', async () => {
  vi.mocked(notificationPermission).mockResolvedValueOnce('default').mockResolvedValueOnce('denied').mockResolvedValueOnce('granted');
  render(<Settings />); await settle();
  expect(api.notifications).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole('checkbox', { name: '启用系统通知' })); await settle();
  expect(notificationPermission).toHaveBeenLastCalledWith(true);
  expect(screen.getByRole('checkbox')).not.toBeChecked();
  expect(screen.getByRole('status')).toHaveTextContent('已被拒绝');
  fireEvent.click(screen.getByRole('checkbox')); await settle();
  expect(screen.getByRole('checkbox')).toBeChecked();
  expect(localStorage.getItem('omnistream.notifications.enabled')).toBe('true');
  expect(api.notifications).toHaveBeenCalledWith(null, undefined);
});

it('stops failed polling until manual retry and resumes at the same cursor', async () => {
  localStorage.setItem('omnistream.notifications.enabled', 'true');
  vi.mocked(api.notifications).mockResolvedValueOnce(baseline).mockRejectedValueOnce(new Error('feed unavailable')).mockResolvedValue(update);
  render(<Settings />); await settle(); await tick();
  expect(screen.getByRole('alert')).toHaveTextContent('feed unavailable');
  await act(async () => { await vi.advanceTimersByTimeAsync(30000); fireEvent.focus(window); });
  expect(api.notifications).toHaveBeenCalledTimes(2);
  fireEvent.click(screen.getByRole('button', { name: '重试' })); await settle();
  expect(api.notifications).toHaveBeenLastCalledWith(10, 'db');
  expect(showSystemNotification).toHaveBeenCalledOnce();
  expect(screen.queryByRole('alert')).toBeNull();
});

it('retries a failed permission request with the original request flag', async () => {
  vi.mocked(notificationPermission).mockResolvedValueOnce('default').mockRejectedValueOnce(new Error('permission error')).mockResolvedValue('granted');
  render(<Settings />); await settle();
  fireEvent.click(screen.getByRole('checkbox')); await settle();
  expect(screen.getByRole('alert')).toHaveTextContent('permission error');
  fireEvent.click(screen.getByRole('button', { name: '重试' })); await settle();
  expect(notificationPermission).toHaveBeenLastCalledWith(true);
  expect(screen.getByRole('checkbox')).toBeChecked();
});

it('keeps the failed event cursor for retry and stops after disabling', async () => {
  localStorage.setItem('omnistream.notifications.enabled', 'true');
  vi.mocked(api.notifications).mockResolvedValueOnce(baseline).mockResolvedValue(update);
  vi.mocked(showSystemNotification).mockRejectedValueOnce(new Error('delivery failed')).mockResolvedValue(undefined);
  render(<Settings />); await settle(); await tick();
  expect(screen.getByRole('alert')).toHaveTextContent('delivery failed');
  fireEvent.click(screen.getByRole('button', { name: '重试' })); await settle();
  expect(api.notifications).toHaveBeenLastCalledWith(10, 'db');
  expect(showSystemNotification).toHaveBeenCalledTimes(2);
  fireEvent.click(screen.getByRole('checkbox')); await settle();
  const count = vi.mocked(api.notifications).mock.calls.length;
  await tick();
  expect(api.notifications).toHaveBeenCalledTimes(count);
});

it('ignores responses from a previous Core and resets the cursor on switching', async () => {
  localStorage.setItem('omnistream.notifications.enabled', 'true');
  let resolveOld!: (page: NotificationPage) => void;
  vi.mocked(api.notifications).mockReturnValueOnce(new Promise(resolve => { resolveOld = resolve; })).mockResolvedValue(baseline);
  const { rerender } = renderHook(({ server }) => useSystemNotifications(server, true, vi.fn()), { initialProps: { server: 'first' } });
  await settle();
  rerender({ server: 'second' }); await settle();
  await act(async () => resolveOld(update));
  expect(showSystemNotification).not.toHaveBeenCalled();
  expect(api.notifications).toHaveBeenLastCalledWith(null, undefined);
});

it('respects disabling notifications from another tab', async () => {
  localStorage.setItem('omnistream.notifications.enabled', 'true');
  const { result } = renderHook(() => useSystemNotifications('core', true, vi.fn())); await settle();
  act(() => window.dispatchEvent(new StorageEvent('storage', { key: 'omnistream.notifications.enabled', newValue: 'false' })));
  expect(result.current.enabled).toBe(false);
  await tick();
  expect(api.notifications).toHaveBeenCalledOnce();
});
