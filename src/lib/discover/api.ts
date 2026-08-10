import { formatInstalls } from './format';
import { extractSourceOwner, parseMetric } from './ranking';
import {
  getDiscoverLeaderboardTransport,
  getDiscoverSkillDetailTransport,
  searchDiscoverSkillsTransport,
} from '@/hooks/useTauriApi';
import type {
  DiscoverAuditRisk,
  DiscoverInstalledOn,
  DiscoverSecurityAudit,
  DiscoverSkillSummary,
  DiscoverTab,
} from './types';

export interface DiscoverSkillDetail extends DiscoverSkillSummary {
  description?: string;
  summaryHtml?: string;
  installCommand?: string;
  repoUrl?: string;
  highlights: string[];
  firstSeen?: string;
  securityAudits: DiscoverSecurityAudit[];
  installedOn: DiscoverInstalledOn[];
  contentHtml?: string;
}

type SearchApiResponse = {
  skills: Array<{
    id: string;
    skillId?: string;
    name: string;
    installs: number;
    source: string;
    summary?: string;
  }>;
};

const DISCOVERY_SITE_BASE = 'https://www.skills.sh';
const ALLOWED_RICH_TEXT_TAGS = new Set([
  'a',
  'blockquote',
  'br',
  'code',
  'em',
  'h1',
  'h2',
  'h3',
  'h4',
  'h5',
  'h6',
  'hr',
  'img',
  'li',
  'ol',
  'p',
  'pre',
  'strong',
  'table',
  'tbody',
  'td',
  'th',
  'thead',
  'tr',
  'ul',
]);

const inflightRequests = new Map<string, Promise<unknown>>();
const leaderboardCache = new Map<string, DiscoverSkillSummary[]>();
const detailCache = new Map<string, DiscoverSkillDetail>();

