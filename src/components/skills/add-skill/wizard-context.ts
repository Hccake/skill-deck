import type { SkillLocationRef } from '@/bindings';

export function parseWizardContext(value: string | null): SkillLocationRef | undefined {
  if (!value) return undefined;
  try {
    const parsed = JSON.parse(value) as unknown;
    if (!parsed || typeof parsed !== 'object') return undefined;
    const { environment, scope } = parsed as Record<string, unknown>;
    if (!environment || typeof environment !== 'object' || !scope || typeof scope !== 'object') {
      return undefined;
    }

    const environmentRecord = environment as Record<string, unknown>;
    const scopeRecord = scope as Record<string, unknown>;
    const validEnvironment = environmentRecord.kind === 'native'
      || (environmentRecord.kind === 'wsl'
        && typeof environmentRecord.distro_name === 'string'
        && environmentRecord.distro_name.length > 0);
    const validScope = scopeRecord.scope === 'global'
      || (scopeRecord.scope === 'project'
        && typeof scopeRecord.project_id === 'string'
        && scopeRecord.project_id.length > 0);
    return validEnvironment && validScope ? parsed as SkillLocationRef : undefined;
  } catch {
    return undefined;
  }
}
