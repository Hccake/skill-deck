export interface PreparedElement {
  text: string;
  protectedTokens: string[];
  fragments: Map<string, { element: Element; closing: boolean; atomic: boolean }>;
}

const atomicTags = new Set(['CODE', 'IMG', 'BR']);
const allowedTags = new Set(['A', 'STRONG', 'EM', 'DEL', 'S', 'CODE', 'IMG', 'BR', 'SPAN']);

const blockTags = new Set(['P', 'H1', 'H2', 'H3', 'H4', 'H5', 'H6', 'BLOCKQUOTE', 'UL', 'OL', 'LI', 'TABLE', 'THEAD', 'TBODY', 'TFOOT', 'TR', 'TH', 'TD', 'CAPTION', 'DIV', 'SECTION', 'ARTICLE', 'HR']);
const excludedContent = 'pre,script,style,input,textarea,[contenteditable="true"],[data-translation-output]';
const contentBoundaries = `${[...blockTags].join(',')},${excludedContent},[data-translation-group]`;

/** 按原有容器中的叶子内容分段；代码块是不可跨越的边界。 */
export function collectElements(root: HTMLElement): HTMLElement[] {
  const result: HTMLElement[] = [];
  const isBoundary = (node: Node): node is HTMLElement => node instanceof HTMLElement
    && (blockTags.has(node.tagName) || node.matches(`${excludedContent},[data-translation-group]`) || Boolean(node.querySelector(contentBoundaries)));
  const visit = (element: HTMLElement) => {
    if (element.matches(excludedContent)) return;
    const children = [...element.childNodes];
    if (!children.some(isBoundary)) {
      if (element.textContent?.trim()) result.push(element);
      return;
    }
    let inline: Node[] = [];
    const flush = () => {
      if (inline.some(node => node.textContent?.trim())) {
        const group = document.createElement('span');
        group.dataset.translationGroup = '';
        element.insertBefore(group, inline[0]);
        group.append(...inline);
        result.push(group);
      }
      inline = [];
    };
    for (const child of children) {
      if (isBoundary(child)) { flush(); visit(child); }
      else inline.push(child);
    }
    flush();
  };
  visit(root);
  return result;
}

export function prepareElement(element: Element): PreparedElement {
  let prefix = '__SD_';
  while (element.textContent?.includes(prefix)) prefix += 'X';
  const fragments: PreparedElement['fragments'] = new Map();
  const token = (node: Element, closing: boolean, atomic: boolean) => {
    const value = `${prefix}${fragments.size}__`;
    fragments.set(value, { element: node, closing, atomic });
    return value;
  };
  const walk = (node: Node): string => {
    if (node.nodeType === Node.TEXT_NODE) return node.textContent ?? '';
    if (!(node instanceof Element)) return '';
    if (atomicTags.has(node.tagName)) return token(node, false, true);
    const text = [...node.childNodes].map(walk).join('');
    if (!allowedTags.has(node.tagName)) return text;
    const open = token(node, false, false);
    return open + text + token(node, true, false);
  };
  const text = [...element.childNodes].map(walk).join('');
  return { text, protectedTokens: [...fragments.keys()], fragments };
}

/** 返回受控 DOM；译文中的 HTML 永远作为文字处理。 */
export function restoreElement(prepared: PreparedElement, text: string): HTMLElement | null {
  const root = document.createElement('span');
  let current: Element = root;
  const stack: Array<{ source: Element; target: Element }> = [];
  const tokens = prepared.protectedTokens;
  for (const token of tokens) if (text.split(token).length !== 2) return null;
  const pattern = tokens.length ? new RegExp(`(${tokens.join('|')})`, 'g') : null;
  for (const part of pattern ? text.split(pattern) : [text]) {
    const fragment = prepared.fragments.get(part);
    if (!fragment) { current.append(document.createTextNode(part)); continue; }
    if (fragment.closing) {
      if (stack.at(-1)?.source !== fragment.element) return null;
      stack.pop();
      current = stack.at(-1)?.target ?? root;
    } else {
      const clone = fragment.element.cloneNode(fragment.atomic) as Element;
      clone.removeAttribute('id');
      clone.removeAttribute('data-document-anchor');
      if (fragment.atomic) current.append(clone);
      else {
        current.append(clone);
        stack.push({ source: fragment.element, target: clone });
        current = clone;
      }
    }
  }
  return stack.length ? null : root;
}
