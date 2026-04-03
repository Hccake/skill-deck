/* @vitest-environment jsdom */

import '@/test-utils';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, waitFor } from '@testing-library/react';
import { useEffect } from 'react';
import { SkillsPage } from '../SkillsPage';

const mocks = vi.hoisted(() => ({
  contextState: {
    selectedContext: 'global',
    projects: [] as string[],
  },
  skillsState: {
    selectedSkill: null as null | { name: string; scope: 'global' | 'project' },
    skillContent: null as string | null,
    loadingContent: false,
    deselectSkill: vi.fn(),
    reloadContent: vi.fn(),
    updateSkill: vi.fn(),
    openDelete: vi.fn(),
    allAgents: [] as Array<{ id: string; name: string }>,
    openManageAgents: vi.fn(),
    closeManageAgents: vi.fn(),
    saveAgentChanges: vi.fn(),
    manageAgentsSkill: null,
    manageAgentsScope: 'global',
    copySkill: null,
    openCopyToProject: vi.fn(),
    closeCopyToProject: vi.fn(),
    executeCopy: vi.fn(),
  },
  resizable: {
    groups: [] as Array<Record<string, unknown>>,
    panels: [] as Array<Record<string, unknown>>,
    lifecycle: [] as string[],
    setLayout: vi.fn(),
    getLayout: vi.fn(() => ({})),
  },
  skillsPanelLifecycle: [] as string[],
}));

vi.mock('@/stores/context', () => ({
  useContextStore: (selector: (state: typeof mocks.contextState) => unknown) => selector(mocks.contextState),
}));

vi.mock('@/stores/skills', () => ({
  useSkillsStore: (selector: (state: typeof mocks.skillsState) => unknown) => selector(mocks.skillsState),
}));

vi.mock('@/components/skills', () => ({
  ContextSidebar: () => <div>context-sidebar</div>,
  SkillsPanel: ({ compact }: { compact?: boolean }) => {
    useEffect(() => {
      mocks.skillsPanelLifecycle.push('mount');
      return () => {
        mocks.skillsPanelLifecycle.push('unmount');
      };
    }, []);
    return <div data-compact={compact ? 'true' : 'false'}>skills-panel</div>;
  },
  SkillDetailPanel: () => <div>skill-detail-panel</div>,
}));

vi.mock('@/components/ui/resizable', () => ({
  ResizablePanelGroup: ({ children, ...props }: React.PropsWithChildren<Record<string, unknown>>) => {
    mocks.resizable.groups.push(props);
    const groupRef = props.groupRef as { current?: { setLayout: typeof mocks.resizable.setLayout; getLayout: typeof mocks.resizable.getLayout } } | undefined;
    if (groupRef) {
      groupRef.current = {
        setLayout: mocks.resizable.setLayout,
        getLayout: mocks.resizable.getLayout,
      };
    }
    useEffect(() => {
      const id = String(props.id ?? 'group');
      mocks.resizable.lifecycle.push(`mount:${id}`);
      return () => {
        mocks.resizable.lifecycle.push(`unmount:${id}`);
      };
    }, [props.id]);
    return <div>{children}</div>;
  },
  ResizablePanel: ({ children, ...props }: React.PropsWithChildren<Record<string, unknown>>) => {
    mocks.resizable.panels.push(props);
    return <div>{children}</div>;
  },
  ResizableHandle: () => <div>handle</div>,
}));

describe('SkillsPage', () => {
  beforeEach(() => {
    mocks.contextState.selectedContext = 'global';
    mocks.skillsState.selectedSkill = null;
    mocks.skillsState.skillContent = null;
    mocks.skillsState.loadingContent = false;
    mocks.skillsState.deselectSkill.mockReset();
    mocks.skillsState.reloadContent.mockReset();
    mocks.skillsState.updateSkill.mockReset();
    mocks.skillsState.openDelete.mockReset();
    mocks.skillsState.allAgents = [];
    mocks.resizable.groups.length = 0;
    mocks.resizable.panels.length = 0;
    mocks.resizable.lifecycle.length = 0;
    mocks.resizable.setLayout.mockReset();
    mocks.resizable.getLayout.mockReset();
    mocks.resizable.getLayout.mockReturnValue({});
    mocks.skillsPanelLifecycle.length = 0;
  });

  it('uses percentage-based panel sizes when a skill detail is open', () => {
    mocks.skillsState.selectedSkill = {
      name: 'test-skill',
      scope: 'global',
    };

    render(<SkillsPage />);

    expect(mocks.resizable.groups[0]).toMatchObject({
      id: 'skills-page-layout',
      orientation: 'horizontal',
    });
    expect(mocks.resizable.panels[0]).toMatchObject({
      id: 'skills-list-panel',
      defaultSize: '22%',
      minSize: '12%',
      maxSize: '85%',
    });
    expect(mocks.resizable.panels[1]).toMatchObject({
      id: 'skill-detail-panel',
      defaultSize: '78%',
      minSize: '15%',
    });
  });

  it('updates the panel layout without remounting the group when entering split view', () => {
    const { rerender } = render(<SkillsPage />);

    expect(mocks.resizable.lifecycle).toEqual(['mount:skills-page-layout']);
    expect(mocks.skillsPanelLifecycle).toEqual(['mount']);

    mocks.resizable.getLayout.mockReturnValue({
      'skills-list-panel': 22,
      'skill-detail-panel': 78,
    });

    mocks.skillsState.selectedSkill = {
      name: 'test-skill',
      scope: 'global',
    };

    rerender(<SkillsPage />);

    expect(mocks.resizable.lifecycle).toEqual(['mount:skills-page-layout']);
    expect(mocks.skillsPanelLifecycle).toEqual(['mount']);
    expect(mocks.resizable.setLayout).toHaveBeenCalledWith({
      'skills-list-panel': 22,
      'skill-detail-panel': 78,
    });
  });

  it('waits for the target panel count before applying the split layout', async () => {
    const { rerender } = render(<SkillsPage />);

    mocks.resizable.getLayout
      .mockReturnValueOnce({ 'skills-list-panel': 100 })
      .mockReturnValueOnce({
        'skills-list-panel': 22,
        'skill-detail-panel': 78,
      });

    mocks.skillsState.selectedSkill = {
      name: 'test-skill',
      scope: 'global',
    };

    rerender(<SkillsPage />);

    expect(mocks.resizable.setLayout).not.toHaveBeenCalled();

    await waitFor(() => {
      expect(mocks.resizable.setLayout).toHaveBeenCalledWith({
        'skills-list-panel': 22,
        'skill-detail-panel': 78,
      });
    });
  });
});
