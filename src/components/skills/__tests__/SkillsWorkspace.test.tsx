/* @vitest-environment jsdom */
import '@/test-utils';
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { PropsWithChildren, PointerEventHandler } from 'react';
import { SkillsWorkspace } from '../SkillsWorkspace';
import { useLayoutStore } from '@/stores/layout';

const resizeEvents = vi.hoisted(() => ({ changed: null as null | ((layout: Record<string, number>) => void) }));

vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock('@/components/ui/resizable', () => ({
  ResizablePanelGroup: ({ children, groupRef, onLayoutChanged, onPointerDownCapture }: PropsWithChildren<{
    groupRef: { current: unknown };
    onLayoutChanged: (layout: Record<string, number>) => void;
    onPointerDownCapture?: PointerEventHandler<HTMLDivElement>;
  }>) => {
    groupRef.current = { getLayout: () => ({ list: 50, detail: 50 }), setLayout: () => {} };
    resizeEvents.changed = onLayoutChanged;
    return <div onPointerDownCapture={onPointerDownCapture}>{children}</div>;
  },
  ResizablePanel: ({ children }: PropsWithChildren) => <div>{children}</div>,
  ResizableHandle: () => <div role="separator" tabIndex={0} />,
}));

let width = 1024;
const observers = new Set<() => void>();
function resize(next: number) {
  act(() => {
    width = next;
    for (const observe of observers) observe();
  });
}

function workspace(contextKey = 'global', kind: 'skills' | 'library' = 'skills', detail: React.ReactNode = <p>Skill details</p>, onCloseDetail = vi.fn()) {
  return (
    <SkillsWorkspace
      kind={kind}
      contextKey={contextKey}
      detailOpen
      onCloseDetail={onCloseDetail}
      sidebar={<aside><button>Other scope</button><button aria-current="location">Selected scope</button></aside>}
      detail={detail}
    >
      {(navigation) => <>{navigation}<input aria-label="Skill search" /></>}
    </SkillsWorkspace>
  );
}

describe('Skills workspace navigation', () => {
  beforeEach(() => {
    width = 1024;
    useLayoutStore.setState({ skillsListWidth: 320, libraryListWidth: 320 });
    vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(() => ({ width, height: 600 }) as DOMRect);
    vi.stubGlobal('ResizeObserver', class {
      private callback: () => void;
      constructor(callback: () => void) { this.callback = callback; }
      observe() { observers.add(this.callback); }
      unobserve() { observers.delete(this.callback); }
      disconnect() { observers.delete(this.callback); }
    });
  });

  afterEach(() => {
    cleanup();
    observers.clear();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it('keeps navigation reachable when resizing hides the focused sidebar', () => {
    render(workspace());
    screen.getByRole('button', { name: 'Selected scope' }).focus();
    resize(720);
    const toggle = screen.getByRole('button', { name: 'context.navigation' });
    expect(document.activeElement).toBe(toggle);
    expect(screen.queryByRole('button', { name: 'Selected scope' })).toBeNull();
    fireEvent.click(toggle);
    expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Selected scope' }));
  });

  it('consumes Escape in temporary navigation and returns focus without closing details', () => {
    render(workspace());
    resize(720);
    const toggle = screen.getByRole('button', { name: 'context.navigation' });
    fireEvent.click(toggle);
    const onWindowKey = vi.fn();
    window.addEventListener('keydown', onWindowKey);
    try {
      fireEvent.keyDown(document.activeElement!, { key: 'Escape' });
      expect(onWindowKey).not.toHaveBeenCalled();
      expect(document.activeElement).toBe(toggle);
      expect(toggle.getAttribute('aria-expanded')).toBe('false');
      expect(screen.getByText('Skill details')).toBeTruthy();
    } finally {
      window.removeEventListener('keydown', onWindowKey);
    }
  });

  it('moves focus into visible navigation when the collapsed toggle disappears', () => {
    render(workspace());
    resize(720);
    screen.getByRole('button', { name: 'context.navigation' }).focus();
    resize(1000);
    expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Selected scope' }));
  });

  it('closes temporary library navigation when the selected library changes', () => {
    const view = render(workspace('library-one', 'library'));
    resize(720);
    const toggle = screen.getByRole('button', { name: 'libraries.navigation' });
    fireEvent.click(toggle);
    view.rerender(workspace('library-two', 'library'));
    expect(toggle.getAttribute('aria-expanded')).toBe('false');
    expect(document.activeElement).toBe(toggle);
  });

  it('saves a resize that starts from the enlarged handle area on an adjacent panel', () => {
    render(workspace());
    act(() => resizeEvents.changed!({ 'skills-list-panel': 45, 'skill-detail-panel': 55 }));
    expect(useLayoutStore.getState().skillsListWidth).toBe(320);
    // 分栏库先聚焦分隔条并消费指针事件，事件目标仍可能是相邻面板。
    screen.getByRole('separator').focus();
    const pointer = new Event('pointerdown', { bubbles: true, cancelable: true });
    pointer.preventDefault();
    fireEvent(screen.getByRole('textbox'), pointer);
    act(() => resizeEvents.changed!({ 'skills-list-panel': 50, 'skill-detail-panel': 50 }));
    expect(useLayoutStore.getState().skillsListWidth).toBe(408);
    resize(720);
    act(() => resizeEvents.changed!({ 'skills-list-panel': 50, 'skill-detail-panel': 50 }));
    expect(useLayoutStore.getState().skillsListWidth).toBe(408);
  });

  it('keeps temporary navigation open while its action opens a dialog', () => {
    render(<>{workspace()}<div role="dialog"><button>Dialog action</button></div></>);
    resize(720);
    const toggle = screen.getByRole('button', { name: 'context.navigation' });
    fireEvent.click(toggle);
    act(() => screen.getByRole('button', { name: 'Dialog action' }).focus());
    expect(toggle.getAttribute('aria-expanded')).toBe('true');
  });

  it('closes an empty detail pane with Escape after navigation has consumed its own Escape', () => {
    const closeDetail = vi.fn();
    render(workspace('global', 'skills', null, closeDetail));
    resize(720);
    const toggle = screen.getByRole('button', { name: 'context.navigation' });
    fireEvent.click(toggle);
    fireEvent.keyDown(document.activeElement!, { key: 'Escape' });
    expect(closeDetail).not.toHaveBeenCalled();
    fireEvent.keyDown(toggle, { key: 'Escape' });
    expect(closeDetail).toHaveBeenCalledTimes(1);
  });
});
