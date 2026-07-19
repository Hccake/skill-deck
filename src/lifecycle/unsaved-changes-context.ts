import { createContext, useContext, useEffect } from 'react';

export interface UnsavedChangesRegistration {
  dirty: boolean;
  discard: () => void | Promise<void>;
}

export interface UnsavedChangesContextValue {
  register: (registration: UnsavedChangesRegistration) => () => void;
  guard: (action: () => void | Promise<void>) => Promise<boolean>;
}

export const UnsavedChangesContext = createContext<UnsavedChangesContextValue | null>(null);

export function useUnsavedChanges(): UnsavedChangesContextValue {
  const context = useContext(UnsavedChangesContext);
  if (!context) throw new Error('useUnsavedChanges must be used within UnsavedChangesProvider');
  return context;
}

export function useOptionalUnsavedChanges(): UnsavedChangesContextValue | null {
  return useContext(UnsavedChangesContext);
}

export function useRegisterUnsavedChanges(registration: UnsavedChangesRegistration): void {
  const context = useOptionalUnsavedChanges();
  useEffect(() => {
    if (!context) return undefined;
    return context.register(registration);
  }, [context, registration]);
}
