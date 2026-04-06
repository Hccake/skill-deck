// src/lib/discover-utils.ts
import type { DiscoverSkillSummary } from '@/lib/discover/types';

export const MIN_LOADING_MS = 180;

export function delay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

export function isSkillInstalled(installedSkillKeys: Set<string>, skill: DiscoverSkillSummary): boolean;
export function isSkillInstalled(installedSkillLocations: Map<string, string[]>, skill: DiscoverSkillSummary): boolean;
export function isSkillInstalled(
  data: Set<string> | Map<string, string[]>,
  skill: DiscoverSkillSummary,
): boolean {
  if (data instanceof Map) {
    return getSkillInstallLocations(data, skill).length > 0;
  }
  const normalizedSource = skill.source.replace('https://github.com/', '');
  return data.has(`${skill.source}::${skill.name}`)
    || data.has(`${normalizedSource}::${skill.name}`);
}

/** 获取 skill 的所有安装位置列表（空数组 = 未安装） */
export function getSkillInstallLocations(
  installedSkillLocations: Map<string, string[]>,
  skill: DiscoverSkillSummary,
): string[] {
  const normalizedSource = skill.source.replace('https://github.com/', '');
  return installedSkillLocations.get(`${skill.source}::${skill.name}`)
    ?? installedSkillLocations.get(`${normalizedSource}::${skill.name}`)
    ?? [];
}
