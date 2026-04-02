/* @vitest-environment jsdom */

import '@/test-utils';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, waitFor } from '@testing-library/react';
import { SkillsPanel } from '../SkillsPanel';

const mocks = vi.hoisted(() => ({
  contextState: {
    selectedContext: 'global',
  },
  skillsState: {
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
    selectSkill: vi.fn<() => Promise<void>>().mockResolvedValue(undefined),
    deselectSkill: vi.fn(),
    selectedSkill: {
      name: 'brainstorming',
      scope: 'global' as const,
    },
    openDelete: vi.fn(),
    openAdd: vi.fn(),
    auditCache: {},
    fetchAuditForSkills: vi.fn<() => Promise<void>>().mockResolvedValue(undefined),
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

vi.mock('@/stores/skills', () => ({
  useSkillsStore: (selector?: (state: typeof mocks.skillsState) => unknown) =>
    selector ? selector(mocks.skillsState) : mocks.skillsState,
}));

vi.mock('../SkillsToolbar', () => ({
  SkillsToolbar: () => <div>skills-toolbar</div>,
}));

vi.mock('../CompactSkillList', () => ({
  CompactSkillList: () => <div>compact-skill-list</div>,
}));

vi.mock('../SkillsSection', () => ({
  SkillsSection: () => <div>skills-section</div>,
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
    mocks.skillsState.globalSkills = [];
    mocks.skillsState.projectSkills = [];
    mocks.skillsState.projectPathExists = true;
    mocks.skillsState.allAgents = [];
    mocks.skillsState.loading = false;
    mocks.skillsState.error = null;
    mocks.skillsState.isSyncing = false;
    mocks.skillsState.checkingUpdateScopes = new Set();
    mocks.skillsState.updatingSkills = new Map();
    mocks.skillsState.syncUpdates.mockClear();
    mocks.skillsState.forceCheckUpdates.mockClear();
    mocks.skillsState.updateAllInSection.mockClear();
    mocks.skillsState.cancelUpdateAll.mockClear();
    mocks.skillsState.fetchSkills.mockClear();
    mocks.skillsState.syncSkills.mockClear();
    mocks.skillsState.updateSkill.mockClear();
    mocks.skillsState.selectSkill.mockClear();
    mocks.skillsState.deselectSkill.mockClear();
    mocks.skillsState.selectedSkill = {
      name: 'brainstorming',
      scope: 'global',
    };
    mocks.skillsState.openDelete.mockClear();
    mocks.skillsState.openAdd.mockClear();
    mocks.skillsState.auditCache = {};
    mocks.skillsState.fetchAuditForSkills.mockClear();
  });

  it('does not clear the selected skill when compact mode mounts', async () => {
    render(<SkillsPanel compact />);

    await waitFor(() => {
      expect(mocks.skillsState.fetchSkills).toHaveBeenCalledTimes(1);
    });

    expect(mocks.skillsState.deselectSkill).not.toHaveBeenCalled();
  });

  it('clears the selected skill when the selected context changes', async () => {
    const { rerender } = render(<SkillsPanel compact />);

    await waitFor(() => {
      expect(mocks.skillsState.fetchSkills).toHaveBeenCalledTimes(1);
    });

    mocks.skillsState.deselectSkill.mockClear();
    mocks.skillsState.fetchSkills.mockClear();
    mocks.contextState.selectedContext = 'D:\\Code\\project-a';

    rerender(<SkillsPanel compact />);

    await waitFor(() => {
      expect(mocks.skillsState.fetchSkills).toHaveBeenCalledTimes(1);
    });

    expect(mocks.skillsState.deselectSkill).toHaveBeenCalledTimes(1);
  });
});
