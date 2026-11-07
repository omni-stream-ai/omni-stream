import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { clientFetch, clientToken } from './clientFetch';
import { clientCredentialKey, loadClientCredential, newClientToken, saveClientCredential, type ClientAuthorization } from './clientCredentials';
import { readOwnClientRequest, requestClientAccess } from './clientAuthorization';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(), isTauri: vi.fn() }));
const server = 'https://core.example/api';
const record: ClientAuthorization = { id: 'request', code: '123ABC', name: 'Phone', platform: 'Android', address: '192.0.2.1', origin: '', status: 'pending', created_at: 1, expires_at: 601, approved_at: null, approved_by: null };
beforeEach(() => { localStorage.clear(); vi.mocked(isTauri).mockReturnValue(false); });
afterEach(() => { vi.restoreAllMocks(); vi.unstubAllGlobals(); vi.resetAllMocks(); delete window.__OMNISTREAM_MOBILE__; });

it('normalizes the full Core address and keeps credentials isolated by origin and path', async () => {
  const token = newClientToken();
  saveClientCredential(server + '/', { token, name: 'Phone' });
  const fetch = vi.fn().mockResolvedValue(new Response('{}'));
  vi.stubGlobal('fetch', fetch);
  await clientFetch('https://CORE.example:443/api/', '/projects');
  expect(fetch).toHaveBeenLastCalledWith(server + '/projects', expect.objectContaining({ headers: { authorization: 'Bearer ' + token }, redirect: 'error' }));
  await clientFetch('https://other.example/api', '/projects');
  expect(fetch.mock.lastCall?.[1].headers).not.toHaveProperty('authorization');
  await clientFetch('https://core.example/other', '/projects');
  expect(fetch.mock.lastCall?.[1].headers).not.toHaveProperty('authorization');
  await expect(clientFetch('https://user:password@core.example', '/projects')).rejects.toThrow();
  await expect(clientFetch(server + '?token=leak', '/projects')).rejects.toThrow();
  expect(fetch).toHaveBeenCalledTimes(3);
});

it('keeps a newer credential when an old in-flight request returns unauthorized', async () => {
  const old = newClientToken(); const next = newClientToken();
  saveClientCredential(server, { token: old, name: 'Old', request: { ...record, status: 'approved' } });
  let finish!: (response: Response) => void;
  vi.stubGlobal('fetch', vi.fn().mockImplementation(() => new Promise(resolve => { finish = resolve; })));
  const pending = clientFetch(server, '/projects');
  await vi.waitFor(() => expect(finish).toBeDefined());
  saveClientCredential(server, { token: next, name: 'New', request: { ...record, status: 'approved' } });
  finish(new Response('{}', { status: 401 })); await pending;
  expect(loadClientCredential(server)?.token).toBe(next);
  expect(loadClientCredential(server)?.request?.status).toBe('approved');
});

it('returns an approved credential to the authorization page after a 401', async () => {
  saveClientCredential(server, { token: newClientToken(), name: 'Phone', request: { ...record, status: 'approved' } });
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response('{}', { status: 401 })));
  await clientFetch(server, '/projects');
  expect(loadClientCredential(server)?.request?.status).toBe('revoked');
});

it('recovers a lost creation response using the same private key and polls only its own request', async () => {
  const fetch = vi.fn().mockRejectedValueOnce(new Error('lost')).mockImplementation(async () => new Response(JSON.stringify(record)));
  vi.stubGlobal('fetch', fetch);
  await expect(requestClientAccess(server, 'Phone', 'Android')).rejects.toThrow();
  const token = loadClientCredential(server)!.token;
  expect(token).toHaveLength(43);
  await requestClientAccess(server, 'Phone', 'Android');
  expect(fetch.mock.calls.map(call => JSON.parse(call[1].body).token)).toEqual([token, token]);
  await readOwnClientRequest(server);
  expect(fetch).toHaveBeenLastCalledWith(server + '/client-auth/requests/request', expect.objectContaining({ headers: expect.objectContaining({ authorization: 'Bearer ' + token }) }));
  expect(localStorage.getItem(clientCredentialKey('https://other.example'))).toBeNull();
});

it('never sends the desktop runtime key to remote servers or mobile clients', async () => {
  vi.mocked(isTauri).mockReturnValue(true);
  vi.mocked(invoke).mockResolvedValue('desktop-private-key');
  expect(await clientToken(server)).toBeUndefined();
  expect(invoke).not.toHaveBeenCalled();
  expect(await clientToken('http://127.0.0.1:4317')).toBe('desktop-private-key');
  expect(invoke).toHaveBeenCalledTimes(1);
  const approved = newClientToken();
  saveClientCredential('http://127.0.0.1:4317', { token: approved, name: 'Manual Core', request: { ...record, status: 'approved' } });
  expect(await clientToken('http://127.0.0.1:4317')).toBe(approved);
  localStorage.removeItem(clientCredentialKey('http://127.0.0.1:4317'));
  window.__OMNISTREAM_MOBILE__ = true;
  expect(await clientToken('http://127.0.0.1:4317')).toBeUndefined();
});

it('allows a new application when the Core has pruned the old saved request', async () => {
  saveClientCredential(server, { token: newClientToken(), name: 'Phone', request: record });
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response('{}', { status: 404 })));
  expect((await readOwnClientRequest(server)).status).toBe('expired');
  expect(loadClientCredential(server)?.request?.status).toBe('expired');
});
