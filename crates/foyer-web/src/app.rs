use leptos::prelude::*;
use leptos_meta::{MetaTags, Stylesheet, Title, provide_meta_context};
use leptos_router::components::{Outlet, ParentRoute, Route, Router, Routes};
use leptos_router::{SsrMode, StaticSegment, path};

use crate::components::{Footer, Header, NotFound, provide_session};
use crate::pages::account::{AccountPage, UnsubscribePage};
use crate::pages::auth::{ForgotPasswordPage, ResetPasswordPage, SignInPage, VerifyEmailPage};
use crate::pages::catalog::{Browse, DeveloperPage, Discover, GameDetail};
use crate::pages::dashboard::{Dashboard, EditGame, NewGame};
use crate::pages::guidelines::Guidelines;
use crate::pages::review::{ReviewGamePage, ReviewQueue};
use crate::pages::tools::{ApiTokens, GameDownloads, GameTesters, TesterKeyInvalid};

/// Asks the browser to prerender a page as soon as the visitor shows intent
/// to open it (hovering a link for a moment, focusing it, or touching it),
/// so the click shows a finished, server-rendered page straight away.
///
/// Only pages are listed: routes with side effects (downloads, sign-in and
/// sign-out, tester keys, one-time email links) and plain files are left
/// out, and so is any link marked `rel="nofollow"` or `data-no-prerender`.
const SPECULATION_RULES: &str = r#"{
  "prerender": [{
    "where": {
      "and": [
        { "href_matches": { "pathname": "/*" } },
        { "not": { "href_matches": [
          { "pathname": "/download/*" },
          { "pathname": "/auth/*" },
          { "pathname": "/api/*" },
          { "pathname": "/tester" },
          { "pathname": "/install.*" },
          { "pathname": "/verify-email" },
          { "pathname": "/reset-password" },
          { "pathname": "/unsubscribe" },
          { "pathname": "/pkg/*" },
          { "pathname": "/fonts/*" }
        ] } },
        { "not": { "selector_matches": "[rel~=nofollow], [data-no-prerender], [download], [target]" } }
      ]
    },
    "eagerness": "moderate"
  }]
}"#;

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
                // Passkey buttons; plain JS, see public/passkey.js.
                <script src="/passkey.js" defer></script>
                // Links load server-rendered documents, see public/navigation.js.
                <script src="/navigation.js"></script>
                <script type="speculationrules" inner_html=SPECULATION_RULES></script>
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
            // Every page renders with `SsrMode::Async`: the server waits for the
            // page's data and sends the finished HTML in one go, so there is no
            // loading state to stream in or fill after the page arrives.
            <Routes fallback=NotFound>
                <ParentRoute path=StaticSegment("") view=Layout>
                    <Route path=StaticSegment("") view=Discover ssr=SsrMode::Async />
                    <Route path=path!("games") view=Browse ssr=SsrMode::Async />
                    <Route path=path!("games/:slug") view=GameDetail ssr=SsrMode::Async />
                    <Route path=path!("developers/:slug") view=DeveloperPage ssr=SsrMode::Async />
                    <Route path=path!("guidelines") view=Guidelines ssr=SsrMode::Async />
                    <Route path=path!("signin") view=SignInPage ssr=SsrMode::Async />
                    <Route path=path!("forgot-password") view=ForgotPasswordPage ssr=SsrMode::Async />
                    <Route path=path!("reset-password") view=ResetPasswordPage ssr=SsrMode::Async />
                    <Route path=path!("verify-email") view=VerifyEmailPage ssr=SsrMode::Async />
                    <Route path=path!("account") view=AccountPage ssr=SsrMode::Async />
                    <Route path=path!("unsubscribe") view=UnsubscribePage ssr=SsrMode::Async />
                    <Route path=path!("review") view=ReviewQueue ssr=SsrMode::Async />
                    <Route path=path!("review/:slug") view=ReviewGamePage ssr=SsrMode::Async />
                    <Route path=path!("dashboard") view=Dashboard ssr=SsrMode::Async />
                    <Route path=path!("dashboard/games/new") view=NewGame ssr=SsrMode::Async />
                    <Route path=path!("dashboard/games/:slug") view=EditGame ssr=SsrMode::Async />
                    <Route path=path!("dashboard/games/:slug/downloads") view=GameDownloads ssr=SsrMode::Async />
                    <Route path=path!("dashboard/games/:slug/testers") view=GameTesters ssr=SsrMode::Async />
                    <Route path=path!("dashboard/tokens") view=ApiTokens ssr=SsrMode::Async />
                    <Route path=path!("tester-key") view=TesterKeyInvalid ssr=SsrMode::Async />
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
