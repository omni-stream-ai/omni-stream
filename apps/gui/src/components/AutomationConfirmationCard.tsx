import { useEffect, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Link } from 'react-router-dom';
import { automationApi, type AutomationConfirmation } from '../lib/api';
import { useI18n } from '../i18n';
import { ErrorState, LoadingState, MutationError } from './StateView';

export function AutomationConfirmationCard({ server, automationId, confirmationId, onOpenConversation }: { server: string; automationId: number; confirmationId: number; onOpenConversation?: (id: string) => void }) {
  const { t } = useI18n();
  const query = useQuery({ queryKey: ['automation-confirmation', server, automationId, confirmationId], queryFn: () => automationApi.confirmation(automationId, confirmationId), retry: false, refetchOnWindowFocus: false, refetchOnReconnect: false, refetchInterval: query => query.state.status === 'error' || (query.state.data && !['pending', 'approved', 'ready'].includes(query.state.data.status)) ? false : 2000 });
  if (query.isError) return <ErrorState compact title={t('confirmation.loadFailed')} description={query.error.message} onRetry={() => void query.refetch()} retrying={query.isFetching} />;
  if (!query.data) return <LoadingState />;
  return <ConfirmationEditor onOpenConversation={onOpenConversation} server={server} automationId={automationId} review={query.data} refresh={() => void query.refetch()} />;
}

function ConfirmationEditor({ server, automationId, review, refresh, onOpenConversation }: { server: string; automationId: number; review: AutomationConfirmation; refresh: () => void; onOpenConversation?: (id: string) => void }) {
  const { t, locale } = useI18n();
  const client = useQueryClient();
  const [saved, setSaved] = useState({ goal: review.goal, revision: review.revision });
  const [goal, setGoal] = useState(review.goal);
  const [future, setFuture] = useState(false);
  const dirty = goal !== saved.goal;
  const stale = saved.revision !== review.revision;
  const active = ['pending', 'approved'].includes(review.status);
  const policy = review.trigger === 'manual' ? 'wait' : review.snapshot.on_timeout ?? (review.deadline === null ? 'wait' : 'skip');
  const expired = (review.status === 'pending' || policy === 'run') && review.deadline !== null && review.deadline <= Date.now() / 1000;
  useEffect(() => {
    if (!dirty && stale) { setGoal(review.goal); setSaved({ goal: review.goal, revision: review.revision }); }
  }, [review.goal, review.revision, dirty, stale]);
  const mutation = useMutation({
    mutationFn: (action: 'update' | 'approve' | 'skip') => automationApi.decide(automationId, review.id, action, { revision: action === 'update' ? saved.revision : review.revision, ...(action === 'update' ? { goal } : action === 'approve' ? { apply_to_future: future } : {}) }),
    onSuccess: updated => {
      setGoal(updated.goal); setSaved({ goal: updated.goal, revision: updated.revision });
      client.setQueryData(['automation-confirmation', server, automationId, review.id], updated);
      void client.invalidateQueries({ queryKey: ['automations', server] });
      void client.invalidateQueries({ queryKey: ['conversations', server] });
    },
    onError: refresh,
  });
  const date = (value: number) => new Date((value + review.snapshot.utc_offset_seconds) * 1000).toLocaleString(locale, { timeZone: 'UTC' });
  const window = review.snapshot.time_window;
  return <section className="automation-confirmation" aria-label={t('confirmation.title')}>
    <div className="section-heading"><strong>{review.snapshot.title} · {t('confirmation.title')}</strong><span className="status">{t(review.status === 'consumed' && review.execution_source === 'timeout' ? 'confirmation.autoConsumed' : 'confirmation.' + review.status)}</span></div>
    <p>{t('confirmation.scheduled')}: {date(review.scheduled_at)} · UTC{review.snapshot.utc_offset_seconds >= 0 ? '+' : ''}{review.snapshot.utc_offset_seconds / 3600}</p>
    {window?.start !== undefined && window.end !== undefined && <p>{t('automation.window')}: {date(window.start)} — {date(window.end)}</p>}
    {window?.error && <ErrorState compact title={t('common.operationFailed')} description={window.error} onRetry={refresh} />}
    <p className="muted">{t(policy === 'run' ? 'confirmation.runHint' : policy === 'wait' ? 'confirmation.waitHint' : 'confirmation.skipHint')}</p>
    <p>{t('automation.targets')}: {t('automation.inApp')}</p>
    {review.snapshot.delivery_targets.filter(target => target.type === 'plugin_action').map(target => <p key={target.id}>{target.plugin} · {target.action} · {JSON.stringify(target.destination)} · {review.snapshot.accounts.find(account => account.target_id === target.id)?.account.account ?? '—'}</p>)}
    <p className="muted">{t('confirmation.revision')}: {review.revision}</p>
    <label>{t('confirmation.goal')}<textarea rows={4} maxLength={40000} value={goal} disabled={!active || expired || mutation.isPending} onChange={event => setGoal(event.target.value)} /></label>
    {['ready', 'consumed'].includes(review.status) && review.execution_goal && review.execution_goal !== review.goal && <div><strong>{t('confirmation.executionGoal')}</strong><p>{review.execution_goal}</p></div>}
    {stale && dirty && <ErrorState compact title={t('confirmation.stale')} description={t('confirmation.reload')} retryLabel={t('confirmation.reload')} onRetry={() => { setGoal(review.goal); setSaved({ goal: review.goal, revision: review.revision }); }} />}
    {review.discussion_active && active && !expired && <p role="status">{t('confirmation.discussing')}</p>}
    {active && !expired && <>
      <label><input type="checkbox" checked={future} disabled={mutation.isPending} onChange={event => setFuture(event.target.checked)} />{t('confirmation.future')}</label>
      <div className="automation-actions">
        <button type="button" className="ghost" disabled={!dirty || stale || !goal.trim() || mutation.isPending} onClick={() => mutation.mutate('update')}>{t('confirmation.save')}</button>
        <button type="button" className="primary" disabled={dirty || stale || review.discussion_active || mutation.isPending || review.status === 'approved'} onClick={() => mutation.mutate('approve')}>{t('confirmation.approve')}</button>
        <button type="button" className="ghost" disabled={mutation.isPending} onClick={() => mutation.mutate('skip')}>{t('confirmation.skip')}</button>
        {onOpenConversation ? <button type="button" className="ghost" onClick={() => onOpenConversation(review.conversation)}>{t('confirmation.discuss')}</button> : <Link to={'/chat/' + encodeURIComponent(review.conversation)}>{t('confirmation.discuss')}</Link>}
      </div>
    </>}
    {mutation.error && <MutationError mutation={mutation} disabled={!active || expired || stale} />}
  </section>;
}
