import { translate } from '../i18n';

export type Approval = {
  id: number;
  task_id?: number;
  action: string;
  payload: Record<string, unknown>;
  status: string;
};

export type ChatReply = { message: string; project: string; agent: string };
export type ConversationMessage = { automation_confirmation?: { automation_id: number; confirmation_id: number }; plugin_setup?: PluginSetupContext; role: 'user' | 'assistant'; content: string; call_id?: number; agent?: string; tool?: string; args?: unknown; result?: unknown; attachments?: { id: string; name: string; type: string; data: string }[] };
export type ApprovalMode = 'default' | 'safe' | 'all';
export type ModelSelection = { provider: string | null; model: string | null; reasoning: string | null };
export type ModelOption = { id: string; name: string };
export type ProviderModels = { models: ModelOption[]; reasoning: string[] };
export type ModelCatalog = { providers: { id: string; name: string }[]; selection: ModelSelection };
export type ConversationSettings = { title?: string | null; archived?: boolean; approval_mode: ApprovalMode };
export type AgentCall = { task_id: number; scope: string; agent: string; task: { id: number; project: string; title: string; status: string; last_output?: string }; events: { event?: { type: string; delta?: string; message?: string; output?: string; method?: string; data?: unknown } }[]; approvals: Approval[] };
export type ConversationDetail = { history: ConversationMessage[]; calls: AgentCall[]; settings: ConversationSettings; approvals?: Approval[] };

const configuredUrl = import.meta.env.VITE_OMNISTREAM_URL as string | undefined;
let baseUrl = (localStorage.getItem('omnistream.url') ?? configuredUrl ?? 'http://127.0.0.1:4317').replace(/\/$/, '');

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  let response: Response;
  try {
    response = await fetch(`${baseUrl}${path}`, {
      ...init,
      headers: { 'content-type': 'application/json', ...(init?.headers ?? {}) },
    });
  } catch {
    throw new Error(translate('connection.addressError'));
  }
  const body = await response.json().catch(() => ({}));
  if (response.status === 404 && path.startsWith('/conversation/')) throw new Error(translate('connection.interfaceError', { url: baseUrl }));
  if (!response.ok) throw new Error(body.error ?? translate('request.failed', { status: response.status }));
  return body as T;
}

export const api = {
  agents: () => request<NamedAgent[]>('/agents'),
  agentEngines: () => request<{ name: string; protocol: string }[]>('/agent-engines'),
  saveAgent: (definition: AgentDefinition, id?: number) => request<NamedAgent>(id === undefined ? '/agents' : `/agents/${id}`, { method: 'POST', body: JSON.stringify(definition) }),
  deleteAgent: (id: number) => request<{ deleted: boolean }>(`/agents/${id}`, { method: 'DELETE' }),
  get baseUrl() { return baseUrl; },
  setBaseUrl(url: string) {
    baseUrl = url.trim().replace(/\/$/, '');
    localStorage.setItem('omnistream.url', baseUrl);
  },
  health: () => request<{ ok: boolean }>('/health'),
  agentMemories: (id: number) => request<AgentMemory[]>(`/agents/${id}/memory`),
  deleteAgentMemory: (id: number, key: string) => request<{ deleted: boolean }>(`/agents/${id}/memory`, { method: 'POST', body: JSON.stringify({ operation: 'delete', key }) }),
  projects: () => request<{ name: string; path: string }[]>('/projects'),
  completeProjectPath: (path: string) => request<string[]>('/projects/complete', { method: 'POST', body: JSON.stringify({ path }) }),
  approvals: () => request<Approval[]>('/approvals'),
  approvalMode: () => request<{ mode: ApprovalMode }>('/settings/approval-mode'),
  setApprovalMode: (mode: ApprovalMode) => request<{ mode: ApprovalMode }>('/settings/approval-mode', { method: 'POST', body: JSON.stringify({ mode }) }),
  conversation: (id: string) => request<ConversationMessage[]>(`/conversation?project=${encodeURIComponent(id)}`),
  conversations: () => request<string[]>('/conversations'),
  conversationDetail: (conversation: string) => request<ConversationDetail>('/conversation/detail', { method: 'POST', body: JSON.stringify({ conversation }) }),
  conversationSettings: (conversation: string) => request<ConversationSettings>('/conversation/settings/read', { method: 'POST', body: JSON.stringify({ conversation }) }),
  saveConversationSettings: (conversation: string, settings: ConversationSettings) => request<ConversationSettings>('/conversation/settings', { method: 'POST', body: JSON.stringify({ conversation, settings }) }),
  renameConversation: (conversation: string, title: string) => request<ConversationSettings>('/conversation/rename', { method: 'POST', body: JSON.stringify({ conversation, title }) }),
  archiveConversation: (conversation: string, archived = true) => request<ConversationSettings>('/conversation/archive', { method: 'POST', body: JSON.stringify({ conversation, archived }) }),
  deleteConversation: (conversation: string) => request<{ deleted: boolean }>('/conversation/delete', { method: 'POST', body: JSON.stringify({ conversation }) }),
  modelCatalog: () => request<ModelCatalog>('/settings/models'),
  providerModels: (provider: string, refresh = false) => request<ProviderModels>('/settings/models/list', { method: 'POST', body: JSON.stringify({ provider, refresh }) }),
  selectModel: (selection: ModelSelection) => request<ModelSelection>('/settings/models/selection', { method: 'POST', body: JSON.stringify(selection) }),
  respond: (id: number, response: unknown) => request(`/approvals/${id}/respond`, { method: 'POST', body: JSON.stringify(response) }),
  chat: (input: { message: string; project?: string; conversation?: string }) =>
    request<ChatReply>('/chat', { method: 'POST', body: JSON.stringify(input) }),
  cancelAgentCall: (id: number) => request<{ status: string }>(`/agent-calls/${id}/cancel`, { method: 'POST' }),
  approve: (id: number, scope: 'turn' | 'session' = 'turn') => request<{ approved: boolean }>(`/approvals/${id}/approve`, { method: 'POST', body: JSON.stringify({ scope }) }),
  reject: (id: number) => request<{ rejected: boolean }>(`/approvals/${id}/reject`, { method: 'POST' }),
};

