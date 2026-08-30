import { useCallback, useMemo, useState } from 'react';
import type {
  AppError,
  EnvironmentRef,
  LibraryAddPreview,
  LibraryAddSkillResult,
  LibraryId,
} from '@/bindings';
import {
  useSourceDiscovery,
  type SourceDiscoveryOutcome,
} from '@/hooks/useSourceDiscovery';
import type {
  LibraryWorkspaceInput,
  LibraryWorkspaceResult,
} from '@/lib/libraries/workspace';
import type { SearchSkill } from '@/components/skills/skill-search/SkillSearch';
import type { SourceSkillCandidate } from '@/components/source-discovery/SourceSkillSelectionPanel';

export interface LibraryAddTarget {
  environment: EnvironmentRef;
  environmentName: string;
  libraryId: LibraryId;
  libraryName: string;
}

export type LibraryAddPhase =
  | 'source'
  | 'selection'
  | 'preparing'
  | 'review'
  | 'executing'
  | 'result';

type ExecuteLibraryCommand = (
  command: LibraryWorkspaceInput,
) => Promise<LibraryWorkspaceResult>;

interface PreparedAdd {
  preview: LibraryAddPreview;
  hasRetryPreviewError: boolean;
}

interface UseLibraryAddFlowOptions {
  target: LibraryAddTarget;
  existingSkillNames: ReadonlySet<string>;
  execute: ExecuteLibraryCommand;
  onClose: () => void;
}

