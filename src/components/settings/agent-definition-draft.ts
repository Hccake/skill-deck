import type {
  CustomAgentDefinition,
  CustomPathSpec,
  CustomScopeDefinition,
} from '@/bindings';

function privateSkillPath(id: string, base: 'home' | 'project'): CustomPathSpec {
  return {
    kind: 'based',
    base,
    relativePath: id ? `.${id}/skills` : '',
  };
}

function detectionPath(id: string): CustomPathSpec {
  return {
    kind: 'based',
    base: 'home',
    relativePath: id ? `.${id}` : '',
  };
}

function samePath(left: CustomPathSpec | null, right: CustomPathSpec): boolean {
  if (!left || left.kind !== right.kind) return false;
  if (left.kind === 'absolute' || right.kind === 'absolute') {
    return left.kind === 'absolute' && right.kind === 'absolute' && left.path === right.path;
  }
  return left.base === right.base && left.relativePath === right.relativePath;
}

function retargetScope(
  scope: CustomScopeDefinition,
  fromId: string,
  toId: string,
  base: 'home' | 'project',
): CustomScopeDefinition {
  const previousDefault = privateSkillPath(fromId, base);
  if (!samePath(scope.privatePath, previousDefault)) return scope;
  return { ...scope, privatePath: privateSkillPath(toId, base) };
}

export function slugifyAgentId(displayName: string): string {
  return displayName
    .normalize('NFKD')
    .toLocaleLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .replace(/-{2,}/g, '-');
}

export function createAgentDraft(id = '', displayName = ''): CustomAgentDefinition {
  return {
    id,
    displayName,
    global: {
      enabled: true,
      location: 'private',
      privatePath: privateSkillPath(id, 'home'),
    },
    project: {
      enabled: true,
      location: 'private',
      privatePath: privateSkillPath(id, 'project'),
    },
    detectionPaths: [detectionPath(id)],
  };
}

export function retargetDefaultAgentPaths(
  draft: CustomAgentDefinition,
  fromId: string,
  toId: string,
): CustomAgentDefinition {
  const previousDetection = detectionPath(fromId);
  return {
    ...draft,
    id: toId,
    global: retargetScope(draft.global, fromId, toId, 'home'),
    project: retargetScope(draft.project, fromId, toId, 'project'),
    detectionPaths: draft.detectionPaths.map((path) => (
      samePath(path, previousDetection) ? detectionPath(toId) : path
    )),
  };
}

export function updateAgentDraft(
  current: CustomAgentDefinition,
  proposed: CustomAgentDefinition,
  detachedFields: Set<string> = new Set(),
): CustomAgentDefinition {
  const directIdChange = proposed.id !== current.id;
  if (directIdChange) detachedFields.add('id');
  if (proposed.id === current.id) {
    if (
      proposed.global.location === current.global.location
      && !samePath(proposed.global.privatePath, current.global.privatePath ?? privateSkillPath('', 'home'))
    ) {
      detachedFields.add('global');
    }
    if (
      proposed.project.location === current.project.location
      && !samePath(proposed.project.privatePath, current.project.privatePath ?? privateSkillPath('', 'project'))
    ) {
      detachedFields.add('project');
    }
    const maxPaths = Math.max(current.detectionPaths.length, proposed.detectionPaths.length);
    for (let index = 0; index < maxPaths; index += 1) {
      const previous = current.detectionPaths[index];
      const next = proposed.detectionPaths[index];
      if (!previous || !next || !samePath(next, previous)) {
        detachedFields.add(`detection:${index}`);
      }
    }
  }

  let next = proposed;
  if (
    proposed.displayName !== current.displayName
    && proposed.id === current.id
    && !detachedFields.has('id')
    && current.id === slugifyAgentId(current.displayName)
  ) {
    next = { ...proposed, id: slugifyAgentId(proposed.displayName) };
  }
  if (next.id === current.id) return next;
  const retargeted = retargetDefaultAgentPaths(next, current.id, next.id);
  return {
    ...retargeted,
    global: detachedFields.has('global') ? next.global : retargeted.global,
    project: detachedFields.has('project') ? next.project : retargeted.project,
    detectionPaths: retargeted.detectionPaths.map((path, index) => (
      detachedFields.has(`detection:${index}`) ? next.detectionPaths[index] : path
    )),
  };
}
