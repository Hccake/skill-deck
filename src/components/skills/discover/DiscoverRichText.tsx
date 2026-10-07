import { useEffect, useLayoutEffect, useMemo, useId, useRef } from 'react';
import { useDetailTranslation } from '@/hooks/useDetailTranslation';
import { collectElements, prepareElement, restoreElement } from '@/lib/translation/document';
import { useTranslation } from 'react-i18next';

/** 展示 API 已清理的 HTML，并为宽表与代码提供独立的键盘滚动区域。 */
export function DiscoverRichText({ html }: { html: string }) {
  const { t } = useTranslation();
  const ref = useRef<HTMLDivElement>(null);
  const prefix = useId();
  const translation = useDetailTranslation();
  const register = translation?.register;
  const prepared = useMemo(() => {
    if (!register) return { html, units: [] as Array<{ id: string; value: ReturnType<typeof prepareElement> }> };
    const container = document.createElement('div');
    container.innerHTML = html;
    const elements = collectElements(container);
    const units = elements.map((element, index) => {
      const id = `${prefix}:${index}`;
      const value = prepareElement(element);
      element.setAttribute('data-translation-source', '');
      const slot = document.createElement('span');
      slot.dataset.translationSlot = id;
      slot.dataset.translationOutput = '';
      slot.lang = 'zh-CN'; slot.className = 'skill-translation'; slot.hidden = true;
      element.append(slot);
      return { id, value };
    });
    return { html: container.innerHTML, units };
  }, [html, register, prefix]);
  useLayoutEffect(() => {
    if (!register) return;
    const cleanups = prepared.units.map(unit => register(unit.id, unit.value));
    return () => cleanups.forEach(cleanup => cleanup());
  }, [prepared, register]);
  const results = translation?.results;
  const enabled = translation?.enabled;
  useLayoutEffect(() => {
    const slots = [...ref.current?.querySelectorAll<HTMLElement>('[data-translation-slot]') ?? []];
    for (const unit of prepared.units) {
      const slot = slots.find(slot => slot.dataset.translationSlot === unit.id);
      if (!slot) continue;
      const result = results?.get(unit.id);
      const restored = enabled && result?.status === 'translated' && result.text.trim() !== unit.value.text.trim()
        ? restoreElement(unit.value, result.text) : null;
      slot.replaceChildren(...restored ? [...restored.childNodes] : []);
      slot.hidden = !restored;
    }
  }, [prepared, results, enabled]);

  useEffect(() => {
    const root = ref.current;
    if (!root) return;
    const regions: HTMLElement[] = [...root.querySelectorAll('pre')];
    for (const table of root.querySelectorAll('table')) {
      let wrapper = table.parentElement!;
      if (!wrapper.classList.contains('discover-table-scroll')) {
        wrapper = document.createElement('div');
        wrapper.className = 'discover-table-scroll';
        table.replaceWith(wrapper);
        wrapper.append(table);
      }
      regions.push(wrapper);
    }
    const measure = () => {
      const overflowing = regions.map(region => region.scrollWidth > region.clientWidth + 1);
      regions.forEach((region, index) => {
        region.tabIndex = overflowing[index] ? 0 : -1;
        if (overflowing[index]) {
          region.setAttribute('role', 'region');
          region.setAttribute('aria-label', t(region.tagName === 'PRE' ? 'skills.detail.codeBlock' : 'skills.detail.documentTable'));
        } else {
          region.removeAttribute('role');
          region.removeAttribute('aria-label');
        }
      });
    };
    const observer = new ResizeObserver(measure);
    for (const region of regions) {
      observer.observe(region);
      if (region.firstElementChild) observer.observe(region.firstElementChild);
    }
    measure();
    return () => observer.disconnect();
  }, [prepared.html, t]);

  const content = useMemo(() => ({ __html: prepared.html }), [prepared.html]);
  return <div ref={ref} className="discover-rich-text skill-prose skill-prose-with-lists" dangerouslySetInnerHTML={content} />;
}
