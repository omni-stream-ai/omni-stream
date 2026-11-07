import { translate } from '../i18n';
import { CoreResponseError, coreRequest } from './clientFetch';
import { type ClientAuthorization, loadClientCredential, newClientToken, saveClientCredential } from './clientCredentials';

export type AuthorizedClients = { clients: ClientAuthorization[]; current_id: string | null };
export const pendingClientRequests = (server: string) => coreRequest<ClientAuthorization[]>(server, '/client-auth/requests');
export const authorizedClients = (server: string) => coreRequest<AuthorizedClients>(server, '/client-auth/clients');
export const decideClientRequest = (server: string, request: ClientAuthorization, approve: boolean) =>
  coreRequest<ClientAuthorization>(server, '/client-auth/requests/' + request.id + (approve ? '/approve' : '/reject'), { method: 'POST', body: JSON.stringify({ code: request.code }) });
export const revokeClient = (server: string, id: string) => coreRequest<{ revoked: boolean }>(server, '/client-auth/clients/' + id, { method: 'DELETE' });

export async function requestClientAccess(server: string, name: string, platform: string) {
  const existing = loadClientCredential(server);
  const credential = existing && !existing.request && existing.name === name ? existing : { token: newClientToken(), name };
  saveClientCredential(server, credential);
  const request = await coreRequest<ClientAuthorization>(server, '/client-auth/requests', {
    method: 'POST', headers: { authorization: 'Bearer ' + credential.token },
    body: JSON.stringify({ name: credential.name, platform, token: credential.token }),
  });
  saveClientCredential(server, { ...credential, request }, credential.token);
  return request;
}

export async function readOwnClientRequest(server: string, cancel = false) {
  const saved = loadClientCredential(server);
  if (!saved?.request) throw new Error(translate('access.required'));
  let request: ClientAuthorization;
  try {
    request = await coreRequest<ClientAuthorization>(server, '/client-auth/requests/' + saved.request.id, {
      method: cancel ? 'DELETE' : 'GET', headers: { authorization: 'Bearer ' + saved.token },
    });
  } catch (error) {
    if (!(error instanceof CoreResponseError) || error.status !== 404) throw error;
    // The server may have pruned an old request or replaced its database.
    request = { ...saved.request, status: 'expired' };
  }
  saveClientCredential(server, { ...saved, request }, saved.token);
  return request;
}
