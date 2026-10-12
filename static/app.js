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
