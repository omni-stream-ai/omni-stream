import { translate } from '../i18n';

export type ClientAuthorization = {
  id: string; name: string; platform: string; address: string; origin: string; code: string;
  status: 'pending' | 'approved' | 'rejected' | 'cancelled' | 'expired' | 'revoked';
  created_at: number; expires_at: number; approved_at: number | null; approved_by: string | null;
};
export type CoreHealth = { ok: boolean; access?: { required: boolean; authorized: boolean; client_id: string | null } };
export type ClientCredential = { token: string; name: string; request?: ClientAuthorization };
export const accessChange = 'omnistream-client-access-change';
const prefix = 'omnistream.client-access:';

export function normalizeCoreUrl(server: string) {
  let url: URL;
  try { url = new URL(server.trim()); } catch { throw new Error(translate('connection.addressError')); }
  if (!['http:', 'https:'].includes(url.protocol) || url.username || url.password || url.search || url.hash) throw new Error(translate('connection.addressError'));
  let normalized = url.href;
  while (normalized.endsWith('/')) normalized = normalized.slice(0, -1);
  return normalized;
}
export const clientCredentialKey = (server: string) => prefix + normalizeCoreUrl(server);
export function loadClientCredential(server: string): ClientCredential | undefined {
  try {
    const value = JSON.parse(localStorage.getItem(clientCredentialKey(server)) ?? 'null') as ClientCredential | null;
    if (value && typeof value.token === 'string' && /^[A-Za-z0-9_-]{43}$/.test(value.token) && typeof value.name === 'string') return value;
  } catch { /* Missing or damaged local credentials require a new explicit request. */ }
}
export function saveClientCredential(server: string, credential: ClientCredential, expectedToken?: string) {
  if (expectedToken && loadClientCredential(server)?.token !== expectedToken) return false;
  localStorage.setItem(clientCredentialKey(server), JSON.stringify(credential));
  window.dispatchEvent(new CustomEvent(accessChange, { detail: normalizeCoreUrl(server) }));
  return true;
}
export function invalidateClientCredential(server: string, token?: string) {
  const saved = loadClientCredential(server);
  if (saved && saved.token === token && saved.request?.status === 'approved') {
    saveClientCredential(server, { ...saved, request: { ...saved.request, status: 'revoked' } }, token);
  } else window.dispatchEvent(new CustomEvent(accessChange, { detail: normalizeCoreUrl(server) }));
}
export function newClientToken() {
  return btoa(String.fromCharCode(...crypto.getRandomValues(new Uint8Array(32))))
    .replaceAll('+', '-').replaceAll('/', '_').replaceAll('=', '');
}
