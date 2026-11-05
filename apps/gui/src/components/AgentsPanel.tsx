import { useState, type FormEvent } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { api, type AgentDefinition, type NamedAgent } from '../lib/api';
import { useI18n } from '../i18n';
import { EmptyState, ErrorState } from './StateView';

const empty: AgentDefinition = { name: '', description: '', instructions: '', engine: '', plugins: null };
const plugins = ['slack', 'github', 'notion', 'figma', 'chrome-devtools'];

export function AgentsPanel({ server }: { server: string }) {
  const { t } = useI18n();
  const client = useQueryClient();
  const queryOptions = { retry: false, refetchOnWindowFocus: false, refetchOnReconnect: false } as const;
  const agents = useQuery({ queryKey: ['agents', server], queryFn: api.agents, ...queryOptions });
  const engines = useQuery({ queryKey: ['agent-engines', server], queryFn: api.agentEngines, ...queryOptions });
  const [editing, setEditing] = useState<number>();
  const [draft, setDraft] = useState<AgentDefinition>(empty);
  const reset = () => { setEditing(undefined); setDraft(empty); };
  const refresh = () => { void client.invalidateQueries({ queryKey: ['agents', server] }); };
  const save = useMutation({ mutationFn: (value: AgentDefinition) => api.saveAgent(value, editing), onSuccess: () => { reset(); refresh(); } });
  const remove = useMutation({ mutationFn: api.deleteAgent, onSuccess: (_, id) => { if (editing === id) reset(); refresh(); } });
  const edit = (agent: NamedAgent) => { setEditing(agent.id); setDraft({ name: agent.name, description: agent.description, instructions: agent.instructions, engine: agent.engine, plugins: agent.plugins }); save.reset(); };
  const submit = (event: FormEvent) => { event.preventDefault(); save.mutate({ ...draft, name: draft.name.trim(), engine: draft.engine || engines.data?.[0]?.name || '' }); };
  const engine = draft.engine || engines.data?.[0]?.name || '';
  return <div className="automation-panel agents-panel">
    <p className="muted">{t('agents.description')}</p>
    {engines.isError && <ErrorState compact title={t('agents.enginesFailed')} description={engines.error.message} onRetry={() => void engines.refetch()} retrying={engines.isFetching} />}
    {engines.isSuccess && !engines.data.length && <EmptyState compact title={t('agents.noEngine')} description={t('agents.configureEngine')} />}
    <form className="dialog-form" onSubmit={submit}>
      <h3>{t(editing === undefined ? 'agents.create' : 'agents.edit')}</h3>
      <label>{t('agents.name')}<input required maxLength={64} value={draft.name} onChange={event => setDraft({ ...draft, name: event.target.value })} placeholder={t('agents.nameExample')} /></label>
      <label>{t('agents.summary')}<input maxLength={1000} value={draft.description} onChange={event => setDraft({ ...draft, description: event.target.value })} /></label>
      <label>{t('agents.engine')}<select required value={engine} onChange={event => setDraft({ ...draft, engine: event.target.value })}><option value="">{t('automation.select')}</option>{draft.engine && !engines.data?.some(item => item.name === draft.engine) && <option value={draft.engine}>{draft.engine} · {t('automation.unavailable')}</option>}{engines.data?.map(item => <option key={item.name} value={item.name}>{item.name} ({item.protocol})</option>)}</select></label>
      <label>{t('agents.instructions')}<textarea required maxLength={10000} rows={5} value={draft.instructions} onChange={event => setDraft({ ...draft, instructions: event.target.value })} placeholder={t('agents.instructionsExample')} /></label>
      <label className="check"><input type="checkbox" checked={draft.plugins === null} onChange={event => setDraft({ ...draft, plugins: event.target.checked ? null : [] })} />{t('agents.inheritPlugins')}</label>
      {draft.plugins !== null && <fieldset><legend>{t('agents.allowedPlugins')}</legend>{plugins.map(plugin => <label className="check" key={plugin}><input type="checkbox" checked={draft.plugins?.includes(plugin) ?? false} onChange={event => setDraft({ ...draft, plugins: event.target.checked ? [...draft.plugins ?? [], plugin] : draft.plugins?.filter(value => value !== plugin) ?? [] })} />{t('plugins.names.' + plugin)}</label>)}</fieldset>}
      <p className="muted">{t('agents.permissionHint')}</p>
      {save.error && <p className="error" role="alert">{save.error.message}</p>}
      <div className="dialog-actions">{editing !== undefined && <button className="ghost" type="button" onClick={reset}>{t('common.cancel')}</button>}<button type="submit" className="primary" disabled={save.isPending || !engine || !draft.name.trim() || !draft.instructions.trim()}>{t(save.isPending ? 'common.loading' : editing === undefined ? 'agents.create' : 'common.save')}</button></div>
    </form>
    {agents.isPending && <p role="status">{t('common.loading')}</p>}
    {agents.isError && <ErrorState compact title={t('agents.loadFailed')} description={agents.error.message} onRetry={() => void agents.refetch()} retrying={agents.isFetching} />}
    {agents.isSuccess && !agents.data.length && <EmptyState compact title={t('agents.empty')} description={t('agents.emptyHint')} />}
    {agents.data?.map(agent => <article className="panel" key={agent.id}><div className="panel-heading"><div><h3>@{agent.name}</h3><p className="muted">{agent.description || t('agents.instructions')}</p></div><div className="panel-actions"><button type="button" className="ghost" disabled={save.isPending || remove.isPending} onClick={() => edit(agent)}>{t('agents.edit')}</button><button type="button" className="ghost danger" disabled={remove.isPending} onClick={() => remove.mutate(agent.id)}>{t('agents.delete')}</button></div></div><p className="muted">{t('agents.engine')}: {agent.engine} · {agent.plugins === null ? t('agents.inheritPlugins') : agent.plugins.length ? agent.plugins.map(id => t('plugins.names.' + id)).join(', ') : t('agents.noPlugins')}</p><AgentMemories server={server} id={agent.id} /></article>)}
    {remove.error && <p className="error" role="alert">{remove.error.message}</p>}
  </div>;
}

