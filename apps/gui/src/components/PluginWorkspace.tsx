import { useEffect, useRef, useState, type FormEvent } from 'react';
import { useMutation } from '@tanstack/react-query';
import { ArrowLeft, ExternalLink, LoaderCircle, Sparkles, Unplug } from 'lucide-react';
import { pluginApi, type MarketplacePlugin, type PluginAction, type PluginAuthMode, type PluginResult, type PluginSetupOptions, type SlackListenScope } from '../lib/api';
import { useI18n } from '../i18n';
import { EmptyState } from './StateView';

const tokenDocs: Record<string, string> = {
  slack: 'https://docs.slack.dev/authentication/tokens/',
  github: 'https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/managing-your-personal-access-tokens',
  notion: 'https://developers.notion.com/docs/create-a-notion-integration',
  figma: 'https://developers.figma.com/docs/rest-api/authentication/',
};
function connectionState(plugin: MarketplacePlugin) {
  const connection = plugin.connection;
  return JSON.stringify([connection.configured, connection.enabled, connection.allow_writes, connection.account, connection.auth_mode ?? 'token', connection.listen ?? false, connection.listen_scope ?? 'related', connection.app_token_configured ?? false, connection.user_id, connection.team_id]);
}

export function PluginWorkspace({ plugin, onBack, onChange, onRefresh, initialOptions, onSetupStarted }: { initialOptions?: PluginSetupOptions; onSetupStarted?: (conversation: string) => void; plugin: MarketplacePlugin; onBack: () => void; onChange: (plugin: MarketplacePlugin) => void; onRefresh: () => void }) {
  const { t, locale } = useI18n();
  const { id, name } = plugin.manifest;
  const connection = plugin.connection;
  const [token, setToken] = useState('');
  const [authMode, setAuthMode] = useState<PluginAuthMode>(initialOptions?.auth_mode ?? connection.auth_mode ?? 'token');
  const [appToken, setAppToken] = useState('');
  const [listen, setListen] = useState(initialOptions?.listen ?? connection.listen ?? false);
  const [listenScope, setListenScope] = useState<SlackListenScope>(initialOptions?.listen_scope ?? connection.listen_scope ?? 'related');
  const modeChanged = authMode !== (connection.auth_mode ?? 'token');
  const needsToken = !connection.configured || modeChanged;
  const needsAppToken = authMode === 'slack_user' && listen && (!connection.app_token_configured || modeChanged);
  const [enabled, setEnabled] = useState(initialOptions?.enabled ?? (connection.configured ? connection.enabled : true));
  const [allowWrites, setAllowWrites] = useState(initialOptions?.allow_writes ?? connection.allow_writes);
  const remoteState = connectionState(plugin);
  const lastRemoteState = useRef(remoteState);
  const ownUpdate = useRef<string | null>(null);
  const [settingsUpdated, setSettingsUpdated] = useState(false);
  const [connectionVersion, setConnectionVersion] = useState(0);
  const acceptSaved = (updated: MarketplacePlugin) => {
    ownUpdate.current = connectionState(updated);
    setToken(''); setSettingsUpdated(false);
    setAppToken(''); setListen(updated.connection.listen ?? false); setListenScope(updated.connection.listen_scope ?? 'related');
    setAuthMode(updated.connection.auth_mode ?? 'token');
    setEnabled(updated.connection.configured ? updated.connection.enabled : true);
    setAllowWrites(updated.connection.allow_writes);
    setConnectionVersion(value => value + 1);
    onChange(updated);
  };
  const setupRequest = useRef<{ key: string; id: string } | null>(null);
  const assist = useMutation({
    mutationFn: () => {
      // Explicitly construct a credential-free payload; never spread form state.
      const options: PluginSetupOptions = { auth_mode: id === 'slack' ? authMode : 'token', listen: id === 'slack' && authMode === 'slack_user' && listen, listen_scope: listenScope, enabled, allow_writes: allowWrites, locale };
      const key = JSON.stringify(options);
      if (setupRequest.current?.key !== key) setupRequest.current = { key, id: crypto.randomUUID() };
      return pluginApi.setup(id, options, setupRequest.current.id);
    },
    retry: false,
    onSuccess: result => { setToken(''); setAppToken(''); onSetupStarted?.(result.conversation); },
  });
  const save = useMutation({ mutationFn: () => pluginApi.connect(id, { token: token.trim() || undefined, ...(id === 'slack' ? { auth_mode: authMode, ...(authMode === 'slack_user' ? { app_token: appToken.trim() || undefined, listen, listen_scope: listenScope } : {}) } : {}), enabled, allow_writes: allowWrites }), retry: false,
    onSuccess: acceptSaved,
  });
  const test = useMutation({ mutationFn: () => pluginApi.test(id), retry: false, onSuccess: onChange, onError: onRefresh });
  const disconnect = useMutation({ mutationFn: () => pluginApi.disconnect(id), retry: false, onSuccess: updated => { save.reset(); test.reset(); acceptSaved(updated); } });
  useEffect(() => {
    if (lastRemoteState.current === remoteState) return;
    lastRemoteState.current = remoteState;
    if (ownUpdate.current === remoteState) { ownUpdate.current = null; return; }
    ownUpdate.current = null;
    setAuthMode(connection.auth_mode ?? 'token');
    setAppToken(''); setListen(connection.listen ?? false); setListenScope(connection.listen_scope ?? 'related');
    setToken(''); setEnabled(connection.configured ? connection.enabled : true); setAllowWrites(connection.allow_writes);
    setSettingsUpdated(true); save.reset(); test.reset(); disconnect.reset();
  }, [remoteState, connection.configured, connection.enabled, connection.allow_writes, connection.auth_mode, save.reset, test.reset, disconnect.reset]);
  const pending = save.isPending || test.isPending || disconnect.isPending || assist.isPending;
  const error = save.error ?? test.error ?? disconnect.error;
  const submit = (event: FormEvent) => { event.preventDefault(); test.reset(); disconnect.reset(); save.mutate(); };
  return <section className="plugin-workspace" aria-label={t('plugins.manageName', { name })}>
    <button type="button" className="ghost plugin-back" onClick={onBack}><ArrowLeft size={14} />{t('plugins.back')}</button>
    <div className="plugin-workspace-heading"><h3>{name}</h3><span className={'plugin-connection-status ' + connection.status}>{t('plugins.connection.' + connection.status)}</span></div>
    {connection.account && <p className="plugin-account">{t('plugins.account', { account: connection.account })}</p>}
    {connection.verified_at && <p className="muted plugin-verified">{t('plugins.verifiedAt', { time: new Date(connection.verified_at * 1000).toLocaleString(locale) })}</p>}
    <form className="dialog-form plugin-connection-form" onSubmit={submit}>
      {id === 'slack' && <label>{t('plugins.slack.authMode')}<select value={authMode} disabled={pending} onChange={event => { setAuthMode(event.target.value as PluginAuthMode); setToken(''); setAppToken(''); setListen(false); setAllowWrites(false); save.reset(); test.reset(); }}><option value="token">{t('plugins.slack.botMode')}</option><option value="slack_user">{t('plugins.slack.userMode')}</option></select></label>}
      <div className="plugin-setup-assist">
        <button type="button" className="primary" disabled={pending} onClick={() => assist.mutate()}>{assist.isPending ? <LoaderCircle size={14} className="spin" /> : <Sparkles size={14} />}{t(assist.isPending ? 'plugins.setupStarting' : 'plugins.setupAssist')}</button>
        <p className="muted">{t('plugins.setupHint')}</p>
        {assist.error && <p className="error" role="alert">{assist.error.message}</p>}
      </div>
      <details className="plugin-manual-setup"><summary>{t('plugins.manualSetup')}</summary>
        <p className="muted">{t('plugins.' + id + (authMode === 'slack_user' ? '.userSetup' : '.setup'))} <a href={authMode === 'slack_user' ? 'https://docs.slack.dev/authentication/installing-with-oauth/' : tokenDocs[id]} target="_blank" rel="noreferrer">{t('plugins.tokenDocs')}<ExternalLink size={12} /></a></p>
        {id === 'slack' && authMode === 'slack_user' && <p className="muted plugin-token-note">{t('plugins.slack.userScopes')}</p>}
        {id === 'slack' && authMode === 'slack_user' && listen && <p className="muted plugin-token-note">{t('slack.socketSetup')} <a href="https://docs.slack.dev/apis/events-api/using-socket-mode/" target="_blank" rel="noreferrer">{t('slack.socketDocs')}<ExternalLink size={12} /></a></p>}
        {id === 'slack' && <p className="muted plugin-token-note">{t('plugins.slack.receiveHint')}</p>}
      </details>
      {modeChanged && <p role="status" className="muted">{t('plugins.slack.modeChanged')}</p>}
      <label>{t('plugins.token')}<input type="password" autoComplete="new-password" spellCheck={false} value={token} disabled={pending} onChange={event => { setToken(event.target.value); save.reset(); }} placeholder={t(!needsToken ? 'plugins.tokenKeep' : authMode === 'slack_user' ? 'plugins.slack.userTokenPlaceholder' : 'plugins.tokenPlaceholder')} required={needsToken} /></label>
      <p className="muted plugin-token-note">{t('plugins.tokenNote')}</p>
      {id === 'slack' && authMode === 'slack_user' && <>
        <label className="plugin-checkbox"><input type="checkbox" checked={listen} disabled={pending} onChange={event => setListen(event.target.checked)} />{t('slack.listen')}</label>
        {listen && <><label>{t('slack.listenScope')}<select aria-label={t('slack.listenScope')} value={listenScope} disabled={pending} onChange={event => setListenScope(event.target.value as SlackListenScope)}><option value="related">{t('slack.scope.related')}</option><option value="all">{t('slack.scope.all')}</option></select></label>
          <label>{t('slack.appToken')}<input type="password" autoComplete="new-password" spellCheck={false} disabled={pending} required={needsAppToken} value={appToken} onChange={event => { setAppToken(event.target.value); save.reset(); }} placeholder={t(needsAppToken ? 'slack.appTokenPlaceholder' : 'plugins.tokenKeep')} /></label>
        </>}
      </>}
      <label className="plugin-checkbox"><input type="checkbox" checked={enabled} disabled={pending} onChange={event => setEnabled(event.target.checked)} />{t('plugins.enabled')}</label>
      <label className="plugin-checkbox"><input type="checkbox" checked={allowWrites} disabled={pending} onChange={event => setAllowWrites(event.target.checked)} />{t('plugins.allowWrites')}</label>
      <p className="muted plugin-token-note">{t('plugins.writePolicy')}</p>
      {settingsUpdated && <p role="status" className="muted">{t('plugins.settingsUpdated')}</p>}
      {error && <p className="error" role="alert">{error.message}</p>}
      {connection.last_error && !error && <p className="error" role="alert">{connection.last_error}</p>}
      {save.isSuccess && <p className="plugin-success" role="status">{t('plugins.saved')}</p>}
      {test.isSuccess && <p className="plugin-success" role="status">{t('plugins.testSuccess')}</p>}
      <div className="plugin-connection-actions">
        <button type="submit" className="primary" disabled={pending || (needsToken && !token.trim()) || (needsAppToken && !appToken.trim())}>{save.isPending ? <LoaderCircle size={14} className="spin" /> : null}{t(save.isPending ? 'common.saving' : enabled ? 'plugins.connectSave' : 'common.save')}</button>
        {connection.configured && <><button type="button" className="ghost" disabled={pending || modeChanged} onClick={() => { save.reset(); disconnect.reset(); test.mutate(); }}>{t(test.isPending ? 'plugins.testing' : 'plugins.test')}</button><button type="button" className="ghost" disabled={pending} onClick={() => disconnect.mutate()}><Unplug size={14} />{t('plugins.disconnect')}</button></>}
        {!connection.configured && connection.status === 'error' && <button type="button" className="ghost" disabled={pending} onClick={() => disconnect.mutate()}>{t('plugins.disconnect')}</button>}
      </div>
    </form>
    <PluginActions key={id + ':' + connectionVersion + ':' + connection.account + ':' + connection.verified_at + ':' + connection.enabled + ':' + connection.allow_writes + ':' + (connection.auth_mode ?? 'token')} plugin={plugin} busy={pending || modeChanged} onAuthFailure={onRefresh} />
  </section>;
}

