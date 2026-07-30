/* @vitest-environment jsdom */

import '@/test-utils';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import type { GithubCredentialStatus } from '@/bindings';
import { TooltipProvider } from '@/components/ui/tooltip';
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

const unconfigured: GithubCredentialStatus = {
  ...verified,
  source: 'none',
  validation: 'unconfigured',
  account: null,
  rateLimitRemaining: null,
  rateLimitLimit: null,
  rateLimitResetAtEpochMs: null,
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

function renderCredential() {
  return render(
    <TooltipProvider>
      <GithubCredentialSection />
    </TooltipProvider>,
  );
}

describe('GithubCredentialSection', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.stubGlobal('ResizeObserver', class {
      observe() {}
      unobserve() {}
      disconnect() {}
    });
    resetStore();
    mockGetGithubCredentialStatus.mockResolvedValue(verified);
    mockClearGithubCredential.mockResolvedValue({
      cleared: true,
      status: { ...verified, source: 'none', validation: 'unconfigured', account: null },
      warnings: [],
    });
  });

  it('configures an unconfigured token from a dialog', async () => {
    mockGetGithubCredentialStatus.mockResolvedValue(unconfigured);
    mockSaveGithubCredential.mockResolvedValue({
      saved: true,
      status: verified,
      warnings: [],
    });
    renderCredential();

    fireEvent.click(await screen.findByRole('button', { name: 'Configure' }));

    const dialog = await screen.findByRole('dialog', { name: 'Configure GitHub Token' });
    const input = screen.getByLabelText('GitHub token') as HTMLInputElement;
    expect(dialog.contains(input)).toBe(true);
    fireEvent.change(input, { target: { value: 'secret-token' } });
    fireEvent.click(screen.getByRole('button', { name: 'Verify and save' }));

    await waitFor(() => expect(mockSaveGithubCredential).toHaveBeenCalledWith('secret-token'));
    await waitFor(() => {
      expect(screen.queryByRole('dialog', { name: 'Configure GitHub Token' })).toBeNull();
    });
    expect(screen.getByText(/octocat/)).toBeTruthy();
  });

  it('replaces or removes a saved token through explicit actions', async () => {
    renderCredential();

    expect(await screen.findByRole('heading', { name: 'GitHub Token' })).toBeTruthy();
    expect(await screen.findByText(/octocat/)).toBeTruthy();
    expect(screen.queryByLabelText('GitHub token')).toBeNull();

    fireEvent.click(screen.getByRole('button', { name: 'Replace' }));
    const replaceDialog = await screen.findByRole('dialog', { name: 'Replace GitHub Token' });
    expect(replaceDialog.contains(screen.getByLabelText('GitHub token'))).toBe(true);
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));

    fireEvent.click(screen.getByRole('button', { name: 'Remove token' }));
    const removeDialog = await screen.findByRole('alertdialog', { name: 'Remove GitHub Token?' });
    expect(mockClearGithubCredential).not.toHaveBeenCalled();
    fireEvent.click(within(removeDialog).getByRole('button', { name: 'Remove token' }));
    await waitFor(() => expect(mockClearGithubCredential).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(screen.queryByRole('alertdialog')).toBeNull());
    expect(screen.getByRole('button', { name: 'Configure' })).toBeTruthy();
  });

  it('keeps the active status and reports an invalid replacement token', async () => {
    mockSaveGithubCredential.mockResolvedValue({
      saved: false,
      status: { ...verified, source: 'none', validation: 'invalid', account: null },
    });
    renderCredential();

    expect(await screen.findByText(/octocat/)).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Replace' }));
    fireEvent.change(screen.getByLabelText('GitHub token'), {
      target: { value: 'secret-token' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Verify and save' }));

    const dialog = await screen.findByRole('dialog', { name: 'Replace GitHub Token' });
    expect(within(dialog).getByText('This token is invalid. Check its value and permissions.')).toBeTruthy();
    expect(screen.getByLabelText('GitHub token').getAttribute('aria-invalid')).toBe('true');
    expect(screen.getByText(/octocat/)).toBeTruthy();
  });

  it('reports suppression cleanup degradation without changing a successful save', async () => {
    mockSaveGithubCredential.mockResolvedValue({
      saved: true,
      status: verified,
      warnings: ['suppressionCleanupFailed'],
    });
    renderCredential();

    await screen.findByText(/octocat/);
    fireEvent.click(screen.getByRole('button', { name: 'Replace' }));
    fireEvent.change(screen.getByLabelText('GitHub token'), {
      target: { value: 'secret-token' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Verify and save' }));

    expect(await screen.findByText(
      'The token change succeeded, but the saved update-check suppression could not be cleared. A later check may remain suppressed.',
    ))
      .toBeTruthy();
  });

  it('keeps the unavailable state compact while exposing the environment fallback from title help', async () => {
    mockGetGithubCredentialStatus.mockResolvedValue({
      ...verified,
      source: 'none',
      storage: 'unavailable',
      validation: 'unconfigured',
      account: null,
    });
    renderCredential();

    expect(await screen.findByText('No system secure storage was detected.')).toBeTruthy();
    expect(screen.queryByText(/GITHUB_TOKEN/)).toBeNull();
    expect(screen.queryByRole('button', { name: 'Configure' })).toBeNull();
    expect(screen.queryByRole('button', { name: 'Remove token' })).toBeNull();

    await userEvent.hover(screen.getByRole('button', { name: 'Other ways to provide a GitHub Token' }));
    expect((await screen.findByRole('tooltip')).textContent).toContain(
      'You can also use the GH_TOKEN or GITHUB_TOKEN environment variable. A saved token takes precedence.',
    );

    fireEvent.click(screen.getByRole('button', { name: 'Recheck' }));
    await waitFor(() => expect(mockGetGithubCredentialStatus).toHaveBeenCalledTimes(2));
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
    renderCredential();

    await screen.findByText(/octocat/);
    fireEvent.click(screen.getByRole('button', { name: 'Replace' }));
    fireEvent.change(screen.getByLabelText('GitHub token'), {
      target: { value: 'rate-limited-token' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Verify and save' }));

    const dialog = await screen.findByRole('dialog', { name: 'Replace GitHub Token' });
    expect(within(dialog).getByText('Try again after retry-time.')).toBeTruthy();
    expect(screen.getByLabelText('GitHub token').getAttribute('aria-invalid')).toBe('false');
  });
});
