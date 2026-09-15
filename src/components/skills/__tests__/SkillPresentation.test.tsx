/* @vitest-environment jsdom */
import '@/test-utils';
import { render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { TooltipProvider } from '@/components/ui/tooltip';
import { SkillCard } from '../SkillCard';
import { SkillDetailPanel } from '../SkillDetailPanel';
import { LibrarySkillCard } from '@/components/library/LibrarySkillCard';
import { LibrarySkillDetailPanel } from '@/components/library/LibrarySkillDetailPanel';
import type { LibrarySkillSummary, SkillUpdateInfo } from '@/bindings';
import type { SkillListItem } from '@/stores/skills-utils';

vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (key: string) => key, i18n: { language: 'en' } }) }));

const installed: SkillListItem = { name: 'toolkit', description: 'Toolkit', path: '/work/.agents/skills/toolkit', canonicalPath: '/work/.agents/skills/toolkit',
  scope: 'project', agents: [], associatedAgents: [], source: 'owner/repo', sourceUrl: 'https://github.com/owner/repo', canRunUpdate: true, canCheckForUpdates: true };
const library: LibrarySkillSummary = { name: 'toolkit', description: 'Toolkit', source: 'owner/repo', sourceType: 'git', sourceUrl: 'https://github.com/owner/repo',
  skillPath: 'skills/toolkit', contentHash: 'hash', updatedAt: null, pluginName: null, refName: null };
const check: SkillUpdateInfo = { name: 'toolkit', source: 'owner/repo', hasUpdate: true, status: 'updateAvailable', reason: null,
  capability: { canRunUpdate: true, canCheckForUpdates: true, reason: null }, gitRef: null, sourceUrl: null, skillPath: null, freshness: 'backingOff',
  error: { kind: 'gitNetworkError', data: { message: 'connection failed' } } };

function renderView(view: string, local: boolean) {
  const skill: SkillListItem = local ? { ...installed, source: null, sourceUrl: null, updateReason: 'local-source', canCheckForUpdates: false, canRunUpdate: false }
    : { ...installed, hasUpdate: true, updateStatus: 'updateAvailable', updateError: check.error };
  const member = local ? { ...library, source: '/work/toolkit', sourceType: 'local', sourceUrl: null } : library;
  const actions = { onUpdate: vi.fn(), onClose: vi.fn(), onDelete: vi.fn(), onManageAgents: vi.fn(), onCopyToProject: vi.fn(), onRetry: vi.fn() };
  const content = 'Skill content';
  render(<TooltipProvider>{view === 'card' ? <SkillCard skill={skill} displayScope="project" {...actions} />
    : view === 'detail' ? <SkillDetailPanel skill={skill} content={content} loading={false} agentDisplayNames={new Map()} {...actions}
      context={{ environment: { kind: 'wsl', distro_name: 'Ubuntu' }, scope: { scope: 'project', project_id: 'app' } }} />
    : view === 'library-card' ? <LibrarySkillCard skill={member} check={local ? undefined : check} onRemove={vi.fn()} onUpdate={vi.fn()} />
    : <LibrarySkillDetailPanel skill={member} check={local ? undefined : check} content={content} loading={false} onClose={vi.fn()} onRemove={vi.fn()} onUpdate={vi.fn()} />
  }</TooltipProvider>);
}

describe.each(['card', 'detail', 'library-card', 'library-detail'])('%s source and status', (view) => {
  it('shows local content as plain source text without a tooltip or status badge', () => {
    renderView(view, true);
    const label = screen.getByText('skills.updateStatusLabel.localSource');
    expect(label.getAttribute('title')).toBeNull();
    expect(label.closest('[data-slot="tooltip-trigger"]')).toBeNull();
    expect(label.closest('h2, h3, [data-testid="skill-card-title"], [data-testid="library-skill-title"]')).toBeNull();
    expect(screen.queryByText('skills.updateStatus.cannotCheck')).toBeNull();
    expect(screen.queryByText('skills.updateReason.local-source')).toBeNull();
    expect(screen.queryByText(/Ubuntu|skills.installPath.project|context.projects/)).toBeNull();
  });

  it('shows the same update conclusion without repeating a temporary error', () => {
    renderView(view, false);
    expect(screen.getByText('skills.updateStatusLabel.available')).toBeTruthy();
    expect(screen.queryByText('skills.card.updateCheckIncomplete')).toBeNull();
    expect(screen.queryByRole('alert')).toBeNull();
    expect(screen.queryByText('connection failed')).toBeNull();
    expect(screen.queryByRole('button', { name: /更多|More/ })).toBeNull();
  });
});
