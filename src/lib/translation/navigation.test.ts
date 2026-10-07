import { expect, it } from 'vitest';
import { saveReadingReturn, getReadingReturn, clearReadingReturn } from './navigation';

it('只保存应用内部详情地址，并保留读取位置与展开状态', () => {
  clearReadingReturn();
  const record = { route: '/libraries?library=one&skill=demo', documentKey: 'one:demo', scrollTop: 240, anchorIndex: 2, offset: -10, descriptionExpanded: true };
  expect(saveReadingReturn(record)).toBe(true);
  expect(getReadingReturn()).toEqual(record);
  expect(saveReadingReturn({ ...record, route: 'https://example.com' })).toBe(false);
  expect(getReadingReturn()).toEqual(record);
  clearReadingReturn();
  expect(getReadingReturn()).toBeNull();
});
