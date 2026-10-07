export type TranslationViewState = {
  generation: number;
  documentKey: string;
  contentRevision: string;
  sessionId: string | null;
  enabled: boolean;
};

export function acceptsTranslation(current: TranslationViewState, incoming: Omit<TranslationViewState, 'enabled'>): boolean {
  return current.enabled && current.generation === incoming.generation
    && current.documentKey === incoming.documentKey && current.contentRevision === incoming.contentRevision
    && current.sessionId === incoming.sessionId;
}