export function useLibraryAddFlow({
  target,
  existingSkillNames,
  execute,
  onClose,
}: UseLibraryAddFlowOptions) {
  const discovery = useSourceDiscovery(target.environment);
  const {
    result: discoveryResult,
    selection: discoverySelection,
    discover: discoverSource,
    retry: retrySource,
    reset: resetDiscovery,
  } = discovery;
  const [phase, setPhase] = useState<LibraryAddPhase>('source');
  const [sourceInput, setSourceInputState] = useState('');
  const [selectionQuery, setSelectionQuery] = useState('');
  const [selectedCandidateIds, setSelectedCandidateIds] = useState<string[]>([]);
  const [prepared, setPrepared] = useState<PreparedAdd | null>(null);
  const [redirectAcknowledged, setRedirectAcknowledged] = useState(false);
  const [results, setResults] = useState<LibraryAddSkillResult[]>([]);
  const [flowError, setFlowError] = useState<AppError | null>(null);
  const [flowIssue, setFlowIssue] = useState<'writeBlocked' | 'previewMissing' | null>(null);

  const candidates = useMemo<SourceSkillCandidate[]>(() => (
    discoveryResult?.skills.map((skill) => {
      const exists = existingSkillNames.has(skill.name);
      return {
        candidateId: skill.relativePath,
        name: skill.name,
        description: skill.description,
        groupName: skill.pluginName,
        selectable: !exists,
        statusLabel: exists ? 'alreadyInLibrary' : undefined,
      };
    }) ?? []
  ), [discoveryResult, existingSkillNames]);

  const selectableCount = useMemo(
    () => candidates.reduce((count, candidate) => count + Number(candidate.selectable), 0),
    [candidates],
  );

  const agentIntentIgnored = Boolean(
    discoverySelection
    && (
      discoverySelection.agentSelectionIntent.wildcardRequested
      || discoverySelection.agentSelectionIntent.explicitAgentIds.length > 0
    )
  );

  const setSourceInput = useCallback((value: string) => {
    setSourceInputState(value);
    setSelectionQuery('');
    setSelectedCandidateIds([]);
    setPrepared(null);
    setRedirectAcknowledged(false);
    setResults([]);
    setFlowError(null);
    setFlowIssue(null);
    setPhase('source');
    resetDiscovery();
  }, [resetDiscovery]);

  const acceptDiscovery = useCallback((outcome: SourceDiscoveryOutcome | null) => {
    if (!outcome) return false;
    const selectedNames = new Set(outcome.selection.selectedSkillNames);
    const selectedPaths = outcome.result.skills
      .filter((skill) => selectedNames.has(skill.name) && !existingSkillNames.has(skill.name))
      .map((skill) => skill.relativePath);
    setSelectedCandidateIds(selectedPaths);
    setSelectionQuery('');
    setFlowError(null);
    setFlowIssue(null);
    setPhase('selection');
    return true;
  }, [existingSkillNames]);

  const readSource = useCallback(async () => {
    if (!sourceInput.trim()) return false;
    return acceptDiscovery(await discoverSource(sourceInput));
  }, [acceptDiscovery, discoverSource, sourceInput]);

  const retryDiscovery = useCallback(async () => (
    acceptDiscovery(await retrySource())
  ), [acceptDiscovery, retrySource]);

  const selectSearchResult = useCallback(async (skill: SearchSkill) => {
    const nextSource = `${skill.source}@${skill.name}`;
    setSourceInputState(nextSource);
    setFlowError(null);
    setFlowIssue(null);
    return acceptDiscovery(await discoverSource(nextSource));
  }, [acceptDiscovery, discoverSource]);

  const prepare = useCallback(async () => {
    const result = discoveryResult;
    const selected = new Set(selectedCandidateIds);
    if (!result || selected.size === 0) return;
    setPhase('preparing');
    setFlowError(null);
    setFlowIssue(null);
    const outcome = await execute({
      kind: 'addSkills',
      libraryId: target.libraryId,
      discovery: result,
      skillPaths: result.skills
        .filter((skill) => selected.has(skill.relativePath))
        .map((skill) => skill.relativePath),
    });
    if (outcome.status === 'failed') {
      setFlowError(outcome.error);
      setPhase('selection');
      return;
    }
    if (outcome.status === 'notRun') {
      setFlowIssue('writeBlocked');
      setPhase('selection');
      return;
    }
    if (!outcome.snapshot.pendingAdd) {
      setFlowIssue('previewMissing');
      setPhase('selection');
      return;
    }
    setPrepared({
      preview: outcome.snapshot.pendingAdd.preview,
      hasRetryPreviewError: false,
    });
    setRedirectAcknowledged(false);
    setPhase('review');
  }, [discoveryResult, execute, selectedCandidateIds, target.libraryId]);

  const executePrepared = useCallback(async () => {
    if (!prepared) return;
    setPhase('executing');
    setFlowError(null);
    setFlowIssue(null);
    const outcome = await execute({
      kind: 'confirmAddSkills',
      acknowledgeRedirect: redirectAcknowledged,
    });
    if (outcome.status === 'failed') {
      setFlowError(outcome.error);
      setPhase('review');
      return;
    }
    if (outcome.status === 'notRun') {
      setFlowIssue('writeBlocked');
      setPhase('review');
      return;
    }
    setResults((current) => {
      const merged = new Map(current.map((result) => [result.skillName, result]));
      for (const result of outcome.snapshot.lastAddResults) {
        merged.set(result.skillName, result);
      }
      return [...merged.values()];
    });
    setPrepared(outcome.snapshot.pendingAdd ? {
      preview: outcome.snapshot.pendingAdd.preview,
      hasRetryPreviewError: false,
    } : outcome.snapshot.retryAdd ? {
      preview: prepared.preview,
      hasRetryPreviewError: true,
    } : null);
    setFlowError(outcome.snapshot.retryAdd?.error ?? null);
    setPhase('result');
  }, [execute, prepared, redirectAcknowledged]);

  const retryFailed = useCallback(async () => {
    if (!prepared) return;
    if (!prepared.hasRetryPreviewError) {
      await executePrepared();
      return;
    }
    setPhase('preparing');
    setFlowError(null);
    setFlowIssue(null);
    const outcome = await execute({ kind: 'retryAddPreview' });
    if (outcome.status === 'failed') {
      setFlowError(outcome.error);
      setPhase('result');
      return;
    }
    if (outcome.status === 'notRun') {
      setFlowIssue('writeBlocked');
      setPhase('result');
      return;
    }
    if (!outcome.snapshot.pendingAdd) {
      setFlowIssue('previewMissing');
      setPhase('result');
      return;
    }
    const nextPrepared = {
      preview: outcome.snapshot.pendingAdd.preview,
      hasRetryPreviewError: false,
    };
    setPrepared(nextPrepared);
    setPhase('review');
  }, [execute, executePrepared, prepared]);

  const back = useCallback(async () => {
    if (phase === 'review') {
      await execute({ kind: 'discardAddSkills' });
      setPrepared(null);
      setFlowError(null);
      setFlowIssue(null);
      setPhase('selection');
      return;
    }
    if (phase === 'selection') {
      setFlowError(null);
      setFlowIssue(null);
      setPhase('source');
    }
  }, [execute, phase]);

  const close = useCallback(async () => {
    if (phase === 'preparing' || phase === 'executing') return;
    resetDiscovery();
    await execute({ kind: 'discardAddSkills' });
    onClose();
  }, [execute, onClose, phase, resetDiscovery]);

  return {
    phase,
    sourceInput,
    selectionQuery,
    selectedCandidateIds,
    candidates,
    selectableCount,
    prepared,
    redirectAcknowledged,
    results,
    flowError,
    flowIssue,
    discovery,
    agentIntentIgnored,
    setSourceInput,
    setSelectionQuery,
    setSelectedCandidateIds,
    setRedirectAcknowledged,
    readSource,
    retryDiscovery,
    selectSearchResult,
    prepare,
    executePrepared,
    retryFailed,
    back,
    close,
  };
}

export type LibraryAddFlow = ReturnType<typeof useLibraryAddFlow>;
export type { ExecuteLibraryCommand };
