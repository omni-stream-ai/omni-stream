import { clientFetch } from '../lib/clientFetch';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useState, useRef, useEffect, useCallback } from 'react';
import { Bot, File, LoaderCircle, ShieldCheck, Square, X } from 'lucide-react';
import { api, AgentCall, ConversationMessage, ConversationSettings, ConversationDetail, type PluginSetupContext } from '../lib/api';
import { Composer, Attachment } from './Composer';
import { MessageContent } from './MessageContent';
import { ImagePreview } from './ImagePreview';
import { EmptyState, ErrorState, LoadingState, MutationError } from './StateView';
import { ApprovalForm } from './ApprovalForm';
import { ModelPicker } from './ModelPicker';
import { AutomationConfirmationCard } from './AutomationConfirmationCard';
import { ExecutionProgress } from './ExecutionProgress';
import { useI18n } from '../i18n';
import { automationDraft, messagesForDisplay } from './conversationMessages';
import { useViewedNotifications, type ReadNotifications } from '../lib/useViewedNotifications';


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
    {output && <div className="call-output"><MessageContent content={output} /></div>}{cancel.error && <MutationError mutation={cancel} />}
    </div>
  </section>;
}

function ToolMessage({ tool, args, result, onOpenAutomation }: { tool: string; args?: unknown; result?: unknown; onOpenAutomation: (id: number) => void }) {
  const { t } = useI18n();
  const parameters = (args && typeof args === 'object' ? args : {}) as Record<string, unknown>;
  const output = (result && typeof result === 'object' ? result : {}) as Record<string, unknown>;
  const automation = automationDraft({ tool, result });
  if (automation) {
    return <div className="agent-call automation-reference"><strong>{automation.title ?? t('automation.details')}</strong><button type="button" className="ghost" onClick={() => onOpenAutomation(automation.id)}>{t('automation.openDraft')}</button></div>;
  }
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

export function ConversationView({ id, server, project, projects, onNew, onConfigurePlugin, onOpenAutomation, onRead }: { onRead?: ReadNotifications; onOpenAutomation: (id: number) => void; onConfigurePlugin?: (context: PluginSetupContext) => void; id: string; server: string; project: string; projects: string[]; onNew: () => void }) {
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
  const [confirmationReady, setConfirmationReady] = useState<Record<string, boolean>>({});
  const reportConfirmationReady = useCallback((key: string, ready: boolean) => {
    setConfirmationReady(previous => previous[key] === ready ? previous : { ...previous, [key]: ready });
  }, []);
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
  const confirmationsReady = (detail.data?.history ?? []).every(message => !message.automation_confirmation
    || confirmationReady[`${message.automation_confirmation.automation_id}:${message.automation_confirmation.confirmation_id}`]);
  useViewedNotifications(scroll, detail.data?.notification_snapshot, detail.isSuccess && confirmationsReady, onRead);
  const confirmation = id.startsWith('automation-confirmation:');
  const setup = detail.data?.history.find(message => message.plugin_setup)?.plugin_setup;
  const send = async (message: string, attachments: Attachment[], selectedProject?: string, agentId?: number) => {
    if (setup && (attachments.length || /(?:xox[pbasre]-|xapp-|github_pat_|gh[pousr]_|ntn_|secret_|figd_)[A-Za-z0-9_.=-]+/.test(message))) {
      setError(t('plugins.setupNoSecrets')); setFailedRequest(null); return;
    }
    setSending(true); setError('');
    setPendingMessages(value => [...value, { role: 'user', content: message, attachments }]);
    try {
      const response = await clientFetch(server, '/chat/stream', { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ utc_offset_minutes: -new Date().getTimezoneOffset(), conversation: id, project: selectedProject || project || undefined, agent_id: agentId, messages: [{ role: 'user', parts: [{ type: 'text', text: message }] }], attachments }) });
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
      {detail.isPending && <LoadingState />}
      {messagesForDisplay([...(detail.data?.history ?? []), ...pendingMessages.filter((pending) => !(detail.data?.history ?? []).some((saved) => saved.role === pending.role && saved.content === pending.content && JSON.stringify(saved.attachments ?? []) === JSON.stringify(pending.attachments ?? [])))]).map((message, index) => {
        if (message.automation_confirmation) return <AutomationConfirmationCard key={index} server={server} automationId={message.automation_confirmation.automation_id} confirmationId={message.automation_confirmation.confirmation_id} onReady={reportConfirmationReady} />;
        if (message.tool) return <ToolMessage key={index} tool={message.tool} args={message.args} result={message.result} onOpenAutomation={onOpenAutomation} />;
        if (message.call_id) { const call = calls.find(item => item.task_id === message.call_id); return call ? <Call setup={!!setup} key={`call-${call.task_id}`} call={call} onChange={refresh} /> : null; }
        const images = (message.attachments ?? []).filter(item => item.type.startsWith('image/'));
        return <article key={index} className={`chat-message ${message.role}`}><span className="message-author">{message.role === 'user' ? t('conversation.user') : t('agent.system')}</span><div className="message-content"><MessageContent content={typeof message.content === 'string' ? message.content : JSON.stringify(message.content)} />{!!message.attachments?.length && <div className="message-attachments">{message.attachments.map(item => <button key={item.id} type="button" title={item.name} disabled={!item.type.startsWith('image/')} onClick={() => setPreview({ images: images.map(image => ({ url: image.data, name: image.name })), index: images.findIndex(image => image.id === item.id) })}>{item.type.startsWith('image/') ? <img src={item.data} alt={item.name} /> : <File size={22} />}<span>{item.name}</span></button>)}</div>}</div></article>;
      })}

      {sending && <div className="conversation-loading" role="status"><LoaderCircle size={16} className="spin" /><div><strong>{t('agent.system')}</strong><p>{t('agent.processing')}</p></div></div>}
      {error && !failedRequest && <ErrorState compact title={t('conversation.requestFailed')} description={error} onRetry={() => setError('')} retryLabel={t('common.edit')} />}
      {error && failedRequest && <ErrorState compact title={t('conversation.requestFailed')} description={error} onRetry={retry} retrying={sending} />}
      {detail.isSuccess && !detail.data.history.length && !pendingMessages.length && !sending && !error && <div className="chat-empty"><EmptyState title={t('conversation.start')} description={t('conversation.noMessages')} /></div>}
    </div>
    <div className="pending-requests">{(detail.data?.approvals ?? []).map(approval => <ApprovalForm key={approval.id} approval={approval} agent={t('plugins.names.' + String(approval.payload.plugin))} task={0} onDone={refresh} />)}{calls.flatMap(call => call.approvals.map(approval => <ApprovalForm key={approval.id} approval={approval} agent={typeof approval.payload.agent === 'string' ? approval.payload.agent : call.agent} task={call.task_id} onDone={refresh} />))}</div>
    {agents.isError && !setup && !confirmation && <ErrorState compact title={t('agents.loadFailed')} description={agents.error.message} onRetry={() => void agents.refetch()} retrying={agents.isFetching} />}
    <Composer busy={sending || save.isPending || !detail.isSuccess || setupRunning} projects={confirmation ? [] : projects} agents={setup || confirmation ? [] : agents.data} searchProjects={confirmation ? undefined : api.completeProjectPath} onSend={send} onCommand={command => { if (command === 'new') onNew(); else { setName(settings.title ?? ''); setRenaming(true); } }} controls={!confirmation && <>
      <label className="composer-select"><ShieldCheck size={14} aria-hidden="true" /><select aria-label={t('approval.mode')} title={t('approval.mode')} value={settings.approval_mode} disabled={save.isPending} onChange={event => save.mutate({ approval_mode: event.target.value as ConversationSettings['approval_mode'] })}><option value="default">{t('approval.default')}</option><option value="safe">{t('approval.safe')}</option><option value="all">{t('approval.all')}</option></select></label>
      {!setup && <ModelPicker server={server} />}
    </>} />
    {save.error && <MutationError mutation={save} />}
    {preview && <ImagePreview images={preview.images} initial={preview.index} onClose={() => setPreview(null)} />}
  </div>;
}
