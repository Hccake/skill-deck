/* @vitest-environment jsdom */

import '@/test-utils';
import { render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { TooltipProvider } from '@/components/ui/tooltip';
import { LibrarySkillCard } from '../LibrarySkillCard';
import type { LibrarySkillSummary, SkillUpdateInfo } from '@/bindings';

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

const check = (status: SkillUpdateInfo['status']): SkillUpdateInfo => ({
  name: 'backend-utils',
  source: 'https://github.com/example/repo',
  hasUpdate: status === 'updateAvailable',
  status,
  capability: { canRunUpdate: true, canCheckForUpdates: true, reason: null },
  reason: null,
  gitRef: null,
  sourceUrl: null,
  skillPath: null,
  freshness: 'fresh',
});

const removeButton = () => screen.getByRole('button', {
  name: 'libraries.removeSkill:{"name":"backend-utils"}',
});
const updateButton = () => screen.getByRole('button', { name: 'libraries.update' });

describe('LibrarySkillCard', () => {
  it('shows the description and source but not the internal library path', () => {
    render(
      <TooltipProvider>
        <LibrarySkillCard skill={sampleSkill} onRemove={vi.fn()} />
      </TooltipProvider>
    );

    expect(screen.getByText('backend-utils')).toBeTruthy();
    expect(screen.getByText('Shared backend utility functions')).toBeTruthy();
    expect(screen.getByText('https://github.com/example/repo')).toBeTruthy();
    // 库内目录结构是实现细节，不进入卡片。
    expect(screen.queryByText('skills/backend-utils')).toBeNull();
  });

  it('renders the commit time when the member has one', () => {
    render(
      <TooltipProvider>
        <LibrarySkillCard
          skill={{ ...sampleSkill, updatedAt: '2020-03-04T05:06:07.000Z' }}
          onRemove={vi.fn()}
        />
      </TooltipProvider>
    );

    // 复用 Skills 页的 skills.updated 文案与 formatSkillCardDate，不输出相对时间。
    expect(screen.getByText(/skills\.updated:.*2020/)).toBeTruthy();
  });

  it('shows Skill metadata that is independent of Agents', () => {
    render(
      <TooltipProvider>
        <LibrarySkillCard
          skill={{ ...sampleSkill, pluginName: 'hccake/skills', refName: 'main' }}
          onRemove={vi.fn()}
        />
      </TooltipProvider>
    );

    expect(screen.getByText('hccake/skills')).toBeTruthy();
    expect(screen.getByText('skills.refBadge:{"ref":"main"}')).toBeTruthy();
  });

  it('explains why removal is blocked while the library is in use', () => {
    render(
      <TooltipProvider>
        <LibrarySkillCard skill={sampleSkill} libraryInUse onRemove={vi.fn()} />
      </TooltipProvider>
    );

    expect(removeButton().getAttribute('aria-disabled')).toBe('true');
  });

  it('offers the update action only when an update is actually available', () => {
    const onUpdate = vi.fn();
    const { rerender } = render(
      <TooltipProvider>
        <LibrarySkillCard skill={sampleSkill} onUpdate={onUpdate} onRemove={vi.fn()} />
      </TooltipProvider>
    );

    // 库页面只有整库检查，没有单成员检查入口，所以检查前不摆一个指向不存在操作的按钮。
    expect(screen.queryByRole('button', { name: 'libraries.update' })).toBeNull();

    rerender(
      <TooltipProvider>
        <LibrarySkillCard
          skill={sampleSkill}
          check={check('upToDate')}
          onUpdate={onUpdate}
          onRemove={vi.fn()}
        />
      </TooltipProvider>
    );
    // 已是最新是默认状态，既不显示标签也不摆按钮。
    expect(screen.queryByRole('button', { name: 'libraries.update' })).toBeNull();
    expect(screen.queryByText('libraries.updateStatus.upToDate')).toBeNull();

    rerender(
      <TooltipProvider>
        <LibrarySkillCard
          skill={sampleSkill}
          check={check('updateAvailable')}
          onUpdate={onUpdate}
          onRemove={vi.fn()}
        />
      </TooltipProvider>
    );
    expect(updateButton()).toBeTruthy();
    expect(screen.getByText('skills.updateStatusLabel.available')).toBeTruthy();
  });

  it('reports source problems on the attention row instead of the title', () => {
    render(
      <TooltipProvider>
        <LibrarySkillCard skill={sampleSkill} check={check('deletedUpstream')} onRemove={vi.fn()} />
      </TooltipProvider>
    );

    expect(screen.getByText('skills.card.sourceMissingUpstream')).toBeTruthy();
  });

  it('shows the batch update phase for members in the current run', () => {
    render(
      <TooltipProvider>
        <LibrarySkillCard skill={sampleSkill} updateStatus="updating" onRemove={vi.fn()} />
      </TooltipProvider>
    );

    expect(screen.getByText('skills.updatePhaseUpdating')).toBeTruthy();
  });
});
