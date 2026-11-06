import { useState, type FormEvent } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { api, automationApi, type Automation, type AutomationDraft, type AutomationSchedule, type AutomationTarget, type AutomationOptions as Options } from '../lib/api';
import { useI18n } from '../i18n';
import { EmptyState, ErrorState, LoadingState, MutationError } from './StateView';
import { MessageContent } from './MessageContent';
import { AutomationOptions } from './AutomationOptions';
import { AutomationConfirmationCard } from './AutomationConfirmationCard';
import { ApprovalForm } from './ApprovalForm';

export function AutomationPanel({ server, conversation, onOpenConversation }: { server: string; conversation: string; onOpenConversation: (id: string) => void }) {
  const { t, locale } = useI18n();
  const client = useQueryClient();
  const items = useQuery({ queryKey: ['automations', server], queryFn: automationApi.list, retry: false, refetchOnWindowFocus: false, refetchOnReconnect: false, refetchInterval: query => query.state.status === 'error' ? false : 2000 });
  const approvals = useQuery({ queryKey: ['automation-approvals', server], queryFn: api.approvals, retry: false, refetchOnWindowFocus: false, refetchOnReconnect: false, refetchInterval: query => query.state.status === 'error' ? false : 2000 });
  const agents = useQuery({ queryKey: ['agents', server], queryFn: api.agents, retry: false, refetchOnWindowFocus: false, refetchOnReconnect: false });
  const [agentId, setAgentId] = useState<number | null>(null);
  const [options, setOptions] = useState<Options>({});
  const [editing, setEditing] = useState<number | null>(null);
  const [title, setTitle] = useState('');
  const [goal, setGoal] = useState('');
  const [kind, setKind] = useState<'daily' | 'interval'>('daily');
  const [time, setTime] = useState('09:00');
  const [offset, setOffset] = useState(-new Date().getTimezoneOffset());
  const [minutes, setMinutes] = useState(60);
  const [project, setProject] = useState('');
  const [destination, setDestination] = useState(conversation);
  const [formError, setFormError] = useState('');
  const refresh = () => { void approvals.refetch(); void client.invalidateQueries({ queryKey: ['automations', server] }); void client.invalidateQueries({ queryKey: ['conversations', server] }); };
  const reset = () => { setEditing(null); setAgentId(null); setOptions({}); setTitle(''); setGoal(''); setProject(''); setDestination(conversation); setFormError(''); };
  const save = useMutation({ mutationFn: (draft: AutomationDraft) => editing === null ? automationApi.create(draft) : automationApi.update(editing, draft), onSuccess: () => { reset(); refresh(); } });
  const action = useMutation({ mutationFn: ({ id, command }: { id: number; command: 'activate' | 'pause' | 'resume' | 'cancel' | 'run' | 'revoke' }) => automationApi.action(id, command), onSuccess: refresh });
  const date = (value: number | null) => value === null ? '—' : new Date(value * 1000).toLocaleString(locale);
  const scheduleLabel = (schedule: AutomationSchedule) => schedule.kind === 'interval'
    ? t('automation.everyMinutes', { count: schedule.seconds / 60 })
    : t('automation.daily') + ' ' + [schedule.hour, schedule.minute, schedule.second ?? 0].map(n => String(n).padStart(2, '0')).join(':') + ' UTC' + (schedule.utc_offset_minutes >= 0 ? '+' : '-') + String(Math.floor(Math.abs(schedule.utc_offset_minutes) / 60)).padStart(2, '0') + ':' + String(Math.abs(schedule.utc_offset_minutes) % 60).padStart(2, '0');
  const targetLabel = (target?: AutomationTarget) => !target ? '' : target.type === 'in_app' ? t('automation.inApp') : [target.plugin, target.action, ...Object.entries(target.destination).map(([key, value]) => key + ': ' + value)].join(' · ');
  const edit = (item: Automation) => {
    setAgentId(item.agent_id ?? null);
    setOptions({ confirmation: item.confirmation, calendar: item.calendar, time_window: item.time_window, delivery_targets: item.delivery_targets, delivery_policy: item.delivery_policy, silent_unmet: item.silent_unmet, allow_calendar_updates: item.allow_calendar_updates });
    setEditing(item.id); setTitle(item.title); setGoal(item.goal); setProject(item.project ?? ''); setDestination(item.conversation ?? conversation); setKind(item.schedule.kind);
    if (item.schedule.kind === 'daily') { setTime([item.schedule.hour, item.schedule.minute, item.schedule.second ?? 0].map(n => String(n).padStart(2, '0')).join(':')); setOffset(item.schedule.utc_offset_minutes); }
    else setMinutes(item.schedule.seconds / 60);
    save.reset(); setFormError('');
  };
  const submit = (event: FormEvent) => {
    event.preventDefault();
    const [hour, minute, second = 0] = time.split(':').map(Number);
    if (kind === 'daily' && (!time || !Number.isInteger(offset))) { setFormError(t('automation.invalidTime')); return; }
    save.mutate({ ...options, agent_id: agentId, title: title.trim() || goal.trim().slice(0, 40), goal: goal.trim(), project: project.trim() || null, conversation: destination.trim() || null,
      schedule: kind === 'daily' ? { kind, hour, minute, ...(second ? { second } : {}), utc_offset_minutes: offset } : { kind, seconds: Math.round(minutes * 60) } });
  };
  return <div className="automation-panel">
    <p className="muted">{t('automation.description')}</p>
    <form className="dialog-form" onSubmit={submit}>
      <h3>{t(editing === null ? 'automation.create' : 'automation.edit')}</h3>
      <label>{t('automation.titleLabel')}<input value={title} onChange={event => setTitle(event.target.value)} placeholder={t('automation.titleExample')} /></label>
      <label>{t('automation.goal')}<textarea required value={goal} onChange={event => setGoal(event.target.value)} rows={3} placeholder={t('automation.example')} /></label>
      <div className="automation-schedule">
        <label>{t('automation.trigger')}<select value={kind} onChange={event => setKind(event.target.value as 'daily' | 'interval')}><option value="daily">{t('automation.daily')}</option><option value="interval">{t('automation.interval')}</option></select></label>
        {kind === 'daily' ? <><label>{t('automation.time')}<input required type="time" step={1} value={time} onChange={event => setTime(event.target.value)} /></label><label>{t('automation.offset')}<input required type="number" min={-720} max={840} step={1} value={offset} onChange={event => setOffset(Number(event.target.value))} /></label></> : <label>{t('automation.minutes')}<input required type="number" min={1} max={525600} value={minutes} onChange={event => setMinutes(Number(event.target.value))} /></label>}
      </div>
      <label>{t('automation.workspace')}<input value={project} onChange={event => setProject(event.target.value)} placeholder={t('automation.workspaceHint')} /></label>
      <p className="muted">{t('automation.delivery')}</p>
      <label>{t('agents.forAutomation')}<select value={agentId ?? ''} onChange={event => setAgentId(event.target.value ? Number(event.target.value) : null)}><option value="">{t('agents.default')}</option>{agentId !== null && !agents.data?.some(agent => agent.id === agentId) && <option value={agentId}>#{agentId} · {t('automation.unavailable')}</option>}{agents.data?.map(agent => <option key={agent.id} value={agent.id}>@{agent.name}</option>)}</select></label>
      {agents.isError && <ErrorState compact title={t('agents.loadFailed')} description={agents.error.message} onRetry={() => void agents.refetch()} retrying={agents.isFetching} />}
      {agents.isPending && <LoadingState lines={1} />}
      <AutomationOptions server={server} value={options} onChange={setOptions} />
      {formError || save.error ? <ErrorState compact title={t('common.operationFailed')} description={formError || save.error?.message || ''} onRetry={() => formError ? setFormError('') : save.variables && save.mutate(save.variables)} retryLabel={formError ? t('common.edit') : t('common.retry')} retrying={save.isPending} /> : null}
      <div className="dialog-actions">{editing !== null && <button type="button" className="ghost" onClick={reset}>{t('common.cancel')}</button>}<button className="primary" disabled={save.isPending || !goal.trim()}>{save.isPending ? t('common.saving') : t(editing === null ? 'automation.createAndEnable' : 'common.save')}</button></div>
    </form>
    {action.error && <MutationError mutation={action} />}
    {items.isPending ? <LoadingState /> : items.isError ? <ErrorState title={t('automation.loadFailed')} description={items.error.message} onRetry={() => void items.refetch()} retrying={items.isFetching} /> : !items.data.length ? <EmptyState compact title={t('automation.empty')} description={t('automation.example')} /> : items.data.map(item => {
      const running = item.runs.some(run => run.status === 'running');
      const queued = item.manual_run_at != null;
      const invoke = (command: 'activate' | 'pause' | 'resume' | 'cancel' | 'run' | 'revoke') => action.mutate({ id: item.id, command });
      return <article className="automation-card" key={item.id}>
        <div className="section-heading"><h3>{item.title}</h3><span className={`status ${item.status}`}>{t(`automation.${item.status}`)}</span></div>
        <p>{item.goal}</p><p className="muted">{scheduleLabel(item.schedule)} · {t('automation.next')}: {date(item.next_run_at)}</p>
        {item.calendar && <p className="muted">{t('automation.weekdays')}: {item.calendar.weekdays?.join(', ') || t('automation.daily')} · {t('automation.include_dates')}: {item.calendar.include_dates?.map(d => d.date).join(', ') || '—'} · {t('automation.exclude_dates')}: {item.calendar.exclude_dates?.map(d => d.date).join(', ') || '—'}</p>}
        {item.time_window && <p>{t('automation.window')}: {item.time_window.kind === 'lookback' ? t('automation.lookback') + ' · ' + item.time_window.seconds + ' s' : t('automation.previousDay') + ' · ' + [item.time_window.hour, item.time_window.minute, item.time_window.second ?? 0].map(n => String(n).padStart(2, '0')).join(':')}</p>}
        {!!item.delivery_targets?.length && <details open><summary>{t('automation.targets')}</summary>{item.delivery_targets.map(target => <p key={target.id}>{target.type === 'in_app' ? t('automation.inApp') : target.plugin + ' · ' + target.action + ' · ' + JSON.stringify(target.destination) + ' · ' + Object.entries(target.payload).map(([key, content]) => key + ': ' + content).join(' · ')} · {item.delivery_grants?.find(g => g.target_id === target.id)?.account.account ?? '—'}</p>)}</details>}
        {item.confirmation && <p className="muted">{t('confirmation.settings')} · {item.confirmation.before_seconds} s · {t(item.confirmation.on_timeout === 'wait' ? 'confirmation.timeoutWait' : item.confirmation.on_timeout === 'run' ? 'confirmation.timeoutRun' : 'confirmation.timeoutSkip')}</p>}
        {(item.confirmations ?? []).filter(review => ['pending', 'approved', 'ready'].includes(review.status)).map(review => <AutomationConfirmationCard key={review.id} server={server} automationId={item.id} confirmationId={review.id} onOpenConversation={onOpenConversation} />)}
        {!!item.confirmations?.some(review => !['pending', 'approved', 'ready'].includes(review.status)) && <details><summary>{t('confirmation.history')}</summary>{[...item.confirmations].reverse().filter(review => !['pending', 'approved', 'ready'].includes(review.status)).map(review => <p key={review.id}>#{review.id} · {date(review.scheduled_at)} · {t('confirmation.' + review.status)}</p>)}</details>}
        <div className="automation-actions">
          {item.status === 'pending' && <button className="primary" disabled={action.isPending} onClick={() => invoke('activate')}>{t('automation.enable')}</button>}
          {item.status === 'active' && <><button className="ghost" disabled={action.isPending || running || queued} onClick={() => invoke('run')}>{t(queued ? 'status.queued' : 'automation.runNow')}</button><button className="ghost" disabled={action.isPending} onClick={() => invoke('pause')}>{t('automation.pause')}</button></>}
          {item.status === 'paused' && <button className="ghost" disabled={action.isPending} onClick={() => invoke('resume')}>{t('automation.resume')}</button>}
          {item.status !== 'cancelled' && <><button className="ghost" disabled={running || action.isPending} onClick={() => edit(item)}>{t('common.edit')}</button><button className="danger subtle" disabled={action.isPending} onClick={() => { if (window.confirm(t('automation.cancelConfirm', { title: item.title }))) invoke('cancel'); }}>{t('automation.stop')}</button></>}
          {!!item.delivery_grants?.length && <button className="ghost" disabled={action.isPending} onClick={() => invoke('revoke')}>{t('automation.revoke')}</button>}
          {item.conversation && <button className="ghost" onClick={() => onOpenConversation(item.conversation!)}>{t('automation.openResults')}</button>}
        </div>
        {approvals.isError && running && <ErrorState compact title={t('automation.approvalFailed')} description={approvals.error.message} onRetry={() => void approvals.refetch()} retrying={approvals.isFetching} />}
        {(approvals.data ?? []).filter(approval => item.runs.some(run => run.status === 'running' && run.scope && run.scope === approval.payload.scope)).map(approval => <ApprovalForm key={approval.id} approval={approval} agent={typeof approval.payload.agent === 'string' ? approval.payload.agent : item.title} task={item.id} onDone={refresh} />)}
        {!!item.authorization_audit?.length && <details><summary>{t('automation.authorizationAudit')}</summary>{item.authorization_audit.map((entry, i) => <p key={i}>{date(entry.at)} · {t('automation.authorization_' + entry.action)}</p>)}</details>}
        {!!item.calendar_audit?.length && <details><summary>{t('automation.audit')}</summary>{item.calendar_audit.map((entry, i) => <p key={i}>{date(entry.at)} · {entry.actor} · {entry.reason}</p>)}</details>}
        <details><summary>{t('automation.history')} ({item.runs.length})</summary>{!item.runs.length && <EmptyState compact title={t('automation.noRuns')} description={t('automation.history')} />}{[...item.runs].reverse().map(run => <section className="automation-run" key={run.id}><strong>#{run.id} · {t(`automation.${run.status}`)} · {date(run.started_at)}</strong>{run.context?.time_window?.start !== undefined && <p>{t('automation.window')}: {date(run.context.time_window.start)} — {date(run.context.time_window.end ?? null)}</p>}{run.skip_reason && <p>{t('automation.skipped')}: {run.skip_reason}</p>}{run.deliveries?.map(receipt => <p key={receipt.target_id}>{receipt.target_id} · {targetLabel(run.context?.delivery_targets?.find(target => target.id === receipt.target_id))} · {t('automation.' + receipt.status)} · {receipt.attempts} {receipt.error && '· ' + receipt.error}</p>)}{run.generation_output && run.generation_output !== run.output && <details><summary>{t('automation.generated')}</summary><MessageContent content={run.generation_output} /></details>}<MessageContent content={run.output || t('automation.running')} /></section>)}</details>
      </article>;
    })}
  </div>;
}
