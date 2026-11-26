import type {} from 'vitest/jsdom';
import { afterEach, expect, it } from 'vitest';

const key = 'skill-deck-test-storage';

afterEach(() => {
  jsdom.window.localStorage.removeItem(key);
  jsdom.window.sessionStorage.removeItem(key);
});

it('shares browser storage with the current jsdom window and keeps session data separate', () => {
  localStorage.setItem(key, 'local');
  sessionStorage.setItem(key, 'session');

  expect(jsdom.window.localStorage.getItem(key)).toBe('local');
  expect(jsdom.window.sessionStorage.getItem(key)).toBe('session');

  jsdom.window.localStorage.removeItem(key);
  expect(localStorage.getItem(key)).toBeNull();
  expect(sessionStorage.getItem(key)).toBe('session');
});
