import { clientFetch, clientToken } from './clientFetch';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { translate } from '../i18n';
import { randomId } from './uuid';
import { putPushRecord } from './notificationReceipts';

declare global { interface Window { __OMNISTREAM_ANDROID__?: boolean } }
export type PushBinding = { id: string; secret: string; server: string; transport: 'android' | 'web'; locale: string; enabled: boolean; registered: boolean; clientToken?: string };
export type PushStatus = { enabled: boolean; android: boolean; web: boolean; vapid_public_key: string | null };
export type PushDeviceStatus = { registered: boolean; state: 'active' | 'paused' | 'disabled' | 'unregistered'; error?: string | null };
const prefix = 'omnistream.push:';
export const bindingKey = (server: string) => prefix + server;
export const remotePushSupported = () => isTauri() ? window.__OMNISTREAM_ANDROID__ === true
  : window.isSecureContext && 'PushManager' in window && 'serviceWorker' in navigator && 'Notification' in window;
export function loadBinding(server: string): PushBinding | undefined {
  const value = localStorage.getItem(bindingKey(server));
  if (!value) return;
  try { return JSON.parse(value) as PushBinding; } catch { return; }
}
export function saveBinding(binding: PushBinding) {
  localStorage.setItem(bindingKey(binding.server), JSON.stringify(binding));
  window.dispatchEvent(new CustomEvent('omnistream-push-change', { detail: binding.server }));
}
export function allBindings(): PushBinding[] {
  const result: PushBinding[] = [];
  for (let i = 0; i < localStorage.length; i++) {
    const key = localStorage.key(i);
    if (!key?.startsWith(prefix)) continue;
    const binding = loadBinding(key.slice(prefix.length));
    if (binding) result.push(binding);
  }
  return result;
}
export async function pushRequest<T>(server: string, path: string, input?: unknown): Promise<T> {
  let response: Response;
  try {
    response = await clientFetch(server, path, { method: input === undefined ? 'GET' : 'POST',
      headers: { 'content-type': 'application/json' }, body: input === undefined ? undefined : JSON.stringify(input), redirect: 'error', signal: AbortSignal.timeout(20000) });
  } catch { throw new Error(translate('connection.addressError')); }
  const value = await response.json().catch(() => ({}));
  if (!response.ok) throw new Error(value.error ?? translate('request.failed', { status: response.status }));
  return value as T;
}
function bytes(encoded: string) {
  const binary = atob(encoded.replaceAll('-', '+').replaceAll('_', '/') + '='.repeat((4 - encoded.length % 4) % 4));
  return Uint8Array.from(binary, char => char.charCodeAt(0));
}
export function newBinding(server: string): PushBinding {
  const secret = btoa(String.fromCharCode(...crypto.getRandomValues(new Uint8Array(32)))).replaceAll('+', '-').replaceAll('/', '_').replaceAll('=', '');
  return { id: randomId(), secret, server, transport: isTauri() ? 'android' : 'web',
    locale: localStorage.getItem('omnistream.locale') === 'en-US' ? 'en-US' : 'zh-CN', enabled: true, registered: false };
}

// The worker uses this same versioned schema to rotate subscriptions while closed.
async function workerBinding(binding: PushBinding) {
  await putPushRecord('bindings', binding);
}
export async function pushTarget(binding: PushBinding, status: PushStatus) {
  if (binding.transport === 'android') return (await invoke<{ token: string }>('remote_push', { action: 'token', input: {} })).token;
  const registration = await navigator.serviceWorker.getRegistration();
  if (!registration?.active || !status.vapid_public_key) throw new Error(translate('push.workerRequired'));
  if (allBindings().some(other => other.server !== binding.server && other.enabled)) throw new Error(translate('push.otherCore'));
  let subscription = await registration.pushManager.getSubscription();
  const key = bytes(status.vapid_public_key);
  if (subscription?.options.applicationServerKey && !new Uint8Array(subscription.options.applicationServerKey).every((value, i) => value === key[i])) {
    throw new Error(translate('push.keyChanged'));
  }
  subscription ??= await registration.pushManager.subscribe({ userVisibleOnly: true, applicationServerKey: key });
  return subscription.toJSON();
}
function deviceInput(binding: PushBinding) {
  return { id: binding.id, secret: binding.secret, server: binding.server, transport: binding.transport, locale: binding.locale };
}
export async function registerRemote(binding: PushBinding, status: PushStatus, activate = false) {
  binding = { ...binding, clientToken: await clientToken(binding.server) };
  const target = await pushTarget(binding, status);
  // Persist the limited binding before registration: a lost HTTP response remains recoverable.
  if (binding.transport === 'android') await invoke('remote_push', { action: 'saveBinding', input: { ...deviceInput(binding), clientToken: binding.clientToken } });
  else await workerBinding(binding);
  const state = await pushRequest<PushDeviceStatus>(binding.server, '/push/devices/register', { ...deviceInput(binding), target, activate });
  binding = { ...binding, registered: true };
  saveBinding(binding);
  if (binding.transport === 'web') await workerBinding(binding);
  return state;
}
export async function disableRemote(binding: PushBinding) {
  const state = await pushRequest<PushDeviceStatus>(binding.server, '/push/devices/disable', { id: binding.id, secret: binding.secret });
  const disabled = { ...binding, enabled: false, registered: state.registered };
  // Disable background refresh before removing the provider subscription.
  if (binding.transport === 'android') await invoke('remote_push', { action: 'removeBinding', input: { server: binding.server } });
  else {
    await workerBinding(disabled);
    const registration = await navigator.serviceWorker.getRegistration();
    await (await registration?.pushManager.getSubscription())?.unsubscribe();
  }
  saveBinding(disabled);
}
export const remoteDeviceInput = (binding: PushBinding) => ({ id: binding.id, secret: binding.secret });
