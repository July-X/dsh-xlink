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
