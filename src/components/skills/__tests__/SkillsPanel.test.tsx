/* @vitest-environment jsdom */

import '@/test-utils';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, waitFor } from '@testing-library/react';
import { SkillsPanel } from '../SkillsPanel';
import type { ContextRef } from '@/bindings';

const mocks = vi.hoisted(() => ({
  contextState: {
    selectedContext: 'global',
    selectedContextRef: {
      environment: { kind: 'host' as const },
      scope: { scope: 'global' as const },
    } as ContextRef,
    hasExplicitContext: false,
  },
  skillsDataState: {
    globalSkills: [],
    projectSkills: [],
    projectPathExists: true,
    allAgents: [],
    loading: false,
    error: null as string | null,
    isSyncing: false,
    checkingUpdateScopes: new Set<string>(),
    updatingSkills: new Map<string, 'queued' | 'updating' | 'done' | 'failed'>(),
    syncUpdates: vi.fn<() => Promise<void>>().mockResolvedValue(undefined),
    forceCheckUpdates: vi.fn<() => Promise<void>>().mockResolvedValue(undefined),
    updateAllInSection: vi.fn<() => Promise<void>>().mockResolvedValue(undefined),
    cancelUpdateAll: vi.fn(),
    fetchSkills: vi.fn<() => Promise<void>>().mockResolvedValue(undefined),
    syncSkills: vi.fn<() => Promise<void>>().mockResolvedValue(undefined),
    updateSkill: vi.fn<() => Promise<void>>().mockResolvedValue(undefined),
    auditCache: {},
    fetchAuditForSkills: vi.fn<() => Promise<void>>().mockResolvedValue(undefined),
  },
  skillDetailState: {
    selectSkill: vi.fn<() => Promise<void>>().mockResolvedValue(undefined),
    deselectSkill: vi.fn(),
    selectedSkillRef: {
      name: 'brainstorming',
      scope: 'global' as const,
    },
  },
  skillDialogState: {
    openDelete: vi.fn(),
    openAdd: vi.fn(),
    openRepairSource: vi.fn(),
  },
}));

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

vi.mock('@/stores/context', () => ({
  useContextStore: (selector?: (state: typeof mocks.contextState) => unknown) =>
    selector ? selector(mocks.contextState) : mocks.contextState,
}));

vi.mock('@/stores/skills-data', () => ({
  useSkillsDataStore: (selector?: (state: typeof mocks.skillsDataState) => unknown) =>
    selector ? selector(mocks.skillsDataState) : mocks.skillsDataState,
}));

vi.mock('@/stores/skill-detail', () => ({
  useSkillDetailStore: (selector?: (state: typeof mocks.skillDetailState) => unknown) =>
    selector ? selector(mocks.skillDetailState) : mocks.skillDetailState,
}));

vi.mock('@/stores/skill-dialog', () => ({
  useSkillDialogStore: (selector?: (state: typeof mocks.skillDialogState) => unknown) =>
    selector ? selector(mocks.skillDialogState) : mocks.skillDialogState,
}));

vi.mock('../SkillsToolbar', () => ({
  SkillsToolbar: () => <div>skills-toolbar</div>,
}));

vi.mock('../CompactSkillList', () => ({
  CompactSkillList: () => <div>compact-skill-list</div>,
}));

vi.mock('../SkillsSection', () => ({
  SkillsSection: ({
    skills,
    onRepairSource,
  }: {
    skills: Array<{ name: string; scope: 'global' | 'project' }>;
    onRepairSource?: (skill: { name: string; scope: 'global' | 'project' }) => void;
  }) => (
    <div>
      skills-section
      {skills.map((skill) => (
        <button
          key={`${skill.scope}:${skill.name}`}
          type="button"
          data-testid={`repair:${skill.scope}:${skill.name}`}
          onClick={() => onRepairSource?.(skill)}
        >
          repair
        </button>
      ))}
    </div>
  ),
}));

vi.mock('../DeleteSkillDialog', () => ({
  DeleteSkillDialog: () => <div>delete-skill-dialog</div>,
}));

vi.mock('../EmptyStates', () => ({
  GlobalEmptyState: () => <div>global-empty-state</div>,
  ProjectEmptyState: () => <div>project-empty-state</div>,
}));

