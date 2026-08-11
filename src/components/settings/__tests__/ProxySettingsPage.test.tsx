/* @vitest-environment jsdom */

import '@/test-utils';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { ProxySettingsPage } from '../ProxySettingsPage';
import type { NetworkProxySettings } from '@/hooks/useTauriApi';
import {
  UnsavedChangesContext,
  type UnsavedChangesRegistration,
} from '@/lifecycle/unsaved-changes-context';
import { useEnvironmentStore } from '@/stores/environment';

const mockGetProxySettings = vi.fn();
const mockSaveProxySettings = vi.fn();
const mockTestProxyConnection = vi.fn();
let mockT = (key: string) => key;

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: mockT }),
}));

vi.mock('@/hooks/useBusinessWriteBlocked', () => ({
  useBusinessWriteBlocked: () => false,
}));

vi.mock('@/hooks/useTauriApi', () => ({
  getProxySettings: (...args: unknown[]) => mockGetProxySettings(...args),
  saveProxySettings: (...args: unknown[]) => mockSaveProxySettings(...args),
  testProxyConnection: (...args: unknown[]) => mockTestProxyConnection(...args),
}));

const directSettings: NetworkProxySettings = {
  mode: 'direct',
  customProxyUrl: null,
  nativeGit: { behavior: 'useExistingGitConfig' },
  wslGit: {},
};

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((done, fail) => {
    resolve = done;
    reject = fail;
  });
  return { promise, resolve, reject };
}

function setWindowsAndWslDistros(...distros: string[]) {
  useEnvironmentStore.setState({
    environments: [
      {
        environment: { kind: 'native' },
        displayName: 'Windows',
        status: 'available' as const,
        revision: 1,
        error: null,
      },
      ...distros.map((distro) => ({
        environment: { kind: 'wsl' as const, distro_name: distro },
        displayName: distro,
        status: 'available' as const,
        revision: 1,
        error: null,
      })),
    ],
    runtimeByEnvironment: {},
  });
}

function setWindowsAndUbuntu() {
  setWindowsAndWslDistros('Ubuntu');
}

async function choose(comboboxName: string, optionName: RegExp) {
  fireEvent.click(screen.getByRole('combobox', { name: comboboxName }));
  fireEvent.click(await screen.findByRole('option', { name: optionName }));
}

