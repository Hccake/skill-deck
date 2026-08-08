import type { InstalledSkill, InstalledSkillLocation } from '@/bindings';

export interface SkillIdentity {
  name: string;
  scope: InstalledSkillLocation;
  projectPath?: string | null;
}

export function getSkillIdentity(
  skill: Pick<InstalledSkill, 'name' | 'scope'>,
  projectPath?: string | null,
): SkillIdentity {
  return {
    name: skill.name,
    scope: skill.scope,
    projectPath: skill.scope === 'project' ? projectPath ?? null : null,
  };
}

export function getSkillIdentityKey(identity: SkillIdentity): string {
  if (identity.scope === 'global') {
    return `global:${identity.name}`;
  }

  return `project:${identity.projectPath ?? ''}:${identity.name}`;
}

export function isSameSkillIdentity(a: SkillIdentity | null, b: SkillIdentity | null): boolean {
  if (!a || !b) return false;
  return (
    a.name === b.name &&
    a.scope === b.scope &&
    (a.projectPath ?? null) === (b.projectPath ?? null)
  );
}

export function findSkillByIdentity(
  identity: SkillIdentity | null,
  globalSkills: InstalledSkill[],
  projectSkills: InstalledSkill[],
  currentProjectPath?: string | null,
): InstalledSkill | null {
  if (!identity) return null;

  if (identity.scope === 'global') {
    return globalSkills.find((skill) => skill.name === identity.name && skill.scope === 'global') ?? null;
  }

  if ((identity.projectPath ?? null) !== (currentProjectPath ?? null)) {
    return null;
  }

  return projectSkills.find((skill) => skill.name === identity.name && skill.scope === 'project') ?? null;
}
