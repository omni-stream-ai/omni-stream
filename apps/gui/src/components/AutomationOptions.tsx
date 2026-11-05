import { useQuery } from '@tanstack/react-query';
import { pluginApi, type AutomationOptions as Options, type AutomationTarget, type DateOverride } from '../lib/api';
import { useI18n } from '../i18n';
import { ErrorState } from './StateView';

export function AutomationOptions({ value, onChange, server }: { value: Options; onChange: (value: Options) => void; server: string }) {
  const { t } = useI18n();
  const plugins = useQuery({ queryKey: ['automation-plugins', server], queryFn: pluginApi.marketplace, retry: false, refetchOnWindowFocus: false, refetchOnReconnect: false });
  const set = (patch: Partial<Options>) => onChange({ ...value, ...patch });
  const calendar = value.calendar ?? {};
  const targets = value.delivery_targets ?? [];
  const changeTarget = (index: number, target: AutomationTarget) => set({ delivery_targets: targets.map((current, i) => i === index ? target : current) });
  const available = (plugins.data ?? []).filter(plugin => plugin.installed && plugin.connection.enabled && plugin.connection.allow_writes && plugin.actions.some(action => action.delivery_destination_fields));
  const dates = (field: 'include_dates' | 'exclude_dates') => {
    const entries = calendar[field] ?? [];
    const update = (next: DateOverride[]) => set({ calendar: { ...calendar, [field]: next } });
    return <fieldset><legend>{t('automation.' + field)}</legend>{entries.map((entry, index) => <div className="automation-schedule" key={index}>
      <label>{t('automation.date')}<input type="date" required value={entry.date} onChange={event => update(entries.map((row, i) => i === index ? { ...row, date: event.target.value } : row))} /></label>
      <label>{t('automation.source')}<input required value={entry.source} onChange={event => update(entries.map((row, i) => i === index ? { ...row, source: event.target.value } : row))} /></label>
      <label>{t('automation.reason')}<input required value={entry.reason} onChange={event => update(entries.map((row, i) => i === index ? { ...row, reason: event.target.value } : row))} /></label>
      <button type="button" className="ghost" onClick={() => update(entries.filter((_, i) => i !== index))}>{t('automation.remove')}</button>
    </div>)}<button type="button" className="ghost" onClick={() => update([...entries, { date: '', source: 'user', reason: '' }])}>{t('automation.addDate')}</button></fieldset>;
  };
  return <details className="automation-options"><summary>{t('automation.options')}</summary>
    <fieldset><legend>{t('confirmation.settings')}</legend>
      <label><input type="checkbox" checked={!!value.confirmation} onChange={event => set({ confirmation: event.target.checked ? { before_seconds: 600, on_timeout: 'skip' } : null })} />{t('confirmation.enable')}</label>
      {value.confirmation && <>
        <label>{t('confirmation.leadSeconds')}<input type="number" min={1} max={604800} required value={value.confirmation.before_seconds} onChange={event => set({ confirmation: { ...value.confirmation!, before_seconds: Number(event.target.value) } })} /></label>
        <label>{t('confirmation.timeout')}<select value={value.confirmation.on_timeout} onChange={event => set({ confirmation: { ...value.confirmation!, on_timeout: event.target.value as 'skip' | 'wait' | 'run' } })}><option value="skip">{t('confirmation.timeoutSkip')}</option><option value="wait">{t('confirmation.timeoutWait')}</option><option value="run">{t('confirmation.timeoutRun')}</option></select></label>
        <p className="muted">{t('confirmation.settingsHint')}</p>
        {value.confirmation.on_timeout === 'run' && <p className="muted">{t('confirmation.runHint')}</p>}
      </>}
    </fieldset>
    <fieldset><legend>{t('automation.weekdays')}</legend><p className="muted">{t('automation.calendarHint')}</p>
      {[1, 2, 3, 4, 5, 6, 7].map(day => <label key={day}><input type="checkbox" checked={calendar.weekdays?.includes(day) ?? false} onChange={event => set({ calendar: { ...calendar, weekdays: event.target.checked ? [...(calendar.weekdays ?? []), day].sort() : calendar.weekdays?.filter(d => d !== day) } })} />{t('automation.weekday' + day)}</label>)}
    </fieldset>
    {dates('include_dates')}{dates('exclude_dates')}
    <label><input type="checkbox" checked={value.allow_calendar_updates ?? false} onChange={event => set({ allow_calendar_updates: event.target.checked })} />{t('automation.allowCalendar')}</label>
    <label>{t('automation.window')}<select aria-label={t('automation.window')} value={value.time_window?.kind ?? ''} onChange={event => set({ time_window: event.target.value === 'lookback' ? { kind: 'lookback', seconds: 86400 } : event.target.value === 'previous_effective_day' ? { kind: 'previous_effective_day', hour: 12, minute: 0, second: 0 } : null })}><option value="">{t('automation.noWindow')}</option><option value="lookback">{t('automation.lookback')}</option><option value="previous_effective_day">{t('automation.previousDay')}</option></select></label>
    {value.time_window?.kind === 'lookback' && <label>{t('automation.windowSeconds')}<input type="number" min={1} max={31536000} required value={value.time_window.seconds} onChange={event => set({ time_window: { kind: 'lookback', seconds: Number(event.target.value) } })} /></label>}
    {value.time_window?.kind === 'previous_effective_day' && <label>{t('automation.cutoff')}<input type="time" step={1} required value={[value.time_window.hour, value.time_window.minute, value.time_window.second ?? 0].map(n => String(n).padStart(2, '0')).join(':')} onChange={event => { const [hour, minute, second = 0] = event.target.value.split(':').map(Number); set({ time_window: { kind: 'previous_effective_day', hour, minute, second } }); }} /></label>}
    <label><input type="checkbox" checked={value.silent_unmet ?? false} onChange={event => set({ silent_unmet: event.target.checked })} />{t('automation.silent')}</label>
    <fieldset><legend>{t('automation.targets')}</legend><p className="muted">{t('automation.targetsHint')}</p>
      {plugins.isError && <ErrorState compact title={t('automation.pluginsFailed')} description={plugins.error.message} onRetry={() => void plugins.refetch()} retrying={plugins.isFetching} />}
      {targets.map((target, index) => {
        const plugin = target.type === 'plugin_action' ? plugins.data?.find(p => p.manifest.id === target.plugin) : undefined;
        const descriptor = target.type === 'plugin_action' ? plugin?.actions.find(a => a.id === target.action) : undefined;
        return <fieldset key={target.id}><legend>{target.id}</legend>
          {target.type === 'in_app' ? <p>{t('automation.inApp')}</p> : <>
            <label>{t('automation.application')}<select required aria-label={t('automation.application')} value={target.plugin} onChange={event => changeTarget(index, { ...target, plugin: event.target.value, action: '', destination: {}, payload: {} })}><option value="">{t('automation.select')}</option>{available.map(p => <option key={p.manifest.id} value={p.manifest.id}>{p.manifest.name} · {p.connection.account}</option>)}{target.plugin && !available.some(p => p.manifest.id === target.plugin) && <option value={target.plugin}>{target.plugin} — {t('automation.unavailable')}</option>}</select></label>
            <label>{t('automation.action')}<select required aria-label={t('automation.action')} value={target.action} onChange={event => changeTarget(index, { ...target, action: event.target.value, destination: {}, payload: {} })}><option value="">{t('automation.select')}</option>{plugin?.actions.filter(a => a.delivery_destination_fields).map(a => <option key={a.id} value={a.id}>{a.id}</option>)}</select></label>
            <p className="muted">{t('automation.account')}: {plugin?.connection.account ?? '—'} · {plugin?.connection.auth_mode ?? ''}</p>
            {descriptor && Object.entries(descriptor.input_schema.properties).map(([key, schema]) => {
              const destination = descriptor.delivery_destination_fields?.includes(key);
              return <label key={key}>{key} · {t(destination ? 'automation.destination' : 'automation.content')}<input required={descriptor.input_schema.required.includes(key)} maxLength={schema.maxLength} value={(destination ? target.destination : target.payload)[key] ?? ''} placeholder={destination ? t('automation.fixedId') : '{{run.output_markdown}}'} onChange={event => { const field = destination ? 'destination' : 'payload'; const next = { ...target[field] }; if (event.target.value) next[key] = event.target.value; else delete next[key]; changeTarget(index, { ...target, [field]: next }); }} /></label>;
            })}
            <label>{t('automation.attempts')}<input type="number" min={1} max={3} required value={target.max_attempts ?? 1} onChange={event => changeTarget(index, { ...target, max_attempts: Number(event.target.value) })} /></label>
          </>}
          <button type="button" className="ghost" onClick={() => set({ delivery_targets: targets.filter((_, i) => i !== index) })}>{t('automation.remove')}</button>
        </fieldset>;
      })}
      <button type="button" className="ghost" disabled={targets.length >= 16} onClick={() => set({ delivery_targets: [...targets, { type: 'plugin_action', id: 'target-' + crypto.randomUUID(), plugin: '', action: '', destination: {}, payload: {} }] })}>{t('automation.addTarget')}</button>
    </fieldset>
    <label>{t('automation.failurePolicy')}<select aria-label={t('automation.failurePolicy')} value={value.delivery_policy ?? 'independent'} onChange={event => set({ delivery_policy: event.target.value as Options['delivery_policy'] })}><option value="independent">{t('automation.independent')}</option><option value="fail_fast">{t('automation.failFast')}</option><option value="fallback_in_app">{t('automation.fallback')}</option></select></label>
  </details>;
}
