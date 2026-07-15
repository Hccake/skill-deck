import { createContext, useContext } from 'react';
import type { LifecycleAction } from '@/bindings';

export interface WindowLifecycleContextValue {
  requestAction: (action: LifecycleAction) => Promise<void>;
}

export const WindowLifecycleContext = createContext<WindowLifecycleContextValue | null>(null);

export function useWindowLifecycle(): WindowLifecycleContextValue {
  const context = useContext(WindowLifecycleContext);
  if (!context) {
    throw new Error('useWindowLifecycle must be used within WindowLifecycleProvider');
  }
  return context;
}
