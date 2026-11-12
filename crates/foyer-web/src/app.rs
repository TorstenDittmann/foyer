use leptos::prelude::*;
use leptos_meta::{HashedStylesheet, MetaTags, Title, provide_meta_context};
use leptos_router::components::{Outlet, ParentRoute, Route, Router, Routes};
use leptos_router::{SsrMode, StaticSegment, path};

use crate::components::{Footer, Header, Loading, NotFound, provide_session};
use crate::pages::account::{AccountPage, UnsubscribePage};
use crate::pages::app_download::GetTheApp;
use crate::pages::auth::{ForgotPasswordPage, ResetPasswordPage, SignInPage, VerifyEmailPage};
use crate::pages::catalog::{Browse, DeveloperPage, Discover, GameDetail};
use crate::pages::dashboard::{Dashboard, EditGame, NewGame};
use crate::pages::design::{DesignGame, DesignPreview};
use crate::pages::guidelines::Guidelines;
use crate::pages::review::{ReviewGamePage, ReviewQueue};
use crate::pages::tools::{ApiTokens, GameDownloads, GameTesters, TesterKeyInvalid};
use crate::routing::Preloaded;

/// The HTML document the server renders around [`App`].
pub fn shell(options: LeptosOptions) -> impl IntoView {
    view! {
        <!DOCTYPE html>
        <html lang="en">
            <head>
                <meta charset="utf-8" />
                <meta name="viewport" content="width=device-width, initial-scale=1" />
                <AutoReload options=options.clone() />
                // Release builds put a content hash in the bundle's file
                // names (`hash-files`), so browsers can cache them for good.
                <HashedStylesheet options=options.clone() id="leptos" />
                <HydrationScripts options />
                // Passkey buttons; plain JS, see public/passkey.js.
                <script src="/passkey.js" defer></script>
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
    let (routing, set_routing) = signal(false);

    view! {
        <Title text="Foyer" />
        <Router set_is_routing=set_routing>
            <ProgressBar active=routing />
            // The first page a visitor opens renders with `SsrMode::Async`: the
            // server waits for its data and sends the finished HTML in one go.
            // After that the router navigates in the browser (see `Preloaded`).
            <SessionProvider>
                <Routes fallback=NotFound>
                <ParentRoute path=StaticSegment("") view=Layout>
                    <Route path=StaticSegment("") view=Preloaded::new(Discover) ssr=SsrMode::Async />
                    <Route path=path!("games") view=Preloaded::new(Browse) ssr=SsrMode::Async />
                    <Route path=path!("games/:slug") view=Preloaded::new(GameDetail) ssr=SsrMode::Async />
                    <Route path=path!("developers/:slug") view=Preloaded::new(DeveloperPage) ssr=SsrMode::Async />
                    <Route path=path!("guidelines") view=Preloaded::new(Guidelines) ssr=SsrMode::Async />
                    <Route path=path!("app") view=Preloaded::new(GetTheApp) ssr=SsrMode::Async />
                    <Route path=path!("signin") view=Preloaded::new(SignInPage) ssr=SsrMode::Async />
                    <Route path=path!("forgot-password") view=Preloaded::new(ForgotPasswordPage) ssr=SsrMode::Async />
                    <Route path=path!("reset-password") view=Preloaded::new(ResetPasswordPage) ssr=SsrMode::Async />
                    <Route path=path!("verify-email") view=Preloaded::new(VerifyEmailPage) ssr=SsrMode::Async />
                    <Route path=path!("account") view=Preloaded::new(AccountPage) ssr=SsrMode::Async />
                    <Route path=path!("unsubscribe") view=Preloaded::new(UnsubscribePage) ssr=SsrMode::Async />
                    <Route path=path!("review") view=Preloaded::new(ReviewQueue) ssr=SsrMode::Async />
                    <Route path=path!("review/:slug") view=Preloaded::new(ReviewGamePage) ssr=SsrMode::Async />
                    <Route path=path!("dashboard") view=Preloaded::new(Dashboard) ssr=SsrMode::Async />
                    <Route path=path!("dashboard/games/new") view=Preloaded::new(NewGame) ssr=SsrMode::Async />
                    <Route path=path!("dashboard/games/:slug") view=Preloaded::new(EditGame) ssr=SsrMode::Async />
                    <Route path=path!("dashboard/games/:slug/downloads") view=Preloaded::new(GameDownloads) ssr=SsrMode::Async />
                    <Route path=path!("dashboard/games/:slug/testers") view=Preloaded::new(GameTesters) ssr=SsrMode::Async />
                    <Route path=path!("dashboard/games/:slug/design") view=Preloaded::new(DesignGame) ssr=SsrMode::Async />
                    <Route path=path!("dashboard/games/:slug/preview") view=Preloaded::new(DesignPreview) ssr=SsrMode::Async />
                    <Route path=path!("dashboard/tokens") view=Preloaded::new(ApiTokens) ssr=SsrMode::Async />
                    <Route path=path!("tester-key") view=Preloaded::new(TesterKeyInvalid) ssr=SsrMode::Async />
                </ParentRoute>
                </Routes>
            </SessionProvider>
        </Router>
    }
}

/// A thin bar along the top of the window while the router is fetching the
/// next page.
#[component]
fn ProgressBar(active: ReadSignal<bool>) -> impl IntoView {
    view! {
        <div
            class="pointer-events-none fixed inset-x-0 top-0 z-50 h-0.5 bg-accent transition-opacity duration-200"
            class:opacity-0=move || !active.get()
            aria-hidden="true"
        ></div>
    }
}

/// Provides the session above the routes, so pages built ahead of a
/// navigation (see [`Preloaded`]) can read it too.
#[component]
fn SessionProvider(children: Children) -> impl IntoView {
    provide_session();
    children()
}

/// Header and footer around every page.
///
/// Navigations wait for the next page's data before swapping (see
/// [`Preloaded`]). The `Transition` around the outlet keeps the current
/// content on screen when a page refetches, for example after a form action,
/// instead of falling back to a loading state.
#[component]
fn Layout() -> impl IntoView {
    view! {
        <div class="flex min-h-screen flex-col">
            <Header />
            <div class="flex-1">
                <Transition fallback=Loading>
                    <Outlet />
                </Transition>
            </div>
            <Footer />
        </div>
    }
}
