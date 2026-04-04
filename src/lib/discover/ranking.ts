import type {
  DiscoverAuditRisk,
  DiscoverFilterOptions,
  DiscoverSkillSummary,
  DiscoverSortOptions,
} from './types';

const UNKNOWN_RISK: DiscoverAuditRisk = 'unknown';

export function parseMetric(input: string): number {
  const trimmed = input.trim();
  if (!trimmed) return 0;

  const suffix = trimmed.at(-1)?.toLowerCase();
  const numericPart = suffix === 'k' || suffix === 'm'
    ? trimmed.slice(0, -1)
    : trimmed;
  const parsed = Number.parseFloat(numericPart.replace(/,/g, ''));
  if (Number.isNaN(parsed)) return 0;

  if (suffix === 'k') return Math.round(parsed * 1_000);
  if (suffix === 'm') return Math.round(parsed * 1_000_000);
  return Math.round(parsed);
}

export function extractSourceOwner(source: string): string {
  const trimmed = source.trim();
  if (!trimmed) return '';

  let normalized = trimmed;
  try {
    if (/^[a-z][a-z0-9+.-]*:\/\//i.test(trimmed)) {
      normalized = new URL(trimmed).pathname;
    } else if (trimmed.startsWith('git@')) {
      const colonIndex = trimmed.indexOf(':');
      normalized = colonIndex >= 0 ? trimmed.slice(colonIndex + 1) : trimmed;
    }
  } catch {
    normalized = trimmed;
  }

  normalized = normalized.replace(/\.git$/, '');
  const parts = normalized.split('/').filter(Boolean);
  return parts[0] ?? '';
}

function getSkillKey(skill: DiscoverSkillSummary): string {
  return `${skill.source}::${skill.name}`;
}

function normalizeRisk(risk?: DiscoverAuditRisk): DiscoverAuditRisk {
  return risk ?? UNKNOWN_RISK;
}

function getInstallSortValue(skill: DiscoverSkillSummary): number {
  return skill.installs ?? skill.displayMetric?.sortValue ?? 0;
}

function getTrendingSortValue(skill: DiscoverSkillSummary): number {
  return skill.displayMetric?.sortValue ?? skill.weeklyInstalls ?? 0;
}

export function filterDiscoverSkills(
  skills: ReadonlyArray<DiscoverSkillSummary>,
  options: DiscoverFilterOptions = {},
): DiscoverSkillSummary[] {
  const installedSkillKeys = options.installedSkillKeys ?? new Set<string>();
  const riskSet = options.risk ? new Set(options.risk) : null;

  return skills.filter((skill) => {
    if (options.officialOnly && !skill.isOfficial) return false;
    if (options.notInstalledOnly && installedSkillKeys.has(getSkillKey(skill))) return false;
    if (riskSet && !riskSet.has(normalizeRisk(skill.auditRisk))) return false;
    return true;
  });
}

export function sortDiscoverSkills(
  skills: ReadonlyArray<DiscoverSkillSummary>,
  options: DiscoverSortOptions,
): DiscoverSkillSummary[] {
  const indexed = skills.map((skill, index) => ({ skill, index }));

  indexed.sort((left, right) => {
    if (options.mode === 'search' && options.sort === 'best-match') {
      const leftHasRelevance = typeof left.skill.relevanceScore === 'number';
      const rightHasRelevance = typeof right.skill.relevanceScore === 'number';

      if (leftHasRelevance !== rightHasRelevance) {
        return leftHasRelevance ? -1 : 1;
      }

      if (leftHasRelevance && rightHasRelevance) {
        if (left.skill.relevanceScore !== right.skill.relevanceScore) {
          return right.skill.relevanceScore! - left.skill.relevanceScore!;
        }
        const leftInstallSortValue = getInstallSortValue(left.skill);
        const rightInstallSortValue = getInstallSortValue(right.skill);
        if (leftInstallSortValue !== rightInstallSortValue) {
          return rightInstallSortValue - leftInstallSortValue;
        }
        if (left.skill.isOfficial !== right.skill.isOfficial) {
          return left.skill.isOfficial ? -1 : 1;
        }
      }

      return left.index - right.index;
    }

    if (options.sort === 'trending') {
      const leftTrend = getTrendingSortValue(left.skill);
      const rightTrend = getTrendingSortValue(right.skill);
      if (leftTrend !== rightTrend) {
        return rightTrend - leftTrend;
      }
    } else if (options.sort === 'installs' || options.mode === 'browse') {
      const leftInstallSortValue = getInstallSortValue(left.skill);
      const rightInstallSortValue = getInstallSortValue(right.skill);
      if (leftInstallSortValue !== rightInstallSortValue) {
        return rightInstallSortValue - leftInstallSortValue;
      }
    }

    if (left.skill.isOfficial !== right.skill.isOfficial) {
      return left.skill.isOfficial ? -1 : 1;
    }

    if (options.sort === 'best-match') {
      return left.index - right.index;
    }

    const leftInstallSortValue = getInstallSortValue(left.skill);
    const rightInstallSortValue = getInstallSortValue(right.skill);
    if (leftInstallSortValue !== rightInstallSortValue) {
      return rightInstallSortValue - leftInstallSortValue;
    }

    return left.index - right.index;
  });

  return indexed.map(({ skill }) => skill);
}
