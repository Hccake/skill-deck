import type { AgentFieldError } from '@/bindings';

export function resolveFirstAgentFieldErrorTargetId(errors: AgentFieldError[]) {
  const field = errors[0]?.field;
  return field === 'id'
    ? 'agent-id'
    : field === 'displayName'
      ? 'agent-name'
      : field?.startsWith('global')
        ? 'global-path'
        : field?.startsWith('project')
          ? 'project-path'
          : field?.startsWith('detectionPaths[')
            ? `detection-path-${field.match(/\[(\d+)\]/)?.[1] ?? '0'}`
            : field === 'detectionPaths'
              ? 'detection-path-add'
            : field === 'scopes'
              ? 'global-enabled'
              : null;
}