export type AutomationSchedule = { kind: 'daily'; hour: number; minute: number; second?: number; utc_offset_minutes: number } | { kind: 'interval'; seconds: number };
export type DateOverride = { date: string; source: string; reason: string };
export type AutomationCalendar = { weekdays?: number[]; include_dates?: DateOverride[]; exclude_dates?: DateOverride[] };
export type AutomationWindow = { kind: 'lookback'; seconds: number } | { kind: 'previous_effective_day'; hour: number; minute: number; second?: number };
export type AutomationTarget = { type: 'in_app'; id: string } | { type: 'plugin_action'; id: string; plugin: string; action: string; destination: Record<string, string>; payload: Record<string, string>; max_attempts?: number | null };
export type AutomationConfirmation = {
  id: number; scheduled_at: number; requested_at: number; request_at: number; deadline: number | null;
  trigger: 'manual' | 'schedule'; status: 'pending' | 'approved' | 'ready' | 'consumed' | 'skipped' | 'expired' | 'invalidated';
  goal: string; revision: number; conversation: string; discussion_active: boolean; execution_source?: 'user' | 'timeout' | null; execution_goal?: string | null;
  snapshot: { title: string; goal?: string; on_timeout?: 'skip' | 'wait' | 'run'; project?: string | null; agent_id?: number | null; utc_offset_seconds: number; time_window: { start?: number; end?: number; error?: string } | null; delivery_targets: AutomationTarget[]; accounts: { target_id: string; account: { account?: string; auth_mode?: string } }[] };
};
export type AutomationOptions = { confirmation?: { before_seconds: number; on_timeout: 'skip' | 'wait' | 'run' } | null; calendar?: AutomationCalendar; time_window?: AutomationWindow | null; delivery_targets?: AutomationTarget[]; delivery_policy?: 'independent' | 'fail_fast' | 'fallback_in_app'; silent_unmet?: boolean; allow_calendar_updates?: boolean };
export type AutomationReceipt = { target_id: string; idempotency_key: string; status: string; attempts: number; updated_at: number; result: unknown; error?: string | null };
export type AgentDefinition = { name: string; description: string; instructions: string; engine: string; plugins: string[] | null };
export type NamedAgent = AgentDefinition & { id: number };
export type AgentMemory = { key: string; content: string; created_at: number; updated_at: number; source: string };
export type AutomationDraft = AutomationOptions & { title: string; goal: string; schedule: AutomationSchedule; project?: string | null; conversation?: string | null; agent_id?: number | null };
export type AutomationRun = { scheduled_at?: number; context?: { delivery_targets?: AutomationTarget[]; time_window?: { start?: number; end?: number; error?: string } }; deliveries?: AutomationReceipt[]; skip_reason?: string | null; generation_output?: string | null; scope?: string | null; id: number; started_at: number; finished_at: number | null; status: string; output: string };
export type Automation = AutomationDraft & { confirmations?: AutomationConfirmation[]; authorization_audit?: { at: number; action: string }[]; delivery_grants?: { target_id: string; generation: string; account: { account?: string; auth_mode?: string; user_id?: string; team_id?: string }; granted_at: number }[]; calendar_audit?: { at: number; actor: string; reason: string; calendar: AutomationCalendar }[]; id: number; status: 'pending' | 'active' | 'paused' | 'cancelled'; next_run_at: number | null; manual_run_at?: number | null; runs: AutomationRun[] };
export const automationApi = {
  confirmation: (id: number, review: number) => request<AutomationConfirmation>(`/automations/${id}/confirmations/${review}`),
  decide: (id: number, review: number, action: 'update' | 'approve' | 'skip', input: { revision: number; goal?: string; apply_to_future?: boolean }) => request<AutomationConfirmation>(`/automations/${id}/confirmations/${review}/${action}`, { method: 'POST', body: JSON.stringify(input) }),
  list: () => request<Automation[]>('/automations'),
  create: (draft: AutomationDraft) => request<Automation>('/automations', { method: 'POST', body: JSON.stringify(draft) }),
  update: (id: number, draft: AutomationDraft) => request<Automation>(`/automations/${id}/update`, { method: 'POST', body: JSON.stringify(draft) }),
  action: (id: number, action: 'activate' | 'pause' | 'resume' | 'cancel' | 'run' | 'revoke') => request<Automation>(`/automations/${id}/${action}`, { method: 'POST' }),
};

