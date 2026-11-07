import { useMutation, useQuery } from '@tanstack/react-query';
import { isTauri } from '@tauri-apps/api/core';
import { ShieldCheck } from 'lucide-react';
import { useEffect, useState } from 'react';
import { useI18n } from '../i18n';
import { readOwnClientRequest, requestClientAccess } from '../lib/clientAuthorization';
import { useClientCredential } from '../lib/useClientCredential';
import { isNativeMobile } from '../lib/platform';
import { ErrorState, LoadingState, MutationError } from './StateView';

export function ClientAuthGate({ server, onAuthorized, onChangeServer }: { server: string; onAuthorized: () => void; onChangeServer: () => void }) {
  const { locale, t } = useI18n();
  const saved = useClientCredential(server);
  const platform = isNativeMobile() ? (window.__OMNISTREAM_ANDROID__ ? 'Android' : 'iOS') : isTauri() ? 'Desktop' : navigator.platform || 'Browser';
  const [name, setName] = useState(saved?.name ?? t('access.defaultName', { platform }));
  const create = useMutation({ mutationFn: (name: string) => requestClientAccess(server, name, platform) });
  const cancel = useMutation({ mutationFn: () => readOwnClientRequest(server, true) });
  const request = saved?.request;
  const own = useQuery({
    queryKey: ['client-auth-own', server, request?.id], queryFn: () => readOwnClientRequest(server),
    enabled: (request?.status === 'pending' || request?.status === 'approved') && !cancel.isPending,
    retry: false, refetchOnWindowFocus: false, refetchOnReconnect: false,
    refetchInterval: query => query.state.status === 'error' || query.state.data?.status !== 'pending' ? false : 3000,
  });
  useEffect(() => { if (request?.status === 'approved') onAuthorized(); }, [request?.id, request?.status, onAuthorized]);
  return <section className="client-auth-gate" aria-label={t('access.required')}>
    <ShieldCheck size={30} aria-hidden="true" /><h1>{t('access.required')}</h1><p className="muted">{t('access.scope')}</p><p className="failure-address">{server}</p>
    {request?.status === 'pending' ? <div className="client-auth-waiting">
      <h2>{t('access.waiting')}</h2><p>{t('access.waitingHint')}</p>
      <output className="client-auth-code" aria-label={t('access.code')}>{request.code}</output>
      <p className="muted">{t('access.expires', { time: new Date(request.expires_at * 1000).toLocaleTimeString(locale) })}</p>
      <details><summary>{t('access.firstClient')}</summary><p>{t('access.terminalHint')}</p><code className="client-auth-command">{'omnistream client-auth approve ' + request.id + ' ' + request.code}</code></details>
      {own.isError && <ErrorState compact title={t('access.statusFailed')} description={own.error.message} onRetry={() => void own.refetch()} retrying={own.isFetching} />}
      <button type="button" className="ghost" disabled={cancel.isPending} onClick={() => cancel.mutate()}>{t('access.cancelRequest')}</button>
      {cancel.error && <MutationError mutation={cancel} />}
    </div> : request?.status === 'approved' ? <>{own.isError ? <ErrorState compact title={t('access.statusFailed')} description={own.error.message} onRetry={() => void own.refetch()} retrying={own.isFetching} /> : <LoadingState label={t('access.entering')} />}<button type="button" className="ghost" onClick={onAuthorized}>{t('common.connect')}</button></> : <>
      {request && <p role="status">{t('access.status.' + request.status)}</p>}
      <form className="dialog-form" onSubmit={event => { event.preventDefault(); create.mutate(name.trim()); }}>
        <label>{t('access.clientName')}<input required maxLength={80} value={name} onChange={event => { setName(event.target.value); create.reset(); }} /></label>
        <button type="submit" className="primary" disabled={create.isPending || !name.trim()}>{t(create.isPending ? 'common.loading' : 'access.request')}</button>
      </form>
      {create.error && <MutationError mutation={create} />}
    </>}
    <button type="button" className="ghost" onClick={onChangeServer}>{t('connection.checkAddress')}</button>
  </section>;
}
