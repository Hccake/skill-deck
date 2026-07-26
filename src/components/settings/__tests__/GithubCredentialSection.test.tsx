/* @vitest-environment jsdom */

import '@/test-utils';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import type { GithubCredentialStatus } from '@/bindings';
import { useSettingsStore } from '@/stores/settings';
import { GithubCredentialSection } from '../GithubCredentialSection';

import enLocale from '@/i18n/locales/en.json';

const mockGetGithubCredentialStatus = vi.fn();
const mockSaveGithubCredential = vi.fn();
const mockClearGithubCredential = vi.fn();

function lookupLocaleKey(key: string): string | undefined {
  let cursor: unknown = enLocale;
  for (const segment of key.split('.')) {
    if (!cursor || typeof cursor !== 'object' || !(segment in cursor)) return undefined;
    cursor = (cursor as Record<string, unknown>)[segment];
  }
  return typeof cursor === 'string' ? cursor : undefined;
}

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    i18n: { language: 'en' },
    t: (key: string, options?: Record<string, unknown>) => {
      const value = lookupLocaleKey(key) ?? key;
      return value.replace(/\{\{(\w+)\}\}/g, (_, name: string) => String(options?.[name] ?? ''));
    },
  }),
}));

vi.mock('@/hooks/useTauriApi', () => ({
  getGithubCredentialStatus: (...args: unknown[]) => mockGetGithubCredentialStatus(...args),
  saveGithubCredential: (...args: unknown[]) => mockSaveGithubCredential(...args),
  clearGithubCredential: (...args: unknown[]) => mockClearGithubCredential(...args),
  getDefaultTargetAgents: vi.fn(),
  saveDefaultTargetAgents: vi.fn(),
  listAgentSelectionGroups: vi.fn(),
  getAgentSettingsSnapshot: vi.fn(),
  listAgents: vi.fn(),
  validateCustomAgentDraft: vi.fn(),
  saveCustomAgent: vi.fn(),
  duplicateCustomAgentDraft: vi.fn(),
  previewCustomAgentDelete: vi.fn(),
  deleteCustomAgent: vi.fn(),
  deleteInvalidCustomAgent: vi.fn(),
}));

const verified: GithubCredentialStatus = {
  source: 'keyring',
  storage: 'available',
  validation: 'verified',
  account: 'octocat',
  rateLimitRemaining: 4_999,
  rateLimitLimit: 5_000,
  rateLimitResetAtEpochMs: 2_000,
  retryAtEpochMs: null,
};

function resetStore() {
  useSettingsStore.setState((state) => ({
    ...state,
    githubCredential: {
      status: null,
      loadState: 'idle',
      requestId: 0,
      saving: false,
      clearing: false,
      error: null,
    },
  }));
}

describe('GithubCredentialSection', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    resetStore();
    mockGetGithubCredentialStatus.mockResolvedValue(verified);
    mockClearGithubCredential.mockResolvedValue({
      cleared: true,
      status: { ...verified, source: 'none', validation: 'unconfigured', account: null },
    });
  });

  it('shows verified keyring metadata without echoing the token and can clear it', async () => {
    render(<GithubCredentialSection />);

    expect(await screen.findByText('octocat')).toBeTruthy();
    const input = screen.getByLabelText('GitHub token') as HTMLInputElement;
    expect(input.type).toBe('password');
    expect(input.value).toBe('');

    fireEvent.click(screen.getByRole('button', { name: 'Clear' }));
    await waitFor(() => expect(mockClearGithubCredential).toHaveBeenCalledTimes(1));
  });

  it('keeps the active status and reports an invalid replacement token', async () => {
    mockSaveGithubCredential.mockResolvedValue({
      saved: false,
      status: { ...verified, source: 'none', validation: 'invalid', account: null },
    });
    render(<GithubCredentialSection />);

    expect(await screen.findByText('octocat')).toBeTruthy();
    fireEvent.change(screen.getByLabelText('GitHub token'), {
      target: { value: 'secret-token' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Verify and save' }));

    expect(await screen.findByText('This token is invalid. Check its value and permissions.'))
      .toBeTruthy();
    expect(screen.getByText('octocat')).toBeTruthy();
  });

  it('explains the environment-variable fallback when secure storage is unavailable', async () => {
    mockGetGithubCredentialStatus.mockResolvedValue({
      ...verified,
      source: 'githubTokenEnv',
      storage: 'unavailable',
      account: 'env-user',
    });
    render(<GithubCredentialSection />);

    expect(await screen.findByText('env-user')).toBeTruthy();
    expect(screen.getAllByText(/GITHUB_TOKEN/).length).toBeGreaterThan(0);
    expect(screen.queryByRole('button', { name: 'Clear' })).toBeNull();
  });

  it('shows when a rate-limited token validation can be retried', async () => {
    vi.spyOn(Date.prototype, 'toLocaleString').mockReturnValue('retry-time');
    mockSaveGithubCredential.mockResolvedValue({
      saved: false,
      status: {
        ...verified,
        source: 'none',
        validation: 'rateLimited',
        account: null,
        retryAtEpochMs: 3_000,
      },
    });
    render(<GithubCredentialSection />);

    await screen.findByText('octocat');
    fireEvent.change(screen.getByLabelText('GitHub token'), {
      target: { value: 'rate-limited-token' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Verify and save' }));

    expect(await screen.findByText('Try again after retry-time.')).toBeTruthy();
  });
});
