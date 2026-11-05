import { beforeEach, describe, expect, it } from 'vitest';
import { translate } from './index';

describe('i18n', () => {
  beforeEach(() => localStorage.clear());

  it('uses Chinese by default and persists English selection', () => {
    expect(translate('nav.conversations')).toBe('会话');
    localStorage.setItem('omnistream.locale', 'en-US');
    expect(translate('nav.conversations')).toBe('Conversations');
    expect(translate('connection.interfaceError', { url: 'http://localhost:4317' })).toContain('http://localhost:4317');
  });
});
