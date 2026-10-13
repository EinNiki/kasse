// Small configuration for the bundled libraries (no application logic).

// A text field the user is typing in keeps its value while a live update morphs the page around it.
if (window.Idiomorph) {
  Idiomorph.defaults.ignoreActiveValue = true;
}

// Live updates must never move the page: remember the scroll position when a request starts and
// put it back if anything (focus handling, re-layout) changed it while the new content was inserted.
let scrollBefore = null;
document.addEventListener('htmx:beforeRequest', () => { scrollBefore = window.scrollY; });
document.addEventListener('htmx:afterSettle', () => {
  if (scrollBefore !== null && Math.abs(window.scrollY - scrollBefore) > 1) {
    window.scrollTo({ top: scrollBefore, behavior: 'instant' });
  }
  scrollBefore = null;
});

// Lager tabs: remember whether the clicked tab lies right or left of the current one and let the
// new content slide in from that side (see .slide-fwd / .slide-back in app.css).
let lagerDirection = null;
document.addEventListener('htmx:beforeRequest', (e) => {
  const link = e.detail.elt;
  const nav = link.closest && link.closest('#lager-tabs');
  if (!nav) return;
  const links = [...nav.querySelectorAll('a')];
  const from = links.findIndex((l) => l.classList.contains('active'));
  const to = links.indexOf(link);
  lagerDirection = to === from ? null : to > from ? 'slide-fwd' : 'slide-back';
});
document.addEventListener('htmx:afterSettle', (e) => {
  const body = document.getElementById('lager-body');
  if (!lagerDirection || !body || e.target !== body) return;
  body.classList.add(lagerDirection);
  body.addEventListener('animationend', (ev) => { if (ev.target === body) body.classList.remove('slide-fwd', 'slide-back'); });
  lagerDirection = null;
});
