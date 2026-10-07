import type { DiscoverSkillSummary } from '@/lib/discover/types';
import type { TranslationResult } from '@/bindings';

export interface ReadingReturn {
  route: string;
  documentKey: string;
  scrollTop: number;
  anchorIndex: number;
  offset: number;
  descriptionExpanded: boolean;
  discoverSkill?: DiscoverSkillSummary;
  translation?: { serviceKey: string; results: Array<{ signature: string; result: TranslationResult }> };
}
let saved: ReadingReturn | null = null;
export function saveReadingReturn(value: ReadingReturn): boolean {
  const target = new URL(value.route, 'https://skill-deck.local');
  if (target.origin !== 'https://skill-deck.local' || !['/', '/libraries', '/discover'].includes(target.pathname)) return false;
  saved = value;
  return true;
}
export function getReadingReturn(): ReadingReturn | null { return saved; }
export function clearReadingReturn() { saved = null; }

export function captureReading(root: HTMLElement): Pick<ReadingReturn, 'scrollTop' | 'anchorIndex' | 'offset' | 'descriptionExpanded'> {
  const viewport = root.querySelector<HTMLElement>('[data-radix-scroll-area-viewport]');
  const sources = [...viewport?.querySelectorAll<HTMLElement>('[data-translation-source]') ?? []];
  const top = viewport?.getBoundingClientRect().top ?? 0;
  const index = sources.findIndex(source => source.getBoundingClientRect().bottom > top);
  return { scrollTop: viewport?.scrollTop ?? 0, anchorIndex: index,
    offset: index >= 0 ? sources[index].getBoundingClientRect().top - top : 0,
    descriptionExpanded: root.querySelector('.skill-description')?.getAttribute('data-collapsed') === 'false' };
}
export function restoreReading(root: HTMLElement, snapshot: Pick<ReadingReturn, 'scrollTop' | 'anchorIndex' | 'offset'>) {
  const viewport = root.querySelector<HTMLElement>('[data-radix-scroll-area-viewport]');
  if (!viewport) return;
  const source = viewport.querySelectorAll<HTMLElement>('[data-translation-source]')[snapshot.anchorIndex];
  viewport.scrollTop = source ? viewport.scrollTop + source.getBoundingClientRect().top - viewport.getBoundingClientRect().top - snapshot.offset : snapshot.scrollTop;
}
