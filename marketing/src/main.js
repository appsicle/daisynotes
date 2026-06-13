// style.css is loaded as a render-blocking <link> in each page's <head> (not
// imported here) so the first paint is already styled — no flash of unstyled
// content on a full load or refresh, which JS-injected CSS can't prevent.

// ── Client-side routing ───────────────────────────────────────────────
// The pages are real static HTML (great for first paint + SEO), but a full
// browser navigation between them re-parses the document and, in dev,
// re-injects the stylesheet — that's the flash of unstyled content. So we
// intercept same-site links, fetch the next page, and swap #app in place.
// The stylesheet is already live, so the new markup paints styled instantly.
// A View Transition adds a soft crossfade where the browser supports it.

const sameSiteHtml = (a) =>
  a instanceof HTMLAnchorElement &&
  a.origin === location.origin &&
  a.pathname.endsWith('.html') &&
  !a.hasAttribute('download') &&
  a.target !== '_blank'

async function render(url) {
  const res = await fetch(url)
  const doc = new DOMParser().parseFromString(await res.text(), 'text/html')
  const next = doc.getElementById('app')
  if (!next) { location.assign(url); return } // unexpected shape → hard nav
  document.title = doc.title
  document.getElementById('app').replaceWith(next)
  window.scrollTo(0, 0)
}

function navigate(url, push = true) {
  if (push) history.pushState(null, '', url)
  if (document.startViewTransition) document.startViewTransition(() => render(url))
  else render(url)
}

addEventListener('click', (e) => {
  if (e.defaultPrevented || e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return
  const a = e.target.closest('a')
  if (!a) return
  const raw = a.getAttribute('href')
  if (!raw || raw.startsWith('#')) return
  if (!sameSiteHtml(a) || a.href === location.href) return
  e.preventDefault()
  navigate(a.href)
})

addEventListener('popstate', () => render(location.href))
