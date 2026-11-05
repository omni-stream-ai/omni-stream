import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useState, useRef, useEffect } from 'react';
import { Bot, File, LoaderCircle, RotateCcw, ShieldCheck, Square, X } from 'lucide-react';
import { api, AgentCall, Approval, ConversationMessage, ConversationSettings, ConversationDetail, type PluginSetupContext } from '../lib/api';
import { Composer, Attachment } from './Composer';
import { MessageContent } from './MessageContent';
import { ImagePreview } from './ImagePreview';
import { EmptyState, ErrorState } from './StateView';
import { SchemaFields } from './SchemaFields';
import { ModelPicker } from './ModelPicker';
import { AutomationConfirmationCard } from './AutomationConfirmationCard';
import { ExecutionProgress } from './ExecutionProgress';
import { useI18n } from '../i18n';

export function ApprovalForm({ approval, agent, task, onDone }: { approval: Approval; agent: string; task: number; onDone: () => void }) {
  const { t } = useI18n();
  const request = (approval.payload.request ?? {}) as { method?: string; params?: Record<string, unknown> };
  const pluginRequest = approval.action === 'plugin.invoke';
  const params = (pluginRequest ? { message: t('plugins.actions.' + String(approval.payload.action)), reason: t('plugins.approvalReason', { account: String(approval.payload.account ?? '') }), permissions: approval.payload.input } : request.params ?? {}) as Record<string, unknown>;
  const questions = (params.questions ?? []) as { id: string; question: string; options?: { label: string; description?: string }[]; isSecret?: boolean; isOther?: boolean }[];
  const [answers, setAnswers] = useState<Record<string, string>>({});
  const [fields, setFields] = useState<Record<string, unknown>>({});
  const structured = request.method === 'item/tool/requestUserInput' || (request.method === 'mcpServer/elicitation/request' && params.mode !== 'url');
  const reason = typeof params.reason === 'string' && params.reason.trim() ? params.reason.trim() : undefined;
  const operation = params.command ?? params.message ?? (request.method === 'item/fileChange/requestApproval'
    ? t('approval.fileOperation')
    : request.method === 'item/commandExecution/requestApproval'
      ? t('approval.commandOperation')
      : request.method === 'item/permissions/requestApproval'
        ? t('approval.permissionOperation')
        : request.method
          ? `${t('approval.operation')} (${request.method})`
          : t('approval.operation'));
  const description = reason ? `${String(operation)}：${reason}` : `${String(operation)}（${t('approval.reasonUnavailable')}）`;
  const mutation = useMutation({ mutationFn: async (choice: 'approve' | 'session' | 'reject' | 'respond') => {
    if (choice === 'reject') return api.reject(approval.id);
    if (choice === 'respond') return api.respond(approval.id, questions.length ? { answers: Object.fromEntries(questions.map(q => [q.id, { answers: [answers[q.id] ?? ''] }])) } : { action: 'accept', content: fields });
    return api.approve(approval.id, choice === 'session' ? 'session' : 'turn');
  }, onSuccess: onDone });
  return <section className="pending-request"><header><ShieldCheck size={16} /><strong>{agent} · {pluginRequest ? t('plugins.writeApproval') : t('agent.call') + ' #' + task}</strong></header>
    <p>{structured && !reason && !params.command && !params.message ? t('approval.needInfo') : description}</p>
    {pluginRequest ? <dl className="plugin-approval-parameters">{Object.entries((params.permissions ?? {}) as Record<string, unknown>).map(([key, value]) => <div key={key}><dt>{t('plugins.fields.' + key)}</dt><dd>{String(value)}</dd></div>)}</dl> : params.permissions ? <pre>{JSON.stringify(params.permissions, null, 2)}</pre> : null}
    {typeof params.url === 'string' && /^https?:\/\//.test(params.url) && <a href={params.url} target="_blank" rel="noreferrer">{t('approval.openUrl')}</a>}
    {questions.map(q => <label key={q.id}>{q.question}{q.options?.length ? <select required value={answers[q.id] ?? ''} onChange={event => setAnswers(value => ({ ...value, [q.id]: event.target.value }))}><option value="">{t('approval.select')}</option>{q.options.map(option => <option key={option.label} value={option.label}>{option.label}{option.description ? ` · ${option.description}` : ''}</option>)}</select> : <input type={q.isSecret ? 'password' : 'text'} value={answers[q.id] ?? ''} onChange={event => setAnswers(value => ({ ...value, [q.id]: event.target.value }))} />}</label>)}
    {questions.filter(q => q.isOther && q.options?.length).map(q => <label key={`other-${q.id}`}>{q.question}（{t('approval.customAnswer')}）<input value={q.options?.some(option => option.label === answers[q.id]) ? '' : answers[q.id] ?? ''} onChange={event => setAnswers(value => ({ ...value, [q.id]: event.target.value }))} /></label>)}
    {structured && !questions.length && <SchemaFields schema={(params.requestedSchema ?? {}) as Parameters<typeof SchemaFields>[0]['schema']} value={fields} onChange={setFields} />}
    <div className="approval-actions">{structured ? <button className="primary" disabled={mutation.isPending || questions.some(q => !answers[q.id]?.trim())} onClick={() => mutation.mutate('respond')}>{t('approval.submit')}</button> : <><button className="primary" disabled={mutation.isPending} onClick={() => mutation.mutate('approve')}>{t('approval.allow')}</button>{/commandExecution|fileChange/.test(request.method ?? '') && <button disabled={mutation.isPending} onClick={() => mutation.mutate('session')}>{t('approval.allowSession')}</button>}</>}
      <button disabled={mutation.isPending} onClick={() => mutation.mutate('reject')}>{t('approval.reject')}</button></div>
    {mutation.error && <p role="alert" className="error">{mutation.error.message}</p>}
  </section>;
}

function Call({ call, onChange, setup = false }: { call: AgentCall; onChange: () => void; setup?: boolean }) {
  const { t } = useI18n();
  const cancel = useMutation({ mutationFn: () => api.cancelAgentCall(call.task_id), onSuccess: onChange });
  const active = ['running', 'queued', 'testing'].includes(call.task.status);
  const status = call.approvals.length ? t('agent.waiting')
    : ['succeeded', 'completed', 'unverified'].includes(call.task.status) ? null
    : ({ running: t('status.running'), queued: t('status.queued'), testing: t('status.testing'), ready_for_review: t('status.readyForReview'), blocked: t('status.blocked'), failed: t('status.failed'), cancelled: t('status.cancelled'), interrupted: t('status.interrupted'), paused: t('status.paused') }[call.task.status] ?? call.task.status);
  const output = active ? call.events.map(item => item.event?.type === 'text' ? item.event.delta : '').join('') : call.task.last_output;
  return <section className="agent-call"><header><Bot size={17} /><strong>{call.agent}</strong>{status && <span className={`status ${call.task.status}`}>{status}</span>}{active && <button className="icon-button" title={t('agent.cancel')} aria-label={t('agent.cancel')} disabled={cancel.isPending} onClick={() => cancel.mutate()}><Square size={14} /></button>}</header>
    <div className="call-body">
    <p className="call-goal">{call.task.title}</p>{!setup && <p className="call-project">{call.task.project} · #{call.task_id}</p>}
    <ExecutionProgress events={call.events} active={active} />
    {output && <div className="call-output"><MessageContent content={output} /></div>}{cancel.error && <p className="error">{cancel.error.message}</p>}
    </div>
  </section>;
}

function ToolMessage({ tool, args, result }: { tool: string; args?: unknown; result?: unknown }) {
  const { t } = useI18n();
  const parameters = (args && typeof args === 'object' ? args : {}) as Record<string, unknown>;
  const output = (result && typeof result === 'object' ? result : {}) as Record<string, unknown>;
  const projectValue = typeof parameters.path === 'string' ? parameters.path : typeof output.project === 'object' && output.project ? (output.project as Record<string, unknown>).path : undefined;
  const project = typeof projectValue === 'string' ? projectValue : undefined;
  const labels: Record<string, string> = { register_project: t('project.known'), list_projects: t('nav.conversations') };
  const title = tool === 'plugin_invoke' ? t('plugins.names.' + String(parameters.plugin)) + ' · ' + t('plugins.actions.' + String(parameters.action)) : tool === 'list_plugins' ? t('plugins.title') : labels[tool] ?? tool;
  const summary = project ? `${title} · ${project}` : title;
  const failed = Boolean(output.error);
  return <details className="agent-call tool-message"><summary>{t('agent.system')} · {summary}{failed ? ` · ${t('agent.toolFailure')}` : ''}</summary>
    {project && <p className="tool-summary">{t('dialog.projectPath')}: {project}</p>}
    <pre className="tool-raw">{JSON.stringify({ parameters: args, result }, null, 2)}</pre>
  </details>;
}

export function ConversationView({ id, server, project, projects, onNew, onConfigurePlugin }: { onConfigurePlugin?: (context: PluginSetupContext) => void; id: string; server: string; project: string; projects: string[]; onNew: () => void }) {
  const { locale, t } = useI18n();
  const client = useQueryClient();
  const detail = useQuery({ queryKey: ['conversation-detail', server, id], queryFn: () => api.conversationDetail(id), refetchInterval: query => query.state.status === 'error' ? false : 3000 });
  const [sending, setSending] = useState(false);
  const agents = useQuery({ queryKey: ['agents', server], queryFn: api.agents, retry: false, refetchOnWindowFocus: false, refetchOnReconnect: false });
  const [pendingMessages, setPendingMessages] = useState<ConversationMessage[]>([]);
  const [error, setError] = useState('');
  const [failedRequest, setFailedRequest] = useState<{ message: string; attachments: Attachment[]; project?: string; agentId?: number } | null>(null);
  const [renaming, setRenaming] = useState(false);
  const [name, setName] = useState('');
  const [preview, setPreview] = useState<{ images: { url: string; name: string }[]; index: number } | null>(null);
  const scroll = useRef<HTMLDivElement>(null);
  const stick = useRef(true);
  const refresh = () => { if (id.startsWith('automation-confirmation:')) void client.invalidateQueries({ queryKey: ['automation-confirmation', server] }); void detail.refetch(); void client.invalidateQueries({ queryKey: ['conversations', server] }); void client.invalidateQueries({ queryKey: ['conversation', server, id] }); };
  const settings = detail.data?.settings ?? { approval_mode: 'default' };
  const save = useMutation({
    mutationFn: (patch: Partial<ConversationSettings>) => api.saveConversationSettings(id, { ...settings, ...patch }),
    onMutate: () => client.cancelQueries({ queryKey: ['conversation-detail', server, id] }),
    onSuccess: updated => {
      client.setQueryData<ConversationDetail>(['conversation-detail', server, id], previous => previous ? { ...previous, settings: updated } : previous);
      client.setQueryData(['conversation-settings', server, id], updated);
      refresh();
    },
  });
  useEffect(() => { if (stick.current) scroll.current?.scrollTo({ top: scroll.current.scrollHeight }); }, [detail.data, sending]);
  const confirmation = id.startsWith('automation-confirmation:');
  const setup = detail.data?.history.find(message => message.plugin_setup)?.plugin_setup;
  const send = async (message: string, attachments: Attachment[], selectedProject?: string, agentId?: number) => {
    if (setup && (attachments.length || /(?:xox[pbasre]-|xapp-|github_pat_|gh[pousr]_|ntn_|secret_|figd_)[A-Za-z0-9_.=-]+/.test(message))) {
      setError(t('plugins.setupNoSecrets')); setFailedRequest(null); return;
    }
    setSending(true); setError('');
    setPendingMessages(value => [...value, { role: 'user', content: message, attachments }]);
    try {
      const response = await fetch(`${server}/chat/stream`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ utc_offset_minutes: -new Date().getTimezoneOffset(), conversation: id, project: selectedProject || project || undefined, agent_id: agentId, messages: [{ role: 'user', parts: [{ type: 'text', text: message }] }], attachments }) });
      if (!response.ok) { const body = await response.json(); throw new Error(body.error ?? t('composer.sendFailed')); }
      await response.text(); setPendingMessages([]); setFailedRequest(null); refresh();
    } catch (cause) { const errorMessage = cause instanceof Error ? cause.message : t('composer.sendFailed'); setError(errorMessage); setFailedRequest({ message, attachments, project: selectedProject, agentId }); }
    finally { setSending(false); refresh(); }
  };
  const retry = () => {
    if (!failedRequest || sending) return;
    setPendingMessages([]);
    void send(failedRequest.message, failedRequest.attachments, failedRequest.project, failedRequest.agentId);
  };
  const calls = detail.data?.calls ?? [];
  const setupRunning = (!!setup || confirmation) && calls.some(call => ['running', 'queued', 'testing'].includes(call.task.status));
  return <div className="chat-panel conversation-view">
    {setup && <aside className="plugin-setup-card" aria-label={t('plugins.setupTitle', { name: t('plugins.names.' + setup.plugin) })}>
      <div><strong>{t('plugins.setupTitle', { name: t('plugins.names.' + setup.plugin) })}</strong><p>{t('plugins.setupConversationHint')}</p>
        <p className="muted">{setup.agent}{' · '}{t(setup.options.allow_writes ? 'plugins.setupWrites' : 'plugins.setupReadOnly')}{setup.plugin === 'slack' ? ' · ' + t(setup.options.auth_mode === 'slack_user' ? 'plugins.slack.userMode' : 'plugins.slack.botMode') + (setup.options.listen ? ' · ' + t('slack.scope.' + setup.options.listen_scope) : '') : ''}</p>
      </div>
      <button type="button" className="primary" onClick={() => onConfigurePlugin?.(setup)}>{t('plugins.returnToSetup')}</button>
    </aside>}
    {renaming && <form className="rename-form" onSubmit={event => { event.preventDefault(); if (name.trim()) save.mutate({ title: name.trim() }, { onSuccess: () => setRenaming(false) }); }}><input autoFocus aria-label={t('conversation.name')} value={name} onChange={event => setName(event.target.value)} /><button type="submit" disabled={save.isPending || !name.trim()}>{t('common.save')}</button><button type="button" aria-label={t('conversation.cancelRename')} onClick={() => setRenaming(false)}><X size={16} /></button></form>}
    <div className="chat-messages" ref={scroll} onScroll={() => { const element = scroll.current; if (element) stick.current = element.scrollHeight - element.scrollTop - element.clientHeight < 80; }}>
      {detail.isError && <ErrorState title={t('conversation.loadFailed')} description={detail.error.message} onRetry={() => void detail.refetch()} retrying={detail.isFetching} />}
      {detail.isPending && <p className="muted">{t('common.loading')}</p>}
      {[...(detail.data?.history ?? []), ...pendingMessages.filter((pending) => !(detail.data?.history ?? []).some((saved) => saved.role === pending.role && saved.content === pending.content && JSON.stringify(saved.attachments ?? []) === JSON.stringify(pending.attachments ?? [])))].map((message, index) => {
        if (message.automation_confirmation) return <AutomationConfirmationCard key={index} server={server} automationId={message.automation_confirmation.automation_id} confirmationId={message.automation_confirmation.confirmation_id} />;
        if (message.tool) return <ToolMessage key={index} tool={message.tool} args={message.args} result={message.result} />;
        if (message.call_id) { const call = calls.find(item => item.task_id === message.call_id); return call ? <Call setup={!!setup} key={`call-${call.task_id}`} call={call} onChange={refresh} /> : null; }
        const images = (message.attachments ?? []).filter(item => item.type.startsWith('image/'));
        return <article key={index} className={`chat-message ${message.role}`}><span className="message-author">{message.role === 'user' ? t('conversation.user') : t('agent.system')}</span><div className="message-content"><MessageContent content={typeof message.content === 'string' ? message.content : JSON.stringify(message.content)} />{!!message.attachments?.length && <div className="message-attachments">{message.attachments.map(item => <button key={item.id} type="button" title={item.name} disabled={!item.type.startsWith('image/')} onClick={() => setPreview({ images: images.map(image => ({ url: image.data, name: image.name })), index: images.findIndex(image => image.id === item.id) })}>{item.type.startsWith('image/') ? <img src={item.data} alt={item.name} /> : <File size={22} />}<span>{item.name}</span></button>)}</div>}</div></article>;
      })}

      {sending && <div className="conversation-loading" role="status"><LoaderCircle size={16} className="spin" /><div><strong>{t('agent.system')}</strong><p>{t('agent.processing')}</p></div></div>}
      {error && !failedRequest && <p className="error" role="alert">{error}</p>}
      {error && failedRequest && <section className="conversation-error" role="alert"><strong>{t('conversation.requestFailed')}</strong><p>{error}</p><button type="button" className="ghost" disabled={sending} onClick={retry}><RotateCcw size={14} />{t('common.retry')}</button></section>}
      {detail.isSuccess && !detail.data.history.length && !sending && <div className="chat-empty"><EmptyState title={t('conversation.start')} description={t('conversation.noMessages')} /></div>}
    </div>
    <div className="pending-requests">{(detail.data?.approvals ?? []).map(approval => <ApprovalForm key={approval.id} approval={approval} agent={t('plugins.names.' + String(approval.payload.plugin))} task={0} onDone={refresh} />)}{calls.flatMap(call => call.approvals.map(approval => <ApprovalForm key={approval.id} approval={approval} agent={typeof approval.payload.agent === 'string' ? approval.payload.agent : call.agent} task={call.task_id} onDone={refresh} />))}</div>
    {agents.isError && !setup && !confirmation && <ErrorState compact title={t('agents.loadFailed')} description={agents.error.message} onRetry={() => void agents.refetch()} retrying={agents.isFetching} />}
    <Composer busy={sending || save.isPending || !detail.isSuccess || setupRunning} projects={confirmation ? [] : projects} agents={setup || confirmation ? [] : agents.data} searchProjects={confirmation ? undefined : api.completeProjectPath} onSend={send} onCommand={command => { if (command === 'new') onNew(); else { setName(settings.title ?? ''); setRenaming(true); } }} controls={!confirmation && <>
      <label className="composer-select"><ShieldCheck size={14} /><select aria-label={t('approval.mode')} title={t('approval.mode')} value={settings.approval_mode} disabled={save.isPending} onChange={event => save.mutate({ approval_mode: event.target.value as ConversationSettings['approval_mode'] })}><option value="default">{t('approval.default')}</option><option value="safe">{t('approval.safe')}</option><option value="all">{t('approval.all')}</option></select></label>
      {!setup && <ModelPicker server={server} />}
    </>} />
    {save.error && <p role="alert" className="error">{save.error.message}</p>}
    {preview && <ImagePreview images={preview.images} initial={preview.index} onClose={() => setPreview(null)} />}
  </div>;
}
