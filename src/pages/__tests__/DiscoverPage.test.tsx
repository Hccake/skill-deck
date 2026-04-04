/* @vitest-environment jsdom */

import '@/test-utils';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import { DiscoverPage } from '../DiscoverPage';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

const mocks = vi.hoisted(() => ({
  skillsState: {
    globalSkills: [] as Array<{ name: string; source?: string | null }>,
    projectSkills: [] as Array<{ name: string; source?: string | null }>,
    openAddWithPrefill: vi.fn(),
  },
  resizable: {
    groups: [] as Array<Record<string, unknown>>,
    panels: [] as Array<Record<string, unknown>>,
  },
}));

vi.mock('@/stores/skills', () => ({
  useSkillsStore: (selector: (state: typeof mocks.skillsState) => unknown) => selector(mocks.skillsState),
}));

vi.mock('@/components/skills/discover/DiscoverListPanel', () => ({
  DiscoverListPanel: () => <div>discover-list-panel</div>,
}));

vi.mock('@/components/skills/discover/DiscoverDetailPanel', () => ({
  DiscoverDetailPanel: () => <div>discover-detail-panel</div>,
}));

vi.mock('@/components/ui/resizable', () => ({
  ResizablePanelGroup: ({ children, ...props }: React.PropsWithChildren<Record<string, unknown>>) => {
    mocks.resizable.groups.push(props);
    return <div>{children}</div>;
  },
  ResizablePanel: ({ children, ...props }: React.PropsWithChildren<Record<string, unknown>>) => {
    mocks.resizable.panels.push(props);
    return <div>{children}</div>;
  },
  ResizableHandle: () => <div>handle</div>,
}));

describe('DiscoverPage', () => {
  beforeEach(() => {
    mocks.skillsState.globalSkills = [];
    mocks.skillsState.projectSkills = [];
    mocks.skillsState.openAddWithPrefill.mockReset();
    mocks.resizable.groups.length = 0;
    mocks.resizable.panels.length = 0;
  });

  it('uses explicit percentage sizing for discover panels', () => {
    render(<DiscoverPage />);

    expect(screen.getByText('discover-list-panel')).toBeTruthy();

    expect(mocks.resizable.groups[0]).toMatchObject({
      id: 'discover-page-layout-fixed',
      orientation: 'horizontal',
    });

    expect(mocks.resizable.panels[0]).toMatchObject({
      id: 'discover-list-fixed',
      defaultSize: '30%',
      minSize: '20%',
      maxSize: '50%',
    });

    expect(mocks.resizable.panels[1]).toMatchObject({
      id: 'discover-detail-fixed',
      defaultSize: '70%',
      minSize: '30%',
    });
  });
});