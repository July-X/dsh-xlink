document.querySelectorAll('.compare').forEach((compare) => {
  const range = compare.querySelector('.compare-range');
  const update = () => {
    const light = Number(range.value);
    compare.style.setProperty('--split', `${light}%`);
    range.setAttribute('aria-valuetext', `浅色界面 ${light}%，深色界面 ${100 - light}%`);
  };
  range.addEventListener('input', update);
  update();
});

// 截图切换只更新公开示例图，不切换应用状态。
document.querySelectorAll('[data-screen]').forEach((screen) => {
  const image = screen.querySelector('[data-screen-image]');
  const link = screen.querySelector('[data-screen-link]');
  const title = screen.querySelector('[data-screen-title]').textContent;
  const buttons = screen.querySelectorAll('[data-screen-theme]');
  screen.classList.add('screen-ready');
  buttons.forEach((button) => button.addEventListener('click', () => {
    const theme = button.dataset.screenTheme;
    const label = theme === 'dark' ? '深色' : '浅色';
    image.src = image.dataset[theme];
    image.alt = `${title}${label}界面，使用示例数据。`;
    link.href = image.dataset[theme];
    link.dataset.caption = `${title} · ${label} · 示例数据`;
    buttons.forEach((item) => item.setAttribute('aria-pressed', String(item === button)));
  }));
});

// 原图链接是无脚本兜底；支持 dialog 的浏览器提供就地预览和原生键盘焦点管理。
const viewer = document.querySelector('#image-viewer');
if (viewer && typeof viewer.showModal === 'function') {
  const image = viewer.querySelector('img');
  const caption = viewer.querySelector('#image-viewer-caption');
  const original = viewer.querySelector('[data-viewer-original]');
  let opener;
  document.querySelectorAll('[data-lightbox]').forEach((link) => {
    link.addEventListener('click', (event) => {
      if (event.ctrlKey || event.metaKey || event.shiftKey || event.altKey || event.button > 0) return;
      event.preventDefault();
      opener = link;
      image.src = link.href;
      image.alt = link.dataset.caption || link.querySelector('img')?.alt || '应用界面';
      caption.textContent = image.alt;
      original.href = link.href;
      if (!viewer.open) viewer.showModal();
      document.documentElement.classList.add('viewer-open');
    });
  });
  viewer.querySelector('[data-viewer-close]').addEventListener('click', () => viewer.close());
  viewer.addEventListener('click', (event) => {
    if (event.target === viewer) viewer.close();
  });
  viewer.addEventListener('close', () => {
    document.documentElement.classList.remove('viewer-open');
    opener?.focus();
  });
}
