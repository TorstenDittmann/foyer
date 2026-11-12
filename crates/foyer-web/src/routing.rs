//! Client-side navigation that waits for data before it swaps pages.
//!
//! Leptos' router navigates in the browser. By default it swaps to the next
//! page straight away and that page then shows an empty space until its
//! resources have loaded. [`Preloaded`] wraps a page so the router waits
//! first: while a navigation is in flight the current page stays on screen,
//! the next page is built off-screen and its resources load, and only then
//! does the router swap in the finished page. Together with the progress bar
//! in `app.rs` this makes navigations feel like a fast, server-rendered site
//! while staying in the browser.
//!
//! It uses the router's own preload step (the same hook lazy routes use to
//! load their code) and `AsyncTransition`, which waits for every resource
//! created while the page is built. Resources that a page only creates after
//! some data arrives (inside a `Suspend`) still load after the swap.
//!
//! On the server, and while hydrating the first page, nothing changes: the
//! page renders as usual.

use std::sync::{Arc, Mutex};

use leptos::prelude::*;
use leptos_router::ChooseView;

/// A route view that loads its data before the router shows it. Use it as
/// `<Route path=... view=Preloaded::new(SomePage) />`.
pub struct Preloaded<F> {
    view: F,
    built: Arc<Mutex<Option<AnyView>>>,
}

impl<F> Preloaded<F> {
    pub fn new(view: F) -> Self {
        Self {
            view,
            built: Arc::default(),
        }
    }
}

impl<F: Clone> Clone for Preloaded<F> {
    fn clone(&self) -> Self {
        Self {
            view: self.view.clone(),
            built: Arc::clone(&self.built),
        }
    }
}

impl<F, V> ChooseView for Preloaded<F>
where
    F: Fn() -> V + Send + Sync + Clone + 'static,
    V: IntoView + 'static,
{
    async fn choose(self) -> AnyView {
        let built = self.built.lock().ok().and_then(|mut b| b.take());
        built.unwrap_or_else(|| (self.view)().into_any())
    }

    async fn preload(&self) {
        // The router calls this before it swaps pages on a client-side
        // navigation, with the next route's params and URL in context.
        let view = self.view.clone();
        let built = Arc::clone(&self.built);
        leptos::reactive::transition::AsyncTransition::run(move || {
            let page = view().into_any();
            if let Ok(mut slot) = built.lock() {
                *slot = Some(page);
            }
            async {}
        })
        .await;
    }
}
