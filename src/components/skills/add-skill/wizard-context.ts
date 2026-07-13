import type { ContextRef } from '@/bindings';

export function parseWizardContext(value: string | null): ContextRef | undefined {
  if (!value) return undefined;
  try {
    const parsed = JSON.parse(value) as ContextRef;
    if (!parsed?.environment || !parsed?.scope) return undefined;
    return parsed;
  } catch {
    return undefined;
  }
}