describe('ProxySettingsPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockT = (key: string) => key;
    Element.prototype.scrollIntoView = vi.fn();
    useEnvironmentStore.setState({
      environments: [],
      runtimeByEnvironment: {},
    });
    mockGetProxySettings.mockResolvedValue(directSettings);
    mockSaveProxySettings.mockImplementation(async (settings) => settings);
    mockTestProxyConnection.mockResolvedValue({
      onlineServices: { status: 'succeeded', elapsedMs: 12, reasonCode: null },
      nativeGit: { status: 'failed', elapsedMs: 24, reasonCode: 'git_network' },
      wslGitByDistro: {},
    });
  });

  it('uses flat Environment sections with Select controls for every connection method', async () => {
    setWindowsAndUbuntu();
    render(<ProxySettingsPage />);

    expect(await screen.findByRole('heading', { name: 'settings.proxy.modeTitle' }))
      .toBeDefined();
    expect(screen.getByRole('heading', { name: 'Windows Git' })).toBeDefined();
    expect(screen.getByRole('heading', { name: 'WSL · Ubuntu Git' })).toBeDefined();
    expect(screen.getByRole('combobox', { name: 'settings.proxy.httpConnectionMode' }))
      .toBeDefined();
    expect(screen.getByRole('combobox', { name: 'Windows Git' })).toBeDefined();
    expect(screen.getByRole('combobox', { name: 'WSL · Ubuntu Git' })).toBeDefined();
  });

  it('reveals only the HTTP address when HTTP requests use a proxy', async () => {
    render(<ProxySettingsPage />);
    await screen.findByRole('combobox', { name: 'settings.proxy.httpConnectionMode' });

    expect(screen.queryByLabelText('settings.proxy.httpProxyAddress')).toBeNull();
    await choose('settings.proxy.httpConnectionMode', /settings\.proxy\.mode\.custom/);

    expect(screen.getByLabelText('settings.proxy.httpProxyAddress').getAttribute('placeholder'))
      .toBe('http://127.0.0.1:7890');
    expect(screen.queryByLabelText('settings.proxy.nativeGitProxyAddress')).toBeNull();
  });

  it('saves an independent Windows Git proxy while HTTP requests stay direct', async () => {
    setWindowsAndUbuntu();
    render(<ProxySettingsPage />);
    await screen.findByRole('combobox', { name: 'Windows Git' });

    await choose('Windows Git', /settings\.proxy\.gitBehavior\.useProxy/);
    fireEvent.change(screen.getByLabelText('settings.proxy.nativeGitProxyAddress'), {
      target: { value: 'http://127.0.0.1:7890' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'settings.proxy.save' }));

    await waitFor(() => expect(mockSaveProxySettings).toHaveBeenCalledWith({
      ...directSettings,
      nativeGit: {
        behavior: 'useProxy',
        proxyUrl: 'http://127.0.0.1:7890',
        scope: 'githubOnly',
      },
    }));
  });

  it('saves the all HTTP and HTTPS repository scope for Windows Git', async () => {
    setWindowsAndUbuntu();
    render(<ProxySettingsPage />);
    await screen.findByRole('combobox', { name: 'Windows Git' });

    await choose('Windows Git', /settings\.proxy\.gitBehavior\.useProxy/);
    fireEvent.change(screen.getByLabelText('settings.proxy.nativeGitProxyAddress'), {
      target: { value: 'http://127.0.0.1:7890' },
    });
    await choose('settings.proxy.scopeLabel', /settings\.proxy\.scope\.allHttpHttps/);
    fireEvent.click(screen.getByRole('button', { name: 'settings.proxy.save' }));

    await waitFor(() => expect(mockSaveProxySettings).toHaveBeenCalledWith({
      ...directSettings,
      nativeGit: {
        behavior: 'useProxy',
        proxyUrl: 'http://127.0.0.1:7890',
        scope: 'allHttpHttps',
      },
    }));
  });

  it('saves a WSL distribution that follows Windows Git', async () => {
    setWindowsAndUbuntu();
    render(<ProxySettingsPage />);
    await screen.findByRole('combobox', { name: 'WSL · Ubuntu Git' });

    await choose('WSL · Ubuntu Git', /settings\.proxy\.wslBehavior\.followNativeGit/);
    fireEvent.click(screen.getByRole('button', { name: 'settings.proxy.save' }));

    await waitFor(() => expect(mockSaveProxySettings).toHaveBeenCalledWith({
      ...directSettings,
      wslGit: { Ubuntu: { behavior: 'followNativeGit' } },
    }));
  });

  it('saves an independent proxy and scope for one WSL distribution', async () => {
    setWindowsAndUbuntu();
    render(<ProxySettingsPage />);
    await screen.findByRole('combobox', { name: 'WSL · Ubuntu Git' });

    await choose('WSL · Ubuntu Git', /settings\.proxy\.wslBehavior\.useProxy/);
    fireEvent.change(screen.getByLabelText('settings.proxy.wslGitProxyAddress'), {
      target: { value: 'http://172.20.0.1:7890' },
    });
    await choose('settings.proxy.scopeLabel', /settings\.proxy\.scope\.allHttpHttps/);
    fireEvent.click(screen.getByRole('button', { name: 'settings.proxy.save' }));

    await waitFor(() => expect(mockSaveProxySettings).toHaveBeenCalledWith({
      ...directSettings,
      wslGit: {
        Ubuntu: {
          behavior: 'useProxy',
          proxyUrl: 'http://172.20.0.1:7890',
          scope: 'allHttpHttps',
        },
      },
    }));
  });

  it('tests the unsaved draft for HTTP and every visible Git Environment', async () => {
    setWindowsAndWslDistros('Ubuntu', 'Debian');
    mockTestProxyConnection.mockResolvedValueOnce({
      onlineServices: { status: 'succeeded', elapsedMs: 12, reasonCode: null },
      nativeGit: { status: 'failed', elapsedMs: 24, reasonCode: 'git_network' },
      wslGitByDistro: {
        Ubuntu: { status: 'succeeded', elapsedMs: 18, reasonCode: null },
        Debian: { status: 'failed', elapsedMs: 31, reasonCode: 'git_network' },
      },
    });
    render(<ProxySettingsPage />);
    await screen.findByRole('combobox', { name: 'Windows Git' });

    await choose('Windows Git', /settings\.proxy\.gitBehavior\.useProxy/);
    fireEvent.change(screen.getByLabelText('settings.proxy.nativeGitProxyAddress'), {
      target: { value: 'http://127.0.0.1:7890' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'settings.proxy.testConnection' }));

    await waitFor(() => expect(mockTestProxyConnection).toHaveBeenCalledTimes(1));
    expect(mockTestProxyConnection).toHaveBeenCalledWith({
      ...directSettings,
      nativeGit: {
        behavior: 'useProxy',
        proxyUrl: 'http://127.0.0.1:7890',
        scope: 'githubOnly',
      },
    }, ['Ubuntu', 'Debian']);
    expect(screen.getByText('https://www.skills.sh/api/search?q=skill&limit=1')).toBeDefined();
    expect(screen.getAllByText('https://github.com/hccake/skill-deck.git')).toHaveLength(3);
    expect(mockSaveProxySettings).not.toHaveBeenCalled();
  });

  it('shows WSL proxy guidance only for a proxied network failure', async () => {
    setWindowsAndUbuntu();
    mockGetProxySettings.mockResolvedValue({
      ...directSettings,
      wslGit: {
        Ubuntu: {
          behavior: 'useProxy',
          proxyUrl: 'http://172.20.0.1:7890',
          scope: 'githubOnly',
        },
      },
    });
    mockTestProxyConnection.mockResolvedValue({
      onlineServices: { status: 'succeeded', elapsedMs: 1, reasonCode: null },
      nativeGit: { status: 'succeeded', elapsedMs: 1, reasonCode: null },
      wslGitByDistro: {
        Ubuntu: { status: 'failed', elapsedMs: 24, reasonCode: 'git_network' },
      },
    });
    render(<ProxySettingsPage />);
    await screen.findByRole('combobox', { name: 'WSL · Ubuntu Git' });
    fireEvent.click(screen.getByRole('button', { name: 'settings.proxy.testConnection' }));

    expect(await screen.findByText('settings.proxy.test.wslProxyHint')).toBeDefined();
    mockTestProxyConnection.mockResolvedValue({
      onlineServices: { status: 'succeeded', elapsedMs: 1, reasonCode: null },
      nativeGit: { status: 'succeeded', elapsedMs: 1, reasonCode: null },
      wslGitByDistro: {
        Ubuntu: { status: 'failed', elapsedMs: 24, reasonCode: 'git_auth' },
      },
    });
    fireEvent.click(screen.getByRole('button', { name: 'settings.proxy.testConnection' }));

    expect(await screen.findByText('settings.proxy.test.reasons.git_auth')).toBeDefined();
    expect(screen.queryByText('settings.proxy.test.wslProxyHint')).toBeNull();
  });

  it('resets connection results when an active proxy address changes', async () => {
    mockGetProxySettings.mockResolvedValue({
      ...directSettings,
      nativeGit: {
        behavior: 'useProxy',
        proxyUrl: 'http://127.0.0.1:7890',
        scope: 'githubOnly',
      },
    });
    render(<ProxySettingsPage />);
    const address = await screen.findByDisplayValue('http://127.0.0.1:7890');
    fireEvent.click(screen.getByRole('button', { name: 'settings.proxy.testConnection' }));
    expect(await screen.findByText('settings.proxy.test.reasons.git_network')).toBeDefined();

    fireEvent.change(address, { target: { value: 'http://127.0.0.1:7891' } });

    expect(screen.queryByText('settings.proxy.test.reasons.git_network')).toBeNull();
    expect(screen.getAllByText('settings.proxy.test.status.idle')).toHaveLength(2);
  });

  it('ignores a connection result completed after the draft changes', async () => {
    const user = userEvent.setup();
    const pending = deferred<{
      onlineServices: { status: 'succeeded'; elapsedMs: number; reasonCode: null };
      nativeGit: { status: 'succeeded'; elapsedMs: number; reasonCode: null };
      wslGitByDistro: Record<string, never>;
    }>();
    mockGetProxySettings.mockResolvedValue({
      ...directSettings,
      nativeGit: {
        behavior: 'useProxy',
        proxyUrl: 'http://127.0.0.1:7890',
        scope: 'githubOnly',
      },
    });
    mockTestProxyConnection.mockReturnValue(pending.promise);
    render(<ProxySettingsPage />);
    const address = await screen.findByDisplayValue('http://127.0.0.1:7890');

    await user.click(screen.getByRole('button', { name: 'settings.proxy.testConnection' }));
    await user.clear(address);
    await user.type(address, 'http://127.0.0.1:7891');
    await act(async () => pending.resolve({
      onlineServices: { status: 'succeeded', elapsedMs: 1, reasonCode: null },
      nativeGit: { status: 'succeeded', elapsedMs: 1, reasonCode: null },
      wslGitByDistro: {},
    }));

    expect(screen.getAllByText('settings.proxy.test.status.idle')).toHaveLength(2);
    expect(screen.queryByText('settings.proxy.test.status.succeeded')).toBeNull();
  });

  it('keeps the newest load result when an earlier language-bound load finishes later', async () => {
    const older = deferred<typeof directSettings>();
    const newer = deferred<typeof directSettings>();
    mockGetProxySettings
      .mockReturnValueOnce(older.promise)
      .mockReturnValueOnce(newer.promise);
    const rendered = render(<ProxySettingsPage />);

    mockT = (key: string) => `new:${key}`;
    rendered.rerender(<ProxySettingsPage />);
    await act(async () => newer.resolve({
      ...directSettings,
      nativeGit: {
        behavior: 'useProxy',
        proxyUrl: 'http://127.0.0.1:7891',
        scope: 'githubOnly',
      },
    }));
    expect(await screen.findByDisplayValue('http://127.0.0.1:7891')).toBeDefined();

    await act(async () => older.resolve(directSettings));
    expect(screen.getByDisplayValue('http://127.0.0.1:7891')).toBeDefined();
  });

  it('does not overwrite edits made while an earlier save is pending', async () => {
    const user = userEvent.setup();
    const pending = deferred<typeof directSettings>();
    mockGetProxySettings.mockResolvedValue({
      ...directSettings,
      nativeGit: {
        behavior: 'useProxy',
        proxyUrl: 'http://127.0.0.1:7890',
        scope: 'githubOnly',
      },
    });
    mockSaveProxySettings.mockReturnValue(pending.promise);
    render(<ProxySettingsPage />);
    const address = await screen.findByDisplayValue('http://127.0.0.1:7890');

    await user.clear(address);
    await user.type(address, 'http://127.0.0.1:7891');
    await user.click(screen.getByRole('button', { name: 'settings.proxy.save' }));
    await user.clear(address);
    await user.type(address, 'http://127.0.0.1:7892');
    await act(async () => pending.resolve({
      ...directSettings,
      nativeGit: {
        behavior: 'useProxy',
        proxyUrl: 'http://127.0.0.1:7891',
        scope: 'githubOnly',
      },
    }));

    expect(screen.getByDisplayValue('http://127.0.0.1:7892')).toBeDefined();
  });

  it('does not show an earlier save failure after the draft changes', async () => {
    const user = userEvent.setup();
    const pending = deferred<typeof directSettings>();
    mockGetProxySettings.mockResolvedValue({
      ...directSettings,
      nativeGit: {
        behavior: 'useProxy',
        proxyUrl: 'http://127.0.0.1:7890',
        scope: 'githubOnly',
      },
    });
    mockSaveProxySettings.mockReturnValue(pending.promise);
    render(<ProxySettingsPage />);
    const address = await screen.findByDisplayValue('http://127.0.0.1:7890');

    await user.clear(address);
    await user.type(address, 'http://127.0.0.1:7891');
    await user.click(screen.getByRole('button', { name: 'settings.proxy.save' }));
    await user.clear(address);
    await user.type(address, 'http://127.0.0.1:7892');
    await act(async () => pending.reject(new Error('stale save failure')));

    expect(screen.queryByRole('alert')).toBeNull();
  });

  it('removes a WSL section and its test result when the distribution disappears', async () => {
    setWindowsAndUbuntu();
    mockTestProxyConnection.mockResolvedValue({
      onlineServices: { status: 'succeeded', elapsedMs: 1, reasonCode: null },
      nativeGit: { status: 'succeeded', elapsedMs: 2, reasonCode: null },
      wslGitByDistro: {
        Ubuntu: { status: 'succeeded', elapsedMs: 3, reasonCode: null },
      },
    });
    render(<ProxySettingsPage />);
    await screen.findByRole('heading', { name: 'WSL · Ubuntu Git' });
    fireEvent.click(screen.getByRole('button', { name: 'settings.proxy.testConnection' }));
    expect(await screen.findByText('3 ms')).toBeDefined();

    act(() => useEnvironmentStore.setState({ environments: [] }));

    expect(screen.queryByRole('heading', { name: 'WSL · Ubuntu Git' })).toBeNull();
    expect(screen.queryByText('3 ms')).toBeNull();
  });

  it('keeps save actions in the page flow and discards the draft', async () => {
    render(<ProxySettingsPage />);
    await screen.findByRole('combobox', { name: 'settings.proxy.httpConnectionMode' });
    const discard = screen.getByRole('button', { name: 'settings.proxy.discard' });
    const save = screen.getByRole('button', { name: 'settings.proxy.save' });
    expect(discard.matches(':disabled')).toBe(true);
    expect(save.matches(':disabled')).toBe(true);
    expect(discard.querySelector('svg')).toBeNull();
    expect(save.querySelector('svg')).toBeNull();

    await choose('settings.proxy.httpConnectionMode', /settings\.proxy\.mode\.custom/);
    expect(screen.getByText('settings.proxy.unsavedChanges')).toBeDefined();
    fireEvent.click(discard);

    expect(screen.getByRole('combobox', { name: 'settings.proxy.httpConnectionMode' }).textContent)
      .toContain('settings.proxy.mode.direct');
    expect(screen.queryByLabelText('settings.proxy.httpProxyAddress')).toBeNull();
    expect(mockSaveProxySettings).not.toHaveBeenCalled();
  });

  it('registers the draft with the global unsaved-change guard', async () => {
    const registrations: UnsavedChangesRegistration[] = [];
    const register = vi.fn((registration: UnsavedChangesRegistration) => {
      registrations.push(registration);
      return vi.fn();
    });
    render(
      <UnsavedChangesContext.Provider value={{ register, guard: vi.fn() }}>
        <ProxySettingsPage />
      </UnsavedChangesContext.Provider>,
    );
    await screen.findByRole('combobox', { name: 'settings.proxy.httpConnectionMode' });

    await choose('settings.proxy.httpConnectionMode', /settings\.proxy\.mode\.custom/);

    await waitFor(() => expect(registrations.at(-1)?.dirty).toBe(true));
    act(() => registrations.at(-1)?.discard());
    expect(screen.queryByLabelText('settings.proxy.httpProxyAddress')).toBeNull();
  });

  it('shows the backend validation error without guessing which proxy field failed', async () => {
    mockSaveProxySettings.mockRejectedValue({
      kind: 'invalidProxySettings',
      data: { code: 'invalidProxyUrl' },
    });
    render(<ProxySettingsPage />);
    await screen.findByRole('combobox', { name: 'settings.proxy.httpConnectionMode' });
    await choose('settings.proxy.httpConnectionMode', /settings\.proxy\.mode\.custom/);
    fireEvent.change(screen.getByLabelText('settings.proxy.httpProxyAddress'), {
      target: { value: 'localhost:7890' },
    });

    fireEvent.click(screen.getByRole('button', { name: 'settings.proxy.save' }));

    expect((await screen.findByRole('alert')).textContent)
      .toContain('settings.proxy.errors.invalidProxyUrl');
  });

  it('shows a retry action when settings cannot be loaded', async () => {
    mockGetProxySettings
      .mockRejectedValueOnce(new Error('unavailable'))
      .mockResolvedValueOnce(directSettings);
    render(<ProxySettingsPage />);

    expect((await screen.findByRole('alert')).textContent).toContain('settings.proxy.loadError');
    fireEvent.click(screen.getByRole('button', { name: 'common.retry' }));
    expect(await screen.findByRole('combobox', {
      name: 'settings.proxy.httpConnectionMode',
    })).toBeDefined();
  });
});
