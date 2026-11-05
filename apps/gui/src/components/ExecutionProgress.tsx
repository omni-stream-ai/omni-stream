import { ChevronDown, FilePenLine, Image, ListChecks, Search, Terminal, Wrench, GitBranch, Activity } from 'lucide-react';
import type { AgentCall } from '../lib/api';
import { executionSteps, progressText, stepDetails, stepState, stepSummary } from '../lib/agentProgress';
import { useI18n } from '../i18n';

const icons = { command: Terminal, files: FilePenLine, image: Image, tool: Wrench, search: Search, plan: ListChecks, context: GitBranch, update: Activity };
const record = (value: unknown): Record<string, unknown> => value && typeof value === 'object' ? value as Record<string, unknown> : {};

export function ExecutionProgress({ events, active }: { events: AgentCall['events']; active: boolean }) {
  const { t } = useI18n();
  const raw = events.filter(entry => entry.event?.type === 'progress');
  if (!raw.length) return null;
  const steps = executionSteps(events);
  return <details className="execution-progress">
    <summary><ChevronDown size={12} />{t('agent.progress')}{steps.length > 0 && <span className="progress-count">{steps.length}</span>}</summary>
    {steps.length > 0 ? <ol className="execution-step-list">{steps.map(step => {
      const Icon = icons[step.kind];
      const state = stepState(step, active);
      const summary = stepSummary(step) || t('progress.noSummary');
      const details = stepDetails(step);
      const files = Array.isArray(step.data.changes) ? step.data.changes.map(record) : [];
      const plan = Array.isArray(step.data.plan) ? step.data.plan.map(record) : [];
      return <li key={step.id}><details className={`execution-step${state === 'failed' ? ' has-error' : ''}`}>
        <summary><Icon size={15} aria-hidden="true" /><div className="execution-step-heading"><strong>{t('progress.' + step.kind)}</strong><span title={summary} className={step.kind === 'command' ? 'step-command' : ''}>{summary}</span></div>{state && <span className={`step-state ${state}`}>{t('progress.state.' + state)}</span>}<ChevronDown className="step-chevron" size={13} /></summary>
        <div className="execution-step-content">
          {files.length > 0 && <ul className="step-files">{files.map((file, index) => {
            const kind = progressText(file.kind) || progressText(record(file.kind).type);
            const action = ({ add: 'add', create: 'add', delete: 'delete', remove: 'delete', update: 'update' } as Record<string, string>)[kind] ?? 'other';
            return <li key={index}><div><span>{t('progress.change.' + action)}</span><code>{progressText(file.path)}</code></div>{progressText(file.diff) && <pre>{progressText(file.diff)}</pre>}</li>;
          })}</ul>}
          {plan.length > 0 && <ol className="step-plan">{plan.map((item, index) => <li key={index}><span>{progressText(item.step)}</span>{['in_progress', 'pending'].includes(progressText(item.status)) && <small>{t('progress.plan.' + progressText(item.status))}</small>}</li>)}</ol>}
          {details.map(field => <section key={field.key}><h4>{t('progress.field.' + field.key)}</h4><pre>{field.value}</pre></section>)}
          {!details.length && !files.length && !plan.length && <p>{t('progress.noDetails')}</p>}
        </div>
      </details></li>;
    })}</ol> : <p className="progress-empty">{t('progress.noOperations')}</p>}
    <details className="execution-raw"><summary>{t('progress.raw', { count: raw.length })}</summary><pre>{JSON.stringify(raw.map(entry => entry.event), null, 2)}</pre></details>
  </details>;
}
