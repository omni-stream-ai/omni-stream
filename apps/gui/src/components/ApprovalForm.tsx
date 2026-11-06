import { useState } from 'react';
import { useMutation } from '@tanstack/react-query';
import { ShieldCheck } from 'lucide-react';
import { api, type Approval } from '../lib/api';
import { useI18n } from '../i18n';
import { SchemaFields } from './SchemaFields';
import { MutationError } from './StateView';

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
    {mutation.error && <MutationError mutation={mutation} />}
  </section>;
}

