/* @vitest-environment jsdom */

import '@/test-utils';
import '@/index.css';
import userEvent from '@testing-library/user-event';
import { TooltipProvider } from '@/components/ui/tooltip';
import { openUrl } from '@tauri-apps/plugin-opener';
import { toast } from 'sonner';
import { act, fireEvent, render as renderComponent, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { DiscoverDetailPanel } from '../DiscoverDetailPanel';
import type { DiscoverSkillSummary } from '@/lib/discover/types';
import { useMutationStore } from '@/stores/mutation';

const render = (ui: React.ReactNode) => renderComponent(ui, { wrapper: TooltipProvider });

vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn(async () => {}) }));
vi.mock('sonner', () => ({ toast: { error: vi.fn() } }));

const mocks = vi.hoisted(() => ({
  getDiscoverSkillDetail: vi.fn(),
}));

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

vi.mock('@/lib/discover/api', async () => {
  const actual = await vi.importActual<typeof import('@/lib/discover/api')>('@/lib/discover/api');
  return {
    ...actual,
    getDiscoverSkillDetail: (...args: unknown[]) => mocks.getDiscoverSkillDetail(...args),
  };
});

function makeSkill(overrides: Partial<DiscoverSkillSummary>): DiscoverSkillSummary {
  return {
    slug: 'find-skills',
    name: 'find-skills',
    source: 'vercel-labs/skills',
    displayMetric: {
      kind: 'installs',
      rawText: '787.5K',
      sortValue: 787500,
    },
    installs: 787500,
    isOfficial: true,
    detailUrl: 'https://skills.sh/vercel-labs/skills/find-skills',
    ...overrides,
  };
}

