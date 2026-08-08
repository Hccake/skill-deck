import type { SkillLocationRef, EnvironmentRef } from '@/bindings';

export function environmentKey(environment: EnvironmentRef): string {
  return environment.kind === 'native'
    ? 'native'
    : `wsl:${encodeURIComponent(environment.distro_name.toLocaleLowerCase('en-US'))}`;
}

export function contextKey(context: SkillLocationRef): string {
  const scopeKey = context.scope.scope === 'global'
    ? 'global'
    : `project:${encodeURIComponent(context.scope.project_id)}`;
  return `${environmentKey(context.environment)}/${scopeKey}`;
}

export function globalContext(environment: EnvironmentRef): SkillLocationRef {
  return {
    environment,
    scope: { scope: 'global' },
  };
}

export function sameEnvironment(left: EnvironmentRef, right: EnvironmentRef): boolean {
  return environmentKey(left) === environmentKey(right);
}

export function sameContext(left: SkillLocationRef, right: SkillLocationRef): boolean {
  return contextKey(left) === contextKey(right);
}
