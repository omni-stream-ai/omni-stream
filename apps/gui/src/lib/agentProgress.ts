import type { AgentCall } from './api';

type RecordValue = Record<string, unknown>;
export type ExecutionStep = {
  id: string;
  kind: 'command' | 'files' | 'image' | 'tool' | 'search' | 'plan' | 'context' | 'update';
  data: RecordValue;
  phase: string;
};
const record = (value: unknown): RecordValue => value && typeof value === 'object' && !Array.isArray(value) ? value as RecordValue : {};
export const progressText = (value: unknown): string => typeof value === 'string' ? value : '';

// Keep wire events intact; this projection joins an operation's lifecycle for display.
export function executionSteps(events: AgentCall['events']): ExecutionStep[] {
  const steps: ExecutionStep[] = [];
  const byId = new Map<string, ExecutionStep>();
  let turn = '';
  for (const [index, entry] of events.entries()) {
    const event = entry.event;
    if (event?.type !== 'progress') continue;
    const method = event.method ?? '';
    const data = record(event.data);
    if (typeof data.turnId === 'string') turn = data.turnId;
    const item = record(data.item);
    const id = progressText(item.id) || progressText(data.itemId);
    const key = `${turn}:${id}`;
    if (/^item\/(commandExecution|fileChange)\/outputDelta$/.test(method)) {
      let existing = id ? byId.get(key) : undefined;
      if (!existing) {
        existing = { id: `step-${index}`, kind: method.includes('commandExecution') ? 'command' : 'files', data: {}, phase: 'running' };
        steps.push(existing); if (id) byId.set(key, existing);
      }
      if (existing.phase === 'completed') continue;
      existing.data = { ...existing.data, output: progressText(existing.data.output) + progressText(data.delta) };
      continue;
    }
    if (method === 'turn/plan/updated') {
      const planKey = `${turn}:plan`;
      const previous = byId.get(planKey);
      if (previous) previous.data = { ...data };
      else { const step: ExecutionStep = { id: `step-${index}`, kind: 'plan', data: { ...data }, phase: '' }; steps.push(step); byId.set(planKey, step); }
      continue;
    }
    if (method === 'omnistream/thread/forked') {
      steps.push({ id: `step-${index}`, kind: 'context', data: { ...data }, phase: '' });
      continue;
    }
    if (method === 'item/started' || method === 'item/completed') {
      const type = progressText(item.type);
      if (['agentMessage', 'userMessage', 'reasoning', 'turnStarted', 'turnComplete'].includes(type)) continue;
      const kinds: Record<string, ExecutionStep['kind']> = { commandExecution: 'command', fileChange: 'files', mcpToolCall: 'tool', dynamicToolCall: 'tool', webSearch: 'search', imageView: 'image', imageGeneration: 'tool' };
      const existing = id ? byId.get(key) : undefined;
      const step: ExecutionStep = existing ?? { id: `step-${index}`, kind: kinds[type] ?? 'update', data: {}, phase: '' };
      step.data = { ...step.data, ...(Object.keys(item).length ? item : data) };
      // A final aggregate supersedes streamed fragments, including an empty result.
      if (typeof item.aggregatedOutput === 'string') step.data.output = item.aggregatedOutput;
      step.phase = method === 'item/completed' ? 'completed' : 'running';
      if (!existing) { steps.push(step); if (id) byId.set(key, step); }
      continue;
    }
    // Transport bookkeeping and streaming text are available in raw events only.
    if (/^(thread\/|turn\/|account\/|item\/(agentMessage|reasoning|plan)\/)/.test(method) || method === 'initialized') continue;
    // Some adapters publish a command directly instead of an item envelope.
    steps.push({ id: `step-${index}`, kind: typeof data.command === 'string' ? 'command' : 'update', data: { ...data }, phase: '' });
  }
  return steps;
}

export function stepState(step: ExecutionStep, active: boolean): 'running' | 'failed' | 'cancelled' | 'waiting' | undefined {
  const status = progressText(step.data.status).replace(/[_-]/g, '').toLowerCase();
  if (step.data.error || (typeof step.data.exitCode === 'number' && step.data.exitCode !== 0) || ['failed', 'error', 'declined'].includes(status)) return 'failed';
  if (['cancelled', 'canceled', 'interrupted'].includes(status)) return 'cancelled';
  if (step.phase === 'completed') return;
  if (['waiting', 'waitingforapproval', 'pendingapproval'].includes(status) && active) return 'waiting';
  if (active && (status ? ['inprogress', 'running', 'pending'].includes(status) : step.phase === 'running')) return 'running';
}

export function stepSummary(step: ExecutionStep): string {
  const data = step.data;
  if (step.kind === 'command') return progressText(data.command);
  if (step.kind === 'files' || step.kind === 'image') return Array.isArray(data.changes) ? data.changes.map(value => progressText(record(value).path)).filter(Boolean).join(', ') : progressText(data.path);
  if (step.kind === 'tool') return [progressText(data.server), progressText(data.tool)].filter(Boolean).join(' / ');
  if (step.kind === 'search') return progressText(data.query) || progressText(record(data.action).query) || progressText(record(data.action).url) || (Array.isArray(record(data.action).queries) ? (record(data.action).queries as unknown[]).map(progressText).filter(Boolean).join(', ') : '');
  return progressText(data.message) || progressText(data.explanation) || progressText(data.title);
}

export function stepDetails(step: ExecutionStep): { key: string; value: string }[] {
  const data = step.data;
  const fields: { key: string; value: unknown }[] = [
    { key: 'command', value: data.command },
    { key: 'directory', value: data.cwd }, { key: 'arguments', value: data.arguments },
    { key: 'output', value: data.output }, { key: 'result', value: data.result },
    { key: 'error', value: data.error }, { key: 'exitCode', value: data.exitCode },
  ];
  return fields.filter(field => field.value !== undefined && field.value !== null && field.value !== '').map(field => ({ key: field.key, value: typeof field.value === 'string' ? field.value : JSON.stringify(field.value, null, 2) }));
}
