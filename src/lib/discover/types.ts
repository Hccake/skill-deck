export type DiscoverTab = 'popular' | 'trending' | 'official';

export type DiscoverSort = 'best-match' | 'installs' | 'trending';

export type DiscoverAuditRisk = 'safe' | 'low' | 'medium' | 'high' | 'critical' | 'unknown';

export interface DiscoverSkillSummary {
  slug: string;
  name: string;
  source: string;
  summary?: string;
  installs: number;
  weeklyInstalls?: number;
  relevanceScore?: number;
  isOfficial: boolean;
  auditRisk?: DiscoverAuditRisk;
  stars?: number;
  detailUrl: string;
}

export interface DiscoverSortOptions {
  mode: 'browse' | 'search';
  sort: DiscoverSort;
}

export interface DiscoverFilterOptions {
  officialOnly?: boolean;
  notInstalledOnly?: boolean;
  installedSkillKeys?: Set<string>;
  risk?: DiscoverAuditRisk[];
}

