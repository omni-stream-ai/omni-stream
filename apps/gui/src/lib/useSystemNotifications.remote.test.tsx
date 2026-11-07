import { act, cleanup, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { api } from './api';
import { useRemotePush } from './useRemotePush';
import { useSystemNotifications } from './useSystemNotifications';
import { notificationPermission } from './systemNotifications';

vi.mock('./useRemotePush', () => ({ useRemotePush: vi.fn() }));
vi.mock('./systemNotifications', () => ({ notificationPermission: vi.fn().mockResolvedValue('granted'),
  listenForNotificationClicks: vi.fn().mockResolvedValue(() => {}), showSystemNotification: vi.fn(), deliverNotificationOnce: vi.fn() }));
let remote: ReturnType<typeof useRemotePush>;
beforeEach(() => {
  vi.useFakeTimers(); localStorage.clear(); localStorage.setItem('omnistream.notifications.enabled', 'true');
  remote = { supported: true, binding: { id: 'device', secret: 'secret', server: 'core', enabled: true, registered: true, transport: 'web', locale: 'zh-CN' },
    status: undefined, device: undefined, busy: false, error: '', enable: vi.fn(), disable: vi.fn().mockResolvedValue(true), disableAll: vi.fn().mockResolvedValue(true),
    retryDelivery: vi.fn(), recheck: vi.fn(), retry: vi.fn() };
  vi.mocked(useRemotePush).mockImplementation(() => remote);
  vi.spyOn(api, 'notifications').mockResolvedValue({ instance: 'db', cursor: 40, items: [] });
});
afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.useRealTimers(); });

it('keeps local polling available if automatic remote registration fails', async () => {
  remote = { ...remote, binding: { ...remote.binding!, registered: false }, error: 'registration failed' };
  renderHook(() => useSystemNotifications('core', true, vi.fn()));
  await act(async () => {});
  expect(api.notifications).toHaveBeenCalledWith(null, undefined);
});

it('does not enable automatic registration before system permission is granted', async () => {
  vi.mocked(notificationPermission).mockResolvedValueOnce('denied');
  renderHook(() => useSystemNotifications('core', true, vi.fn()));
  await act(async () => {});
  expect(useRemotePush).toHaveBeenLastCalledWith('core', false, true);
});

it('stops local polling for remote bindings and resumes from a fresh baseline after disabling remote push', async () => {
  const { rerender } = renderHook(() => useSystemNotifications('core', true, vi.fn()));
  await act(async () => {});
  await act(async () => { await vi.advanceTimersByTimeAsync(10000); });
  expect(api.notifications).not.toHaveBeenCalled();
  remote = { ...remote, binding: { ...remote.binding!, enabled: false } };
  rerender(); await act(async () => {});
  expect(api.notifications).toHaveBeenCalledWith(null, undefined);
});

it('keeps the master preference enabled if Core revocation fails', async () => {
  vi.mocked(remote.disableAll).mockResolvedValue(false);
  const { result } = renderHook(() => useSystemNotifications('core', true, vi.fn()));
  await act(async () => {});
  await act(async () => { await result.current.updateEnabled(false); });
  expect(result.current.enabled).toBe(true);
  expect(localStorage.getItem('omnistream.notifications.enabled')).toBe('true');
  expect(api.notifications).not.toHaveBeenCalled();
  vi.mocked(remote.disableAll).mockResolvedValue(true);
  await act(async () => { await result.current.updateEnabled(false); });
  expect(result.current.enabled).toBe(false);
  expect(localStorage.getItem('omnistream.notifications.enabled')).toBe('false');
});
