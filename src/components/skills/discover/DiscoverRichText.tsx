import { useEffect, useRef } from 'react';
import { useTranslation } from 'react-i18next';

/** 展示 API 已清理的 HTML，并为宽表与代码提供独立的键盘滚动区域。 */
export function DiscoverRichText({ html }: { html: string }) {
  const { t } = useTranslation();
  const ref = useRef<HTMLDivElement>(null);

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
  }, [html, t]);

  return <div ref={ref} className="discover-rich-text skill-prose skill-prose-with-lists" dangerouslySetInnerHTML={{ __html: html }} />;
}
