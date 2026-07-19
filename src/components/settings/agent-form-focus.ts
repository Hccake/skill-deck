import type { AgentFieldError } from '@/bindings';

export function focusFirstAgentFieldError(errors: AgentFieldError[]) {
  const field = errors[0]?.field;
  const targetId = field === 'id'
    ? 'agent-id'
    : field === 'displayName'
      ? 'agent-name'
      : field?.startsWith('global')
        ? 'global-path'
        : field?.startsWith('project')
          ? 'project-path'
          : field?.startsWith('detectionPaths[')
            ? `detection-path-${field.match(/\[(\d+)\]/)?.[1] ?? '0'}`
            : field === 'scopes'
              ? 'global-enabled'
              : null;
  if (targetId) window.setTimeout(() => document.getElementById(targetId)?.focus(), 0);
}