function defaults(action?: PluginAction): Record<string, unknown> {
  return Object.fromEntries(Object.entries(action?.input_schema.properties ?? {}).filter(([, field]) => field.default !== undefined).map(([key, field]) => [key, field.default]));
}

function PluginActions({ plugin, busy, onAuthFailure }: { plugin: MarketplacePlugin; busy: boolean; onAuthFailure: () => void }) {
  const { t } = useI18n();
  const [actionId, setActionId] = useState(plugin.actions[0]?.id ?? '');
  const action = plugin.actions.find(item => item.id === actionId);
  const [input, setInput] = useState<Record<string, unknown>>(() => defaults(action));
  const run = useMutation({ mutationFn: (input: Record<string, unknown>) => pluginApi.invoke(plugin.manifest.id, actionId, input), retry: false, onError: onAuthFailure });
  const available = plugin.connection.configured && plugin.connection.enabled && (!action?.write || plugin.connection.allow_writes);
  const blocked = busy || run.isPending;
  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (available && !blocked) run.mutate(Object.fromEntries(Object.entries(input).filter(([, value]) => value !== '' && value !== undefined)));
  };
  const next = () => {
    if (!run.data || !action) return;
    const nextInput = { ...run.variables };
    if (run.data.next_page !== null) nextInput.page = run.data.next_page;
    if (run.data.next_cursor) nextInput['start_cursor' in action.input_schema.properties ? 'start_cursor' : 'cursor'] = run.data.next_cursor;
    setInput(nextInput); run.mutate(nextInput);
  };
  return <section className="plugin-operations" aria-label={t('plugins.operations')}>
    <h4>{t('plugins.operations')}</h4>
    <p className="muted">{t('plugins.chatHint')}</p>
    {!plugin.connection.enabled && <p className="muted">{t('plugins.connectFirst')}</p>}
    {!plugin.connection.allow_writes && <p className="muted plugin-token-note">{t('plugins.writesDisabled')}</p>}
    <form className="dialog-form" onSubmit={submit}>
      <label>{t('plugins.operation')}<select value={actionId} disabled={blocked} onChange={event => { setActionId(event.target.value); setInput(defaults(plugin.actions.find(action => action.id === event.target.value))); run.reset(); }}>{plugin.actions.map(action => <option value={action.id} key={action.id} disabled={action.write && !plugin.connection.allow_writes}>{t('plugins.actions.' + action.id)}{action.write ? ' · ' + t('plugins.write') : ''}</option>)}</select></label>
      <fieldset disabled={blocked || !available} className="plugin-inputs">
        {Object.entries(action?.input_schema.properties ?? {}).map(([key, field]) => {
          const required = action?.input_schema.required.includes(key);
          const update = (value: unknown) => setInput(current => ({ ...current, [key]: value }));
          return <label key={key}>{t('plugins.fields.' + key)}{required ? t('schema.required') : ''}
            {field.enum ? <select value={String(input[key] ?? '')} required={required} onChange={event => update(event.target.value)}>{field.enum.map(value => <option value={value} key={value}>{t('plugins.values.' + value)}</option>)}</select>
              : ['body', 'text', 'content', 'message'].includes(key) ? <textarea value={String(input[key] ?? '')} maxLength={field.maxLength} rows={3} required={required} onChange={event => update(event.target.value)} />
              : <input type={field.type === 'integer' ? 'number' : 'text'} value={String(input[key] ?? '')} min={field.minimum} max={field.maximum} maxLength={field.maxLength} required={required} onChange={event => update(field.type === 'integer' && event.target.value !== '' ? Number(event.target.value) : event.target.value)} />}
          </label>;
        })}
      </fieldset>
      {action?.write && <p className="plugin-write-note">{t('plugins.directWrite')}</p>}
      <div><button type="submit" className="primary" disabled={blocked || !available || !action}>{run.isPending && <LoaderCircle size={14} className="spin" />}{t(run.isPending ? 'plugins.running' : 'plugins.actions.' + actionId)}</button></div>
      {run.error && <p role="alert" className="error">{run.error.message}{action?.write ? ' ' + t('plugins.checkBeforeRetry') : ''}</p>}
    </form>
    {run.data && <><PluginResults result={run.data} />{!action?.write && (run.data.next_cursor || run.data.next_page !== null) && <button type="button" className="ghost" disabled={blocked || !available} onClick={next}>{t('plugins.nextPage')}</button>}</>}
  </section>;
}

