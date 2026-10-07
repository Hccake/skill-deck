import { expect, it } from 'vitest';
import { prepareElement, restoreElement, collectElements } from './document';

it('翻译完整句子并保留代码和链接的实际目标', () => {
  const source = document.createElement('p');
  source.innerHTML = 'Run <code>npm install</code> and read <a href="https://example.com">the guide</a>.';
  const prepared = prepareElement(source);
  expect(prepared.text).not.toContain('npm install');
  expect(prepared.text).toContain('the guide');
  const restored = restoreElement(prepared, prepared.text.replace('Run ', '运行 '));
  expect(restored?.querySelector('code')?.textContent).toBe('npm install');
  expect(restored?.querySelector('a')?.getAttribute('href')).toBe('https://example.com');
  expect(restoreElement(prepared, '标记丢失')).toBeNull();
});

it('分段不重复处理嵌套列表，跳过代码块和已经生成的译文', () => {
  const root = document.createElement('div');
  root.innerHTML = '<p>Intro</p><ul><li>First<ul><li>Second</li></ul></li></ul><pre><code>ignored</code></pre><p data-translation-output>译文</p>';
  expect(collectElements(root).map(item => item.textContent)).toEqual(['Intro', 'First', 'Second']);
});

it('引用内的直接文字、段落和嵌套引用分别配对，原容器保持完整', () => {
  const root = document.createElement('div');
  root.innerHTML = '<blockquote>Opening<p>Second</p><blockquote>Nested</blockquote><pre>Keep this diagram</pre>Ending</blockquote>';
  const quote = root.firstElementChild;
  const pre = root.querySelector('pre');
  expect(collectElements(root).map(item => item.textContent)).toEqual(['Opening', 'Second', 'Nested', 'Ending']);
  expect(root.firstElementChild).toBe(quote);
  expect(root.querySelector('pre')).toBe(pre);
  expect(quote?.querySelector('blockquote')?.textContent).toBe('Nested');
});

it('纯 pre、text 代码块和只包含代码块的外层容器均不产生翻译单元', () => {
  for (const html of ['<pre>Read this diagram</pre>', '<pre><code class="language-text">Read this diagram</code></pre>', '<blockquote><pre>Read this diagram</pre></blockquote>', '<a href="/guide"><pre>Read this diagram</pre></a>']) {
    const root = document.createElement('div');
    root.innerHTML = html;
    expect(collectElements(root)).toEqual([]);
    expect(root.innerHTML).toBe(html);
  }
});
