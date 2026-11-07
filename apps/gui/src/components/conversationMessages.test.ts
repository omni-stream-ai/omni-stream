import { expect, it } from 'vitest';
import type { ConversationMessage } from '../lib/api';
import { messagesForDisplay } from './conversationMessages';
const user: ConversationMessage = { role: 'user', content: '创建草案' };
const draft: ConversationMessage = { role: 'assistant', content: '', tool: 'propose_automation', result: { automation: { id: 1, title: '日报' } } };
const reply: ConversationMessage = { role: 'assistant', content: '草案已保存，尚待启用。' };
it('places draft references after the reply without changing stored history', () => {
  const otherTool: ConversationMessage = { role: 'assistant', content: '', tool: 'list_automations' };
  const history = [user, draft, otherTool, reply];
  expect(messagesForDisplay(history)).toEqual([user, otherTool, reply, draft]);
  expect(history).toEqual([user, draft, otherTool, reply]);
});
it('keeps unfinished-turn drafts accessible and never moves them across a user message', () => {
  const nextUser: ConversationMessage = { role: 'user', content: '下一条消息' };
  expect(messagesForDisplay([user, draft])).toEqual([user, draft]);
  expect(messagesForDisplay([user, draft, nextUser, reply])).toEqual([user, draft, nextUser, reply]);
});
it('preserves multiple drafts and leaves failed creations in their original position', () => {
  const second = { ...draft, result: { automation: { id: 2 } } };
  const failed = { ...draft, result: { error: 'failed' } };
  expect(messagesForDisplay([user, draft, failed, second, reply])).toEqual([user, failed, reply, draft, second]);
});
it('does not treat agent calls or confirmation cards as the final reply', () => {
  const call: ConversationMessage = { role: 'assistant', content: 'Agent output', call_id: 1 };
  const review: ConversationMessage = { role: 'assistant', content: '请确认', automation_confirmation: { automation_id: 1, confirmation_id: 1 } };
  expect(messagesForDisplay([user, draft, call, review, reply])).toEqual([user, call, review, reply, draft]);
});
