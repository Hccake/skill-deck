export type DiscoverTab = 'popular' | 'trending' | 'hot';

export type DiscoverSort = 'best-match' | 'installs' | 'trending';

export type DiscoverAuditRisk = 'safe' | 'low' | 'medium' | 'high' | 'critical' | 'unknown';

export type DiscoverDisplayMetricKind = 'installs' | 'trending-24h' | 'hot';

export interface DiscoverDisplayMetric {
  kind: DiscoverDisplayMetricKind;
  rawText: string;
  sortValue: number;
}

export interface DiscoverSecurityAudit {
  name: string;
  status: 'pass' | 'warn' | 'fail' | 'unknown';
  url: string;
}

export interface DiscoverInstalledOn {
  agent: string;
  installsText: string;
  installs?: number;
}

export interface DiscoverSkillSummary {
  slug: string;
  name: string;
  source: string;
  summary?: string;
  installs?: number;
  displayMetric: DiscoverDisplayMetric;
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