export type ChromeMcpSummary = { enabled: boolean; status: 'disabled' | 'unverified' | 'ready' | 'error'; package_version: string; verified_at: number | null; last_error: string | null };
export type MarketplacePlugin = {
  browser?: ChromeMcpSummary;
  manifest: { id: string; name: string; version: string; runtime: string; capabilities: string[]; permissions: string[]; actions: string[] };
  installed: boolean;
  installed_version: string | null;
  connection: PluginConnection;
  actions: PluginAction[];
};
export type PluginAuthMode = 'token' | 'slack_user';
export type SlackListenScope = 'related' | 'all';
export type PluginSetupOptions = { auth_mode: PluginAuthMode; listen: boolean; listen_scope: SlackListenScope; enabled: boolean; allow_writes: boolean; locale: 'zh-CN' | 'en-US' };
export type PluginSetupContext = { plugin: string; options: PluginSetupOptions; agent: string };
export type PluginSetupResult = { conversation: string; call_id: number };
export type PluginConnection = { status: 'unconfigured' | 'connected' | 'disabled' | 'error'; auth_mode?: PluginAuthMode; app_token_configured?: boolean; listen?: boolean; listen_scope?: SlackListenScope; user_id?: string | null; team_id?: string | null; configured: boolean; enabled: boolean; allow_writes: boolean; account: string | null; verified_at: number | null; last_error: string | null };
export type PluginAction = { delivery_destination_fields?: string[] | null; id: string; write: boolean; input_schema: { properties: Record<string, { type: string; minimum?: number; maximum?: number; maxLength?: number; enum?: string[]; default?: unknown }>; required: string[] } };
export type PluginResult = { data: unknown; next_cursor: string | null; next_page: number | null; sync_warning?: string };
export type SlackMessage = { id: number; ts: string; user: string; own: boolean; text: string; thread_ts: string | null; deleted: boolean; edited: boolean };
export type SlackMessages = { messages: SlackMessage[]; next_before: string | null };
export type SlackInboxState = { connection: PluginConnection; listener: { state: 'stopped' | 'connecting' | 'listening' | 'reconnecting' | 'error'; error: string | null; connected_at: number | null; last_event_at: number | null }; conversations: { channel: string; channel_type: string; ts: string; text: string; count: number }[] };
export const slackApi = {
  inbox: () => request<SlackInboxState>('/plugins/slack/inbox'),
  messages: (channel: string, before?: string) => request<SlackMessages>('/plugins/slack/inbox/read', { method: 'POST', body: JSON.stringify({ channel, before }) }),
  reconnect: () => request<{ reconnecting: boolean }>('/plugins/slack/listener/reconnect', { method: 'POST' }),
  send: (input: { team_id: string; user_id: string; channel: string; text: string; thread_ts?: string }) => request<PluginResult>('/plugins/slack/inbox/send', { method: 'POST', body: JSON.stringify(input) }),
};
export const pluginApi = {
  setBrowserEnabled: (enabled: boolean) => request<MarketplacePlugin>('/plugins/chrome-devtools/connection', { method: 'POST', body: JSON.stringify({ enabled }) }),
  setup: (id: string, options: PluginSetupOptions, requestId: string) => request<PluginSetupResult>('/plugins/' + encodeURIComponent(id) + '/setup', { method: 'POST', body: JSON.stringify({ request_id: requestId, options }) }),
  connect: (id: string, input: { token?: string; auth_mode?: PluginAuthMode; app_token?: string; listen?: boolean; listen_scope?: SlackListenScope; enabled: boolean; allow_writes: boolean }) => request<MarketplacePlugin>('/plugins/' + encodeURIComponent(id) + '/connection', { method: 'POST', body: JSON.stringify(input) }),
  disconnect: (id: string) => request<MarketplacePlugin>('/plugins/' + encodeURIComponent(id) + '/connection', { method: 'DELETE' }),
  test: (id: string) => request<MarketplacePlugin>('/plugins/' + encodeURIComponent(id) + '/test', { method: 'POST' }),
  invoke: (id: string, action: string, input: Record<string, unknown>) => request<PluginResult>('/plugins/' + encodeURIComponent(id) + '/invoke', { method: 'POST', body: JSON.stringify({ action, input }) }),
  marketplace: () => request<MarketplacePlugin[]>('/plugins/marketplace'),
  install: (id: string) => request<MarketplacePlugin>(`/plugins/${encodeURIComponent(id)}/install`, { method: 'POST' }),
  uninstall: (id: string) => request<MarketplacePlugin>(`/plugins/${encodeURIComponent(id)}`, { method: 'DELETE' }),
};
