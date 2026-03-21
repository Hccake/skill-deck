import { fetch } from '@tauri-apps/plugin-http';
import { extractSourceOwner, parseMetric, sortDiscoverSkills } from './ranking';
import type { DiscoverAuditRisk, DiscoverSkillSummary } from './types';

export interface DiscoverSkillDetail extends DiscoverSkillSummary {
  description?: string;
  installCommand?: string;
  repoUrl?: string;
  highlights: string[];
}

const SEARCH_API_BASE = 'https://skills.sh';
const LEADERBOARD_BASE = 'https://skills.sh';
const OFFICIAL_PATH = '/official';

const inflightRequests = new Map<string, Promise<unknown>>();
const leaderboardCache = new Map<string, DiscoverSkillSummary[]>();
const detailCache = new Map<string, DiscoverSkillDetail>();
const officialOwnersCache = new Map<string, Set<string>>();

export function __resetDiscoverApiState(): void {
  inflightRequests.clear();
  leaderboardCache.clear();
  detailCache.clear();
  officialOwnersCache.clear();
}

function dedupeRequest<T>(key: string, loader: () => Promise<T>): Promise<T> {
  const cached = inflightRequests.get(key) as Promise<T> | undefined;
  if (cached) return cached;

  const promise = loader().finally(() => {
    inflightRequests.delete(key);
  });

  inflightRequests.set(key, promise as Promise<unknown>);
  return promise;
}

function createDocument(html: string): Document {
  return new DOMParser().parseFromString(html, 'text/html');
}

