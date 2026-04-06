// src/lib/discover-utils.ts
import type { DiscoverSkillSummary } from '@/lib/discover/types';

export const MIN_LOADING_MS = 180;

export function delay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

export function isSkillInstalled(installedSkillKeys: Set<string>, skill: DiscoverSkillSummary): boolean {
  const normalizedSource = skill.source.replace('https://github.com/', '');
  return installedSkillKeys.has(`${skill.source}::${skill.name}`)
    || installedSkillKeys.has(`${normalizedSource}::${skill.name}`);
}
