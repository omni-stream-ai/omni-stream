import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { ShieldCheck } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import { useI18n } from '../i18n';
import { authorizedClients, decideClientRequest, pendingClientRequests, revokeClient } from '../lib/clientAuthorization';
import { invalidateClientCredential, loadClientCredential, type ClientAuthorization } from '../lib/clientCredentials';
import { Dialog } from './Dialog';
import { EmptyState, ErrorState, LoadingState, MutationError } from './StateView';

export function ClientAccessManager({ server, enabled }: { server: string; enabled: boolean }) {
  const { locale, t } = useI18n();
  const client = useQueryClient();
  const [open, setOpen] = useState(false);
  const [automatic, setAutomatic] = useState(false);
  const [tab, setTab] = useState<'pending' | 'clients'>('pending');
  const [confirm, setConfirm] = useState<string>();
  const dismissed = useRef(new Set<string>());
  const pending = useQuery({
    queryKey: ['client-auth-pending', server], queryFn: () => pendingClientRequests(server), enabled,
    retry: false, refetchOnWindowFocus: false, refetchOnReconnect: false,
    refetchInterval: query => query.state.status === 'error' ? false : 3000,
  });
  const clients = useQuery({ queryKey: ['client-auth-clients', server], queryFn: () => authorizedClients(server), enabled: enabled && open && tab === 'clients', retry: false, refetchOnWindowFocus: false, refetchOnReconnect: false });
  const refresh = () => {
    void client.invalidateQueries({ queryKey: ['client-auth-pending', server] });
    void client.invalidateQueries({ queryKey: ['client-auth-clients', server] });
  };
  const decide = useMutation({ mutationFn: ({ request, approve }: { request: ClientAuthorization; approve: boolean }) => decideClientRequest(server, request, approve), onSuccess: refresh, onError: refresh });
  const revoke = useMutation({ mutationFn: (id: string) => revokeClient(server, id), onSuccess: (_, id) => {
    setConfirm(undefined);
    if (id === clients.data?.current_id) invalidateClientCredential(server, loadClientCredential(server)?.token);
    refresh();
  } });
  useEffect(() => {
    if (!enabled) { setOpen(false); return; }
    if (!open && pending.data?.some(request => !dismissed.current.has(request.id))) { decide.reset(); revoke.reset(); setConfirm(undefined); setTab('pending'); setAutomatic(true); setOpen(true); }
    if (open && automatic && pending.isSuccess && pending.data.length === 0) setOpen(false);
  }, [enabled, open, automatic, pending.data, pending.isSuccess]);
  const close = () => {
    for (const request of pending.data ?? []) dismissed.current.add(request.id);
    setOpen(false); setAutomatic(false); decide.reset(); revoke.reset();
  };
  if (!enabled) return null;
  return <>
    <button type="button" className={'icon-button client-access-trigger' + (pending.isError ? ' has-error' : '')} aria-label={t('access.manage')} title={t('access.manage')} onClick={() => { setAutomatic(false); setOpen(true); }}><ShieldCheck size={18} />{!!pending.data?.length && <span className="client-access-badge">{pending.data.length}</span>}</button>
    {open && <Dialog title={t('access.manage')} onClose={close} className="client-access-dialog">
      <div className="client-access-tabs"><button type="button" className={tab === 'pending' ? 'primary' : 'ghost'} onClick={() => { setTab('pending'); setAutomatic(false); }}>{t('access.pending')}</button><button type="button" className={tab === 'clients' ? 'primary' : 'ghost'} onClick={() => { setTab('clients'); setAutomatic(false); }}>{t('access.clients')}</button></div>
      {tab === 'pending' ? <>
        <p className="muted">{t('access.verifyHint')}</p>
        {pending.isPending && <LoadingState />}
        {pending.isError && <ErrorState compact title={t('access.requestsFailed')} description={pending.error.message} onRetry={() => void pending.refetch()} retrying={pending.isFetching} />}
        {pending.isSuccess && pending.data.length === 0 && <EmptyState compact title={t('access.noPending')} description={t('access.noPendingHint')} />}
        {(pending.data ?? []).map(request => <article className="client-access-card" key={request.id}>
          <h3>{request.name}</h3><p>{request.platform} · {request.address}</p>{request.origin && <p className="muted">{request.origin}</p>}
          <output className="client-auth-code" aria-label={t('access.code')}>{request.code}</output>
          <p className="muted">{t('access.expires', { time: new Date(request.expires_at * 1000).toLocaleTimeString(locale) })}</p>
          <div className="client-access-actions"><button type="button" className="primary" disabled={decide.isPending} onClick={() => decide.mutate({ request, approve: true })}>{t('access.approve')}</button><button type="button" className="ghost" disabled={decide.isPending} onClick={() => decide.mutate({ request, approve: false })}>{t('access.reject')}</button></div>
        </article>)}
        {decide.error && <MutationError mutation={decide} />}
      </> : <>
        {clients.isPending && <LoadingState />}
        {clients.isError && <ErrorState compact title={t('access.clientsFailed')} description={clients.error.message} onRetry={() => void clients.refetch()} retrying={clients.isFetching} />}
        {clients.isSuccess && !clients.data.clients.length && <EmptyState compact title={t('access.noClients')} description={t('access.noClientsHint')} />}
        {(clients.data?.clients ?? []).map(item => <article className="client-access-card" key={item.id}>
          <h3>{item.name}{item.id === clients.data?.current_id ? ' · ' + t('access.current') : ''}</h3><p>{item.platform} · {item.address}</p><p className="muted">{t('access.status.' + item.status)}</p>
          {item.status === 'approved' && (confirm === item.id ? <><p>{t('access.revokeHint')}</p><div className="client-access-actions"><button type="button" className="danger" disabled={revoke.isPending} onClick={() => revoke.mutate(item.id)}>{t('access.confirmRevoke')}</button><button type="button" className="ghost" disabled={revoke.isPending} onClick={() => setConfirm(undefined)}>{t('common.cancel')}</button></div></> : <button type="button" className="ghost" onClick={() => setConfirm(item.id)}>{t('access.revoke')}</button>)}
        </article>)}
        {revoke.error && <MutationError mutation={revoke} />}
      </>}
      <div className="dialog-actions"><button type="button" className="ghost" onClick={close}>{t('access.later')}</button></div>
    </Dialog>}
  </>;
}
