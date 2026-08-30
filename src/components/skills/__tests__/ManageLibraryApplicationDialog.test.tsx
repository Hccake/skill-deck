/* @vitest-environment jsdom */

import '@/test-utils';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { LibraryAgentOptions, LibraryApplicationSummary, SkillLocationRef } from '@/bindings';
import { TooltipProvider } from '@/components/ui/tooltip';
import { makeAgentSelectionSnapshot } from '@/test-utils';
import { ManageLibraryApplicationDialog } from '../ManageLibraryApplicationDialog';
import {
  applyLibraryApplication,
  listSkillLibraries,
  getLibraryAgentOptions,
  previewLibraryApplication,
  retryLibraryApplication,
} from '@/hooks/useTauriApi';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock('@/hooks/useTauriApi', () => ({
  applyLibraryApplication: vi.fn(),
  getLibraryAgentOptions: vi.fn(),
  listSkillLibraries: vi.fn(),
  previewLibraryApplication: vi.fn(),
  retryLibraryApplication: vi.fn(),
}));

const context: SkillLocationRef = {
  environment: { kind: 'native' },
  scope: { scope: 'global' },
};

function renderDialog(
  application: LibraryApplicationSummary,
  onApplied = vi.fn(async () => {}),
  target: { context?: SkillLocationRef; projectName?: string } = {},
) {
  const onOpenChange = vi.fn();
  render(
    <TooltipProvider>
      <ManageLibraryApplicationDialog
        open
        context={target.context ?? context}
        projectName={target.projectName}
        application={application}
        onOpenChange={onOpenChange}
        onApplied={onApplied}
      />
    </TooltipProvider>,
  );
  return { onApplied, onOpenChange };
}

function libraryAgentOptions(
  overrides: Partial<LibraryAgentOptions> = {},
): LibraryAgentOptions {
  return {
    selection: makeAgentSelectionSnapshot(),
    migrations: [],
    unsupportedAgentNames: [],
    ...overrides,
  } as LibraryAgentOptions;
}

