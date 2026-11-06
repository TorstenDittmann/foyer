use leptos::prelude::*;
use leptos_meta::{MetaTags, Stylesheet, Title, provide_meta_context};
use leptos_router::StaticSegment;
use leptos_router::components::{Route, Router, Routes};

/// The HTML document the server renders around [`App`].
pub fn shell(options: LeptosOptions) -> impl IntoView {
    view! {
        <!DOCTYPE html>
        <html lang="en">
            <head>
                <meta charset="utf-8" />
                <meta name="viewport" content="width=device-width, initial-scale=1" />
                <AutoReload options=options.clone() />
                <HydrationScripts options />
                <MetaTags />
            </head>
            <body class="min-h-screen bg-zinc-950 text-zinc-100 antialiased">
                <App />
            </body>
        </html>
    }
}

#[component]
pub fn App() -> impl IntoView {
    provide_meta_context();

    view! {
        <Stylesheet id="leptos" href="/pkg/foyer-web.css" />
        <Title text="Foyer" />
        <Router>
            <Routes fallback=NotFound>
                <Route path=StaticSegment("") view=Home />
            </Routes>
        </Router>
    }
}

#[component]
fn Home() -> impl IntoView {
    view! {
        <main class="mx-auto flex max-w-3xl flex-col gap-6 px-6 py-24">
            <h1 class="text-5xl font-semibold tracking-tight">"Foyer"</h1>
            <p class="text-lg text-zinc-400">
                "A hand-picked showcase of indie games. Free to download, "
                "with a direct line to support the people who made them."
            </p>
        </main>
    }
}

#[component]
fn NotFound() -> impl IntoView {
    view! {
        <main class="mx-auto max-w-3xl px-6 py-24">
            <h1 class="text-2xl font-semibold">"Page not found"</h1>
            <a class="mt-4 inline-block text-zinc-400 underline" href="/">"Back to Foyer"</a>
        </main>
    }
}
