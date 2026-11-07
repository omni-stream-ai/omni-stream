import { act, cleanup, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { allBindings, disableRemote, newBinding, pushRequest, registerRemote, saveBinding, type PushBinding } from './remotePush';
import { useRemotePush } from './useRemotePush';

vi.mock('./remotePush', async importOriginal => ({ ...await importOriginal<typeof import('./remotePush')>(),
  remotePushSupported: () => true, pushRequest: vi.fn(), registerRemote: vi.fn(), disableRemote: vi.fn() }));
beforeEach(() => {
  vi.useFakeTimers(); localStorage.clear();
  vi.mocked(pushRequest).mockResolvedValue({ enabled: true, web: true, android: true, vapid_public_key: 'public-key' });
  vi.mocked(registerRemote).mockImplementation(async binding => {
    saveBinding({ ...binding, registered: true });
    return { registered: true, state: 'active' };
  });
});
afterEach(() => { cleanup(); vi.useRealTimers(); vi.resetAllMocks(); });

it('automatically registers with the connected Core after notifications are enabled', async () => {
  const { result } = renderHook(() => useRemotePush('https://core.example', true, true));
  await act(async () => {});
  expect(registerRemote).toHaveBeenCalledWith(expect.objectContaining({ server: 'https://core.example' }), expect.objectContaining({ enabled: true }));
  expect(result.current.binding).toMatchObject({ enabled: true, registered: true });
  expect(result.current.device?.state).toBe('active');
});

it('does not automatically enable a binding the user explicitly disabled', async () => {
  saveBinding({ ...newBinding('core'), enabled: false, registered: true });
  const { result } = renderHook(() => useRemotePush('core', true, true));
  await act(async () => {});
  expect(registerRemote).not.toHaveBeenCalled();
  await act(async () => { await result.current.enable(); });
  expect(registerRemote).toHaveBeenCalledOnce();
  expect(result.current.binding?.enabled).toBe(true);
});

it('waits for notification consent and connection, and does not register with an unavailable Core', async () => {
  const { rerender } = renderHook(({ enabled, connected }) => useRemotePush('core', enabled, connected),
    { initialProps: { enabled: false, connected: true } });
  await act(async () => {});
  expect(pushRequest).not.toHaveBeenCalled();
  rerender({ enabled: true, connected: false });
  await act(async () => {});
  expect(pushRequest).not.toHaveBeenCalled();
  vi.mocked(pushRequest).mockResolvedValue({ enabled: false, android: false, web: false });
  rerender({ enabled: true, connected: true });
  await act(async () => {});
  expect(registerRemote).not.toHaveBeenCalled();
});

it('preserves an incomplete automatic registration for a manual retry with the same identity', async () => {
  vi.mocked(registerRemote).mockRejectedValueOnce(new Error('registration offline'));
  const { result } = renderHook(() => useRemotePush('core', true, true));
  await act(async () => {});
  const pending = result.current.binding;
  expect(pending?.registered).toBe(false);
  expect(result.current.error).toContain('registration offline');
  await act(async () => { await vi.advanceTimersByTimeAsync(60000); });
  expect(registerRemote).toHaveBeenCalledOnce();
  await act(async () => { await result.current.retry(); });
  expect(registerRemote).toHaveBeenLastCalledWith(expect.objectContaining({ id: pending?.id, secret: pending?.secret }), expect.anything());
  expect(result.current.binding?.registered).toBe(true);
});

it('stops failed status polling until the user retries explicitly', async () => {
  vi.mocked(pushRequest).mockResolvedValueOnce({ enabled: true }).mockRejectedValueOnce(new Error('offline')).mockResolvedValue({ enabled: true });
  const { result } = renderHook(() => useRemotePush('core', true, true));
  await act(async () => {});
  await act(async () => { await vi.advanceTimersByTimeAsync(15000); });
  expect(result.current.error).toContain('offline');
  await act(async () => { await vi.advanceTimersByTimeAsync(60000); window.dispatchEvent(new Event('focus')); });
  expect(pushRequest).toHaveBeenCalledTimes(2);
  await act(async () => result.current.retry());
  expect(pushRequest).toHaveBeenCalledTimes(3);
  expect(result.current.error).toBe('');
});

it('the master switch disables bindings for every Core and preserves unfinished failures', async () => {
  const first: PushBinding = { ...newBinding('https://first.example'), transport: 'android', registered: true };
  const second: PushBinding = { ...newBinding('https://second.example'), transport: 'android', registered: true };
  saveBinding(first); saveBinding(second);
  vi.mocked(disableRemote).mockImplementationOnce(async binding => saveBinding({ ...binding, enabled: false }))
    .mockRejectedValueOnce(new Error('offline'));
  const { result } = renderHook(() => useRemotePush('https://other-current.example', false, true));
  let success = true;
  await act(async () => { success = await result.current.disableAll(); });
  expect(success).toBe(false);
  expect(disableRemote).toHaveBeenCalledTimes(2);
  expect(result.current.error).toContain(second.server);
  expect(allBindings().filter(binding => binding.enabled).map(binding => binding.server)).toEqual([second.server]);
  vi.mocked(disableRemote).mockImplementation(async binding => saveBinding({ ...binding, enabled: false }));
  await act(async () => { success = await result.current.disableAll(); });
  expect(success).toBe(true);
  expect(allBindings().some(binding => binding.enabled)).toBe(false);
});
