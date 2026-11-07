import type { ConversationMessage } from '../lib/api';

export function automationDraft(message: Pick<ConversationMessage, 'tool' | 'result'>): { id: number; title?: string } | undefined {
  if (message.tool !== 'propose_automation' || !message.result || typeof message.result !== 'object') return;
  const result = message.result as { error?: unknown; automation?: { id?: unknown; title?: unknown } };
  const draft = result.automation;
  if (result.error || !draft || typeof draft.id !== 'number' || !Number.isSafeInteger(draft.id) || draft.id <= 0) return;
  return { id: draft.id, title: typeof draft.title === 'string' ? draft.title : undefined };
}

// Keep persisted history intact. Place successful draft references after the
// following reply, but never carry them into a later user turn.
export function messagesForDisplay(history: ConversationMessage[]): ConversationMessage[] {
  const visible: ConversationMessage[] = [];
  let drafts: ConversationMessage[] = [];
  const flush = () => { visible.push(...drafts); drafts = []; };
  for (const message of history) {
    if (message.role === 'user') flush();
    if (message.role === 'assistant' && automationDraft(message)) {
      drafts.push(message);
      continue;
    }
    visible.push(message);
    if (message.role === 'assistant' && !message.tool && !message.call_id && !message.automation_confirmation && !message.plugin_setup && typeof message.content === 'string' && message.content.trim()) flush();
  }
  // A saved draft stays accessible even if the final reply fails or is pending.
  flush();
  return visible;
}
