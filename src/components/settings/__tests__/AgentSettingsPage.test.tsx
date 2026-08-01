/* @vitest-environment jsdom */

import '@/test-utils';
import {
  act,
  fireEvent,
  render as testingRender,
  screen,
  waitFor,
  within,
} from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createMemoryRouter, RouterProvider, useSearchParams } from 'react-router-dom';
import { AgentSettingsPage } from '../AgentSettingsPage';
import { UnsavedChangesProvider } from '@/lifecycle/UnsavedChangesProvider';
import type { AgentSettingsSnapshot } from '@/bindings';
import { TooltipProvider } from '@/components/ui/tooltip';

globalThis.ResizeObserver = class ResizeObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
};
Element.prototype.scrollIntoView = vi.fn();

function render(ui: React.ReactElement) {
  return testingRender(ui, { wrapper: TooltipProvider });
}

function selectCustomTab() {
  fireEvent.click(screen.getByRole('tab', { name: /settings\.agents\.tabs\.custom/ }));
}

function openCustomEditor() {
  selectCustomTab();
  fireEvent.click(screen.getByRole('button', { name: 'settings.agents.editNamed' }));
}

function selectCustomMenuAction(name: string) {
  selectCustomTab();
  fireEvent.pointerDown(screen.getByRole('button', { name: 'settings.agents.moreActionsNamed' }), {
    button: 0,
    ctrlKey: false,
  });
  fireEvent.click(screen.getByRole('menuitem', { name }));
}

const context = { environment: { kind: 'host' }, scope: { scope: 'global' } } as const;

function RoutedAgentSettingsHarness() {
  const [searchParams, setSearchParams] = useSearchParams();
  return (
    <UnsavedChangesProvider>
      <AgentSettingsPage
        context={context}
        view={searchParams.get('view')}
        agentId={searchParams.get('id')}
        configurationAgentId={searchParams.get('configureAgent')}
        onNavigate={(view, agentId) => {
          const nextParams = new URLSearchParams(searchParams);
          if (view === 'list') {
            nextParams.delete('view');
            nextParams.delete('id');
            nextParams.delete('configureAgent');
          } else {
            nextParams.set('view', view);
            if (agentId) nextParams.set('id', agentId);
            else nextParams.delete('id');
          }
          setSearchParams(nextParams);
        }}
      />
    </UnsavedChangesProvider>
  );
}

function renderRoutedAgentSettings(initialEntry = '/settings?section=agents') {
  const router = createMemoryRouter([{
    path: '*',
    element: <RoutedAgentSettingsHarness />,
  }], { initialEntries: [initialEntry] });
  render(<RouterProvider router={router} />);
  return router;
}

const actions = {
  loadSettings: vi.fn(async () => undefined),
  validateDraft: vi.fn(async () => null),
  saveDraft: vi.fn(async (_context?: unknown, _draft?: unknown, _revision?: unknown) => undefined),
  duplicateDraft: vi.fn(),
  loadDeleteImpact: vi.fn(async () => ({
    agentId: 'my-agent', displayName: 'My Agent', registryRevision: 'registry-1',
    environmentRevision: 'environment-1', scopes: [], losesManagementCapability: true,
    filesWillBeDeleted: false,
  })),
  deleteAgent: vi.fn(async (_context?: unknown, _id?: unknown, _revision?: unknown) => []),
  deleteInvalid: vi.fn((_context?: unknown, _index?: unknown, _revision?: unknown) => undefined),
};
const completeRequest = vi.fn(async (_agentId: string, _outcome: unknown) => undefined);
const listRuntimeAgents = vi.fn();
const toasts = vi.hoisted(() => ({ success: vi.fn(), error: vi.fn(), warning: vi.fn() }));
const registryState = vi.hoisted(() => ({
  snapshot: null as AgentSettingsSnapshot | null,
  state: 'ready' as 'idle' | 'loading' | 'ready' | 'error',
  error: null as unknown,
}));
const pageState = vi.hoisted(() => ({
  switchEnvironment: vi.fn(async () => undefined),
  discover: vi.fn(async () => undefined),
  pendingEnvironment: null as unknown,
  environments: [
    {
      environment: { kind: 'host' as const },
      displayName: 'Host',
      status: 'available' as const,
      revision: 1,
      error: null,
    },
    {
      environment: { kind: 'wsl' as const, distro_name: 'Ubuntu' },
      displayName: 'Ubuntu',
      status: 'available' as const,
      revision: 1,
      error: null,
    },
  ],
}));

vi.mock('@/hooks/useTauriApi', () => ({
  completeAgentConfiguration: (agentId: string, outcome: unknown) => completeRequest(agentId, outcome),
  listAgents: (selectedContext: unknown) => listRuntimeAgents(selectedContext),
}));

vi.mock('@/workflows/agent-definitions', () => ({
  agentDefinitionWorkflow: {
    save: async (...args: unknown[]) => {
      await actions.saveDraft(args[0], args[1], args[2]);
      return registryState.snapshot;
    },
    delete: async (...args: unknown[]) => ({
      settings: registryState.snapshot,
      warnings: await actions.deleteAgent(args[0], args[1], args[2]),
    }),
    deleteInvalid: async (...args: unknown[]) => {
      await actions.deleteInvalid(args[0], args[1], args[2]);
      return { settings: registryState.snapshot, warnings: [] };
    },
  },
}));

vi.mock('sonner', () => ({ toast: toasts }));

const snapshot: AgentSettingsSnapshot = {
  registryRevision: 'registry-1',
  activeBuiltin: [{
    id: 'codex', displayName: 'Codex', source: 'builtin', aliases: [],
    global: { enabled: true, readsShared: true, privatePath: null },
    project: { enabled: true, readsShared: true, privatePath: null },
    detection: { kind: 'anyPathExists', paths: [] }, legacyPaths: [], adapter: 'standard',
  }],
  activeCustom: [{
    definition: {
      id: 'my-agent', displayName: 'My Agent',
      global: { enabled: true, location: 'both', privatePath: { kind: 'based', base: 'home', relativePath: '.my-agent/skills' } },
      project: { enabled: true, location: 'private', privatePath: { kind: 'based', base: 'project', relativePath: '.my-agent/skills' } },
      detectionPaths: [
        { kind: 'based', base: 'home', relativePath: '.my-agent' },
        { kind: 'absolute', path: '/opt/my-agent' },
      ],
    },
    raw: {},
  }],
  disabledConflicts: [],
  invalidCustomRecords: [],
  currentEnvironment: { kind: 'host' },
  customStorageIssue: null,
};

vi.mock('@/stores/agent-registry', () => ({
  useAgentRegistryStore: (selector: (state: unknown) => unknown) => selector({
    settingsByEnvironment: {
      host: { data: registryState.snapshot, state: registryState.state, requestId: 1, error: registryState.error },
      'wsl:ubuntu': { data: registryState.snapshot, state: registryState.state, requestId: 1, error: registryState.error },
    },
    ...actions,
  }),
}));
vi.mock('@/stores/environment', () => ({
  environmentKey: (environment: { kind: string; distro_name?: string }) => (
    environment.kind === 'host' ? 'host' : `wsl:${environment.distro_name?.toLocaleLowerCase()}`
  ),
  useEnvironmentStore: (selector: (state: unknown) => unknown) => selector({
    environments: pageState.environments,
    discoveryState: 'ready',
    discoveryError: null,
    errorsByEnvironment: {},
    discover: pageState.discover,
  }),
}));
vi.mock('@/stores/workspace-context', () => ({
  useWorkspaceContextStore: (selector: (state: unknown) => unknown) => selector({
    pendingEnvironment: pageState.pendingEnvironment,
    switchEnvironment: pageState.switchEnvironment,
  }),
}));

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

