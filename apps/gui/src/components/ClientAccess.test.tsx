import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import * as authorization from '../lib/clientAuthorization';
import { saveClientCredential, type ClientAuthorization } from '../lib/clientCredentials';
import { ClientAccessManager } from './ClientAccessManager';
import { ClientAuthGate } from './ClientAuthGate';

vi.mock('../lib/clientAuthorization', () => ({ pendingClientRequests: vi.fn(), authorizedClients: vi.fn(), decideClientRequest: vi.fn(), revokeClient: vi.fn(), requestClientAccess: vi.fn(), readOwnClientRequest: vi.fn() }));
const server = 'https://core.example';
const request: ClientAuthorization = { id: 'phone', code: 'ABC123', name: 'Phone', platform: 'Android', address: '192.0.2.1', origin: 'https://app.example', status: 'pending', created_at: 1, expires_at: 9999999999, approved_at: null, approved_by: null };
let client: QueryClient;
beforeEach(() => {
  localStorage.clear(); vi.useFakeTimers();
  client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  Object.defineProperty(HTMLDialogElement.prototype, 'showModal', { configurable: true, value: function (this: HTMLDialogElement) { this.setAttribute('open', ''); } });
  vi.mocked(authorization.pendingClientRequests).mockResolvedValue([]);
});
afterEach(() => { cleanup(); client.clear(); vi.useRealTimers(); vi.restoreAllMocks(); vi.resetAllMocks(); });
const settle = () => act(async () => { await vi.advanceTimersByTimeAsync(1); });
const tick = (ms = 3000) => act(async () => { await vi.advanceTimersByTimeAsync(ms); });
function manager() { return render(<QueryClientProvider client={client}><ClientAccessManager server={server} enabled /></QueryClientProvider>); }

it('automatically shows a request and approves the displayed code, then removes it', async () => {
  vi.mocked(authorization.pendingClientRequests).mockResolvedValue([request]);
  vi.mocked(authorization.decideClientRequest).mockImplementation(async () => { vi.mocked(authorization.pendingClientRequests).mockResolvedValue([]); return { ...request, status: 'approved' }; });
  manager(); await settle();
  expect(screen.getByRole('dialog')).toHaveTextContent('ABC123');
  expect(screen.getByRole('dialog')).toHaveTextContent('192.0.2.1');
  fireEvent.click(screen.getByRole('button', { name: '批准此客户端' })); await settle();
  expect(authorization.decideClientRequest).toHaveBeenCalledWith(server, request, true);
  expect(screen.queryByRole('dialog')).toBeNull();
});

it('postpones existing requests but prompts for a new request and syncs another client decision', async () => {
  vi.mocked(authorization.pendingClientRequests).mockResolvedValue([request]);
  manager(); await settle();
  fireEvent.click(screen.getByRole('button', { name: '稍后处理' })); await settle(); await tick();
  expect(screen.queryByRole('dialog')).toBeNull();
  vi.mocked(authorization.pendingClientRequests).mockResolvedValue([request, { ...request, id: 'other', name: 'Tablet' }]);
  await tick();
  expect(screen.getByRole('dialog')).toHaveTextContent('Tablet');
  vi.mocked(authorization.pendingClientRequests).mockResolvedValue([]); await tick();
  expect(screen.queryByRole('dialog')).toBeNull();
});

it('stops a failed pending poll and offers a manual retry from the manager', async () => {
  vi.mocked(authorization.pendingClientRequests).mockRejectedValue(new Error('offline'));
  manager(); await settle();
  await tick(30000);
  expect(authorization.pendingClientRequests).toHaveBeenCalledOnce();
  fireEvent.click(screen.getByRole('button', { name: '客户端授权' })); await settle();
  expect(screen.getByRole('alert')).toHaveTextContent('offline');
  vi.mocked(authorization.pendingClientRequests).mockResolvedValue([request]);
  fireEvent.click(screen.getByRole('button', { name: '重试' })); await settle();
  expect(screen.getByRole('dialog')).toHaveTextContent(request.code);
});

it('resumes a saved request, stops on network failure, and enters only after approval', async () => {
  const token = 'a'.repeat(43);
  saveClientCredential(server, { token, name: 'Phone', request });
  const onAuthorized = vi.fn();
  vi.mocked(authorization.readOwnClientRequest).mockRejectedValueOnce(new Error('network')).mockImplementation(async () => {
    const approved = { ...request, status: 'approved' as const };
    saveClientCredential(server, { token, name: 'Phone', request: approved }); return approved;
  });
  render(<QueryClientProvider client={client}><ClientAuthGate server={server} onAuthorized={onAuthorized} onChangeServer={vi.fn()} /></QueryClientProvider>);
  await settle();
  expect(screen.getByLabelText('核对码')).toHaveTextContent('ABC123');
  await tick(30000);
  expect(authorization.readOwnClientRequest).toHaveBeenCalledOnce();
  expect(onAuthorized).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole('button', { name: '重试' })); await settle();
  expect(onAuthorized).toHaveBeenCalledOnce();
  await tick(30000);
  expect(authorization.readOwnClientRequest).toHaveBeenCalledTimes(2);
});

it('rechecks a saved approval if health denies access after revocation while closed', async () => {
  const token = 'a'.repeat(43);
  saveClientCredential(server, { token, name: 'Phone', request: { ...request, status: 'approved' } });
  vi.mocked(authorization.readOwnClientRequest).mockImplementation(async () => {
    const revoked = { ...request, status: 'revoked' as const };
    saveClientCredential(server, { token, name: 'Phone', request: revoked }); return revoked;
  });
  render(<QueryClientProvider client={client}><ClientAuthGate server={server} onAuthorized={vi.fn()} onChangeServer={vi.fn()} /></QueryClientProvider>);
  await settle();
  expect(screen.getByRole('button', { name: '申请授权' })).toBeVisible();
  expect(screen.getByRole('status')).toHaveTextContent('已撤销');
});
