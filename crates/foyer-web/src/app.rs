use leptos::prelude::*;
use leptos_meta::{MetaTags, Stylesheet, Title, provide_meta_context};
use leptos_router::components::{Outlet, ParentRoute, Route, Router, Routes};
use leptos_router::{SsrMode, StaticSegment, path};

use crate::components::{Footer, Header, NotFound, provide_session};
use crate::pages::account::{AccountPage, UnsubscribePage};
use crate::pages::app_download::GetTheApp;
use crate::pages::auth::{ForgotPasswordPage, ResetPasswordPage, SignInPage, VerifyEmailPage};
use crate::pages::catalog::{Browse, DeveloperPage, Discover, GameDetail};
use crate::pages::dashboard::{Dashboard, EditGame, NewGame};
use crate::pages::design::{DesignGame, DesignPreview};
use crate::pages::guidelines::Guidelines;
use crate::pages::review::{ReviewGamePage, ReviewQueue};
use crate::pages::tools::{ApiTokens, GameDownloads, GameTesters, TesterKeyInvalid};

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
                    // Async so a missing game answers 404 and crawlers get the full page.
                    <Route path=path!("games/:slug") view=GameDetail ssr=SsrMode::Async />
                    <Route path=path!("developers/:slug") view=DeveloperPage ssr=SsrMode::Async />
                    <Route path=path!("guidelines") view=Guidelines />
                    <Route path=path!("app") view=GetTheApp />
                    <Route path=path!("signin") view=SignInPage />
                    <Route path=path!("forgot-password") view=ForgotPasswordPage />
                    <Route path=path!("reset-password") view=ResetPasswordPage />
                    <Route path=path!("verify-email") view=VerifyEmailPage />
                    <Route path=path!("account") view=AccountPage />
                    <Route path=path!("unsubscribe") view=UnsubscribePage />
                    <Route path=path!("review") view=ReviewQueue />
                    <Route path=path!("review/:slug") view=ReviewGamePage />
                    <Route path=path!("dashboard") view=Dashboard />
                    <Route path=path!("dashboard/games/new") view=NewGame />
                    <Route path=path!("dashboard/games/:slug") view=EditGame />
                    <Route path=path!("dashboard/games/:slug/downloads") view=GameDownloads />
                    <Route path=path!("dashboard/games/:slug/testers") view=GameTesters />
                    <Route path=path!("dashboard/games/:slug/design") view=DesignGame />
                    <Route path=path!("dashboard/games/:slug/preview") view=DesignPreview />
                    <Route path=path!("dashboard/tokens") view=ApiTokens />
                    <Route path=path!("tester-key") view=TesterKeyInvalid />
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
