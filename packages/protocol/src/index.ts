export type Id = string;

export type RunStatus = 'queued' | 'running' | 'paused' | 'succeeded' | 'failed' | 'cancelled';
export type TestStatus = 'passed' | 'failed' | 'skipped' | 'timed_out' | 'cancelled';

export interface Project {
  id: Id; name: string; rootPath: string; config?: Record<string, unknown>;
}
export interface TestProfile {
  id: Id; name: string; commands: string[]; workingDirectory?: string;
  environment?: Record<string, string>; timeoutMs?: number; required?: boolean;
}
export interface Script {
  id: Id; projectId: Id; name: string; path: string; contentHash: string;
  version: number; description?: string; parametersSchema?: Record<string, unknown>;
}
export interface Task {
  id: Id; projectId: Id; title: string; goal: string; status: RunStatus;
  dependsOn: Id[]; agentId?: Id; testProfileIds: Id[]; scriptIds: Id[];
  metadata: Record<string, unknown>;
}
export interface AgentMessage { role: 'user' | 'agent' | 'system'; content: string; }
export interface AgentAction { type: string; payload: Record<string, unknown>; risk: 'low' | 'high'; }
export interface AgentEvent { type: string; sessionId: Id; timestamp: string; payload: Record<string, unknown>; }
export interface AgentSession { id: Id; agentId: Id; taskId: Id; }
export interface AgentAdapter {
  discoverCapabilities(): Promise<string[]>;
  startRun(input: { task: Task; context: Record<string, unknown> }): Promise<AgentSession>;
  sendMessage(sessionId: Id, message: AgentMessage): Promise<void>;
  subscribe(sessionId: Id): AsyncIterable<AgentEvent>;
  executeAction(sessionId: Id, action: AgentAction): Promise<{ accepted: boolean; output?: unknown }>;
  cancel(sessionId: Id): Promise<void>;
  healthCheck(): Promise<{ healthy: boolean; message?: string }>;
}
export interface PluginManifest {
  id: string; version: string; name: string; runtime: 'node' | 'perry' | 'external';
  capabilities: string[]; permissions: string[]; actions?: string[];
}