describe('AgentSettingsPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    registryState.snapshot = structuredClone(snapshot);
    registryState.state = 'ready';
    registryState.error = null;
    actions.validateDraft.mockResolvedValue(null);
    actions.saveDraft.mockResolvedValue(undefined);
    actions.deleteAgent.mockResolvedValue([]);
    completeRequest.mockResolvedValue(undefined);
    listRuntimeAgents.mockReturnValue(new Promise(() => undefined));
    actions.loadDeleteImpact.mockResolvedValue({
      agentId: 'my-agent', displayName: 'My Agent', registryRevision: 'registry-1',
      environmentRevision: 'environment-1', scopes: [], losesManagementCapability: true,
      filesWillBeDeleted: false,
    });
  });

  it('leaves Environment switching to the main-window header', () => {
    render(<AgentSettingsPage context={context} />);

    expect(screen.queryByRole('combobox', { name: 'context.environmentLabel' })).toBeNull();
  });

  it('uses a Custom-first shared toolbar above a card grid without an Inspector', async () => {
    render(<AgentSettingsPage context={context} />);

    const tabs = screen.getAllByRole('tab');
    expect(tabs[0].textContent).toContain('settings.agents.tabs.custom');
    expect(tabs[0].parentElement?.className).toContain('grid-cols-2');
    expect(document.getElementById(tabs[0].getAttribute('aria-controls') ?? '')?.getAttribute('role'))
      .toBe('tabpanel');
    expect(screen.getByLabelText('settings.agents.search.custom')).toBeDefined();
    expect(screen.getAllByRole('article')).toHaveLength(1);
    expect(screen.getByRole('article').textContent).toContain('My Agent');
    expect(screen.queryByLabelText('settings.agents.inspectorLabel')).toBeNull();
    expect(screen.queryByText('Codex')).toBeNull();

    fireEvent.mouseDown(screen.getByRole('tab', { name: /settings\.agents\.tabs\.builtin/ }), {
      button: 0,
      ctrlKey: false,
    });
    expect(screen.getAllByText('Codex').length).toBeGreaterThan(0);
    expect(screen.queryByText('My Agent')).toBeNull();
    await waitFor(() => expect(actions.loadSettings).not.toHaveBeenCalled());
  });

  it('shows only the search empty state when no card matches', () => {
    render(<AgentSettingsPage context={context} />);

    fireEvent.change(screen.getByLabelText('settings.agents.search.custom'), {
      target: { value: 'missing-agent' },
    });

    expect(screen.getByText('settings.agents.empty.searchTitle')).toBeDefined();
    expect(screen.queryByLabelText('settings.agents.inspectorLabel')).toBeNull();
    expect(screen.queryByRole('article')).toBeNull();
  });

  it('shows a loading state instead of an empty Custom registry before Settings load', () => {
    registryState.snapshot = null;
    registryState.state = 'loading';

    render(<AgentSettingsPage context={context} />);

    expect(screen.getByRole('status').textContent).toContain('common.loading');
    expect(screen.queryByText('settings.agents.empty.customTitle')).toBeNull();
  });

  it('keeps cached definitions visible while Settings refreshes', () => {
    registryState.state = 'loading';

    render(<AgentSettingsPage context={context} />);

    expect(screen.getByRole('article').textContent).toContain('My Agent');
    expect(screen.getByRole('status').textContent).toContain('settings.agents.refreshing');
  });

  it('shows the shared directory rules once while the Environment path resolves', async () => {
    render(<AgentSettingsPage context={context} />);

    const reference = screen.getByRole('group', {
      name: 'settings.agents.sharedDirectories.title',
    });
    expect(reference.compareDocumentPosition(screen.getByRole('tablist'))
      & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(within(reference).getByText('settings.agents.sharedDirectories.title')).toBeDefined();
    expect(within(reference).getByText('settings.agents.global.title')).toBeDefined();
    expect(within(reference).getByText('settings.agents.project.title')).toBeDefined();

    const globalPath = within(reference).getByText('~/.agents/skills');
    const projectPath = within(reference).getByText('.agents/skills');
    expect(globalPath.getAttribute('tabindex')).toBe('0');
    expect(globalPath.getAttribute('title')).toBeNull();
    expect(globalPath.className).toContain('inline-block');
    expect(globalPath.className).toContain('w-fit');
    expect(globalPath.className).toContain('max-w-full');
    expect(projectPath.getAttribute('tabindex')).toBe('0');

    fireEvent.focus(globalPath);
    expect((await screen.findByRole('tooltip')).textContent)
      .toContain('settings.agents.pathLoading');
  });

  it('uses one compact property grid for Skill directories and detection paths', async () => {
    listRuntimeAgents.mockResolvedValue({
      registryRevision: 'registry-1',
      environmentRevision: 'environment-1',
      environment: context.environment,
      availability: 'available',
      projectPath: null,
      agents: {
        'my-agent': {
          definition: snapshot.activeBuiltin[0],
          detection: 'detected',
          detectionReason: null,
          global: {
            enabled: true,
            readsShared: true,
            sharedPath: '/home/me/.agents/skills',
            privatePath: '/home/me/.my-agent/skills',
            readPaths: ['/home/me/.agents/skills', '/home/me/.my-agent/skills'],
            sharedPresence: 'present', privatePresence: 'present', legacyPaths: [],
          },
          project: {
            enabled: true,
            readsShared: false,
            sharedPath: '/work/.agents/skills',
            privatePath: '/work/.my-agent/skills',
            readPaths: ['/work/.my-agent/skills'],
            sharedPresence: 'missing', privatePresence: 'present', legacyPaths: [],
          },
        },
      },
    });
    render(<AgentSettingsPage context={context} />);
    selectCustomTab();

    expect((await screen.findAllByLabelText('settings.agents.preview.detection.detected')).length)
      .toBeGreaterThan(0);
    const card = screen.getByRole('article');
    expect(card.textContent).not.toContain('settings.agents.skillReading.title');
    expect(card.textContent).not.toContain('settings.agents.installDetection.title');
    expect(card.textContent).not.toContain('settings.agents.directoryQualifier.shared');
    expect(card.textContent).not.toContain('settings.agents.directoryQualifier.agent');
    expect(within(card).queryByRole('img', { name: 'settings.agents.directoryKind.shared' }))
      .toBeNull();
    expect(within(card).queryByRole('img', { name: 'settings.agents.directoryKind.private' }))
      .toBeNull();
    const globalRow = within(card).getByRole('group', {
      name: 'settings.agents.sharedDirectories.bothAriaLabel',
    });
    const projectRow = within(card).getByRole('group', {
      name: 'settings.agents.sharedDirectories.privateAriaLabel',
    });
    const detectionRow = within(card).getByRole('group', {
      name: 'settings.agents.detection.cardTooltip',
    });
    expect(globalRow.className).toContain('h-12');
    expect(projectRow.className).toContain('h-12');
    expect(detectionRow.className).toContain('h-12');
    const propertyRows = within(card).getAllByRole('group');
    expect(propertyRows).toHaveLength(3);
    expect(propertyRows.every((row) => (
      row.className.includes('grid-cols-[5rem_minmax(0,1fr)]')
    ))).toBe(true);
    const propertyLabels = card.querySelectorAll('[data-slot="agent-property-label"]');
    expect(propertyLabels).toHaveLength(3);
    expect(propertyLabels[0].className).toContain('bg-muted/20');
    expect([...propertyLabels].every((label) => label.className.includes('whitespace-nowrap')))
      .toBe(true);
    expect(within(card).getByText('settings.agents.detection.cardLabel')).toBeDefined();
    expect(within(globalRow).getByText('settings.agents.sharedDirectories.cardLabel')).toBeDefined();
    expect(within(globalRow).getByText('+').getAttribute('aria-hidden')).toBe('true');
    expect(globalRow.textContent).not.toContain('~/.agents/skills');
    expect(card.textContent).toContain('~/.my-agent/skills');
    expect(card.textContent).toContain('.my-agent/skills');
    expect(card.textContent).toContain('~/.my-agent');
    expect(card.textContent).toContain('/opt/my-agent');
    expect(card.textContent).not.toContain('+1');
    expect(card.textContent).not.toContain('settings.agents.directoryKind.shared');
    expect(card.textContent).not.toContain('settings.agents.directoryKind.private');
    const reference = screen.getByRole('group', {
      name: 'settings.agents.sharedDirectories.title',
    });
    const sharedPath = within(reference).getByText('~/.agents/skills');
    const privatePath = within(globalRow).getByText('~/.my-agent/skills');
    expect(privatePath.getAttribute('title')).toBeNull();
    expect(privatePath.className).toContain('inline-block');
    expect(privatePath.className).toContain('w-fit');
    expect(privatePath.className).toContain('max-w-full');
    expect(detectionRow.querySelector('svg')).toBeNull();
    expect([...card.querySelectorAll('[data-slot="agent-property-value"]')])
      .toHaveLength(3);

    fireEvent.focus(sharedPath);
    const tooltip = await screen.findByRole('tooltip');
    const tooltipContent = tooltip.closest('[data-slot="tooltip-content"]');
    expect(tooltipContent?.className).toContain('bg-popover');
    expect(tooltipContent?.className).toContain('text-popover-foreground');
    expect(tooltipContent?.className).toContain('text-left');
    expect(tooltip.textContent).not.toContain('Host');
    expect(tooltip.textContent).not.toContain('settings.agents.project.relativeHint');
    expect(tooltip.querySelector('[data-slot="agent-path-tooltip-kind"]')).toBeNull();
    expect(tooltip.querySelector('[data-slot="agent-path-tooltip-value"]')?.textContent)
      .toBe('/home/me/.agents/skills');
  });

  it('renders shared and unsupported read modes without repeating shared paths', () => {
    registryState.snapshot = {
      ...structuredClone(snapshot),
      activeCustom: [{
        ...structuredClone(snapshot.activeCustom[0]),
        definition: {
          ...structuredClone(snapshot.activeCustom[0].definition),
          global: { enabled: false, location: 'shared', privatePath: null },
          project: { enabled: false, location: 'shared', privatePath: null },
        },
      }],
    };
    render(<AgentSettingsPage context={context} />);

    const unsupportedCard = screen.getByRole('article');
    expect(within(unsupportedCard).getByRole('group', {
      name: 'settings.agents.readMode.globalUnsupported',
    })).toBeDefined();
    expect(within(unsupportedCard).getByRole('group', {
      name: 'settings.agents.readMode.projectUnsupported',
    })).toBeDefined();

    fireEvent.mouseDown(screen.getByRole('tab', { name: /settings\.agents\.tabs\.builtin/ }), {
      button: 0,
      ctrlKey: false,
    });
    const sharedCard = screen.getByRole('article');
    const sharedRows = within(sharedCard).getAllByRole('group', {
      name: 'settings.agents.sharedDirectories.sharedAriaLabel',
    });
    expect(sharedRows).toHaveLength(2);
    expect(sharedRows.every((row) => (
      within(row).getByText('settings.agents.sharedDirectories.cardLabel') !== null
    ))).toBe(true);
    expect(sharedCard.textContent).not.toContain('~/.agents/skills');
    expect(sharedCard.textContent).not.toContain('.agents/skills');
  });

  it('uses the Agent icon height for a two-line identity and a fixed-height card skeleton', () => {
    render(<AgentSettingsPage context={context} />);

    const card = screen.getByRole('article');
    const header = card.querySelector('header');
    const list = card.closest('[role="list"]');
    const listItem = card.closest('[role="listitem"]');
    const name = within(card).getByText('My Agent');
    const id = within(card).getByText('my-agent');

    expect(header?.className).toContain('items-center');
    expect(header?.className).not.toContain('items-start');
    expect(name.parentElement).toBe(id.parentElement);
    expect(name.parentElement?.className).toContain('flex-col');
    expect(name.className).not.toContain('max-w-40');
    expect(card.className).toContain('h-full');
    expect(card.className).toContain('grid-rows-[3.5rem_repeat(3,3rem)]');
    expect(card.className).not.toContain('grid-rows-[auto_1fr_auto]');
    expect(list?.className).toContain('items-stretch');
    expect(list?.className).toContain('min(100%,18rem)');
    expect(list?.className).not.toContain('min(100%,23rem)');
    expect(list?.className).not.toContain('items-start');
    expect(listItem?.className).toContain('h-full');
  });

  it('keeps more than two detection paths behind a fixed-height overflow entry', async () => {
    registryState.snapshot = {
      ...structuredClone(snapshot),
      activeCustom: [{
        ...structuredClone(snapshot.activeCustom[0]),
        definition: {
          ...structuredClone(snapshot.activeCustom[0].definition),
          detectionPaths: [
            { kind: 'based', base: 'home', relativePath: '.my-agent' },
            { kind: 'absolute', path: '/opt/my-agent' },
            { kind: 'based', base: 'project', relativePath: '.my-agent-marker' },
          ],
        },
      }],
    };
    render(<AgentSettingsPage context={context} />);

    const card = screen.getByRole('article');
    expect(card.textContent).toContain('~/.my-agent');
    expect(card.textContent).not.toContain('/opt/my-agent');
    expect(card.textContent).not.toContain('.my-agent-marker');
    const overflow = within(card).getByText('+2');
    expect(overflow.getAttribute('tabindex')).toBe('0');

    fireEvent.focus(overflow);
    const tooltip = await screen.findByRole('tooltip');
    expect(tooltip.textContent).toContain('/opt/my-agent');
    expect(tooltip.textContent).toContain('.my-agent-marker');
  });

  it('exposes the reason when detection is indeterminate', async () => {
    listRuntimeAgents.mockResolvedValue({
      registryRevision: 'registry-1',
      environmentRevision: 'environment-1',
      environment: context.environment,
      availability: 'available',
      projectPath: null,
      agents: {
        'my-agent': {
          definition: snapshot.activeBuiltin[0],
          detection: 'indeterminate',
          detectionReason: 'projectContextRequired',
          global: {
            enabled: true, readsShared: true, sharedPath: '/home/me/.agents/skills',
            privatePath: '/home/me/.my-agent/skills', readPaths: [],
            sharedPresence: 'present', privatePresence: 'present', legacyPaths: [],
          },
          project: {
            enabled: true, readsShared: false, sharedPath: null,
            privatePath: null, readPaths: [], sharedPresence: 'projectNotSelected',
            privatePresence: 'projectNotSelected', legacyPaths: [],
          },
        },
      },
    });

    render(<AgentSettingsPage context={context} />);

    const status = await screen.findByLabelText('settings.agents.preview.detection.indeterminate');
    expect(status.textContent).toBe('');
    expect(status.getAttribute('title')).toBeNull();
    expect(status.getAttribute('tabindex')).toBe('0');
    expect(status.className).toContain('size-6');
    expect(status.querySelector('svg')?.getAttribute('aria-hidden')).toBe('true');
    fireEvent.focus(status);
    expect((await screen.findByRole('tooltip')).textContent).toContain(
      'settings.agents.detectionReasons.projectContextRequired',
    );
  });

  it('keeps Project paths relative without a Project preview control', async () => {
    listRuntimeAgents.mockResolvedValue({
      registryRevision: 'registry-1',
      environmentRevision: 'environment-1',
      environment: context.environment,
      availability: 'available',
      projectPath: null,
      agents: {},
    });
    render(<AgentSettingsPage context={context} />);

    await waitFor(() => expect(listRuntimeAgents).toHaveBeenCalledWith({
      environment: { kind: 'host' },
      scope: { scope: 'global' },
    }));
    expect(screen.getByText('.my-agent/skills')).toBeDefined();
    expect(screen.queryByRole('combobox', { name: 'settings.agents.projectPreview.label' })).toBeNull();

    const projectPath = screen.getByText('.my-agent/skills');
    fireEvent.focus(projectPath);
    const projectTooltip = await screen.findByRole('tooltip');
    expect(projectTooltip.querySelector('[data-slot="agent-path-tooltip-kind"]')?.textContent)
      .toBe('settings.agents.directoryKind.private');
    expect(projectTooltip.querySelector('[data-slot="agent-path-tooltip-value"]')?.textContent)
      .toBe('.my-agent/skills');
    expect(projectTooltip.textContent).not.toContain('settings.agents.project.relativeHint');

    fireEvent.focus(screen.getByText('~/.my-agent'));
    const detectionTooltip = await screen.findByRole('tooltip');
    expect(detectionTooltip.querySelector('[data-slot="agent-path-tooltip-kind"]')).toBeNull();
    expect(detectionTooltip.querySelector('[data-slot="agent-path-tooltip-value"]')?.textContent)
      .toBe('~/.my-agent');
  });

  it('clears resolved runtime data while switching Environment', async () => {
    const wslContext = {
      environment: { kind: 'wsl' as const, distro_name: 'Ubuntu' },
      scope: { scope: 'global' as const },
    };
    let resolveWsl: ((value: unknown) => void) | undefined;
    const wslRuntime = new Promise((resolve) => { resolveWsl = resolve; });
    listRuntimeAgents
      .mockResolvedValueOnce({
        registryRevision: 'registry-1',
        environmentRevision: 'environment-host',
        environment: context.environment,
        availability: 'available',
        projectPath: null,
        agents: {
          'my-agent': {
            definition: snapshot.activeBuiltin[0],
            detection: 'detected',
            detectionReason: null,
            global: {
              enabled: true, readsShared: true,
              sharedPath: '/home/host/.agents/skills', privatePath: '/home/host/.my-agent/skills',
              readPaths: ['/home/host/.agents/skills'], sharedPresence: 'present', privatePresence: 'present', legacyPaths: [],
            },
            project: {
              enabled: true, readsShared: false, sharedPath: null, privatePath: null,
              readPaths: [], sharedPresence: 'projectNotSelected', privatePresence: 'projectNotSelected', legacyPaths: [],
            },
          },
        },
      })
      .mockReturnValueOnce(wslRuntime as never);

    const { rerender } = render(<AgentSettingsPage context={context} />);
    const reference = await screen.findByRole('group', {
      name: 'settings.agents.sharedDirectories.title',
    });
    fireEvent.focus(within(reference).getByText('~/.agents/skills'));
    expect((await screen.findAllByText('/home/host/.agents/skills')).length).toBeGreaterThan(0);
    const privatePath = await screen.findByText('~/.my-agent/skills');
    fireEvent.focus(privatePath);
    expect((await screen.findAllByText('/home/host/.my-agent/skills')).length).toBeGreaterThan(0);

    rerender(<AgentSettingsPage context={wslContext} />);
    expect(screen.queryAllByText('/home/host/.agents/skills')).toHaveLength(0);
    expect(screen.queryAllByText('/home/host/.my-agent/skills')).toHaveLength(0);

    resolveWsl?.({
      registryRevision: 'registry-1',
      environmentRevision: 'environment-wsl',
      environment: wslContext.environment,
      availability: 'available',
      projectPath: null,
      agents: {
        'my-agent': {
          definition: snapshot.activeBuiltin[0],
          detection: 'notDetected',
          detectionReason: null,
          global: {
            enabled: true, readsShared: true,
            sharedPath: '/home/wsl/.agents/skills', privatePath: '/home/wsl/.my-agent/skills',
            readPaths: ['/home/wsl/.agents/skills'], sharedPresence: 'missing', privatePresence: 'missing', legacyPaths: [],
          },
          project: {
            enabled: true, readsShared: false, sharedPath: null, privatePath: null,
            readPaths: [], sharedPresence: 'projectNotSelected', privatePresence: 'projectNotSelected', legacyPaths: [],
          },
        },
      },
    });
    fireEvent.focus(within(screen.getByRole('group', {
      name: 'settings.agents.sharedDirectories.title',
    })).getByText('~/.agents/skills'));
    expect((await screen.findAllByText('/home/wsl/.agents/skills')).length).toBeGreaterThan(0);
    fireEvent.focus(screen.getByText('~/.my-agent/skills'));
    expect((await screen.findAllByText('/home/wsl/.my-agent/skills')).length).toBeGreaterThan(0);
  });

  it('keeps definitions visible and offers retry when runtime resolution fails', async () => {
    listRuntimeAgents
      .mockRejectedValueOnce(new Error('runtime unavailable'))
      .mockResolvedValueOnce({
        registryRevision: 'registry-1',
        environmentRevision: 'environment-2',
        environment: context.environment,
        availability: 'available',
        projectPath: null,
        agents: {},
      });

    render(<AgentSettingsPage context={context} />);

    expect(await screen.findByText('settings.agents.runtimeError')).toBeDefined();
    expect(screen.getByRole('article').textContent).toContain('My Agent');
    expect(screen.getByLabelText('settings.agents.preview.detection.unavailable')).toBeDefined();
    const reference = screen.getByRole('group', {
      name: 'settings.agents.sharedDirectories.title',
    });
    fireEvent.focus(within(reference).getByText('~/.agents/skills'));
    expect((await screen.findByRole('tooltip')).textContent)
      .toContain('settings.agents.pathUnavailable');

    fireEvent.click(screen.getByRole('button', { name: 'common.retry' }));
    await waitFor(() => expect(listRuntimeAgents).toHaveBeenCalledTimes(2));
  });

  it('renders a recoverable Settings error without starting an automatic request loop', () => {
    registryState.snapshot = null;
    registryState.state = 'error';
    registryState.error = { kind: 'custom', data: { message: 'load failed' } };

    render(<AgentSettingsPage context={context} />);

    expect(actions.loadSettings).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'common.retry' }));
    expect(actions.loadSettings).toHaveBeenCalledTimes(1);
  });

  it('opens the Agent form as an independent page without keeping the list behind it', async () => {
    render(<AgentSettingsPage context={context} />);

    fireEvent.click(screen.getByRole('button', { name: 'settings.agents.add' }));

    expect(screen.queryByRole('dialog')).toBeNull();
    expect(screen.getByRole('heading', { name: 'settings.agents.form.title.create' })).toBeDefined();
    expect(screen.getByRole('button', { name: 'settings.agents.backToList' })).toBeDefined();
    expect(screen.queryByRole('list', { name: 'settings.agents.listLabel' })).toBeNull();
    expect(screen.getByLabelText('settings.agents.fields.id')).toBeDefined();
    expect(screen.getAllByRole('radiogroup', {
      name: 'settings.agents.skillReading.readMethod',
    })).toHaveLength(2);
    expect(screen.getByRole('textbox', {
      name: 'settings.agents.detection.pathInput 1',
    })).toBeDefined();
    await waitFor(() => expect(document.activeElement).toBe(
      screen.getByLabelText('settings.agents.fields.displayName'),
    ));
  });

  it('closes a pristine routed create page without reopening it', async () => {
    const router = renderRoutedAgentSettings();

    fireEvent.click(screen.getByRole('button', { name: 'settings.agents.add' }));
    expect(await screen.findByRole('heading', { name: 'settings.agents.form.title.create' })).toBeDefined();
    await waitFor(() => expect(router.state.location.search).toContain('view=new'));

    fireEvent.click(screen.getByRole('button', { name: 'settings.agents.backToList' }));

    await waitFor(() => expect(screen.queryByLabelText('settings.agents.fields.id')).toBeNull());
    expect(router.state.location.search).not.toContain('view=');
  });

  it('returns a pristine routed form to the Agent list on browser back', async () => {
    const router = renderRoutedAgentSettings();
    fireEvent.click(screen.getByRole('button', { name: 'settings.agents.add' }));
    await waitFor(() => expect(router.state.location.search).toContain('view=new'));

    await act(async () => {
      await router.navigate(-1);
    });

    await waitFor(() => expect(screen.queryByRole('form', {
      name: 'settings.agents.form.title.create',
    })).toBeNull());
    expect(screen.getByRole('list', { name: 'settings.agents.listLabel' })).toBeDefined();
  });

  it('loads a different pristine Agent when the edit URL changes', async () => {
    registryState.snapshot = {
      ...structuredClone(snapshot),
      activeCustom: [
        ...structuredClone(snapshot.activeCustom),
        {
          definition: {
            ...structuredClone(snapshot.activeCustom[0].definition),
            id: 'second-agent',
            displayName: 'Second Agent',
          },
          raw: {},
        },
      ],
    };
    const router = renderRoutedAgentSettings('/settings?section=agents&view=edit&id=my-agent');
    expect(await screen.findByDisplayValue('My Agent')).toBeDefined();

    await act(async () => {
      await router.navigate('/settings?section=agents&view=edit&id=second-agent');
    });

    const secondName = await screen.findByDisplayValue('Second Agent');
    await waitFor(() => expect(document.activeElement).toBe(secondName));
    expect(screen.queryByDisplayValue('My Agent')).toBeNull();
  });

  it('does not carry an alternate path draft into another routed Agent', async () => {
    registryState.snapshot = {
      ...structuredClone(snapshot),
      activeCustom: [
        ...structuredClone(snapshot.activeCustom),
        {
          definition: {
            ...structuredClone(snapshot.activeCustom[0].definition),
            id: 'second-agent',
            displayName: 'Second Agent',
            detectionPaths: [
              { kind: 'based', base: 'home', relativePath: '.second-agent' },
            ],
          },
          raw: {},
        },
      ],
    };
    const router = renderRoutedAgentSettings('/settings?section=agents&view=edit&id=my-agent');
    const firstGroup = await screen.findByRole('group', {
      name: 'settings.agents.detection.pathLabel 1',
    });
    fireEvent.click(within(firstGroup).getByRole('combobox'));
    fireEvent.click(screen.getByRole('option', { name: 'settings.agents.pathLocations.absolute' }));
    fireEvent.change(within(firstGroup).getByRole('textbox'), {
      target: { value: '/discarded-agent' },
    });
    fireEvent.click(within(firstGroup).getByRole('combobox'));
    fireEvent.click(screen.getByRole('option', { name: 'settings.agents.pathLocations.home' }));

    await act(async () => {
      await router.navigate('/settings?section=agents&view=edit&id=second-agent');
    });
    const secondGroup = await screen.findByRole('group', {
      name: 'settings.agents.detection.pathLabel 1',
    });
    expect((within(secondGroup).getByRole('textbox') as HTMLInputElement).value)
      .toBe('.second-agent');
    fireEvent.click(within(secondGroup).getByRole('combobox'));
    fireEvent.click(screen.getByRole('option', { name: 'settings.agents.pathLocations.absolute' }));

    expect((within(secondGroup).getByRole('textbox') as HTMLInputElement).value).toBe('');
  });

  it('returns to the list when a routed edit target does not exist', async () => {
    const router = renderRoutedAgentSettings('/settings?section=agents&view=edit&id=my-agent');
    expect(await screen.findByDisplayValue('My Agent')).toBeDefined();

    await act(async () => {
      await router.navigate('/settings?section=agents&view=edit&id=missing-agent');
    });

    await waitFor(() => expect(screen.queryByDisplayValue('My Agent')).toBeNull());
    expect(screen.getByRole('list', { name: 'settings.agents.listLabel' })).toBeDefined();
    expect(router.state.location.search).not.toContain('view=');
    expect(router.state.location.search).not.toContain('id=');
  });

  it('closes a routed edit page restored from the URL without reopening it', async () => {
    const router = renderRoutedAgentSettings('/settings?section=agents&view=edit&id=my-agent');

    expect(await screen.findByRole('heading', { name: 'settings.agents.form.title.edit' })).toBeDefined();
    fireEvent.click(screen.getByRole('button', { name: 'common.cancel' }));

    await waitFor(() => expect(screen.queryByLabelText('settings.agents.fields.id')).toBeNull());
    expect(router.state.location.search).not.toContain('view=');
    expect(router.state.location.search).not.toContain('id=');
  });

  it('closes a dirty routed page after one discard confirmation', async () => {
    const router = renderRoutedAgentSettings();

    fireEvent.click(screen.getByRole('button', { name: 'settings.agents.add' }));
    fireEvent.change(screen.getByLabelText('settings.agents.fields.displayName'), {
      target: { value: 'Unfinished Agent' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'common.cancel' }));
    fireEvent.click(await screen.findByRole('button', {
      name: 'settings.agents.dirtyNavigation.discard',
    }));

    await waitFor(() => expect(screen.queryByLabelText('settings.agents.fields.id')).toBeNull());
    expect(router.state.location.search).not.toContain('view=');
  });

  it('uses read-only Agent IDs in edit and configure pages so they remain focusable', async () => {
    render(<AgentSettingsPage context={context} />);
    openCustomEditor();

    const id = screen.getByLabelText('settings.agents.fields.id') as HTMLInputElement;
    expect(id.readOnly).toBe(true);
    expect(id.disabled).toBe(false);
    id.focus();
    expect(document.activeElement).toBe(id);
  });

  it('clears search and returns to Custom when opening create or Wizard configuration', async () => {
    const { rerender } = render(<AgentSettingsPage context={context} />);
    fireEvent.change(screen.getByLabelText('settings.agents.search.custom'), {
      target: { value: 'hidden' },
    });
    fireEvent.click(screen.getByRole('tab', { name: /settings\.agents\.tabs\.builtin/ }));
    fireEvent.click(screen.getByRole('button', { name: 'settings.agents.add' }));

    expect(screen.queryByRole('tab')).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'common.cancel' }));
    expect(document.querySelector('[role="tab"][data-state="active"]')?.textContent)
      .toContain('settings.agents.tabs.custom');
    expect((screen.getByLabelText('settings.agents.search.custom') as HTMLInputElement).value).toBe('');

    rerender(<AgentSettingsPage context={context} configurationAgentId="wizard-agent" />);
    expect(await screen.findByDisplayValue('wizard-agent')).toBeDefined();
    expect(screen.queryByRole('tab')).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'settings.agents.form.cancel.configure' }));
    await waitFor(() => expect(completeRequest).toHaveBeenCalledWith('wizard-agent', 'cancelled'));
    expect(document.querySelector('[role="tab"][data-state="active"]')?.textContent)
      .toContain('settings.agents.tabs.custom');
  });

  it('derives private Global, Project and Detection paths from a generated Agent ID', () => {
    render(<AgentSettingsPage context={context} />);

    fireEvent.click(screen.getByRole('button', { name: 'settings.agents.add' }));
    fireEvent.change(screen.getByLabelText('settings.agents.fields.displayName'), {
      target: { value: 'Foo Code' },
    });

    expect((screen.getByLabelText('settings.agents.fields.id') as HTMLInputElement).value)
      .toBe('foo-code');
    expect(screen.getAllByDisplayValue('.foo-code/skills')).toHaveLength(2);
    expect(screen.getByDisplayValue('.foo-code')).toBeDefined();
    const globalSection = screen.getByRole('region', { name: 'settings.agents.global.readTitle' });
    const projectSection = screen.getByRole('region', { name: 'settings.agents.project.readTitle' });
    expect(within(globalSection).getByRole('radio', {
      name: 'settings.agents.locations.private',
    }).getAttribute('data-state')).toBe('checked');
    expect(within(projectSection).getByRole('radio', {
      name: 'settings.agents.locations.private',
    }).getAttribute('data-state')).toBe('checked');
  });

  it('keeps the create page editable while typing a display name character by character', async () => {
    const user = userEvent.setup();
    render(<AgentSettingsPage context={context} />);

    await user.click(screen.getByRole('button', { name: 'settings.agents.add' }));
    const nameInput = screen.getByLabelText('settings.agents.fields.displayName') as HTMLInputElement;

    await user.type(nameInput, 'Foo Code');

    expect(nameInput.value).toBe('Foo Code');
    expect(document.activeElement).toBe(nameInput);
    expect((screen.getByLabelText('settings.agents.fields.id') as HTMLInputElement).value)
      .toBe('foo-code');
  });

  it('does not regenerate the Agent ID from an unfinished IME composition', () => {
    render(<AgentSettingsPage context={context} />);
    fireEvent.click(screen.getByRole('button', { name: 'settings.agents.add' }));
    const nameInput = screen.getByLabelText('settings.agents.fields.displayName') as HTMLInputElement;
    const idInput = screen.getByLabelText('settings.agents.fields.id') as HTMLInputElement;

    fireEvent.compositionStart(nameInput);
    fireEvent.change(nameInput, { target: { value: 'pin yin' } });

    expect(nameInput.value).toBe('pin yin');
    expect(idInput.value).toBe('');

    fireEvent.change(nameInput, { target: { value: '拼音' } });
    fireEvent.compositionEnd(nameInput, { data: '拼音' });

    expect(nameInput.value).toBe('拼音');
    expect(idInput.value).toBe('');
  });

  it('does not add Project preview state to the Custom Agent editor', () => {
    render(<AgentSettingsPage context={context} />);
    openCustomEditor();

    expect(screen.queryByRole('combobox', { name: 'settings.agents.projectPreview.label' })).toBeNull();
    const projectSection = screen.getByRole('region', { name: 'settings.agents.project.readTitle' });
    expect((within(projectSection).getByLabelText('settings.agents.directoryKind.private') as HTMLInputElement).value)
      .toBe('.my-agent/skills');
  });

  it('keeps the Agent ID and default paths stable when an existing Agent is renamed', () => {
    render(<AgentSettingsPage context={context} />);
    openCustomEditor();

    fireEvent.change(screen.getByLabelText('settings.agents.fields.displayName'), {
      target: { value: 'Renamed Agent' },
    });

    expect((screen.getByLabelText('settings.agents.fields.id') as HTMLInputElement).value)
      .toBe('my-agent');
    expect(screen.getAllByDisplayValue('.my-agent/skills')).toHaveLength(2);
    expect(screen.getByDisplayValue('.my-agent')).toBeDefined();
  });

  it('uses an auto-fill card grid without an Inspector region', () => {
    render(<AgentSettingsPage context={context} />);

    const list = screen.getByRole('list', { name: 'settings.agents.listLabel' });
    expect(list.className).toContain('auto-fill');
    expect(screen.queryByRole('region', { name: 'settings.agents.inspectorLabel' })).toBeNull();
  });

  it('protects a dirty draft before returning to the Agent list', async () => {
    render(<AgentSettingsPage context={context} />);

    fireEvent.click(screen.getByRole('button', { name: 'settings.agents.add' }));
    fireEvent.change(screen.getByLabelText('settings.agents.fields.displayName'), {
      target: { value: 'Unfinished Agent' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'common.cancel' }));

    await screen.findByText('settings.agents.dirtyNavigation.title');
    expect(screen.getByDisplayValue('Unfinished Agent')).toBeDefined();
  });

  it('edits every declarative detection path without discarding additional entries', () => {
    render(<AgentSettingsPage context={context} />);

    openCustomEditor();

    expect(screen.getAllByRole('textbox', {
      name: /settings\.agents\.detection\.pathInput/,
    })).toHaveLength(2);
    expect(screen.getByDisplayValue('.my-agent')).toBeDefined();
    expect(screen.getByDisplayValue('/opt/my-agent')).toBeDefined();

    fireEvent.click(screen.getByRole('button', { name: 'settings.agents.detection.add' }));
    expect(screen.getAllByRole('textbox', {
      name: /settings\.agents\.detection\.pathInput/,
    })).toHaveLength(3);
  });

  it('previews definition-only deletion before confirming', async () => {
    render(<AgentSettingsPage context={context} />);

    selectCustomMenuAction('settings.agents.delete');

    await screen.findByText('settings.agents.deleteFilesSafe');
    expect(actions.loadDeleteImpact).toHaveBeenCalledWith(context, 'my-agent', 'registry-1');

    fireEvent.change(screen.getByLabelText('settings.agents.deleteConfirmId'), {
      target: { value: 'my-agent' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'settings.agents.confirmDelete' }));
    await waitFor(() => expect(actions.deleteAgent).toHaveBeenCalledWith(context, 'my-agent', 'registry-1'));
  });

  it('opens the Agent deletion shell immediately while loading its impact', () => {
    actions.loadDeleteImpact.mockImplementation(() => new Promise(() => undefined));
    render(<AgentSettingsPage context={context} />);

    selectCustomMenuAction('settings.agents.delete');

    expect(screen.getByText('settings.agents.deleteTitle')).toBeDefined();
    expect(screen.getByRole('status').textContent)
      .toContain('settings.agents.deletePreviewLoading');
  });

  it('keeps a delete preview failure in the dialog and retries it in place', async () => {
    actions.loadDeleteImpact
      .mockRejectedValueOnce(new Error('preview failed'))
      .mockResolvedValueOnce({
        agentId: 'my-agent', displayName: 'My Agent', registryRevision: 'registry-1',
        environmentRevision: 'environment-1', scopes: [], losesManagementCapability: true,
        filesWillBeDeleted: false,
      });
    render(<AgentSettingsPage context={context} />);

    selectCustomMenuAction('settings.agents.delete');

    expect((await screen.findByRole('alert')).textContent)
      .toContain('settings.agents.deletePreviewError');
    fireEvent.click(screen.getByRole('button', { name: 'settings.agents.retryDeletePreview' }));
    expect(await screen.findByText('settings.agents.deleteFilesSafe')).toBeDefined();
    expect(actions.loadDeleteImpact).toHaveBeenCalledTimes(2);
  });

  it('keeps the confirmation after a delete failure and retries without another preview', async () => {
    actions.deleteAgent
      .mockRejectedValueOnce(new Error('delete failed'))
      .mockResolvedValueOnce([]);
    render(<AgentSettingsPage context={context} />);
    selectCustomMenuAction('settings.agents.delete');
    await screen.findByText('settings.agents.deleteFilesSafe');
    const confirmation = screen.getByLabelText('settings.agents.deleteConfirmId') as HTMLInputElement;
    fireEvent.change(confirmation, { target: { value: 'my-agent' } });

    fireEvent.click(screen.getByRole('button', { name: 'settings.agents.confirmDelete' }));

    expect((await screen.findByRole('alert')).textContent)
      .toContain('settings.agents.deleteError');
    expect(confirmation.value).toBe('my-agent');
    fireEvent.click(screen.getByRole('button', { name: 'settings.agents.retryDelete' }));
    await waitFor(() => expect(actions.deleteAgent).toHaveBeenCalledTimes(2));
  });

  it('re-previews stale Agent deletion impact and clears the typed confirmation', async () => {
    actions.loadDeleteImpact
      .mockResolvedValueOnce({
        agentId: 'my-agent', displayName: 'My Agent', registryRevision: 'registry-1',
        environmentRevision: 'environment-1', scopes: [], losesManagementCapability: true,
        filesWillBeDeleted: false,
      })
      .mockResolvedValueOnce({
        agentId: 'my-agent', displayName: 'My Agent', registryRevision: 'registry-2',
        environmentRevision: 'environment-1', scopes: [], losesManagementCapability: true,
        filesWillBeDeleted: false,
      });
    actions.deleteAgent.mockRejectedValueOnce({
      kind: 'staleRegistryRevision', expected: 'registry-1', actual: 'registry-2',
    });
    render(<AgentSettingsPage context={context} />);
    selectCustomMenuAction('settings.agents.delete');
    await screen.findByText('settings.agents.deleteFilesSafe');
    const confirmation = screen.getByLabelText('settings.agents.deleteConfirmId') as HTMLInputElement;
    fireEvent.change(confirmation, { target: { value: 'my-agent' } });

    fireEvent.click(screen.getByRole('button', { name: 'settings.agents.confirmDelete' }));

    await waitFor(() => expect(actions.loadDeleteImpact).toHaveBeenCalledTimes(2));
    expect(actions.loadDeleteImpact).toHaveBeenLastCalledWith(context, 'my-agent', 'registry-2');
    expect(confirmation.value).toBe('');
    expect(screen.getByRole('alert').textContent).toContain('settings.agents.deleteStale');
  });

  it('prefills an Agent request and reports cancellation to the Wizard', async () => {
    const finished = vi.fn();
    render(
      <AgentSettingsPage
        context={context}
        configurationAgentId="new-agent"
        onConfigurationRequestFinished={finished}
      />,
    );

    expect((await screen.findByLabelText('settings.agents.fields.id') as HTMLInputElement).value).toBe('new-agent');
    expect(screen.getByRole('heading', { name: 'settings.agents.form.title.configure' })).toBeDefined();
    expect(screen.queryByRole('list', { name: 'settings.agents.listLabel' })).toBeNull();
    expect(screen.queryByText('settings.agents.form.description.configure')).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'settings.agents.form.cancel.configure' }));

    await waitFor(() => expect(completeRequest).toHaveBeenCalledWith('new-agent', 'cancelled'));
    expect(finished).toHaveBeenCalled();
  });

  it('retries only Wizard completion after the Agent definition was saved', async () => {
    const finished = vi.fn();
    actions.validateDraft.mockResolvedValue({} as never);
    completeRequest
      .mockRejectedValueOnce(new Error('completion unavailable'))
      .mockResolvedValueOnce(undefined);
    render(
      <AgentSettingsPage
        context={context}
        configurationAgentId="new-agent"
        onConfigurationRequestFinished={finished}
      />,
    );

    fireEvent.change(await screen.findByLabelText('settings.agents.fields.displayName'), {
      target: { value: 'New Agent' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'settings.agents.form.action.configure' }));

    await waitFor(() => expect(actions.saveDraft).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(completeRequest).toHaveBeenCalledTimes(1));
    expect(toasts.error).toHaveBeenCalledWith('settings.agents.configurationCompletionError');
    expect(screen.getByText('settings.agents.configurationPending.description')).toBeDefined();
    expect(screen.queryByRole('button', { name: 'settings.agents.form.cancel.configure' })).toBeNull();

    fireEvent.click(screen.getByRole('button', {
      name: 'settings.agents.form.action.completeConfiguration',
    }));
    await waitFor(() => expect(completeRequest).toHaveBeenCalledTimes(2));
    expect(actions.saveDraft).toHaveBeenCalledTimes(1);
    expect(finished).toHaveBeenCalledTimes(1);
  });

  it('discards a dirty requested draft once before returning to the Agent list', async () => {
    const finished = vi.fn();
    const navigate = vi.fn();
    const router = createMemoryRouter([{
      path: '*',
      element: (
        <UnsavedChangesProvider>
          <AgentSettingsPage
            context={context}
            configurationAgentId="new-agent"
            onConfigurationRequestFinished={finished}
            onNavigate={navigate}
          />
        </UnsavedChangesProvider>
      ),
    }]);
    render(<RouterProvider router={router} />);

    await screen.findByLabelText('settings.agents.fields.id');
    fireEvent.change(screen.getByLabelText('settings.agents.fields.displayName'), {
      target: { value: 'Configured Agent' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'settings.agents.form.cancel.configure' }));
    fireEvent.click(await screen.findByRole('button', {
      name: 'settings.agents.dirtyNavigation.discard',
    }));

    await waitFor(() => expect(completeRequest).toHaveBeenCalledTimes(1));
    expect(finished).toHaveBeenCalledTimes(1);
    expect(navigate).toHaveBeenCalledTimes(1);
    expect(navigate).toHaveBeenCalledWith('list');
  });

  it('keeps the requested draft open when cancellation completion fails', async () => {
    const finished = vi.fn();
    completeRequest.mockRejectedValueOnce(new Error('completion unavailable'));
    render(
      <AgentSettingsPage
        context={context}
        configurationAgentId="new-agent"
        onConfigurationRequestFinished={finished}
      />,
    );

    await screen.findByLabelText('settings.agents.fields.id');
    fireEvent.click(screen.getByRole('button', { name: 'settings.agents.form.cancel.configure' }));

    await waitFor(() => expect(completeRequest).toHaveBeenCalledTimes(1));
    expect(screen.getByLabelText('settings.agents.fields.id')).toBeDefined();
    expect(finished).not.toHaveBeenCalled();
    expect(toasts.error).toHaveBeenCalledWith('settings.agents.configurationCompletionError');
  });

  it('keeps a dirty draft and cancels an incoming request when Stay is chosen', async () => {
    const finished = vi.fn();
    const { rerender } = render(
      <AgentSettingsPage context={context} onConfigurationRequestFinished={finished} />,
    );

    fireEvent.click(screen.getByRole('button', { name: 'settings.agents.add' }));
    fireEvent.change(screen.getByLabelText('settings.agents.fields.displayName'), {
      target: { value: 'Unfinished Agent' },
    });

    rerender(
      <AgentSettingsPage
        context={context}
        configurationAgentId="new-agent"
        onConfigurationRequestFinished={finished}
      />,
    );

    await screen.findByText('settings.agents.dirtyRequest.title');
    fireEvent.click(screen.getByRole('button', { name: 'settings.agents.dirtyRequest.stay' }));

    await waitFor(() => expect(completeRequest).toHaveBeenCalledWith('new-agent', 'cancelled'));
    expect(screen.getByDisplayValue('Unfinished Agent')).toBeDefined();
    expect(finished).toHaveBeenCalled();
  });

  it('keeps an incoming request visible when Stay cancellation fails', async () => {
    const finished = vi.fn();
    completeRequest.mockRejectedValueOnce(new Error('completion unavailable'));
    const { rerender } = render(
      <AgentSettingsPage context={context} onConfigurationRequestFinished={finished} />,
    );

    fireEvent.click(screen.getByRole('button', { name: 'settings.agents.add' }));
    fireEvent.change(screen.getByLabelText('settings.agents.fields.displayName'), {
      target: { value: 'Unfinished Agent' },
    });
    rerender(
      <AgentSettingsPage
        context={context}
        configurationAgentId="new-agent"
        onConfigurationRequestFinished={finished}
      />,
    );

    await screen.findByText('settings.agents.dirtyRequest.title');
    fireEvent.click(screen.getByRole('button', { name: 'settings.agents.dirtyRequest.stay' }));

    await waitFor(() => expect(completeRequest).toHaveBeenCalledTimes(1));
    expect(screen.getByText('settings.agents.dirtyRequest.title')).toBeDefined();
    expect(screen.getByDisplayValue('Unfinished Agent')).toBeDefined();
    expect(finished).not.toHaveBeenCalled();
    expect(toasts.error).toHaveBeenCalledWith('settings.agents.configurationCompletionError');
  });

  it('supports an absolute Global private path without offering absolute Project paths', () => {
    render(<AgentSettingsPage context={context} />);
    openCustomEditor();

    expect(screen.getByLabelText('settings.agents.global.directoryLocation')).toBeDefined();
    expect(screen.queryByLabelText('settings.agents.project.directoryLocation')).toBeNull();
  });

  it('shows backend field validation inline and focuses the first invalid field', async () => {
    actions.validateDraft.mockRejectedValue({
      kind: 'invalidDraft',
      errors: [{ field: 'id', code: 'invalidAgentId' }],
    });
    render(<AgentSettingsPage context={context} />);
    fireEvent.click(screen.getByRole('button', { name: 'settings.agents.add' }));
    const idInput = screen.getByLabelText('settings.agents.fields.id');
    fireEvent.change(idInput, { target: { value: 'Invalid ID' } });
    fireEvent.change(screen.getByLabelText('settings.agents.fields.displayName'), {
      target: { value: 'Invalid Agent' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'settings.agents.form.action.create' }));

    expect(await screen.findByText('settings.agents.validation.invalidAgentId')).toBeDefined();
    expect(document.activeElement).toBe(idInput);
  });

  it('keeps background validation errors hidden until the draft changes or save is attempted', async () => {
    actions.validateDraft.mockRejectedValue({
      kind: 'invalidDraft',
      errors: [{ field: 'displayName', code: 'required' }],
    });
    render(<AgentSettingsPage context={context} />);
    fireEvent.click(screen.getByRole('button', { name: 'settings.agents.add' }));

    await waitFor(() => expect(actions.validateDraft).toHaveBeenCalled());
    expect(screen.queryByText('settings.agents.validation.required')).toBeNull();

    fireEvent.submit(screen.getByRole('form', {
      name: 'settings.agents.form.title.create',
    }));
    expect(await screen.findByText('settings.agents.validation.required')).toBeDefined();
  });

  it('keeps validation visible after an edited field returns to its initial value', async () => {
    actions.validateDraft.mockRejectedValue({
      kind: 'invalidDraft',
      errors: [{ field: 'displayName', code: 'required' }],
    });
    render(<AgentSettingsPage context={context} />);
    fireEvent.click(screen.getByRole('button', { name: 'settings.agents.add' }));
    await waitFor(() => expect(actions.validateDraft).toHaveBeenCalled());

    const name = screen.getByLabelText('settings.agents.fields.displayName');
    fireEvent.change(name, { target: { value: 'Temporary' } });
    fireEvent.change(name, { target: { value: '' } });

    expect(await screen.findByText('settings.agents.validation.required')).toBeDefined();
  });

  it('runs Backend validation when required fields are empty', async () => {
    actions.validateDraft.mockRejectedValue({
      kind: 'invalidDraft',
      errors: [{ field: 'displayName', code: 'required' }],
    });
    render(<AgentSettingsPage context={context} />);
    fireEvent.click(screen.getByRole('button', { name: 'settings.agents.add' }));
    await waitFor(() => expect(actions.validateDraft).toHaveBeenCalled());
    actions.validateDraft.mockClear();

    fireEvent.click(screen.getByRole('button', { name: 'settings.agents.form.action.create' }));

    await waitFor(() => expect(actions.validateDraft).toHaveBeenCalled());
  });

  it('focuses the add-path control when detection paths fail collection validation', async () => {
    actions.validateDraft.mockRejectedValue({
      kind: 'invalidDraft',
      errors: [{ field: 'detectionPaths', code: 'required' }],
    });
    render(<AgentSettingsPage context={context} />);
    fireEvent.click(screen.getByRole('button', { name: 'settings.agents.add' }));

    const form = screen.getByRole('form', { name: 'settings.agents.form.title.create' });
    fireEvent.submit(form);

    const addPath = screen.getByRole('button', { name: 'settings.agents.detection.add' });
    await waitFor(() => expect(document.activeElement).toBe(addPath));
  });

  it('does not render resolved validation output inside the editor', async () => {
    actions.validateDraft.mockResolvedValue({
      registryRevision: 'registry-1',
      environmentRevision: 'environment-1',
      environment: { kind: 'host' },
      resolved: {
        definition: snapshot.activeBuiltin[0],
        detection: 'detected',
        detectionReason: null,
        global: {
          enabled: true, readsShared: true, sharedPath: '/home/me/.agents/skills',
          privatePath: '/home/me/.my-agent/skills',
          readPaths: ['/home/me/.agents/skills', '/home/me/.my-agent/skills'],
          sharedPresence: 'present', privatePresence: 'present', legacyPaths: [],
        },
        project: {
          enabled: true, readsShared: false, sharedPath: '/work/.agents/skills',
          privatePath: '/work/.my-agent/skills', readPaths: ['/work/.my-agent/skills'],
          sharedPresence: 'missing', privatePresence: 'present', legacyPaths: [],
        },
      },
    } as never);
    render(<AgentSettingsPage context={context} />);
    openCustomEditor();

    await waitFor(() => expect(actions.validateDraft).toHaveBeenCalled());
    expect(screen.queryByText('/home/me/.my-agent/skills')).toBeNull();
    expect(screen.queryByText('settings.agents.preview.detection.detected')).toBeNull();
  });

  it('keeps definitions readable but disables every write action for read-only storage', () => {
    registryState.snapshot = {
      ...structuredClone(snapshot),
      customStorageIssue: {
        code: 'customAgentStorageUnavailable',
        message: 'raw backend path must not be primary copy',
        readOnly: true,
      },
    };
    render(<AgentSettingsPage context={context} />);

    expect(screen.getByRole('alert').textContent).toContain(
      'settings.agents.storageIssues.customAgentStorageUnavailable',
    );
    expect((screen.getByRole('button', { name: 'settings.agents.add' }) as HTMLButtonElement).disabled).toBe(true);
    selectCustomTab();
    expect((screen.getByRole('button', { name: 'settings.agents.editNamed' }) as HTMLButtonElement).disabled).toBe(true);
  });

  it('disables the empty-state add action when Custom storage is read-only', () => {
    registryState.snapshot = {
      ...structuredClone(snapshot),
      activeCustom: [],
      customStorageIssue: {
        code: 'customAgentStorageUnavailable',
        message: 'storage unavailable',
        readOnly: true,
      },
    };

    render(<AgentSettingsPage context={context} />);

    expect(screen.getAllByRole('button', { name: 'settings.agents.add' })
      .every((button) => (button as HTMLButtonElement).disabled)).toBe(true);
  });

  it('requires the Agent ID after showing delete paths, counts and default references', async () => {
    actions.loadDeleteImpact.mockResolvedValue({
      agentId: 'my-agent', displayName: 'My Agent', registryRevision: 'registry-1',
      environmentRevision: 'environment-1', losesManagementCapability: true,
      filesWillBeDeleted: false,
      scopes: [{
        scope: 'global', defaultReferenced: true,
        paths: [{
          kind: 'private', logicalPath: { kind: 'home', relativePath: '.my-agent/skills' },
          resolvedPath: '/home/me/.my-agent/skills', presence: 'present',
          observedSkillCount: 3, observedSkillCountTruncated: false, unavailableReason: null,
        }],
      }],
    } as never);
    actions.deleteAgent.mockResolvedValue([{ code: 'defaultCleanupFailed' }] as never);
    render(<AgentSettingsPage context={context} />);
    selectCustomMenuAction('settings.agents.delete');

    expect(await screen.findByText('/home/me/.my-agent/skills')).toBeDefined();
    expect(screen.getByText('settings.agents.deleteObservedSkillCount')).toBeDefined();
    expect(screen.getByText('settings.agents.deleteDefaultReferenced')).toBeDefined();
    const confirmButton = screen.getByRole('button', { name: 'settings.agents.confirmDelete' }) as HTMLButtonElement;
    expect(confirmButton.disabled).toBe(true);
    fireEvent.change(screen.getByLabelText('settings.agents.deleteConfirmId'), {
      target: { value: 'my-agent' },
    });
    fireEvent.click(confirmButton);

    await waitFor(() => expect(actions.deleteAgent).toHaveBeenCalled());
    expect(toasts.warning).toHaveBeenCalledWith('settings.agents.warnings.defaultCleanupFailed');
  });

  it('keeps a stale draft for explicit reload and review instead of overwriting', async () => {
    actions.validateDraft.mockResolvedValue({
      registryRevision: 'registry-2',
      environmentRevision: 'environment-1',
      environment: { kind: 'host' },
      resolved: {
        definition: snapshot.activeBuiltin[0], detection: 'notDetected', detectionReason: null,
        global: {
          enabled: true, readsShared: true, sharedPath: '/home/me/.agents/skills', privatePath: null,
          readPaths: ['/home/me/.agents/skills'], sharedPresence: 'present', privatePresence: null, legacyPaths: [],
        },
        project: {
          enabled: true, readsShared: true, sharedPath: '/work/.agents/skills', privatePath: null,
          readPaths: ['/work/.agents/skills'], sharedPresence: 'present', privatePresence: null, legacyPaths: [],
        },
      },
    } as never);
    actions.saveDraft.mockRejectedValue({
      kind: 'staleRegistryRevision', expected: 'registry-1', actual: 'registry-2',
    });
    render(<AgentSettingsPage context={context} />);
    openCustomEditor();
    fireEvent.change(screen.getByLabelText('settings.agents.fields.displayName'), {
      target: { value: 'My Reviewed Agent' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'settings.agents.form.action.edit' }));

    expect(await screen.findByText('settings.agents.stale.title')).toBeDefined();
    expect(screen.getByDisplayValue('My Reviewed Agent')).toBeDefined();
    expect((screen.getByRole('button', {
      name: 'settings.agents.form.action.edit',
    }) as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(screen.getByRole('button', { name: 'settings.agents.stale.reload' }));
    await waitFor(() => expect(actions.loadSettings).toHaveBeenCalledWith(context));
    expect((screen.getByRole('button', {
      name: 'settings.agents.form.action.edit',
    }) as HTMLButtonElement).disabled).toBe(true);
  });

  it('does not clear stale protection when background validation later succeeds', async () => {
    actions.validateDraft.mockResolvedValue({} as never);
    actions.saveDraft.mockRejectedValueOnce({
      kind: 'staleRegistryRevision', expected: 'registry-1', actual: 'registry-2',
    });
    render(<AgentSettingsPage context={context} />);
    openCustomEditor();
    fireEvent.change(screen.getByLabelText('settings.agents.fields.displayName'), {
      target: { value: 'First change' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'settings.agents.form.action.edit' }));
    expect(await screen.findByText('settings.agents.stale.title')).toBeDefined();

    fireEvent.change(screen.getByLabelText('settings.agents.fields.displayName'), {
      target: { value: 'Second change' },
    });
    await new Promise((resolve) => window.setTimeout(resolve, 350));

    expect(screen.getByText('settings.agents.stale.title')).toBeDefined();
    expect((screen.getByRole('button', {
      name: 'settings.agents.form.action.edit',
    }) as HTMLButtonElement).disabled).toBe(true);
  });

  it('explains when the definition was deleted while reloading a stale edit', async () => {
    actions.validateDraft.mockResolvedValue({} as never);
    actions.saveDraft.mockRejectedValueOnce({
      kind: 'staleRegistryRevision', expected: 'registry-1', actual: 'registry-2',
    });
    const { rerender } = render(<AgentSettingsPage context={context} />);
    openCustomEditor();
    fireEvent.change(screen.getByLabelText('settings.agents.fields.displayName'), {
      target: { value: 'Edited name' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'settings.agents.form.action.edit' }));
    fireEvent.click(await screen.findByRole('button', { name: 'settings.agents.stale.reload' }));

    registryState.snapshot = {
      ...structuredClone(snapshot),
      registryRevision: 'registry-2',
      activeCustom: [],
    };
    rerender(<AgentSettingsPage context={context} />);

    expect(await screen.findByText('settings.agents.stale.deletedTitle')).toBeDefined();
    expect(screen.queryByRole('button', { name: 'settings.agents.stale.reload' })).toBeNull();
  });

  it('makes card details keyboard reachable and labels actions with the Agent name', async () => {
    render(<AgentSettingsPage context={context} />);

    expect(screen.getByRole('button', { name: 'settings.agents.editNamed' })).toBeDefined();
    expect(screen.getByRole('button', { name: 'settings.agents.moreActionsNamed' })).toBeDefined();
    expect(screen.getByLabelText('settings.agents.preview.detection.loading').getAttribute('tabindex')).toBe('0');
    expect(screen.getByText('/opt/my-agent').getAttribute('tabindex')).toBe('0');
    expect(screen.getByText('.my-agent/skills').getAttribute('tabindex')).toBe('0');
  });

  it('shows and searches built-in detection fallbacks', () => {
    registryState.snapshot = {
      ...structuredClone(snapshot),
      activeBuiltin: [{
        ...snapshot.activeBuiltin[0],
        detection: {
          kind: 'anyPathExists',
          paths: [{
            kind: 'environmentVariable',
            name: 'CODEX_HOME',
            relativePath: '',
            fallback: {
              kind: 'firstExisting',
              candidates: [{ kind: 'home', relativePath: '.codex' }],
              fallback: { kind: 'absolute', path: '/etc/codex' },
            },
          }],
        },
      }],
    };
    render(<AgentSettingsPage context={context} />);
    fireEvent.mouseDown(screen.getByRole('tab', { name: /settings\.agents\.tabs\.builtin/ }), {
      button: 0,
      ctrlKey: false,
    });

    expect(screen.getByRole('article').textContent).toContain('/etc/codex');
    expect(screen.getByRole('article').textContent).not.toContain('absolute / /etc/codex');
    fireEvent.change(screen.getByLabelText('settings.agents.search.builtin'), {
      target: { value: '/etc/codex' },
    });
    expect(screen.getByRole('article').textContent).toContain('Codex');
  });

  it('shows conflicts and invalid records in Needs attention with recovery actions', async () => {
    registryState.snapshot = {
      ...structuredClone(snapshot),
      disabledConflicts: [{
        definition: snapshot.activeCustom[0].definition,
        builtin: snapshot.activeBuiltin[0],
        raw: { id: 'my-agent' },
      }],
      invalidCustomRecords: [{
        index: 3,
        raw: { id: 'broken-agent' },
        errors: [{ field: 'displayName', code: 'required' }],
      }],
    };
    actions.duplicateDraft.mockResolvedValue({
      ...snapshot.activeCustom[0].definition,
      id: 'my-agent-copy',
    });
    render(<AgentSettingsPage context={context} />);

    const needsAttention = screen.getByText('settings.agents.needsAttentionTitle');
    const customTitle = screen.getByRole('tab', { name: /settings\.agents\.tabs\.custom/ });
    expect(needsAttention.compareDocumentPosition(customTitle) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(screen.getByText('broken-agent')).toBeDefined();
    expect(screen.getByText('settings.agents.validation.required')).toBeDefined();
    fireEvent.click(screen.getAllByLabelText('settings.agents.duplicate')[0]);
    await waitFor(() => expect(actions.duplicateDraft).toHaveBeenCalledWith('my-agent', 'my-agent-copy'));
    expect(screen.getByRole('heading', { name: 'settings.agents.form.title.duplicate' })).toBeDefined();
    expect(screen.queryByRole('list', { name: 'settings.agents.listLabel' })).toBeNull();
  });

  it('reports duplicate failures and keeps delete-preview failures retryable', async () => {
    actions.duplicateDraft.mockRejectedValueOnce(new Error('duplicate failed'));
    actions.loadDeleteImpact.mockRejectedValueOnce(new Error('preview failed'));
    render(<AgentSettingsPage context={context} />);

    selectCustomMenuAction('settings.agents.duplicate');
    await waitFor(() => expect(toasts.error).toHaveBeenCalledWith(
      'settings.agents.duplicateError',
    ));
    expect((screen.getByRole('button', { name: 'settings.agents.moreActionsNamed' }) as HTMLButtonElement).disabled).toBe(false);

    selectCustomMenuAction('settings.agents.delete');
    expect((await screen.findByRole('alert')).textContent)
      .toContain('settings.agents.deletePreviewError');
    expect((screen.getByRole('button', {
      name: 'settings.agents.retryDeletePreview',
    }) as HTMLButtonElement).disabled).toBe(false);
  });

  it('reviews invalid raw data before a confirmed deletion and reports failure', async () => {
    registryState.snapshot = {
      ...structuredClone(snapshot),
      invalidCustomRecords: [{
        index: 3,
        raw: { id: 'broken-agent', extra: true },
        errors: [{ field: 'displayName', code: 'required' }],
      }],
    };
    actions.deleteInvalid.mockRejectedValueOnce(new Error('delete failed'));
    render(<AgentSettingsPage context={context} />);

    fireEvent.click(screen.getByRole('button', { name: 'settings.agents.reviewInvalid' }));
    expect(await screen.findByText(/"extra": true/)).toBeDefined();
    fireEvent.click(screen.getByRole('button', { name: 'settings.agents.confirmInvalidDelete' }));

    await waitFor(() => expect(actions.deleteInvalid).toHaveBeenCalledWith(
      context,
      3,
      'registry-1',
    ));
    expect(toasts.error).toHaveBeenCalledWith('settings.agents.invalidDeleteError');
    expect(screen.getByText(/"extra": true/)).toBeDefined();
  });
});