export function __resetDiscoverApiState(): void {
  inflightRequests.clear();
  leaderboardCache.clear();
  detailCache.clear();
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

function normalizeWhitespace(input?: string | null): string {
  return input?.replace(/\s+/g, ' ').trim() ?? '';
}

function extractPathParts(reference: string): string[] {
  const trimmed = reference.trim().replace(/\.git$/, '');
  if (!trimmed) return [];

  let normalized = trimmed;

  if (/^git@[^:]+:/i.test(normalized)) {
    const colonIndex = normalized.indexOf(':');
    normalized = colonIndex >= 0 ? normalized.slice(colonIndex + 1) : normalized;
  } else if (/^[a-z][a-z0-9+.-]*:\/\//i.test(normalized)) {
    try {
      normalized = new URL(normalized).pathname;
    } catch {
      return [];
    }
  }

  return normalized.replace(/^\/+/, '').split('/').filter(Boolean);
}

function toAbsoluteUrl(pathOrUrl: string): string {
  if (/^[a-z][a-z0-9+.-]*:\/\//i.test(pathOrUrl)) return pathOrUrl;
  return `${DISCOVERY_SITE_BASE}${pathOrUrl.startsWith('/') ? pathOrUrl : `/${pathOrUrl}`}`;
}

function formatDisplayMetricValue(count: number): string {
  const formatted = formatInstalls(count);
  return formatted.endsWith('k') ? `${formatted.slice(0, -1)}K` : formatted;
}

function parseStructuredGithubReference(input: string): { owner: string; repo: string } | null {
  const trimmed = input.trim().replace(/\.git$/, '');
  if (!trimmed) return null;

  if (/^[a-z][a-z0-9+.-]*:\/\//i.test(trimmed)) {
    try {
      const parsed = new URL(trimmed);
      if (!parsed.hostname.endsWith('github.com')) return null;
    } catch {
      return null;
    }
  }

  const parts = extractPathParts(trimmed);
  if (parts.length < 2) return null;
  return { owner: parts[0], repo: parts[1] };
}

function normalizeSource(reference: string, fallbackPath?: string): string {
  const githubReference = parseStructuredGithubReference(reference);
  if (githubReference) {
    return `${githubReference.owner}/${githubReference.repo}`;
  }

  const referenceParts = extractPathParts(reference);
  if (referenceParts[0] === 'site' && referenceParts.length >= 2) {
    return referenceParts[1];
  }

  if (referenceParts.length >= 2) {
    return `${referenceParts[0]}/${referenceParts[1]}`;
  }

  const fallbackParts = fallbackPath ? extractPathParts(fallbackPath) : [];
  if (fallbackParts[0] === 'site' && fallbackParts.length >= 2) {
    return fallbackParts[1];
  }

  if (fallbackParts.length >= 2) {
    return `${fallbackParts[0]}/${fallbackParts[1]}`;
  }

  return reference.trim().replace(/^\/+/, '');
}

function buildGithubUrl(reference: string): string | null {
  const normalizedSource = normalizeSource(reference);
  const parsed = parseStructuredGithubReference(normalizedSource);
  if (!parsed) return null;
  return `https://github.com/${parsed.owner}/${parsed.repo}`;
}

function parseOfficialCreatorSlug(href: string): string | null {
  const trimmed = href.trim();
  if (!/^\/[^/]+$/.test(trimmed)) return null;
  return trimmed.slice(1);
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

function parseAuditStatus(text: string): DiscoverSecurityAudit['status'] {
  const normalized = text.toLowerCase();
  if (normalized.includes('pass')) return 'pass';
  if (normalized.includes('warn')) return 'warn';
  if (normalized.includes('fail')) return 'fail';
  return 'unknown';
}

function stripAuditStatusLabel(text: string): string {
  return text.replace(/\b(pass|warn|fail|unknown)\b$/i, '').trim();
}

function getLeaderboardMetricKind(tab: DiscoverTab): 'installs' | 'trending-24h' | 'hot' {
  if (tab === 'trending') return 'trending-24h';
  if (tab === 'hot') return 'hot';
  return 'installs';
}

function extractLeaderboardMetricText(text: string, source: string): string {
  const sourceIndex = text.lastIndexOf(source);
  if (sourceIndex >= 0) {
    return text.slice(sourceIndex + source.length).trim();
  }

  const parts = text.split(/\s+/);
  return parts.at(-1) ?? '';
}

function getLeaderboardName(anchor: Element, fallbackSlug: string): string {
  return normalizeWhitespace(anchor.querySelector('h3')?.textContent) || fallbackSlug;
}

function getLeaderboardSource(anchor: Element, hrefParts: string[]): string {
  const renderedSource = normalizeWhitespace(anchor.querySelector('p')?.textContent);
  if (renderedSource) {
    return renderedSource;
  }

  if (hrefParts[0] === 'site' && hrefParts.length >= 2) {
    return hrefParts[1];
  }

  if (hrefParts.length >= 2) {
    return `${hrefParts[0]}/${hrefParts[1]}`;
  }

  return '';
}

function getStructuredLeaderboardMetricText(anchor: Element): string | undefined {
  if (anchor.children.length < 3) {
    return undefined;
  }

  const metricContainer = anchor.lastElementChild;
  if (!metricContainer) {
    return undefined;
  }

  const metricParts = Array.from(metricContainer.children)
    .map((child) => normalizeWhitespace(child.textContent))
    .filter(Boolean);

  if (metricParts.length === 0) {
    return undefined;
  }

  return metricParts.join(' ');
}

function parseLeaderboardHtml(
  html: string,
  tab: DiscoverTab,
  officialCreators: Set<string> = new Set(),
): DiscoverSkillSummary[] {
  const document = createDocument(html);
  const anchors = Array.from(document.querySelectorAll('a[href]'));

  return anchors.flatMap((anchor) => {
    const href = anchor.getAttribute('href') ?? '';
    const parts = extractPathParts(href);
    if (parts.length < 3) return [];

    const slug = parts.at(-1) ?? '';
    const owner = parts[0] ?? '';
    const source = getLeaderboardSource(anchor, parts);
    const metricText = getStructuredLeaderboardMetricText(anchor)
      ?? extractLeaderboardMetricText(normalizeWhitespace(anchor.textContent), source);
    if (!metricText) return [];

    const displayMetric = {
      kind: getLeaderboardMetricKind(tab),
      rawText: metricText,
      sortValue: parseMetric(metricText),
    };

    return [{
      slug,
      name: getLeaderboardName(anchor, slug),
      source,
      summary: undefined,
      installs: tab === 'popular' ? displayMetric.sortValue : undefined,
      displayMetric,
      isOfficial: officialCreators.has(owner),
      detailUrl: toAbsoluteUrl(href),
    } satisfies DiscoverSkillSummary];
  });
}

function parseOfficialOwners(html: string): Set<string> {
  const document = createDocument(html);
  const creators = new Set<string>();

  for (const anchor of Array.from(document.querySelectorAll('a[href]'))) {
    const creatorSlug = parseOfficialCreatorSlug(anchor.getAttribute('href') ?? '');
    if (creatorSlug) {
      creators.add(creatorSlug);
    }
  }

  return creators;
}

function findLabeledBlock(document: Document, label: string): Element | undefined {
  const normalizedLabel = label.toLowerCase();

  return Array.from(document.querySelectorAll('div')).find((element) => {
    const firstChildText = normalizeWhitespace(element.firstElementChild?.textContent).toLowerCase();
    return firstChildText === normalizedLabel;
  });
}

function readLabeledBlockText(document: Document, label: string): string | undefined {
  const block = findLabeledBlock(document, label);
  if (block) {
    const children = Array.from(block.children);
    const valueNode = children[1];
    const text = normalizeWhitespace(valueNode?.textContent);
    if (text) return text;
  }

  const valueNode = findValueElementByLabel(document, label);
  const text = normalizeWhitespace(valueNode?.textContent);
  return text || undefined;
}

function findExactTextElement(document: Document, label: string): Element | undefined {
  const normalizedLabel = label.toLowerCase();

  return Array.from(document.querySelectorAll('div, span, h1, h2, h3, h4, h5, h6, p, a, button, code')).find((element) => {
    const text = normalizeWhitespace(element.textContent).toLowerCase();
    if (text !== normalizedLabel) return false;

    return !Array.from(element.children).some((child) => normalizeWhitespace(child.textContent).toLowerCase() === normalizedLabel);
  });
}

function findNextMeaningfulSibling(element: Element | null | undefined): Element | undefined {
  let sibling = element?.nextElementSibling ?? null;

  while (sibling) {
    if (normalizeWhitespace(sibling.textContent)) {
      return sibling;
    }

    sibling = sibling.nextElementSibling;
  }

  return undefined;
}

function findValueElementByLabel(document: Document, label: string): Element | undefined {
  const labelElement = findExactTextElement(document, label);
  if (!labelElement) return undefined;

  return findNextMeaningfulSibling(labelElement)
    ?? findNextMeaningfulSibling(labelElement.parentElement)
    ?? findNextMeaningfulSibling(labelElement.parentElement?.parentElement);
}

function findRichContentAfterLabel(document: Document, label: string): Element | undefined {
  const labelElement = findExactTextElement(document, label);
  if (!labelElement) return undefined;

  const candidates = [
    findNextMeaningfulSibling(labelElement),
    findNextMeaningfulSibling(labelElement.parentElement),
    findNextMeaningfulSibling(labelElement.parentElement?.parentElement),
  ].filter((candidate): candidate is Element => Boolean(candidate));

  for (const candidate of candidates) {
    if (candidate.matches('.prose, article')) {
      return candidate;
    }

    const prose = candidate.querySelector('.prose, article');
    if (prose) {
      return prose;
    }
  }

  return undefined;
}

function parseAuditRow(entry: Element): DiscoverSecurityAudit | null {
  const link = entry.matches('a[href]') ? entry : entry.querySelector('a[href]');
  if (!link) return null;

  const children = Array.from(entry.children).filter((child) => normalizeWhitespace(child.textContent));
  const statusText = children.length >= 2
    ? normalizeWhitespace(children.at(-1)?.textContent)
    : normalizeWhitespace(link.textContent);
  const nameSource = children.length >= 2
    ? normalizeWhitespace(children[0]?.textContent)
    : normalizeWhitespace(link.textContent);
  const name = stripAuditStatusLabel(nameSource);

  return {
    name: name || normalizeWhitespace(link.textContent),
    status: parseAuditStatus(statusText || link.textContent || ''),
    url: toAbsoluteUrl(link.getAttribute('href') ?? ''),
  };
}

function parseSecurityAudits(document: Document): DiscoverSecurityAudit[] {
  const legacyBlock = findLabeledBlock(document, 'Security Audits');
  if (legacyBlock) {
    const legacyRows = Array.from(legacyBlock.querySelectorAll('a[href]'))
      .map((anchor) => parseAuditRow(anchor))
      .filter((entry): entry is DiscoverSecurityAudit => Boolean(entry));

    if (legacyRows.length > 0) {
      return legacyRows;
    }
  }

  const block = findValueElementByLabel(document, 'Security Audits');
  if (!block) return [];

  return Array.from(block.children)
    .map((entry) => parseAuditRow(entry))
    .filter((entry): entry is DiscoverSecurityAudit => Boolean(entry));
}

function parseInstalledOnEntry(entry: Element): DiscoverInstalledOn | null {
  const children = Array.from(entry.children).filter((child) => normalizeWhitespace(child.textContent));
  if (children.length >= 2) {
    const agent = normalizeWhitespace(children[0]?.textContent);
    const installsText = normalizeWhitespace(children.at(-1)?.textContent);

    if (agent && installsText && agent !== installsText) {
      return {
        agent,
        installsText,
        installs: parseMetric(installsText),
      } satisfies DiscoverInstalledOn;
    }
  }

  const text = normalizeWhitespace(entry.textContent);
  if (!text) return null;

  const match = text.match(/^(.+?)\s+([\d.,]+\s*[kKmM]?)$/);
  if (!match) {
    return {
      agent: text,
      installsText: text,
    } satisfies DiscoverInstalledOn;
  }

  return {
    agent: match[1],
    installsText: match[2],
    installs: parseMetric(match[2]),
  } satisfies DiscoverInstalledOn;
}

function getStructuredSectionRows(container: Element, skipFirstChild = false): Element[] {
  const directChildren = Array.from(container.children).filter((child) => normalizeWhitespace(child.textContent));
  const candidates = skipFirstChild ? directChildren.slice(1) : directChildren;

  if (candidates.length === 1) {
    const nestedRows = Array.from(candidates[0].children).filter((child) => normalizeWhitespace(child.textContent));
    if (nestedRows.length > 0) {
      return nestedRows;
    }
  }

  return candidates;
}

function parseInstalledOn(document: Document): DiscoverInstalledOn[] {
  const legacyBlock = findLabeledBlock(document, 'Installed on');
  if (legacyBlock) {
    const legacyRows = getStructuredSectionRows(legacyBlock, true)
      .map((entry) => parseInstalledOnEntry(entry))
      .filter((entry): entry is DiscoverInstalledOn => Boolean(entry));

    if (legacyRows.length > 0) {
      return legacyRows;
    }
  }

  const block = findValueElementByLabel(document, 'Installed on');
  if (!block) return [];

  return getStructuredSectionRows(block)
    .map((entry) => parseInstalledOnEntry(entry))
    .filter((entry): entry is DiscoverInstalledOn => Boolean(entry));
}

function unwrapElement(element: Element): void {
  const parent = element.parentNode;
  if (!parent) return;

  while (element.firstChild) {
    parent.insertBefore(element.firstChild, element);
  }

  parent.removeChild(element);
}

function sanitizeRichTextHtml(root: Element | null | undefined): string | undefined {
  if (!root) return undefined;

  const clone = root.cloneNode(true) as HTMLElement;

  for (const element of Array.from(clone.querySelectorAll('*'))) {
    const tagName = element.tagName.toLowerCase();
    if (!ALLOWED_RICH_TEXT_TAGS.has(tagName)) {
      unwrapElement(element);
      continue;
    }

    for (const attribute of Array.from(element.attributes)) {
      const name = attribute.name.toLowerCase();
      const value = attribute.value;

      if (name.startsWith('on') || name === 'style' || name === 'class') {
        element.removeAttribute(attribute.name);
        continue;
      }

      if (tagName === 'a' && name === 'href') {
        const href = toAbsoluteUrl(value);
        if (!/^https?:\/\//i.test(href)) {
          element.removeAttribute(attribute.name);
          continue;
        }

        element.setAttribute('href', href);
        element.setAttribute('target', '_blank');
        element.setAttribute('rel', 'noreferrer');
        continue;
      }

      if (tagName === 'img' && name === 'src') {
        const src = toAbsoluteUrl(value);
        if (!/^https?:\/\//i.test(src)) {
          element.removeAttribute(attribute.name);
          continue;
        }

        element.setAttribute('src', src);
        continue;
      }

      const isAllowedAnchorAttribute = tagName === 'a' && ['href', 'title', 'target', 'rel'].includes(name);
      const isAllowedImageAttribute = tagName === 'img' && ['src', 'alt', 'title'].includes(name);

      if (!isAllowedAnchorAttribute && !isAllowedImageAttribute) {
        element.removeAttribute(attribute.name);
      }
    }
  }

  const html = clone.innerHTML.trim();
  return html || undefined;
}

function normalizeInstallCommand(input?: string | null): string | undefined {
  const text = normalizeWhitespace(input);
  if (!text) return undefined;

  return text.replace(/^\$\s*/, '').trim() || undefined;
}

function extractHighlightsFromElement(element: Element | null | undefined): string[] {
  if (!element) return [];

  return Array.from(element.querySelectorAll('li'))
    .map((item) => normalizeWhitespace(item.textContent))
    .filter(Boolean);
}

function parseDetailHtml(html: string, fallback: DiscoverSkillSummary): DiscoverSkillDetail {
  const document = createDocument(html);
  const summaryContent = findRichContentAfterLabel(document, 'Summary');
  const summaryHtml = sanitizeRichTextHtml(summaryContent);
  const summary = normalizeWhitespace(summaryContent?.querySelector('p')?.textContent) || fallback.summary;
  const name = normalizeWhitespace(document.querySelector('h1')?.textContent) || fallback.name;
  const installCommand = normalizeInstallCommand(
    document.querySelector('button[title*="Copy"] code, button code, pre code, code')?.textContent,
  );
  const repositoryBlock = findLabeledBlock(document, 'Repository') ?? findValueElementByLabel(document, 'Repository');
  const repositoryAnchor = repositoryBlock?.matches('a[href]')
    ? repositoryBlock
    : repositoryBlock?.querySelector('a[href]')
    ?? Array.from(document.querySelectorAll('a[href]')).find((anchor) => (anchor.getAttribute('href') ?? '').includes('github.com'));
  const repoUrl = repositoryAnchor?.getAttribute('href') ?? buildGithubUrl(fallback.source) ?? undefined;
  const weeklyInstallsText = readLabeledBlockText(document, 'Weekly Installs');
  const starsText = readLabeledBlockText(document, 'GitHub Stars');
  const riskText = readLabeledBlockText(document, 'Risk');
  const legacyHighlights = Array.from(document.querySelectorAll('section[aria-label="summary-highlights"] li'))
    .map((item) => normalizeWhitespace(item.textContent))
    .filter(Boolean);
  const highlights = legacyHighlights.length > 0 ? legacyHighlights : extractHighlightsFromElement(summaryContent);

  return {
    ...fallback,
    name,
    source: normalizeSource(fallback.source, fallback.detailUrl),
    summary,
    description: summary,
    summaryHtml,
    installCommand,
    repoUrl,
    stars: starsText ? parseMetric(starsText) : fallback.stars,
    weeklyInstalls: weeklyInstallsText ? parseMetric(weeklyInstallsText) : fallback.weeklyInstalls,
    auditRisk: riskText ? parseRisk(riskText) : fallback.auditRisk,
    highlights,
    firstSeen: readLabeledBlockText(document, 'First Seen'),
    securityAudits: parseSecurityAudits(document),
    installedOn: parseInstalledOn(document),
    contentHtml: sanitizeRichTextHtml(findRichContentAfterLabel(document, 'SKILL.md') ?? document.querySelector('article')),
  };
}

async function loadLeaderboard(tab: DiscoverTab): Promise<DiscoverSkillSummary[]> {
  const cacheKey = `leaderboard:${tab}`;
  const cached = leaderboardCache.get(cacheKey);
  if (cached) return cached;

  return dedupeRequest(cacheKey, async () => {
    const payload = await getDiscoverLeaderboardTransport(tab);
    const officialCreators = new Set(payload.officialCreators ?? []);

    const results = parseLeaderboardHtml(payload.leaderboardHtml, tab, officialCreators);
    leaderboardCache.set(cacheKey, results);
    return results;
  });
}

function createFallbackSummary(pathOrSlug: string, detailUrl: string): DiscoverSkillSummary {
  const slug = extractPathParts(pathOrSlug).at(-1) ?? pathOrSlug.replace(/^\/+/, '');

  return {
    slug,
    name: slug,
    source: normalizeSource(pathOrSlug, detailUrl),
    summary: undefined,
    installs: undefined,
    displayMetric: {
      kind: 'installs',
      rawText: '0',
      sortValue: 0,
    },
    isOfficial: false,
    detailUrl,
  };
}

export async function searchDiscoverSkills(query: string): Promise<DiscoverSkillSummary[]> {
  const payload = await searchDiscoverSkillsTransport(query);
  const data = JSON.parse(payload.searchJson) as SearchApiResponse;
  const officialCreators = new Set(payload.officialCreators ?? []);

  return data.skills.map((skill) => {
    const detailPath = skill.id.startsWith('/') ? skill.id : `/${skill.id}`;
    const source = normalizeSource(skill.source, detailPath);
    const installs = skill.installs;

    return {
      slug: skill.skillId ?? extractPathParts(skill.id).at(-1) ?? skill.name,
      name: skill.name,
      source,
      summary: skill.summary,
      installs,
      displayMetric: {
        kind: 'installs',
        rawText: formatDisplayMetricValue(installs),
        sortValue: installs,
      },
      isOfficial: officialCreators.has(extractSourceOwner(source)),
      detailUrl: toAbsoluteUrl(detailPath),
    } satisfies DiscoverSkillSummary;
  });
}

export async function getDiscoverLeaderboard(tab: DiscoverTab): Promise<DiscoverSkillSummary[]> {
  return loadLeaderboard(tab);
}

export async function getDiscoverSkillDetail(pathOrSlug: string): Promise<DiscoverSkillDetail> {
  const detailUrl = toAbsoluteUrl(pathOrSlug);
  const cached = detailCache.get(detailUrl);
  if (cached) return cached;

  return dedupeRequest(`detail:${detailUrl}`, async () => {
    const parts = extractPathParts(pathOrSlug);
    const siteDetail = parts[0] === 'site' && parts.length >= 3;
    const source = siteDetail ? parts[1] : parts.slice(0, 2).join('/');
    const skill = parts.at(-1) ?? '';
    if (!source || !skill || (!siteDetail && parts.length < 3)) {
      throw new Error('Invalid Discover detail reference');
    }
    const html = await getDiscoverSkillDetailTransport(source, skill);
    const detail = parseDetailHtml(html, createFallbackSummary(pathOrSlug, detailUrl));
    detail.repoUrl = detail.repoUrl ?? buildGithubUrl(detail.source) ?? undefined;
    detailCache.set(detailUrl, detail);
    return detail;
  });
}

export {
  parseDetailHtml,
  parseOfficialOwners,
  parseLeaderboardHtml,
};