describe('DiscoverDetailPanel', () => {
  it('keeps name, source and stars together while install statistics stay outside the header', async () => {
    const skill = makeSkill({});
    mocks.getDiscoverSkillDetail.mockResolvedValue({ ...skill, repoUrl: 'https://github.com/vercel-labs/skills', stars: 32000, weeklyInstalls: 12500,
      highlights: [], securityAudits: [], installedOn: [{ agent: 'codex', installsText: '8K' }], contentHtml: '<p>Read this Skill</p>',
    });
    render(<DiscoverDetailPanel skill={skill} installLocations={[]} onClose={vi.fn()} onInstall={vi.fn()} />);
    const header = screen.getByRole('group', { name: 'skills.discover.currentSkill' });
    expect(within(header).getByText(skill.source)).toBeTruthy();
    await screen.findByText('Read this Skill');
    expect(within(header).getByRole('heading', { name: skill.name })).toBeTruthy();
    expect(within(header).getByRole('link', { name: skill.source })).toBeTruthy();
    expect(within(header).getByText('skills.discover.starsLabel')).toBeTruthy();
    expect(within(header).queryByText('skills.discover.weeklyInstalls')).toBeNull();
    expect(screen.getByText('skills.discover.weeklyInstalls')).toBeTruthy();
    expect(screen.getByText('codex')).toBeTruthy();
  });

  it('immediately identifies a same-name Skill from another source and clears the previous stars', async () => {
    vi.useFakeTimers();
    const first = makeSkill({ name: 'code-review', source: 'team-a/skills', detailUrl: 'https://skills.sh/team-a/skills/code-review' });
    const second = { ...first, source: 'team-b/skills', detailUrl: 'https://skills.sh/team-b/skills/code-review' };
    let finish!: (value: unknown) => void;
    mocks.getDiscoverSkillDetail.mockResolvedValueOnce({ ...first, stars: 12500, highlights: [], securityAudits: [], installedOn: [] })
      .mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }));
    const view = render(<DiscoverDetailPanel key={first.detailUrl} skill={first} installLocations={[]} onClose={vi.fn()} onInstall={vi.fn()} />);
    await act(() => vi.advanceTimersByTimeAsync(200));
    expect(screen.getByText('skills.discover.starsLabel')).toBeTruthy();
    view.rerender(<DiscoverDetailPanel key={second.detailUrl} skill={second} installLocations={[]} onClose={vi.fn()} onInstall={vi.fn()} />);
    const header = screen.getByRole('group', { name: 'skills.discover.currentSkill' });
    expect(within(header).getByText(second.name)).toBeTruthy();
    expect(within(header).getByText(second.source)).toBeTruthy();
    expect(within(header).queryByText(first.source)).toBeNull();
    expect(within(header).queryByText('skills.discover.starsLabel')).toBeNull();
    await act(async () => { finish({ ...second, stars: 860, highlights: [], securityAudits: [], installedOn: [] }); await vi.advanceTimersByTimeAsync(200); });
    expect(within(header).getByText('860')).toBeTruthy();
  });

  it('handles nested HTML and audit links including modified and middle clicks through the same opener', async () => {
    const target = 'https://example.com/guide?q=中文#details';
    mocks.getDiscoverSkillDetail.mockResolvedValue({ ...makeSkill({}), highlights: [], installedOn: [],
      repoUrl: 'javascript:alert(1)', securityAudits: [{ name: 'Audit', status: 'pass', url: 'https://example.com/audit' }],
      contentHtml: `<p><a href="${target}"><strong>Read guide</strong></a></p><a href="javascript:alert(1)">Invalid</a>`,
    });
    const { container } = render(<DiscoverDetailPanel skill={makeSkill({})} installLocations={['global']} onClose={vi.fn()} onInstall={vi.fn()} />);
    const text = await screen.findByText('Read guide');
    fireEvent.click(text, { ctrlKey: true });
    fireEvent(text, new MouseEvent('auxclick', { bubbles: true, cancelable: true, button: 1 }));
    await waitFor(() => expect(openUrl).toHaveBeenCalledTimes(2));
    expect(openUrl).toHaveBeenLastCalledWith(target);
    fireEvent(text, new MouseEvent('auxclick', { bubbles: true, cancelable: true, button: 2 }));
    expect(openUrl).toHaveBeenCalledTimes(2);
    fireEvent.click(screen.getByRole('link', { name: /Audit/ }));
    expect(openUrl).toHaveBeenLastCalledWith('https://example.com/audit');
    const invalid = container.querySelector('a[href^="javascript:"]')!;
    expect(fireEvent.click(invalid)).toBe(false);
    expect(openUrl).toHaveBeenCalledTimes(3);
    expect(screen.queryByRole('link', { name: 'vercel-labs/skills' })).toBeNull();
  });

  it('keeps reading ahead of supplementary information and exposes long project paths on focus', async () => {
    const user = userEvent.setup();
    const project = '/workspace/very-long-project-directory-for-discover-review';
    mocks.getDiscoverSkillDetail.mockResolvedValue({ ...makeSkill({}), highlights: [], securityAudits: [{ name: 'Audit', status: 'warn', url: 'https://example.com/audit' }],
      installedOn: [{ agent: 'codex', installsText: '1.2K' }], firstSeen: 'Jan 26, 2026', contentHtml: '<p>Reading content</p>',
    });
    render(<DiscoverDetailPanel skill={makeSkill({})} installLocations={[project]} onClose={vi.fn()} onInstall={vi.fn()} />);
    const content = await screen.findByText('Reading content');
    const audit = screen.getByRole('link', { name: /Audit/ });
    expect(content.compareDocumentPosition(audit) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(content.compareDocumentPosition(screen.getByText('codex')) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    const fullPath = screen.getByText(project);
    act(() => fullPath.focus());
    expect((await screen.findByRole('tooltip')).textContent).toContain(project);
    await user.tab();
  });

  it('opens the source through the system browser and reports failure without losing the target', async () => {
    const user = userEvent.setup();
    const skill = makeSkill({});
    mocks.getDiscoverSkillDetail.mockResolvedValue({ ...skill, repoUrl: 'https://github.com/vercel-labs/skills', highlights: [], securityAudits: [], installedOn: [], contentHtml: '<p>Keep reading</p>' });
    const install = vi.fn();
    render(<DiscoverDetailPanel skill={skill} installLocations={[]} onClose={vi.fn()} onInstall={install} />);
    await screen.findByText('Keep reading');
    const link = screen.getByRole('link', { name: /vercel-labs\/skills/ });
    act(() => link.focus());
    vi.mocked(openUrl).mockRejectedValueOnce(new Error('browser unavailable'));
    await user.keyboard('{Enter}');
    expect(openUrl).toHaveBeenCalledWith('https://github.com/vercel-labs/skills');
    expect(toast.error).toHaveBeenCalledWith('skills.detail.linkOpenFailed');
    expect(document.activeElement).toBe(link);
    await user.click(link);
    expect(openUrl).toHaveBeenCalledTimes(2);
    await user.click(screen.getByRole('button', { name: 'skills.discover.install' }));
    expect(install).toHaveBeenCalledWith(skill);
  });

  it('retries the current detail with stable focus and ignores a late response from another skill', async () => {
    vi.useFakeTimers();
    const log = vi.spyOn(console, 'error').mockImplementation(() => {});
    const first = makeSkill({});
    const second = makeSkill({ name: 'second', detailUrl: 'https://skills.sh/other/repo/second' });
    let resolveRetry!: (value: unknown) => void;
    mocks.getDiscoverSkillDetail.mockRejectedValueOnce(new Error('offline'))
      .mockImplementationOnce(() => new Promise(resolve => { resolveRetry = resolve; }))
      .mockResolvedValueOnce({ ...second, highlights: [], securityAudits: [], installedOn: [], contentHtml: '<p>Current content</p>' });
    const view = render(<DiscoverDetailPanel skill={first} installLocations={[]} onClose={vi.fn()} onInstall={vi.fn()} />);
    try {
      await act(() => vi.advanceTimersByTimeAsync(200));
      const retry = screen.getByRole('button', { name: 'skills.discover.retry' });
      expect((screen.getByRole('button', { name: 'skills.discover.install' }) as HTMLButtonElement).disabled).toBe(false);
      act(() => retry.focus());
      fireEvent.click(retry);
      expect(document.activeElement).toBe(screen.getByRole('heading', { name: first.name }));
      expect(mocks.getDiscoverSkillDetail).toHaveBeenLastCalledWith(first.detailUrl);
      view.rerender(<DiscoverDetailPanel skill={second} installLocations={[]} onClose={vi.fn()} onInstall={vi.fn()} />);
      await act(() => vi.advanceTimersByTimeAsync(200));
      await act(async () => { resolveRetry({ ...first, highlights: [], securityAudits: [], installedOn: [], contentHtml: '<p>Stale content</p>' }); });
      expect(screen.getByText('Current content')).toBeTruthy();
      expect(screen.queryByText('Stale content')).toBeNull();
    } finally { log.mockRestore(); }
  });

  beforeEach(() => {
    mocks.getDiscoverSkillDetail.mockReset();
    vi.mocked(openUrl).mockReset().mockResolvedValue();
    vi.mocked(toast.error).mockClear();
    useMutationStore.setState({ activeMutation: null, cancelling: false, loading: false });
  });

  it('disables install but keeps close available during another mutation', () => {
    mocks.getDiscoverSkillDetail.mockReturnValue(new Promise(() => undefined));
    useMutationStore.setState({
      activeMutation: {
        kind: 'update',
        target: { kind: 'skillLocation', environment: { kind: 'native' }, scope: { scope: 'global' } },
        id: 'mutation-1',
        phase: 'preparing',
        progress: null,
        cancelable: true,
      },
    });

    render(
      <DiscoverDetailPanel
        skill={makeSkill({})}
        installLocations={[]}
        onClose={vi.fn()}
        onInstall={vi.fn()}
      />
    );

    expect((screen.getByRole('button', { name: 'skills.discover.install' }) as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByRole('button', { name: 'common.close' }) as HTMLButtonElement).disabled).toBe(false);
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('renders skill content and hides unknown risk badge', async () => {
    mocks.getDiscoverSkillDetail.mockResolvedValue({
      ...makeSkill({}),
      description: 'Discover and install specialized agent skills from the open ecosystem.',
      summaryHtml: '<p><strong>Discover and install specialized agent skills from the open ecosystem.</strong></p><ul><li>Helps identify relevant skills by domain and task</li></ul>',
      highlights: ['Helps identify relevant skills by domain and task'],
      installCommand: 'npx skills add https://github.com/vercel-labs/skills --skill find-skills',
      repoUrl: 'https://github.com/vercel-labs/skills',
      weeklyInstalls: 847500,
      stars: 12800,
      firstSeen: 'Jan 26, 2026',
      securityAudits: [
        { name: 'Socket Pass', status: 'pass', url: 'https://skills.sh/socket' },
        { name: 'Snyk Warn', status: 'warn', url: 'https://skills.sh/snyk' },
      ],
      installedOn: [
        { agent: 'codex', installsText: '787.5K', installs: 787500 },
      ],
      auditRisk: 'unknown',
      contentHtml: '<h2>Usage</h2><p>Rendered article content</p>',
    });

    render(
      <DiscoverDetailPanel
        skill={makeSkill({})}
        installLocations={[]}
        onClose={() => undefined}
        onInstall={() => undefined}
      />
    );

    await waitFor(() => {
      expect(screen.getByText('skills.discover.installViaCli')).toBeTruthy();
    });

    expect(screen.queryByText('skills.discover.riskBadge.unknown')).toBeNull();
    expect(screen.getByText('skills.discover.weeklyInstalls')).toBeTruthy();
    expect(screen.getByText('skills.discover.starsLabel')).toBeTruthy();
    expect(screen.getByText('Helps identify relevant skills by domain and task')).toBeTruthy();
    expect(screen.queryByText('npx skills add https://github.com/vercel-labs/skills --skill find-skills')).toBeNull();

    fireEvent.click(screen.getByText('skills.discover.installViaCli'));

    expect(screen.getByText('npx skills add https://github.com/vercel-labs/skills --skill find-skills')).toBeTruthy();
    expect(screen.getByText('Rendered article content')).toBeTruthy();
    expect(screen.getByText('Usage')).toBeTruthy();
  });

  it('shows skeleton again when switching skills after a detail was loaded', async () => {
    vi.useFakeTimers();

    mocks.getDiscoverSkillDetail
      .mockResolvedValueOnce({
        ...makeSkill({}),
        highlights: [],
        securityAudits: [],
        installedOn: [],
        contentHtml: '<p>Alpha</p>',
      })
      .mockResolvedValueOnce({
        ...makeSkill({
          detailUrl: 'https://skills.sh/vercel-labs/skills/other-skill',
          name: 'other-skill',
          slug: 'other-skill',
        }),
        highlights: [],
        securityAudits: [],
        installedOn: [],
        contentHtml: '<p>Beta</p>',
      });

    const { rerender } = render(
      <DiscoverDetailPanel
        skill={makeSkill({})}
        installLocations={[]}
        onClose={() => undefined}
        onInstall={() => undefined}
      />
    );

    await act(async () => {
      await vi.advanceTimersByTimeAsync(220);
    });

    expect(screen.getByText('Alpha')).toBeTruthy();

    rerender(
      <DiscoverDetailPanel
        skill={makeSkill({
          detailUrl: 'https://skills.sh/vercel-labs/skills/other-skill',
          name: 'other-skill',
          slug: 'other-skill',
        })}
        installLocations={[]}
        onClose={() => undefined}
        onInstall={() => undefined}
      />
    );

    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });

    expect(screen.getByTestId('discover-detail-skeleton')).toBeTruthy();
  });
});
