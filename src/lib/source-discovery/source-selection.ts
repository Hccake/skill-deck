import type {
  AgentSelectionIntent,
  AvailableSkill,
  SourceSelectionIntent,
} from '@/bindings';
import { isSkillsShPackUrl, parseSkillsCommand } from '@/utils/parse-skills-command';

interface SourceDiscoverySelection {
  skills: AvailableSkill[];
  skillFilter: string | null;
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
  const availableNames = new Set(result.skills.map((skill) => skill.name));
  const selectedFromFilter = result.skillFilter && availableNames.has(result.skillFilter)
    ? [result.skillFilter]
    : [];
  const selectedFromCommand = parsed.skills.filter((name) => availableNames.has(name));
  const hasExplicitSelection = Boolean(result.skillFilter) || parsed.skills.length > 0;

  let selectedSkillNames: string[];
  if (parsed.wildcardRequested || (!hasExplicitSelection && isSkillsShPackUrl(source))) {
    selectedSkillNames = result.skills.map((skill) => skill.name);
  } else {
    selectedSkillNames = [...new Set([...selectedFromFilter, ...selectedFromCommand])];
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
