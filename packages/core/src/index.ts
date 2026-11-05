import type { AgentAdapter, AgentAction, AgentEvent, Id, Task } from '@omnistream/protocol';

export interface EventStore { append(event: AgentEvent): Promise<void>; list(sessionId: Id): Promise<AgentEvent[]>; }
export interface ActionPolicy { requiresApproval(action: AgentAction, task: Task): boolean; }

export class DefaultActionPolicy implements ActionPolicy {
  constructor(private readonly approvedTypes = new Set<string>()) {}
  requiresApproval(action: AgentAction): boolean { return action.risk === 'high' && !this.approvedTypes.has(action.type); }
}

export class InMemoryEventStore implements EventStore {
  private readonly events: AgentEvent[] = [];
  async append(event: AgentEvent) { this.events.push(event); }
  async list(sessionId: Id) { return this.events.filter(e => e.sessionId === sessionId); }
}

export interface SchedulerOptions { concurrency?: number; eventStore?: EventStore; policy?: ActionPolicy; }
export class Scheduler {
  private active = 0;
  private readonly limit: number;
  constructor(private readonly adapter: AgentAdapter, private readonly options: SchedulerOptions = {}) { this.limit = options.concurrency ?? 1; }
  async run(task: Task, context: Record<string, unknown> = {}): Promise<{ status: 'succeeded' | 'failed'; sessionId?: Id; error?: string }> {
    if (this.active >= this.limit) return { status: 'failed', error: 'scheduler concurrency limit reached' };
    this.active++;
    try {
      const session = await this.adapter.startRun({ task, context });
      for await (const event of this.adapter.subscribe(session.id)) await this.options.eventStore?.append(event);
      return { status: 'succeeded', sessionId: session.id };
    } catch (error) { return { status: 'failed', error: error instanceof Error ? error.message : String(error) }; }
    finally { this.active--; }
  }
}

export function resolveTestProfiles<T>(system: T[], project: T[], task: T[], override: T[] = []): T[] {
  return [...system, ...project, ...task, ...override];
}