function toAbsoluteUrl(pathOrUrl: string): string {
  if (/^[a-z][a-z0-9+.-]*:\/\//i.test(pathOrUrl)) return pathOrUrl;
  return `${SEARCH_API_BASE}${pathOrUrl.startsWith('/') ? pathOrUrl : `/${pathOrUrl}`}`;
}

function parseStructuredGithubReference(input: string): { owner: string; repo: string } | null {
  const trimmed = input.trim().replace(/\.git$/, '');
  if (!trimmed) return null;

  let normalized = trimmed;
  if (/^git@[^:]+:/i.test(normalized)) {
    const colonIndex = normalized.indexOf(':');
    normalized = colonIndex >= 0 ? normalized.slice(colonIndex + 1) : normalized;
  } else if (/^[a-z][a-z0-9+.-]*:\/\//i.test(normalized)) {
    try {
      const parsed = new URL(normalized);
      if (!parsed.hostname.endsWith('github.com')) return null;
      normalized = parsed.pathname;
    } catch {
      return null;
    }
  }

  const parts = normalized.replace(/^\/+/, '').split('/').filter(Boolean);
  if (parts.length < 2) return null;
  return { owner: parts[0], repo: parts[1] };
}

function buildGithubUrl(reference: string): string | null {
  const parsed = parseStructuredGithubReference(reference);
  if (!parsed) return null;
  return `https://github.com/${parsed.owner}/${parsed.repo}`;
}

function parseRisk(text: string): DiscoverAuditRisk {
  const normalized = text.toLowerCase();
  if (normalized.includes('critical')) return 'critical';
  if (normalized.includes('high')) return 'high';
  if (normalized.includes('medium')) return 'medium';
  if (normalized.includes('low')) return 'low';
  if (normalized.includes('safe')) return 'safe';
  return 'unknown';
}

function parseLeaderboardHtml(html: string): DiscoverSkillSummary[] {
  const document = createDocument(html);
  const anchors = Array.from(document.querySelectorAll('a[href]'));

  return anchors
    .map((anchor) => {
      const href = anchor.getAttribute('href') ?? '';
      if (!href.includes('/skills/')) return null;

      const absoluteUrl = toAbsoluteUrl(href);
      const parts = href.replace(/^\/+/, '').split('/').filter(Boolean);
      const owner = parts[0] ?? extractSourceOwner(href);
      const repo = parts[1] ?? 'skills';
      const slug = parts.at(-1) ?? anchor.textContent?.trim().split(/\s+/)[0] ?? repo;
      const source = `https://github.com/${owner}/${repo}`;
      const text = anchor.textContent?.replace(/\s+/g, ' ').trim() ?? '';
      const installs = parseMetric(text.match(/([\d.,]+\s*[kKmM]?)(?!.*[\d.,]+\s*[kKmM]?)/)?.[1] ?? text.split(/\s+/).at(-1) ?? '0');
      const summary = text && text !== slug ? text.replace(slug, '').trim() : undefined;

      return {
        slug,
        name: slug,
        source,
        summary,
        installs,
        isOfficial: false,
        detailUrl: absoluteUrl,
      } satisfies DiscoverSkillSummary;
    })
    .filter((item): item is DiscoverSkillSummary => Boolean(item));
}

function parseOfficialOwners(html: string): Set<string> {
  const document = createDocument(html);
  const owners = new Set<string>();

  for (const anchor of Array.from(document.querySelectorAll('a[href]'))) {
    const href = anchor.getAttribute('href') ?? '';
    const hrefReference = parseStructuredGithubReference(href);
    if (hrefReference) owners.add(hrefReference.owner);

    const textReference = parseStructuredGithubReference(anchor.textContent ?? '');
    if (textReference) owners.add(textReference.owner);
  }

  return owners;
}

function parseLeaderboardSource(tab: 'popular' | 'trending'): string {
  if (tab === 'trending') return `${LEADERBOARD_BASE}/trending`;
  return LEADERBOARD_BASE;
}

function parseDetailHtml(html: string, fallback: DiscoverSkillSummary): DiscoverSkillDetail {
  const document = createDocument(html);
  const bodyText = (document.body.textContent ?? '').replace(/\s+/g, ' ').trim();
  const summary = document.querySelector('p')?.textContent?.replace(/\s+/g, ' ').trim() || fallback.summary || '';

  const codeBlock = document.querySelector('pre code, code');
  const installCommand = codeBlock?.textContent?.replace(/\s+/g, ' ').trim();

  const repoAnchor = Array.from(document.querySelectorAll('a[href]')).find((anchor) => {
    const href = anchor.getAttribute('href') ?? '';
    return href.includes('github.com');
  });
  const repoUrl = repoAnchor?.getAttribute('href') ?? fallback.repoUrl ?? buildGithubUrl(fallback.source) ?? fallback.source;

  const starsMatch = bodyText.match(/stars?\s*([\d,]+)/i);
  const stars = starsMatch ? Number.parseInt(starsMatch[1].replace(/,/g, ''), 10) : fallback.stars;

  const weeklyMatch = bodyText.match(/weekly\s+installs?\s+([\d.,]+\s*[kKmM]?)/i);
  const weeklyInstalls = weeklyMatch ? parseMetric(weeklyMatch[1]) : fallback.weeklyInstalls;

  const riskMatch = bodyText.match(/risk\s+(safe|low|medium|high|critical)/i);
  const auditRisk = riskMatch ? parseRisk(riskMatch[1]) : fallback.auditRisk;

  const highlights = Array.from(document.querySelectorAll('li'))
    .map((item) => item.textContent?.replace(/\s+/g, ' ').trim())
    .filter((item): item is string => Boolean(item));

  return {
    ...fallback,
    summary,
    description: summary,
    installCommand,
    repoUrl,
    source: buildGithubUrl(fallback.source) ?? fallback.source,
    stars,
    weeklyInstalls,
    auditRisk,
    highlights,
  };
}

async function fetchText(url: string): Promise<string> {
  const response = await fetch(url);
  if (!response.ok) {
    throw new Error(`HTTP ${response.status}`);
  }
  return await response.text();
}

async function fetchJson<T>(url: string): Promise<T> {
  const response = await fetch(url);
  if (!response.ok) {
    throw new Error(`HTTP ${response.status}`);
  }
  return await response.json() as T;
}

async function loadPopularLeaderboard(): Promise<DiscoverSkillSummary[]> {
  const cacheKey = 'leaderboard:popular';
  const cached = leaderboardCache.get(cacheKey);
  if (cached) return cached;

  return dedupeRequest(cacheKey, async () => {
    const html = await fetchText(parseLeaderboardSource('popular'));
    const sorted = sortDiscoverSkills(parseLeaderboardHtml(html), { mode: 'browse', sort: 'installs' });
    leaderboardCache.set(cacheKey, sorted);
    return sorted;
  });
}

async function loadTrendingLeaderboard(): Promise<DiscoverSkillSummary[]> {
  const cacheKey = 'leaderboard:trending';
  const cached = leaderboardCache.get(cacheKey);
  if (cached) return cached;

  return dedupeRequest(cacheKey, async () => {
    const html = await fetchText(parseLeaderboardSource('trending'));
    const sorted = sortDiscoverSkills(parseLeaderboardHtml(html), { mode: 'browse', sort: 'trending' });
    leaderboardCache.set(cacheKey, sorted);
    return sorted;
  });
}

async function loadOfficialOwners(): Promise<Set<string>> {
  const cacheKey = 'official-owners';
  const cached = officialOwnersCache.get(cacheKey);
  if (cached) return cached;

  return dedupeRequest(cacheKey, async () => {
    const html = await fetchText(`${LEADERBOARD_BASE}${OFFICIAL_PATH}`);
    const owners = parseOfficialOwners(html);
    officialOwnersCache.set(cacheKey, owners);
    return owners;
  });
}

export async function searchDiscoverSkills(query: string): Promise<DiscoverSkillSummary[]> {
  const url = `${SEARCH_API_BASE}/api/search?q=${encodeURIComponent(query)}&limit=50`;
  const data = await fetchJson<{
    skills: Array<{
      id: string;
      name: string;
      installs: number;
      source: string;
      summary?: string;
      isOfficial?: boolean;
    }>;
  }>(url);

  const mapped = data.skills.map((skill) => ({
    slug: skill.id,
    name: skill.name,
    source: skill.source || `https://skills.sh/${skill.id}`,
    summary: skill.summary,
    installs: skill.installs,
    isOfficial: skill.isOfficial ?? false,
    detailUrl: `https://skills.sh/${skill.id}`,
  } satisfies DiscoverSkillSummary));

  return sortDiscoverSkills(mapped, { mode: 'browse', sort: 'installs' });
}

export async function getDiscoverLeaderboard(tab: 'popular' | 'trending' | 'official'): Promise<DiscoverSkillSummary[]> {
  if (tab === 'popular') return loadPopularLeaderboard();
  if (tab === 'trending') return loadTrendingLeaderboard();

  const cacheKey = 'leaderboard:official';
  const cached = leaderboardCache.get(cacheKey);
  if (cached) return cached;

  return dedupeRequest(cacheKey, async () => {
    const [officialOwners, popular] = await Promise.all([
      loadOfficialOwners(),
      loadPopularLeaderboard(),
    ]);

    const filtered = popular.filter((skill) => officialOwners.has(extractSourceOwner(skill.source)));
    const sorted = sortDiscoverSkills(filtered, { mode: 'browse', sort: 'installs' });
    leaderboardCache.set(cacheKey, sorted);
    return sorted;
  });
}

export async function getDiscoverSkillDetail(pathOrSlug: string): Promise<DiscoverSkillDetail> {
  const detailUrl = toAbsoluteUrl(pathOrSlug);
  const cached = detailCache.get(detailUrl);
  if (cached) return cached;

  return dedupeRequest(`detail:${detailUrl}`, async () => {
    const html = await fetchText(detailUrl);
    const fallbackReference = buildGithubUrl(pathOrSlug) ?? buildGithubUrl(detailUrl) ?? pathOrSlug;
    const fallbackRepoUrl = buildGithubUrl(pathOrSlug) ?? buildGithubUrl(detailUrl) ?? detailUrl;
    const slug = pathOrSlug.replace(/^\/+/, '').split('/').filter(Boolean).at(-1) ?? pathOrSlug;
    const fallback: DiscoverSkillSummary = {
      slug,
      name: slug,
      source: fallbackReference,
      summary: undefined,
      installs: 0,
      isOfficial: false,
      detailUrl,
    };

    const detail = parseDetailHtml(html, fallback);
    detail.repoUrl = detail.repoUrl ?? fallbackRepoUrl;
    detailCache.set(detailUrl, detail);
    return detail;
  });
}

export {
  parseDetailHtml,
  parseOfficialOwners,
  parseLeaderboardHtml,
};

