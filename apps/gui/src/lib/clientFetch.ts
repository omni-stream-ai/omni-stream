import { invoke, isTauri } from '@tauri-apps/api/core';
import { translate } from '../i18n';
import { invalidateClientCredential, loadClientCredential, normalizeCoreUrl } from './clientCredentials';
import { isNativeMobile } from './platform';

let desktopToken: Promise<string | undefined> | undefined;

export class CoreResponseError extends Error {
  constructor(message: string, public readonly status: number) { super(message); }
}

export async function clientToken(server: string): Promise<string | undefined> {
  const normalized = normalizeCoreUrl(server);
  const saved = loadClientCredential(normalized);
  // A separately started local Core does not share this window's sidecar key.
  if (saved?.request?.status === 'approved') return saved.token;
  const url = new URL(normalized);
  if (isTauri() && !isNativeMobile() && window.__OMNISTREAM_ANDROID__ !== true
      && url.protocol === 'http:' && ['127.0.0.1', 'localhost', '[::1]'].includes(url.hostname)
      && url.port === '4317' && url.pathname === '/') {
    desktopToken ??= invoke<string>('desktop_client_token').catch(() => undefined);
    const token = await desktopToken;
    if (token) return token;
  }
  return saved?.token;
}

export async function clientHeaders(server: string, input?: HeadersInit) {
  const headers = new Headers(input);
  if (!headers.has('authorization')) {
    const token = await clientToken(server);
    if (token) headers.set('authorization', 'Bearer ' + token);
  }
  return Object.fromEntries(headers.entries());
}

export async function clientFetch(server: string, path: string, init?: RequestInit) {
  const normalized = normalizeCoreUrl(server);
  const headers = await clientHeaders(normalized, init?.headers);
  const response = await fetch(normalized + path, { ...init, headers, redirect: 'error' });
  if (response.status === 401) {
    const authorization = headers.authorization;
    invalidateClientCredential(normalized, authorization?.startsWith('Bearer ') ? authorization.slice(7) : undefined);
  }
  return response;
}

export async function coreRequest<T>(server: string, path: string, init?: RequestInit): Promise<T> {
  let response: Response;
  try {
    response = await clientFetch(server, path, { ...init, headers: { 'content-type': 'application/json', ...Object.fromEntries(new Headers(init?.headers).entries()) } });
  } catch { throw new Error(translate('connection.addressError')); }
  const body = await response.json().catch(() => ({}));
  if (response.status === 404 && path.startsWith('/conversation/')) throw new Error(translate('connection.interfaceError', { url: server }));
  if (response.status === 401) throw new Error(translate('access.required'));
  if (!response.ok) throw new CoreResponseError(body.error ?? translate('request.failed', { status: response.status }), response.status);
  return body as T;
}
