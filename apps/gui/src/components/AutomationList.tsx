import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { ArrowRight, Clock3, Plus } from 'lucide-react';
import { automationApi, type Automation } from '../lib/api';
import { useI18n } from '../i18n';
import { EmptyState, ErrorState, LoadingState } from './StateView';
import { scheduleLabel } from './automationDisplay';

const statuses = ['pending', 'active', 'paused', 'cancelled'] as const;
export function AutomationList({ server, onOpen, onCreate }: { server: string; onOpen: (id: number) => void; onCreate: () => void }) {
  const { t, locale } = useI18n();
  const [filter, setFilter] = useState<'all' | Automation['status']>('all');
  const items = useQuery({ queryKey: ['automations', server], queryFn: automationApi.list, retry: false, refetchOnWindowFocus: false, refetchOnReconnect: false, refetchInterval: query => query.state.status === 'error' ? false : 2000 });
  const all = items.data ?? [];
  const visible = all.filter(item => filter === 'all' || item.status === filter).sort((a, b) => statuses.indexOf(a.status) - statuses.indexOf(b.status) || b.id - a.id);
  return <div className="automation-list">
    <div className="automation-list-toolbar"><p className="muted">{t('automation.listHint')}</p><button type="button" className="primary" onClick={onCreate}><Plus size={16} />{t('automation.create')}</button></div>
    {items.isPending ? <LoadingState /> : items.isError ? <ErrorState compact title={t('automation.loadFailed')} description={items.error.message} onRetry={() => void items.refetch()} retrying={items.isFetching} /> : <>
      <div className="automation-filters" role="group" aria-label={t('automation.filter')}>
        {(['all', ...statuses] as const).map(status => <button type="button" key={status} aria-pressed={filter === status} onClick={() => setFilter(status)}>{t('automation.' + status)} <span>{status === 'all' ? all.length : all.filter(item => item.status === status).length}</span></button>)}
      </div>
      {!visible.length ? <EmptyState compact title={t(all.length ? 'automation.noMatches' : 'automation.empty')} description={t(all.length ? 'automation.filterHint' : 'automation.example')} /> : <div className="automation-list-items">{visible.map(item => {
        const running = item.runs.some(run => run.status === 'running');
        const waiting = item.confirmations?.some(review => review.status === 'pending');
        const latest = item.runs.at(-1);
        const notice = running ? t('automation.running') : item.manual_run_at != null ? t('status.queued') : waiting ? t('automation.needsReview') : latest && ['failed', 'interrupted'].includes(latest.status) ? t('automation.lastFailed') : null;
        const next = item.status === 'pending' ? t('automation.awaitActivation') : item.status === 'paused' ? t('automation.pausedHint') : item.status === 'cancelled' ? t('automation.cancelledHint') : item.next_run_at == null ? t('automation.noNext') : new Date(item.next_run_at * 1000).toLocaleString(locale);
        return <article className="automation-list-card" key={item.id}>
          <div className="automation-list-heading"><h3>{item.title}</h3><span className={'status ' + item.status}>{t('automation.' + item.status)}</span></div>
          <p className="automation-list-goal">{item.goal}</p>
          <p className="automation-list-schedule"><Clock3 size={15} aria-hidden="true" /><span>{scheduleLabel(item, t)}</span></p>
          <div className="automation-list-footer"><div><span className="automation-next-label">{t('automation.next')}</span><p>{next}</p>{notice && <span className="automation-notice">{notice}</span>}</div><button type="button" className="ghost" aria-label={t('automation.openNamed', { title: item.title })} onClick={() => onOpen(item.id)}>{t('automation.openDraft')}<ArrowRight size={15} /></button></div>
        </article>;
      })}</div>}
    </>}
  </div>;
}
