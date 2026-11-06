import type {
  AgentSelectionIntent,
  AvailableSkill,
  SourceSelectionIntent,
  SourceSelectionResult,
} from '@/bindings';
import { isSkillsShPackUrl, parseSkillsCommand } from '@/utils/parse-skills-command';

interface SourceDiscoverySelection {
  skills: AvailableSkill[];
  skillFilter: string | null;
  selectionResult?: SourceSelectionResult | null;
}

export interface ResolvedSourceSelection {
  source: string;
  selectedSkillNames: string[];
  sourceSelectionIntent: SourceSelectionIntent;
  agentSelectionIntent: AgentSelectionIntent;
}

export function resolveSourceSelection(
  input: string,
  result: SourceDiscoverySelection,
): ResolvedSourceSelection {
  const parsed = parseSkillsCommand(input);
  const source = parsed.isCommand ? parsed.source : input.trim();
  const hasExplicitSelection = Boolean(result.skillFilter) || parsed.skills.length > 0;

  let selectedSkillNames: string[];
  if (parsed.wildcardRequested || (!hasExplicitSelection && isSkillsShPackUrl(source))) {
    selectedSkillNames = result.skills.map((skill) => skill.name);
  } else {
    selectedSkillNames = result.selectionResult?.matchedNames ?? [];
  }

  return {
    source,
    selectedSkillNames,
    sourceSelectionIntent: {
      wildcardRequested: parsed.wildcardRequested,
      explicitSkillNames: parsed.skills,
    },
    agentSelectionIntent: {
      wildcardRequested: parsed.agentWildcardRequested,
      explicitAgentIds: parsed.agents,
    },
  };
}

/** 图形化搜索已明确选中 Skill，使用显式名称语法避开版本引用。 */
export function sourceWithSkill(source: string, name: string): string {
  return /^(?:github:)?[^/:#@]+\/[^/#@]+$/.test(source)
    ? `${source}#@${encodeURIComponent(name)}`
    : `${source}@${name}`;
}