describe('SkillsPanel', () => {
  beforeEach(() => {
    mocks.contextState.selectedContext = 'global';
    mocks.contextState.selectedContextRef = {
      environment: { kind: 'host' },
      scope: { scope: 'global' },
    };
    mocks.contextState.hasExplicitContext = false;
    mocks.skillsDataState.globalSkills = [];
    mocks.skillsDataState.projectSkills = [];
    mocks.skillsDataState.projectPathExists = true;
    mocks.skillsDataState.allAgents = [];
    mocks.skillsDataState.loading = false;
    mocks.skillsDataState.error = null;
    mocks.skillsDataState.isSyncing = false;
    mocks.skillsDataState.checkingUpdateScopes = new Set();
    mocks.skillsDataState.updatingSkills = new Map();
    mocks.skillsDataState.syncUpdates.mockClear();
    mocks.skillsDataState.forceCheckUpdates.mockClear();
    mocks.skillsDataState.updateAllInSection.mockClear();
    mocks.skillsDataState.cancelUpdateAll.mockClear();
    mocks.skillsDataState.fetchSkills.mockClear();
    mocks.skillsDataState.syncSkills.mockClear();
    mocks.skillsDataState.updateSkill.mockClear();
    mocks.skillsDataState.auditCache = {};
    mocks.skillsDataState.fetchAuditForSkills.mockClear();
    mocks.skillDetailState.selectSkill.mockClear();
    mocks.skillDetailState.deselectSkill.mockClear();
    mocks.skillDetailState.selectedSkillRef = {
      name: 'brainstorming',
      scope: 'global',
    };
    mocks.skillDialogState.openDelete.mockClear();
    mocks.skillDialogState.openAdd.mockClear();
    mocks.skillDialogState.openRepairSource.mockClear();
  });

  it('does not clear the selected skill when compact mode mounts', async () => {
    render(<SkillsPanel compact />);

    await waitFor(() => {
      expect(mocks.skillsDataState.fetchSkills).toHaveBeenCalledTimes(1);
    });

    expect(mocks.skillDetailState.deselectSkill).not.toHaveBeenCalled();
  });

  it('opens the repair source dialog for repairable skills instead of the install wizard', async () => {
    mocks.skillsDataState.globalSkills = [
      {
        name: 'toolkit',
        description: '',
        path: '/skills/toolkit',
        canonicalPath: '/canonical/toolkit',
        scope: 'global',
        agents: [],
        hasUpdate: false,
        canRunUpdate: false,
        canCheckForUpdates: false,
        updateReason: 'missing-skill-path',
        source: 'owner/repo',
        sourceUrl: 'https://github.com/owner/repo',
      },
    ] as never;

    render(<SkillsPanel compact={false} />);

    await waitFor(() => {
      expect(mocks.skillsDataState.fetchSkills).toHaveBeenCalledTimes(1);
    });

    document.querySelector<HTMLButtonElement>('[data-testid="repair:global:toolkit"]')?.click();

    expect(mocks.skillDialogState.openRepairSource).toHaveBeenCalledWith(
      expect.objectContaining({ name: 'toolkit', scope: 'global' }),
      'global'
    );
    expect(mocks.skillDialogState.openAdd).not.toHaveBeenCalled();
  });

  it('clears the selected skill when the selected context changes', async () => {
    const { rerender } = render(<SkillsPanel compact />);

    await waitFor(() => {
      expect(mocks.skillsDataState.fetchSkills).toHaveBeenCalledTimes(1);
    });

    mocks.skillDetailState.deselectSkill.mockClear();
    mocks.skillsDataState.fetchSkills.mockClear();
    mocks.contextState.selectedContext = 'D:\\Code\\project-a';

    rerender(<SkillsPanel compact />);

    await waitFor(() => {
      expect(mocks.skillsDataState.fetchSkills).toHaveBeenCalledTimes(1);
    });

    expect(mocks.skillDetailState.deselectSkill).toHaveBeenCalledTimes(1);
  });

  it('reloads and clears details when only the explicit environment changes', async () => {
    mocks.contextState.hasExplicitContext = true;
    const { rerender } = render(<SkillsPanel compact />);

    await waitFor(() => {
      expect(mocks.skillsDataState.fetchSkills).toHaveBeenCalledTimes(1);
    });

    mocks.skillDetailState.deselectSkill.mockClear();
    mocks.skillsDataState.fetchSkills.mockClear();
    mocks.contextState.selectedContextRef = {
      environment: { kind: 'wsl', distro_name: 'Ubuntu' },
      scope: { scope: 'global' },
    };

    rerender(<SkillsPanel compact />);

    await waitFor(() => {
      expect(mocks.skillsDataState.fetchSkills).toHaveBeenCalledTimes(1);
    });
    expect(mocks.skillDetailState.deselectSkill).toHaveBeenCalledTimes(1);
  });
});