function stringValue(value: unknown): string { return typeof value === 'string' ? value : ''; }
function richText(value: unknown): string {
  return Array.isArray(value) ? value.map(item => stringValue(item?.plain_text) || stringValue(item?.text?.content)).join('') : '';
}
function resultRow(value: unknown, index: number) {
  const item = (value && typeof value === 'object' ? value : {}) as Record<string, any>;
  const properties = Object.values(item.properties ?? {}) as Record<string, any>[];
  const title = stringValue(item.title) || richText(item.title) || stringValue(item.real_name) || stringValue(item.profile?.display_name) || stringValue(item.name) || stringValue(item.full_name) || richText(properties.find(property => property.type === 'title')?.title) || stringValue(item.message) || stringValue(item.message?.text) || stringValue(item.text) || richText(item[item.type]?.rich_text) || stringValue(item.id) || '#' + (index + 1);
  const url = stringValue(item.html_url) || stringValue(item.url);
  const description = stringValue(item.body) || stringValue(item.description) || stringValue(item.state) || (item.profile ? stringValue(item.id) : '');
  return { title, description, url: url.startsWith('https://') || url.startsWith('http://') ? url : '' };
}
function PluginResults({ result }: { result: PluginResult }) {
  const { t } = useI18n();
  const [visibleCount, setVisibleCount] = useState(100);
  useEffect(() => setVisibleCount(100), [result]);
  const data = result.data as Record<string, any> | null;
  const list = Array.isArray(data) ? data : [data?.members, data?.channels, data?.messages, data?.results, data?.comments].find(Array.isArray);
  const rows = (list ?? (data ? [data.channel && typeof data.channel === 'object' ? data.channel : data.user && typeof data.user === 'object' ? data.user : data] : [])).map(resultRow);
  const raw = JSON.stringify(result, null, 2);
  const download = () => {
    const url = URL.createObjectURL(new Blob([raw], { type: 'application/json' }));
    const anchor = document.createElement('a'); anchor.href = url; anchor.download = 'plugin-result.json'; anchor.click(); window.setTimeout(() => URL.revokeObjectURL(url), 1000);
  };
  return <section className="plugin-results" aria-label={t('plugins.results')}>
    <div className="section-heading"><h4>{t('plugins.results')}</h4><span className="plugin-success" role="status">{t('plugins.operationSuccess')}</span></div>
    {!rows.length ? <EmptyState compact title={t('plugins.noData')} description={t('plugins.noDataHint')} /> : <><p className="muted plugin-token-note">{t('plugins.resultCount', { shown: Math.min(visibleCount, rows.length), total: rows.length })}</p><ul>{rows.slice(0, visibleCount).map((row, index) => <li key={index}>{row.url ? <a href={row.url} target="_blank" rel="noreferrer">{row.title}</a> : <strong>{row.title}</strong>}{row.description && <p>{row.description}</p>}</li>)}</ul>{visibleCount < rows.length && <button type="button" className="ghost" onClick={() => setVisibleCount(count => Math.min(count + 100, rows.length))}>{t('plugins.showMore', { count: rows.length - visibleCount })}</button>}</>}
    <details><summary>{t('plugins.rawResult')}</summary><pre>{raw.slice(0, 100000)}</pre>{raw.length > 100000 && <p>{t('plugins.previewTruncated')}</p>}</details>
    <button type="button" className="ghost" onClick={download}>{t('plugins.export')}</button>
  </section>;
}
