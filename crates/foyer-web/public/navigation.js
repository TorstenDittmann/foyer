// Page navigation for the Foyer website.
//
// Every page is rendered on the server with its data already in it, and the
// browser loads it as a normal document. Navigations still feel instant
// because the shell asks the browser to prerender a link as soon as the
// visitor shows intent (hover, focus or touch, via the speculation rules in
// `app.rs`), so the click swaps straight to a finished page.
//
// Leptos' router would otherwise take over link clicks and render the next
// page in the browser, fetching its data after the click and showing a
// loading state. This marks same-origin links as `rel="external"` just before
// the router sees the click, so the browser does a real navigation and can
// use the prerendered page. It runs in the capture phase, ahead of the
// router's listener on `window`.
(function () {
  "use strict";

  window.addEventListener(
    "click",
    function (ev) {
      if (ev.defaultPrevented || ev.button !== 0) return;
      if (ev.metaKey || ev.ctrlKey || ev.shiftKey || ev.altKey) return;
      var target = ev.target;
      var a = target && target.closest ? target.closest("a[href]") : null;
      if (!a || a.target || a.hasAttribute("download")) return;
      if (a.origin !== window.location.origin) return;
      a.relList.add("external");
    },
    true
  );
})();
