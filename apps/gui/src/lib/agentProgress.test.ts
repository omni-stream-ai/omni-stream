import { describe, expect, it } from 'vitest';
import { executionSteps, stepDetails, stepState, stepSummary } from './agentProgress';

const progress = (method: string, data: unknown) => ({ event: { type: 'progress', method, data } });
const start = (item: object, turnId = 'turn-1') => progress('item/started', { turnId, item });
const end = (item: object, turnId = 'turn-1') => progress('item/completed', { turnId, item });

describe('execution steps', () => {
  it('joins a command lifecycle and uses the final aggregate without duplicating streamed output', () => {
    const events = [start({ id: 'cmd', type: 'commandExecution', command: 'pnpm test', cwd: '/project', status: 'inProgress' }),
      progress('item/commandExecution/outputDelta', { turnId: 'turn-1', itemId: 'cmd', delta: 'Tests ' }),
      progress('item/commandExecution/outputDelta', { turnId: 'turn-1', itemId: 'cmd', delta: 'passed' }),
      end({ id: 'cmd', type: 'commandExecution', status: 'completed', aggregatedOutput: 'Tests passed\n', exitCode: 0 })];
    const original = JSON.stringify(events);
    const steps = executionSteps(events);
    expect(steps).toHaveLength(1);
    expect(stepSummary(steps[0])).toBe('pnpm test');
    expect(stepDetails(steps[0])).toEqual([{ key: 'command', value: 'pnpm test' }, { key: 'directory', value: '/project' }, { key: 'output', value: 'Tests passed\n' }, { key: 'exitCode', value: '0' }]);
    expect(stepState(steps[0], true)).toBeUndefined();
    expect(JSON.stringify(events)).toBe(original);
  });
  it('retains partial output and reports running only while the call is active', () => {
    const [step] = executionSteps([start({ id: 'cmd', type: 'commandExecution', command: 'cargo check' }), progress('item/commandExecution/outputDelta', { itemId: 'cmd', delta: 'Checking…' })]);
    expect(step.data.output).toBe('Checking…');
    expect(stepState(step, true)).toBe('running');
    expect(stepState(step, false)).toBeUndefined();
    const [finished] = executionSteps([start({ id: 'cmd', type: 'commandExecution', status: 'inProgress' }), end({ id: 'cmd', type: 'commandExecution', exitCode: 0 })]);
    expect(stepState(finished, true)).toBeUndefined();
  });
  it('keeps operations in separate turns distinct and preserves failures and file patches', () => {
    const steps = executionSteps([start({ id: 'same', type: 'commandExecution', command: 'first' }), end({ id: 'same', type: 'commandExecution', exitCode: 2, aggregatedOutput: 'Test failed' }), start({ id: 'same', type: 'commandExecution', command: 'retry' }, 'turn-2'), end({ id: 'edit', type: 'fileChange', changes: [{ path: 'src/app.ts', kind: { type: 'update' }, diff: '-old\n+new' }], status: 'completed' }, 'turn-2')]);
    expect(steps).toHaveLength(3);
    expect(stepState(steps[0], true)).toBe('failed');
    expect(stepSummary(steps[1])).toBe('retry');
    expect(stepSummary(steps[2])).toBe('src/app.ts');
    expect(steps[2].data.changes).toEqual([{ path: 'src/app.ts', kind: { type: 'update' }, diff: '-old\n+new' }]);
  });
  it('shows only the latest plan for a turn and excludes bookkeeping and duplicate messages', () => {
    const steps = executionSteps([progress('thread/tokenUsage/updated', { tokens: 500 }), progress('turn/started', { turnId: 'turn-1' }), start({ id: 'text', type: 'agentMessage', text: 'hello' }), progress('item/reasoning/summaryTextDelta', { delta: 'reasoning' }), progress('turn/plan/updated', { turnId: 'turn-1', plan: [{ step: 'Inspect', status: 'pending' }] }), progress('turn/plan/updated', { turnId: 'turn-1', plan: [{ step: 'Inspect', status: 'completed' }, { step: 'Fix', status: 'in_progress' }] })]);
    expect(steps).toHaveLength(1);
    expect(steps[0].data.plan).toEqual([{ step: 'Inspect', status: 'completed' }, { step: 'Fix', status: 'in_progress' }]);
  });
  it('keeps unfamiliar events and handles sanitized or malformed payloads without inventing operations', () => {
    const steps = executionSteps([progress('extension/update', { message: 'Waiting for a remote resource' }), progress('item/started', { message: 'Plugin setup in progress' }), progress('other/event', null), end({ type: 'imageView', path: 'preview.png' })]);
    expect(steps.map(step => step.kind)).toEqual(['update', 'update', 'update', 'image']);
    expect(stepSummary(steps[0])).toBe('Waiting for a remote resource');
    expect(stepSummary(steps[1])).toBe('Plugin setup in progress');
    expect(stepState(steps[2], true)).toBeUndefined();
    expect(stepSummary(steps[3])).toBe('preview.png');
  });
  it('merges tool results and reports tool errors independently of command exit codes', () => {
    const [step] = executionSteps([start({ id: 'tool', type: 'mcpToolCall', server: 'github', tool: 'list_issues', arguments: { repo: 'demo' } }), end({ id: 'tool', type: 'mcpToolCall', status: 'failed', error: { message: 'Permission denied' } })]);
    expect(stepSummary(step)).toBe('github / list_issues');
    expect(stepState(step, false)).toBe('failed');
    expect(stepDetails(step).find(field => field.key === 'error')?.value).toContain('Permission denied');
  });
  it('retains streamed output when its start event is missing and honors empty final output', () => {
    const steps = executionSteps([progress('item/commandExecution/outputDelta', { itemId: 'missing-start', delta: 'temporary' }), progress('item/completed', { item: { id: 'missing-start', type: 'commandExecution', aggregatedOutput: '', exitCode: 0 } })]);
    expect(steps).toHaveLength(1);
    expect(stepDetails(steps[0])).toEqual([{ key: 'exitCode', value: '0' }]);
  });
});
