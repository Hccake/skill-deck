import { expect, it } from 'vitest';
import { acceptsTranslation } from './session';

it('同一对象重新打开后拒绝旧会话和关闭后的结果', () => {
  const current = { generation: 3, documentKey: 'native:global:demo', contentRevision: 'r1', sessionId: 'new', enabled: true };
  expect(acceptsTranslation(current, { ...current, generation: 1, sessionId: 'old' })).toBe(false);
  expect(acceptsTranslation(current, current)).toBe(true);
  expect(acceptsTranslation({ ...current, enabled: false }, current)).toBe(false);
});