describe('ManageLibraryApplicationDialog', () => {
  beforeEach(() => {
    vi.mocked(listSkillLibraries).mockResolvedValue({
      environment: { kind: 'native' },
      libraries: [],
      revision: 'empty',
      usageProjection: [],
    });
    vi.mocked(getLibraryAgentOptions).mockResolvedValue(libraryAgentOptions());
    vi.mocked(retryLibraryApplication).mockResolvedValue({
      application: { orderedLibraries: [], selectedAgentIds: [], pending: false },
      units: [],
    });
    vi.mocked(applyLibraryApplication).mockResolvedValue({
      application: { orderedLibraries: [], selectedAgentIds: [], pending: false },
      units: [],
    });
    vi.mocked(previewLibraryApplication).mockResolvedValue({
      token: {
        generation: 'preview-1',
        registryRevision: 'registry-1',
        environmentRevision: 'environment-1',
        contextRevision: 'context-1',
      },
      current: { orderedLibraryIds: [], selectedAgentIds: [] },
      target: { orderedLibraryIds: [], selectedAgentIds: [] },
      addedSkillNames: [],
      removedSkillNames: [],
      switchedSkillNames: [],
      changedDirectorySkillNames: [],
      overriddenByDirectSkillNames: [],
    });
  });

  it('identifies the Global Skill Library target in the dialog title', async () => {
    renderDialog({ orderedLibraries: [], selectedAgentIds: [], pending: false });

    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'libraries.manageGlobal' })).toBeTruthy();
      expect(listSkillLibraries).toHaveBeenCalled();
      expect(getLibraryAgentOptions).toHaveBeenCalled();
    });
  });

  it('identifies the selected Project in the dialog title', async () => {
    renderDialog(
      { orderedLibraries: [], selectedAgentIds: [], pending: false },
      vi.fn(async () => {}),
      {
        context: {
          environment: { kind: 'native' },
          scope: { scope: 'project', project_id: 'project-a' },
        },
        projectName: 'Team App',
      },
    );

    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'libraries.manageProject' })).toBeTruthy();
      expect(listSkillLibraries).toHaveBeenCalled();
      expect(getLibraryAgentOptions).toHaveBeenCalled();
    });
  });

  it('continues the recorded target instead of opening a new selection while pending', async () => {
    const { onApplied } = renderDialog({ orderedLibraries: [], selectedAgentIds: [], pending: true });

    fireEvent.click(screen.getByRole('button', { name: 'libraries.continue' }));

    await waitFor(() => expect(retryLibraryApplication).toHaveBeenCalledWith(context));
    expect(onApplied).toHaveBeenCalledOnce();
  });

  it('shows empty libraries with their identity while keeping them unavailable', async () => {
    vi.mocked(listSkillLibraries).mockResolvedValue({
      environment: { kind: 'native' },
      libraries: [{ id: 'empty', name: 'Empty', skillCount: 0 }],
      revision: 'empty-library',
      usageProjection: [],
    });
    renderDialog({ orderedLibraries: [], selectedAgentIds: [], pending: false });

    const emptyLibrary = await screen.findByRole('checkbox', { name: 'Empty' });
    expect(emptyLibrary.hasAttribute('disabled')).toBe(true);
    expect(screen.getByText('libraries.skillCount')).toBeTruthy();
    expect(screen.getByTestId('library-icon')).toBeTruthy();
  });

  it('keeps save disabled until the draft changes', async () => {
    vi.mocked(listSkillLibraries).mockResolvedValue({
      environment: { kind: 'native' },
      libraries: [{ id: 'backend', name: 'Backend', skillCount: 1 }],
      revision: 'dirty-state',
      usageProjection: [],
    });
    renderDialog({ orderedLibraries: [], selectedAgentIds: [], pending: false });

    const save = await screen.findByRole('button', { name: 'libraries.save' });
    expect(save.hasAttribute('disabled')).toBe(true);
    fireEvent.click(screen.getByRole('checkbox', { name: 'Backend' }));
    expect(save.hasAttribute('disabled')).toBe(false);
  });

  it('keeps the selection open when execution returns a failed unit', async () => {
    vi.mocked(listSkillLibraries).mockResolvedValue({
      environment: { kind: 'native' },
      libraries: [{ id: 'backend', name: 'Backend', skillCount: 1 }],
      revision: 'failed-apply',
      usageProjection: [],
    });
    vi.mocked(applyLibraryApplication).mockResolvedValue({
      application: { orderedLibraries: [], selectedAgentIds: [], pending: false },
      units: [{ status: 'failed' } as never],
    });
    renderDialog({ orderedLibraries: [], selectedAgentIds: [], pending: false });

    fireEvent.click(await screen.findByRole('checkbox'));
    fireEvent.click(screen.getByRole('button', { name: 'libraries.save' }));

    expect((await screen.findByRole('alert')).textContent).toContain('libraries.saveError');
    expect(previewLibraryApplication).toHaveBeenCalledOnce();
    expect(applyLibraryApplication).toHaveBeenCalledWith(expect.objectContaining({
      expectedToken: expect.objectContaining({ generation: 'preview-1' }),
    }));
    expect(screen.queryByText('libraries.previewSummary')).toBeNull();
    expect(screen.getByRole('button', { name: 'libraries.save' })).toBeTruthy();
  });

  it('identifies a conflicting Agent target and lets the user cancel that association', async () => {
    vi.mocked(listSkillLibraries).mockResolvedValue({
      environment: { kind: 'native' },
      libraries: [{ id: 'backend', name: 'Backend', skillCount: 1 }],
      revision: 'agent-conflict',
      usageProjection: [],
    });
    vi.mocked(getLibraryAgentOptions).mockResolvedValue(libraryAgentOptions({
      selection: makeAgentSelectionSnapshot({
        agents: [{
          kind: 'standard',
          id: 'agent-demo',
          displayName: 'Agent demo',
          detection: 'detected',
          directoryAccess: 'privateOnly',
          installOptionId: 'agent-demo-option',
          groupId: null,
        }],
        installOptions: [{
          id: 'agent-demo-option',
          kind: 'standardDirectory',
          agentIds: ['agent-demo'],
          displayName: 'Agent demo',
          path: '/home/user/.agent-demo/skills',
          groupId: null,
          selectable: true,
          modeConstraint: 'userSelectable',
          disabledReason: null,
        }],
      }),
    }));
    vi.mocked(previewLibraryApplication).mockRejectedValue({
      kind: 'skillPlacementTargetConflict',
      data: {
        skillName: 'demo',
        agentIds: ['agent-demo'],
        targetPath: '/home/user/.agent-demo/skills/demo',
        targetKind: 'file',
      },
    });
    renderDialog({
      orderedLibraries: [{ id: 'backend', name: 'Backend', skillCount: 1 }],
      selectedAgentIds: [],
      pending: false,
    });

    const agent = await screen.findByRole('checkbox', { name: /Agent demo/ });
    fireEvent.click(agent);
    fireEvent.click(screen.getByRole('button', { name: 'libraries.save' }));

    expect((await screen.findByRole('alert')).textContent)
      .toContain('libraries.targetConflictAgent');
    fireEvent.click(screen.getByRole('button', { name: 'libraries.cancelConflictingAgents' }));

    expect(screen.queryByRole('alert')).toBeNull();
    expect(agent.getAttribute('data-state')).toBe('unchecked');
    expect(screen.getByRole('button', { name: 'libraries.save' }).hasAttribute('disabled'))
      .toBe(true);
  });

  it('changes the persisted priority order with keyboard-accessible controls', async () => {
    vi.mocked(listSkillLibraries).mockResolvedValue({
      environment: { kind: 'native' },
      libraries: [
        { id: 'first', name: 'First', skillCount: 1 },
        { id: 'second', name: 'Second', skillCount: 1 },
      ],
      revision: 'ordered',
      usageProjection: [],
    });
    renderDialog({
      orderedLibraries: [
        { id: 'first', name: 'First', skillCount: 1 },
        { id: 'second', name: 'Second', skillCount: 1 },
      ],
      selectedAgentIds: [],
      pending: false,
    });
    await screen.findByText('First');

    fireEvent.click(screen.getAllByRole('button', { name: 'libraries.moveDown' })[0]);
    fireEvent.click(screen.getByRole('button', { name: 'libraries.save' }));

    await waitFor(() => expect(previewLibraryApplication).toHaveBeenCalledWith(expect.objectContaining({
      orderedLibraryIds: ['second', 'first'],
    })));
  });

  it('selects one physical private directory for every Agent that shares it', async () => {
    vi.mocked(listSkillLibraries).mockResolvedValue({
      environment: { kind: 'native' },
      libraries: [{ id: 'backend', name: 'Backend', skillCount: 1 }],
      revision: 'agents',
      usageProjection: [],
    });
    vi.mocked(getLibraryAgentOptions).mockResolvedValue(libraryAgentOptions({
      selection: makeAgentSelectionSnapshot({
        agents: [
          {
            kind: 'standard',
            id: 'claude-code',
            displayName: 'Claude Code',
            detection: 'detected',
            directoryAccess: 'privateOnly',
            installOptionId: 'shared-directory',
            groupId: null,
          },
          {
            kind: 'standard',
            id: 'cursor',
            displayName: 'Cursor',
            detection: 'notDetected',
            directoryAccess: 'privateOnly',
            installOptionId: 'shared-directory',
            groupId: null,
          },
        ],
        installOptions: [{
          id: 'shared-directory',
          kind: 'standardDirectory',
          agentIds: ['claude-code', 'cursor'],
          displayName: 'Shared directory',
          path: '/home/user/.shared/skills',
          groupId: null,
          selectable: true,
          modeConstraint: 'userSelectable',
          disabledReason: null,
        }],
      }),
      unsupportedAgentNames: ['Eve'],
    }));
    renderDialog({
      orderedLibraries: [{ id: 'backend', name: 'Backend', skillCount: 1 }],
      selectedAgentIds: [],
      pending: false,
    });
    await screen.findByRole('checkbox', { name: /Claude Code/ });
    expect(screen.getByText('libraries.copyOnlyUnsupported')).toBeTruthy();

    const sharedAgents = screen.getByRole('checkbox', { name: /Claude Code/ });
    expect(sharedAgents.closest('[data-slot="agent-selection-row"]')).toBeTruthy();
    expect(screen.getByText('agentSelection.detectedCount')).toBeTruthy();

    fireEvent.click(sharedAgents);
    fireEvent.click(screen.getByRole('button', { name: 'libraries.save' }));

    await waitFor(() => expect(previewLibraryApplication).toHaveBeenCalledWith(expect.objectContaining({
      selectedAgentIds: ['claude-code', 'cursor'],
    })));
  });

  it('shows a stable loading state and blocks actions until both selections are ready', () => {
    vi.mocked(listSkillLibraries).mockReturnValue(new Promise(() => {}));
    vi.mocked(getLibraryAgentOptions).mockReturnValue(new Promise(() => {}));

    renderDialog({ orderedLibraries: [], selectedAgentIds: [], pending: false });

    expect(screen.getByRole('status').textContent).toContain('common.loading');
    expect(screen.getByRole('button', { name: 'libraries.save' }).hasAttribute('disabled')).toBe(true);
    expect(screen.getByRole('dialog').getAttribute('aria-busy')).toBe('true');
  });

  it('cancels a changed Library selection without replacing the footer actions', async () => {
    vi.mocked(listSkillLibraries).mockResolvedValue({
      environment: { kind: 'native' },
      libraries: [{ id: 'backend', name: 'Backend', skillCount: 1 }],
      revision: 'discard',
      usageProjection: [],
    });
    const { onOpenChange } = renderDialog({ orderedLibraries: [], selectedAgentIds: [], pending: false });

    fireEvent.click(await screen.findByRole('checkbox', { name: 'Backend' }));
    fireEvent.click(screen.getByRole('button', { name: 'common.cancel' }));

    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(screen.queryByText('libraries.discardConfirm')).toBeNull();
    expect(screen.queryByRole('button', { name: 'libraries.discardChanges' })).toBeNull();
  });

  it('prevents dismissal while saving', async () => {
    vi.mocked(listSkillLibraries).mockResolvedValue({
      environment: { kind: 'native' },
      libraries: [{ id: 'backend', name: 'Backend', skillCount: 1 }],
      revision: 'busy',
      usageProjection: [],
    });
    vi.mocked(previewLibraryApplication).mockReturnValue(new Promise(() => {}));
    renderDialog({ orderedLibraries: [], selectedAgentIds: [], pending: false });

    fireEvent.click(await screen.findByRole('checkbox', { name: 'Backend' }));
    fireEvent.click(screen.getByRole('button', { name: 'libraries.save' }));

    expect(screen.getByRole('button', { name: 'libraries.saving' }).hasAttribute('disabled')).toBe(true);
    expect(screen.getByRole('dialog').getAttribute('aria-busy')).toBe('true');
    expect(screen.queryByRole('button', { name: 'common.close' })).toBeNull();
  });
});
