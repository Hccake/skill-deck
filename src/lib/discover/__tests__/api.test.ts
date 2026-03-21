import { beforeEach, describe, expect, it, vi } from 'vitest';
import { __resetDiscoverApiState, getDiscoverLeaderboard, getDiscoverSkillDetail, searchDiscoverSkills } from '../api';
import { extractSourceOwner } from '../ranking';

const fetchMock = vi.fn();

vi.mock('@tauri-apps/plugin-http', () => ({
  fetch: (...args: unknown[]) => fetchMock(...args),
}));

function makeResponse(body: string, ok = true, status = 200) {
  return {
    ok,
    status,
    text: async () => body,
    json: async () => JSON.parse(body),
  };
}

describe('discover api adapter', () => {
  beforeEach(() => {
    fetchMock.mockReset();
    __resetDiscoverApiState();
  });

  it('maps search results and sorts by installs descending', async () => {
    fetchMock.mockResolvedValue(
      makeResponse(JSON.stringify({
        skills: [
          { id: 'b', name: 'B', installs: 10, source: 'https://github.com/b/b' },
          { id: 'a', name: 'A', installs: 100, source: 'https://github.com/a/a' },
        ],
      })),
    );

    const result = await searchDiscoverSkills('react');

    expect(result.map((skill) => skill.slug)).toEqual(['a', 'b']);
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it('parses popular leaderboard results and caches them', async () => {
    fetchMock.mockResolvedValue(
      makeResponse(`
        <main>
          <a href="/vercel-labs/skills/find-skills">
            <span>find-skills</span>
            <span>651.3K</span>
          </a>
        </main>
      `),
    );

    const first = await getDiscoverLeaderboard('popular');
    const second = await getDiscoverLeaderboard('popular');

    expect(first).toHaveLength(1);
    expect(first[0].slug).toBe('find-skills');
    expect(first[0].installs).toBe(651300);
    expect(extractSourceOwner(first[0].source)).toBe('vercel-labs');
    expect(second).toEqual(first);
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it('shares in-flight popular leaderboard requests', async () => {
    fetchMock.mockResolvedValue(
      makeResponse(`
        <main>
          <a href="/vercel-labs/skills/find-skills">
            <span>find-skills</span>
            <span>651.3K</span>
          </a>
        </main>
      `),
    );

    const [left, right] = await Promise.all([
      getDiscoverLeaderboard('popular'),
      getDiscoverLeaderboard('popular'),
    ]);

    expect(left).toEqual(right);
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it('ignores plain prose when extracting official owners and filters by structured owner links', async () => {
    fetchMock
      .mockResolvedValueOnce(makeResponse('<main><p>vercel-labs should not count as an official owner</p></main>'))
      .mockResolvedValueOnce(makeResponse(`
        <main>
          <a href="/vercel-labs/skills/find-skills">
            <span>find-skills</span>
            <span>651.3K</span>
          </a>
          <a href="/other/skills/skip-me">
            <span>skip-me</span>
            <span>10</span>
          </a>
        </main>
      `));

    const result = await getDiscoverLeaderboard('official');

    expect(result).toHaveLength(0);
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });

  it('parses official leaderboard by filtering popular results with official owners', async () => {
    fetchMock
      .mockResolvedValueOnce(makeResponse('<main><a href="https://github.com/vercel-labs/skills">vercel-labs/skills</a></main>'))
      .mockResolvedValueOnce(makeResponse(`
        <main>
          <a href="/vercel-labs/skills/find-skills">
            <span>find-skills</span>
            <span>651.3K</span>
          </a>
          <a href="/other/skills/skip-me">
            <span>skip-me</span>
            <span>10</span>
          </a>
        </main>
      `));

    const resultPromise = getDiscoverLeaderboard('official');
    expect(fetchMock).toHaveBeenCalledTimes(2);

    const result = await resultPromise;

    expect(result).toHaveLength(1);
    expect(result[0].slug).toBe('find-skills');
    expect(extractSourceOwner(result[0].source)).toBe('vercel-labs');
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });

  it('parses detail fields and falls back to the repository owner and repo', async () => {
    fetchMock.mockResolvedValue(
      makeResponse(`
        <main>
          <h1>find-skills</h1>
          <p>Discover skills quickly</p>
          <pre><code>npx skills add vercel-labs/agent-skills/find-skills</code></pre>
          <span>stars 1234</span>
          <span>weekly installs 12.3K</span>
          <span>risk low</span>
          <li>Fast search</li>
          <li>Helpful details</li>
        </main>
      `),
    );

    const detail = await getDiscoverSkillDetail('/vercel-labs/agent-skills/find-skills');

    expect(detail.summary).toBe('Discover skills quickly');
    expect(detail.installCommand).toBe('npx skills add vercel-labs/agent-skills/find-skills');
    expect(detail.detailUrl).toBe('https://skills.sh/vercel-labs/agent-skills/find-skills');
    expect(detail.repoUrl).toBe('https://github.com/vercel-labs/agent-skills');
    expect(detail.source).toBe('https://github.com/vercel-labs/agent-skills');
    expect(detail.stars).toBe(1234);
    expect(detail.weeklyInstalls).toBe(12300);
    expect(detail.auditRisk).toBe('low');
    expect(detail.highlights).toEqual(['Fast search', 'Helpful details']);
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it('caches skill detail requests', async () => {
    fetchMock.mockResolvedValue(
      makeResponse(`
        <main>
          <h1>find-skills</h1>
          <p>Discover skills quickly</p>
          <pre><code>npx skills add vercel-labs/skills/find-skills</code></pre>
          <a href="https://github.com/vercel-labs/skills">repo</a>
          <span>stars 1234</span>
          <span>weekly installs 12.3K</span>
          <span>risk low</span>
          <li>Fast search</li>
          <li>Helpful details</li>
        </main>
      `),
    );

    const first = await getDiscoverSkillDetail('/vercel-labs/skills/find-skills');
    const second = await getDiscoverSkillDetail('/vercel-labs/skills/find-skills');

    expect(first.summary).toBe('Discover skills quickly');
    expect(first.installCommand).toBe('npx skills add vercel-labs/skills/find-skills');
    expect(first.repoUrl).toBe('https://github.com/vercel-labs/skills');
    expect(first.stars).toBe(1234);
    expect(first.weeklyInstalls).toBe(12300);
    expect(first.auditRisk).toBe('low');
    expect(first.highlights).toEqual(['Fast search', 'Helpful details']);
    expect(second).toEqual(first);
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });
});

