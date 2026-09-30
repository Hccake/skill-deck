import GithubSlugger from 'github-slugger';
import type { Element, ElementContent, Root } from 'hast';
import type { Nodes, Root as MarkdownRoot } from 'mdast';

declare module 'hast' {
  interface ElementData {
    originalTarget?: string;
    linkTarget?: string;
    linkTitle?: string;
  }
}

export function remarkDetailTargets() {
  return (tree: MarkdownRoot) => {
    const definitions = new Map<string, string>();
    const walk = (node: Nodes, visit: (node: Nodes) => void) => {
      visit(node);
      if ('children' in node) node.children.forEach(child => walk(child, visit));
    };
    walk(tree, node => {
      if (node.type === 'definition' && !definitions.has(node.identifier)) definitions.set(node.identifier, node.url);
    });
    walk(tree, node => {
      const target = node.type === 'link' || node.type === 'image' ? node.url
        : node.type === 'linkReference' || node.type === 'imageReference' ? definitions.get(node.identifier) : undefined;
      if (target !== undefined) {
        node.data = { ...node.data, hProperties: { ...node.data?.hProperties, 'data-original-target': target } };
      }
    });
  };
}

function textContent(node: ElementContent): string {
  if (node.type === 'text') return node.value;
  if (node.type !== 'element') return '';
  if (node.tagName === 'img') return String(node.properties.alt ?? '');
  return node.children.map(textContent).join('');
}

export function originalTarget(node: Element | undefined, fallback = ''): string {
  const target = node?.data?.originalTarget;
  return typeof target === 'string' ? target : fallback;
}

function containsImage(node: ElementContent): boolean {
  return node.type === 'element' && (node.tagName === 'img' || node.children.some(containsImage));
}

// 将图片与链接内的文字分开，让图片重试按钮始终位于导航链接之外。
function separateLinkedImages(link: Element) {
  const target = originalTarget(link);
  const properties = { ...link.properties };
  const split = (children: ElementContent[]): ElementContent[] => {
    const result: ElementContent[] = [];
    let text: ElementContent[] = [];
    const flush = () => {
      if (text.length) result.push({ type: 'element', tagName: 'a', properties: { ...properties }, data: { originalTarget: target }, children: text });
      text = [];
    };
    for (const child of children) {
      if (child.type === 'element' && containsImage(child)) {
        flush();
        if (child.tagName === 'img') child.data = { ...child.data, linkTarget: target, linkTitle: typeof properties.title === 'string' ? properties.title : undefined };
        else child.children = split(child.children);
        result.push(child);
      } else text.push(child);
    }
    flush();
    return result;
  };
  link.tagName = 'span';
  link.properties = {};
  link.children = split(link.children);
}

export function rehypeDetailDocument({ prefix }: { prefix: string }) {
  return (tree: Root) => {
    const slugger = new GithubSlugger();
    const elements: Element[] = [];
    const existingIds = new Map<string, string>();
    const visit = (parent: Root | Element) => {
      for (const node of parent.children) {
        if (node.type !== 'element') continue;
        elements.push(node);
        const existingId = typeof node.properties.id === 'string' ? node.properties.id : undefined;
        const anchor = existingId ?? (/^h[1-6]$/.test(node.tagName) ? slugger.slug(textContent(node)) : undefined);
        if (anchor !== undefined) {
          node.properties.id = `${prefix}-${anchor}`;
          node.properties['data-document-anchor'] = anchor;
          if (node.tagName !== 'a') node.properties.tabIndex = -1;
          if (existingId) existingIds.set(existingId, node.properties.id);
        }
        if (node.tagName === 'a' || node.tagName === 'img') {
          const value = node.data?.originalTarget ?? node.properties['data-original-target'] ?? node.properties[node.tagName === 'a' ? 'href' : 'src'];
          // URL 过滤仍由渲染器执行，原始值仅用于分类、提示和复制。
          node.data = { ...node.data, originalTarget: typeof value === 'string' ? value : '' };
          delete node.properties['data-original-target'];
        }
        if (node.tagName === 'a' && node.children.some(containsImage)) separateLinkedImages(node);
        visit(node);
      }
    };
    visit(tree);
    for (const node of elements) {
      for (const attribute of ['ariaDescribedBy', 'ariaLabelledBy']) {
        const value = node.properties[attribute];
        if (Array.isArray(value)) node.properties[attribute] = value.map(id => existingIds.get(String(id)) ?? id);
      }
    }
  };
}