function AgentMemories({ server, id }: { server: string; id: number }) {
  const { t, locale } = useI18n();
  const client = useQueryClient();
  const [open, setOpen] = useState(false);
  const queryKey = ['agent-memory', server, id];
  const memories = useQuery({ queryKey, queryFn: () => api.agentMemories(id), enabled: open, retry: false, refetchOnWindowFocus: false, refetchOnReconnect: false });
  const remove = useMutation({ mutationFn: (key: string) => api.deleteAgentMemory(id, key), onSuccess: () => { void client.invalidateQueries({ queryKey }); } });
  return <details className="agent-memory" onToggle={event => setOpen(event.currentTarget.open)}>
    <summary>{t('agents.memory')}</summary>
    {open && <div>
      <p className="muted">{t('agents.memoryHint')}</p>
      <button type="button" className="ghost" disabled={memories.isFetching} onClick={() => void memories.refetch()}>{t(memories.isFetching ? 'common.loading' : 'agents.refreshMemory')}</button>
      {memories.isPending && <p role="status">{t('common.loading')}</p>}
      {memories.isError && <ErrorState compact title={t('agents.memoryFailed')} description={memories.error.message} onRetry={() => void memories.refetch()} retrying={memories.isFetching} />}
      {memories.isSuccess && !memories.data.length && <EmptyState compact title={t('agents.memoryEmpty')} description={t('agents.memoryEmptyHint')} />}
      {memories.data?.map(memory => <article key={memory.key}>
        <div className="panel-heading"><strong>{memory.key}</strong><button type="button" className="ghost danger" disabled={remove.isPending} aria-label={t('agents.deleteMemoryKey', { key: memory.key })} onClick={() => remove.mutate(memory.key)}>{t('agents.deleteMemory')}</button></div>
        <p className="agent-memory-content">{memory.content}</p>
        <p className="muted">{t('agents.memoryUpdated', { time: new Date(memory.updated_at * 1000).toLocaleString(locale) })}</p>
      </article>)}
      {remove.error && <p role="alert" className="error">{remove.error.message}</p>}
    </div>}
  </details>;
}
