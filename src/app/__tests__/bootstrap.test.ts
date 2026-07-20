import { beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
  changeLanguage: vi.fn(),
  applyTheme: vi.fn(),
}));

vi.mock('@/i18n', () => ({ default: { changeLanguage: mocks.changeLanguage } }));
vi.mock('@/stores/settings', () => ({
  useSettingsStore: {
    getState: () => ({ theme: 'dark', locale: 'zh-CN' }),
  },
  applyPersistedAppearance: mocks.applyTheme,
}));

import { bootstrapAppPreferences } from '../bootstrap';

describe('bootstrapAppPreferences', () => {
  beforeEach(() => {
    mocks.changeLanguage.mockReset();
    mocks.applyTheme.mockReset();
  });

  it('applies persisted appearance before the window renders', () => {
    bootstrapAppPreferences();

    expect(mocks.changeLanguage).toHaveBeenCalledWith('zh-CN');
    expect(mocks.applyTheme).toHaveBeenCalledWith('dark');
  });
});
