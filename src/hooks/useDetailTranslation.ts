import { createContext, useContext } from 'react';
import type { TranslationResult } from '@/bindings';
import type { PreparedElement } from '@/lib/translation/document';

export interface ReadingTranslation {
  enabled: boolean;
  busy: boolean;
  outdated: boolean;
  error: string | null;
  results: Map<string, TranslationResult>;
  register: (id: string, value: PreparedElement) => () => void;
  toggle: () => void;
  retry: () => void;
  total: number;
  completed: number;
  rememberReading: () => void;
}
export const TranslationContext = createContext<ReadingTranslation | null>(null);
export function useDetailTranslation() { return useContext(TranslationContext); }
export const TranslationLayoutContext = createContext<{ documentKey: string; expanded: boolean; preserve: () => void } | null>(null);
export function useTranslationLayout() { return useContext(TranslationLayoutContext); }
