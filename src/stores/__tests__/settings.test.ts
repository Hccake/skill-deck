import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { GithubCredentialStatus } from '@/bindings';

const mockGetGithubCredentialStatus = vi.fn();
const mockSaveGithubCredential = vi.fn();
const mockClearGithubCredential = vi.fn();

vi.mock('@/hooks/useTauriApi', () => ({
  getGithubCredentialStatus: (...args: unknown[]) => mockGetGithubCredentialStatus(...args),
  saveGithubCredential: (...args: unknown[]) => mockSaveGithubCredential(...args),
  clearGithubCredential: (...args: unknown[]) => mockClearGithubCredential(...args),
}));

import { useSettingsStore } from '../settings';
import { useSkillsDataStore } from '../skills-data';
import { useInstallWizardSessionStore } from '../install-wizard-session';

const verifiedCredential: GithubCredentialStatus = {
  source: 'keyring',
  storage: 'available',
  validation: 'verified',
  account: 'octocat',
  rateLimitRemaining: 4_999,
  rateLimitLimit: 5_000,
  rateLimitResetAtEpochMs: 2_000,
  retryAtEpochMs: null,
};

describe('useSettingsStore', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockGetGithubCredentialStatus.mockResolvedValue(verifiedCredential);
    mockClearGithubCredential.mockResolvedValue({
      cleared: true,
      status: { ...verifiedCredential, source: 'none', validation: 'unconfigured' },
      warnings: [],
    });
    useSettingsStore.setState({
      githubCredential: {
        status: null,
        loadState: 'idle',
        requestId: 0,
        saving: false,
        clearing: false,
        error: null,
      },
    });
    useInstallWizardSessionStore.setState({ revision: 0, active: false, loading: false });
  });

  it('updates theme and locale', () => {
    useSettingsStore.setState({ theme: 'light', locale: 'en' });
    useSettingsStore.getState().toggleTheme();
    useSettingsStore.getState().setLocale('zh-CN');

    expect(useSettingsStore.getState().theme).toBe('dark');
    expect(useSettingsStore.getState().locale).toBe('zh-CN');
  });

  it('loads GitHub credential status without storing the token', async () => {
    await useSettingsStore.getState().loadGithubCredential();

    expect(useSettingsStore.getState().githubCredential.status).toEqual(verifiedCredential);
    expect(JSON.stringify(useSettingsStore.getState())).not.toContain('secret-token');
  });

  it('does not replace the active credential when a new token is invalid', async () => {
    mockSaveGithubCredential.mockResolvedValue({
      saved: false,
      status: {
        ...verifiedCredential,
        source: 'none',
        validation: 'invalid',
        account: null,
      },
      warnings: [],
    });
    useSettingsStore.setState((state) => ({
      githubCredential: {
        ...state.githubCredential,
        status: verifiedCredential,
        loadState: 'ready',
      },
    }));

    const result = await useSettingsStore.getState().saveGithubCredential('secret-token');

    expect(result?.saved).toBe(false);
    expect(useSettingsStore.getState().githubCredential.status).toEqual(verifiedCredential);
    expect(JSON.stringify(useSettingsStore.getState())).not.toContain('secret-token');
  });

  it('publishes secure-storage unavailability returned by a failed save', async () => {
    const unavailableCredential = {
      ...verifiedCredential,
      source: 'none' as const,
      storage: 'unavailable' as const,
      validation: 'unavailable' as const,
      account: null,
    };
    mockSaveGithubCredential.mockResolvedValue({
      saved: false,
      status: unavailableCredential,
      warnings: [],
    });
    useSettingsStore.setState((state) => ({
      githubCredential: {
        ...state.githubCredential,
        status: verifiedCredential,
        loadState: 'ready',
      },
    }));

    await useSettingsStore.getState().saveGithubCredential('secret-token');

    expect(useSettingsStore.getState().githubCredential.status).toEqual(unavailableCredential);
  });

  it('clears stale Host GitHub cooldown UI only after credential maintenance succeeds', async () => {
    const clearCooldown = vi.fn();
    useSkillsDataStore.setState({ clearHostGithubProviderCooldown: clearCooldown });
    mockSaveGithubCredential
      .mockResolvedValueOnce({ saved: true, status: verifiedCredential, warnings: [] })
      .mockResolvedValueOnce({
        saved: true,
        status: verifiedCredential,
        warnings: ['suppressionCleanupFailed'],
      });

    await useSettingsStore.getState().saveGithubCredential('first-token');
    await useSettingsStore.getState().saveGithubCredential('second-token');

    expect(clearCooldown).toHaveBeenCalledTimes(1);
  });

  it('keeps appearance settings available but blocks credential writes during the wizard', async () => {
    useSettingsStore.setState({ theme: 'light' });
    useInstallWizardSessionStore.setState({ revision: 1, active: true });

    useSettingsStore.getState().toggleTheme();
    const saved = await useSettingsStore.getState().saveGithubCredential('secret-token');
    const cleared = await useSettingsStore.getState().clearGithubCredential();

    expect(useSettingsStore.getState().theme).toBe('dark');
    expect(saved).toBeNull();
    expect(cleared).toBeNull();
    expect(mockSaveGithubCredential).not.toHaveBeenCalled();
    expect(mockClearGithubCredential).not.toHaveBeenCalled();
  });
});
