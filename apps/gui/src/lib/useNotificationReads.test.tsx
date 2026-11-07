import { act, cleanup, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { useNotificationReads } from './useNotificationReads';
import { pushRequest, saveBinding } from './remotePush';
import { dismissSystemNotifications } from './systemNotifications';
import { isNotificationRead } from './notificationReceipts';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(), isTauri: vi.fn() }));
vi.mock('./remotePush', async original => ({ ...await original<typeof import('./remotePush')>(), pushRequest: vi.fn() }));
vi.mock('./systemNotifications', () => ({ dismissSystemNotifications: vi.fn() }));
const snapshot = { scope: '["chat","one"]', instance: 'db', cursor: 10 };
beforeEach(() => {
  localStorage.clear(); window.__OMNISTREAM_ANDROID__ = true;
  vi.mocked(isTauri).mockReturnValue(true); vi.mocked(invoke).mockResolvedValue(undefined);
  vi.mocked(dismissSystemNotifications).mockResolvedValue(undefined); vi.mocked(pushRequest).mockResolvedValue({ read: true });
  saveBinding({ id: 'device', secret: 'device-secret', server: 'core', transport: 'android', enabled: true, registered: true, locale: 'zh-CN' });
});
afterEach(() => { cleanup(); vi.resetAllMocks(); delete window.__OMNISTREAM_ANDROID__; });

it('persists a receipt locally and on Android before acknowledging it to its own Core', async () => {
  const { result } = renderHook(() => useNotificationReads('core'));
  await act(async () => { await result.current.read(snapshot); });
  expect(invoke).toHaveBeenCalledWith('remote_push', { action: 'markRead', input: { server: 'core', ...snapshot } });
  expect(dismissSystemNotifications).toHaveBeenCalledWith('core', snapshot);
  expect(pushRequest).toHaveBeenCalledWith('core', '/push/devices/read', { id: 'device', secret: 'device-secret', read: snapshot });
  expect(isNotificationRead('core', 'db', { id: 10, kind: 'task_completed', title: '', conversation: 'one', channel: null })).toBe(true);
  expect(isNotificationRead('other', 'db', { id: 10, kind: 'task_completed', title: '', conversation: 'one', channel: null })).toBe(false);
});

it('stops failed remote sync until manual retry while keeping newer reads effective locally', async () => {
  vi.mocked(pushRequest).mockRejectedValueOnce(new Error('offline'));
  const { result } = renderHook(() => useNotificationReads('core'));
  await act(async () => { await result.current.read(snapshot); });
  expect(result.current.error).toContain('offline');
  await act(async () => { await result.current.read({ ...snapshot, cursor: 12 }); });
  expect(pushRequest).toHaveBeenCalledOnce();
  expect(invoke).toHaveBeenLastCalledWith('remote_push', { action: 'markRead', input: { server: 'core', ...snapshot, cursor: 12 } });
  await act(async () => { await result.current.retry(); });
  expect(pushRequest).toHaveBeenLastCalledWith('core', '/push/devices/read', expect.objectContaining({ read: { ...snapshot, cursor: 12 } }));
  expect(result.current.error).toBe('');
});
