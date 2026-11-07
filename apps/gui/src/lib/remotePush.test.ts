import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { disableRemote, loadBinding, newBinding, pushRequest, pushTarget, registerRemote, remotePushSupported, saveBinding, type PushStatus } from './remotePush';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(), isTauri: vi.fn() }));
const server = 'https://core.example';
const status: PushStatus = { enabled: true, android: true, web: true, vapid_public_key: btoa(String.fromCharCode(...new Uint8Array(65).fill(4))) };

beforeEach(() => { localStorage.clear(); vi.mocked(isTauri).mockReturnValue(true); window.__OMNISTREAM_ANDROID__ = true; });
afterEach(() => { vi.restoreAllMocks(); vi.unstubAllGlobals(); vi.resetAllMocks(); delete window.__OMNISTREAM_ANDROID__; });

it('only supports native remote push on Android and creates device-limited credentials', () => {
  expect(remotePushSupported()).toBe(true);
  window.__OMNISTREAM_ANDROID__ = false;
  expect(remotePushSupported()).toBe(false);
  const binding = newBinding(server);
  expect(atob(binding.secret.replaceAll('-', '+').replaceAll('_', '/'))).toHaveLength(32);
  expect(newBinding(server).secret).not.toBe(binding.secret);
  saveBinding(binding);
  expect(loadBinding('https://other-core.example')).toBeUndefined();
  expect(loadBinding(server)?.id).toBe(binding.id);
});

it('persists the Android binding before registration and recovers a lost response with the same credentials', async () => {
  const binding = newBinding(server);
  saveBinding(binding);
  vi.mocked(invoke).mockImplementation(async (_command, args) => (args as { action: string }).action === 'token' ? { token: 'private-fcm-token' } : undefined);
  const fetch = vi.fn().mockRejectedValueOnce(new Error('network')).mockResolvedValue({ ok: true, json: async () => ({ registered: true, state: 'active' }) });
  vi.stubGlobal('fetch', fetch);
  await expect(registerRemote(binding, status, true)).rejects.toThrow();
  expect(loadBinding(server)?.id).toBe(binding.id);
  expect(invoke).toHaveBeenCalledWith('remote_push', expect.objectContaining({ action: 'saveBinding' }));
  await registerRemote(binding, status, true);
  expect(loadBinding(server)?.registered).toBe(true);
  const requests = fetch.mock.calls.map(call => JSON.parse(call[1].body));
  expect(requests[0].secret).toBe(requests[1].secret);
  expect(requests[1]).toMatchObject({ target: 'private-fcm-token', activate: true });
  expect(requests[1]).not.toHaveProperty('pairing_code');
});

it('keeps remote enabled if disabling Core fails and removes the native binding after success', async () => {
  const binding = { ...newBinding(server), registered: true };
  saveBinding(binding);
  const fetch = vi.fn().mockResolvedValueOnce({ ok: false, json: async () => ({ error: 'offline' }) })
    .mockResolvedValue({ ok: true, json: async () => ({ registered: false, state: 'unregistered' }) });
  vi.stubGlobal('fetch', fetch);
  await expect(disableRemote(binding)).rejects.toThrow('offline');
  expect(loadBinding(server)?.enabled).toBe(true);
  expect(invoke).not.toHaveBeenCalled();
  await disableRemote(binding);
  expect(invoke).toHaveBeenCalledWith('remote_push', { action: 'removeBinding', input: { server } });
  expect(loadBinding(server)).toMatchObject({ enabled: false, registered: false });
});

it('does not switch push subscription keys or unsubscribe another Core implicitly', async () => {
  vi.mocked(isTauri).mockReturnValue(false);
  const binding = newBinding(server);
  saveBinding({ ...newBinding('https://other-core.example'), enabled: true });
  const subscribe = vi.fn();
  const getSubscription = vi.fn().mockResolvedValue({ options: { applicationServerKey: new Uint8Array(65).fill(5).buffer } });
  Object.defineProperty(navigator, 'serviceWorker', { configurable: true, value: { getRegistration: vi.fn().mockResolvedValue({ active: {}, pushManager: { getSubscription, subscribe } }) } });
  await expect(pushTarget(binding, status)).rejects.toThrow('另一个 Core');
  localStorage.clear();
  await expect(pushTarget(binding, status)).rejects.toThrow('密钥已更换');
  expect(subscribe).not.toHaveBeenCalled();
});

it('uses the requested Core even if the active app connection changes', async () => {
  const fetch = vi.fn().mockResolvedValue({ ok: true, json: async () => ({ enabled: true }) });
  vi.stubGlobal('fetch', fetch);
  await pushRequest(server, '/push/status');
  expect(fetch).toHaveBeenCalledWith(server + '/push/status', expect.objectContaining({ method: 'GET' }));
});
