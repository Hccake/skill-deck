/* @vitest-environment jsdom */

import '@/test-utils';
import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { TooltipProvider } from '@/components/ui/tooltip';
import { LibrarySkillDetailPanel } from '../LibrarySkillDetailPanel';
import type { LibrarySkillSummary } from '@/bindings';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, values?: Record<string, unknown>) => (
      values ? `${key}:${JSON.stringify(values)}` : key
    ),
    i18n: { language: 'en-US' },
  }),
}));

const sampleSkill: LibrarySkillSummary = {
  name: 'backend-utils',
  description: 'Shared backend utility functions',
  source: 'https://github.com/example/repo',
  sourceType: 'git',
  sourceUrl: 'https://github.com/example/repo',
  skillPath: 'skills/backend-utils',
  pluginName: null,
  refName: null,
  contentHash: 'hash-123',
  updatedAt: null,
};

describe('LibrarySkillDetailPanel', () => {
  const renderPanel = (skill: LibrarySkillSummary, onRetry = vi.fn()) => {
    render(
      <TooltipProvider>
        <LibrarySkillDetailPanel
          skill={skill}
          content={null}
          loading={false}
          contentError
          onClose={vi.fn()}
          onRetry={onRetry}
        />
      </TooltipProvider>
    );
    return onRetry;
  };

  it('renders error state and handles retry', () => {
    const onRetry = renderPanel(sampleSkill);

    expect(screen.getByText('libraries.contentError')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'skills.detail.retry' }));
    expect(onRetry).toHaveBeenCalledTimes(1);
  });

  it('shows the same Skill metadata the card shows, plus the full source path', () => {
    renderPanel({
      ...sampleSkill,
      pluginName: 'hccake/skills',
      refName: 'main',
      updatedAt: '2020-03-04T05:06:07.000Z',
    });

    // 详情是卡片的超集：卡片上有的元数据，详情不能反而缺。
    expect(screen.getByText('libraries.pluginName')).toBeTruthy();
    expect(screen.getByText('hccake/skills')).toBeTruthy();
    expect(screen.getByText('libraries.refName')).toBeTruthy();
    expect(screen.getByText('main')).toBeTruthy();
    expect(screen.getByText('skills.detail.updated')).toBeTruthy();
  });

  it('does not expose the internal library path', () => {
    renderPanel(sampleSkill);

    // 库内目录结构是实现细节，卡片和详情都不展示。
    expect(screen.queryByText('skills/backend-utils')).toBeNull();
    expect(screen.queryByText('skills.detail.installPath')).toBeNull();
  });
});
