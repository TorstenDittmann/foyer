use leptos::prelude::*;
use leptos_meta::{MetaTags, Stylesheet, Title, provide_meta_context};
use leptos_router::components::{Outlet, ParentRoute, Route, Router, Routes};
use leptos_router::{StaticSegment, path};

use crate::components::{Footer, Header, NotFound, provide_session};
use crate::pages::auth::SignInPage;
use crate::pages::catalog::{Browse, DeveloperPage, Discover, GameDetail};
use crate::pages::dashboard::{Dashboard, EditGame, NewGame};
use crate::pages::guidelines::Guidelines;

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
            <body class="min-h-screen bg-canvas text-ink antialiased">
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
                <ParentRoute path=StaticSegment("") view=Layout>
                    <Route path=StaticSegment("") view=Discover />
                    <Route path=path!("games") view=Browse />
                    <Route path=path!("games/:slug") view=GameDetail />
                    <Route path=path!("developers/:slug") view=DeveloperPage />
                    <Route path=path!("guidelines") view=Guidelines />
                    <Route path=path!("signin") view=SignInPage />
                    <Route path=path!("dashboard") view=Dashboard />
                    <Route path=path!("dashboard/games/new") view=NewGame />
                    <Route path=path!("dashboard/games/:slug") view=EditGame />
                </ParentRoute>
            </Routes>
        </Router>
    }
}

/// Header and footer around every page, plus the session context.
#[component]
fn Layout() -> impl IntoView {
    provide_session();
    view! {
        <div class="flex min-h-screen flex-col">
            <Header />
            <div class="flex-1">
                <Outlet />
            </div>
            <Footer />
        </div>
    }
}
