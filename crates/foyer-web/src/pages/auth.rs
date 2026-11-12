//! Sign in, create account, email verification and password reset.

use leptos::prelude::*;
use leptos_meta::Title;
use leptos_router::components::A;
use leptos_router::hooks::use_query_map;

use crate::api::{
    Register, RequestPasswordReset, ResendVerification, ResetPassword, SignIn, is_mock,
    verify_email,
};
use crate::components::*;

#[component]
pub fn SignInPage() -> impl IntoView {
    let query = use_query_map();
    let next = move || {
        query
            .read()
            .get("next")
            .unwrap_or_else(|| "/account".into())
    };
    let sign_in = ServerAction::<SignIn>::new();
    let register = ServerAction::<Register>::new();
    let mock = Resource::new(|| (), |_| is_mock());
    // Set by the Discord redirect and the password reset form.
    let notice = move || {
        let q = query.read();
        if q.get("reset").is_some() {
            return Some(
                view! {
                    <p role="status" class="card mt-6 px-4 py-3 text-sm text-good">
                        "Your password was changed. Sign in with the new one."
                    </p>
                }
                .into_any(),
            );
        }
        let text = match q.get("error").as_deref() {
            Some("discord_off") => "Discord sign-in isn't set up on this site.",
            Some(_) => "Discord sign-in didn't go through. Try again.",
            None => return None,
        };
        Some(view! { <div class="mt-6"><ErrorNote message=text /></div> }.into_any())
    };

    view! {
        <Title text="Sign in · Foyer" />
        <PageShell>
            <div class="mx-auto max-w-4xl">
                <h1 class="font-display text-3xl font-semibold">"Sign in to Foyer"</h1>
                <p class="mt-2 max-w-prose text-muted">
                    "You never need an account to download games. With one, players keep a wishlist "
                    "and follow developers, and developers create game pages and send them for review."
                </p>
                {notice}
                <div class="mt-8 flex max-w-sm flex-col gap-3">
                    // Handled by public/passkey.js.
                    <button
                        type="button"
                        class="btn-secondary w-full"
                        data-passkey="login"
                        data-next=next
                        data-status="passkey-login-status"
                    >
                        "Sign in with a passkey"
                    </button>
                    <p id="passkey-login-status" class="hint mt-0" role="status" hidden></p>
                    // A plain link: `/auth/discord` is a server redirect, not a page.
                    <a href="/auth/discord" rel="external" class="btn-secondary w-full">
                        "Continue with Discord"
                    </a>
                </div>
                <div class="mt-10 grid gap-8 md:grid-cols-2">
                    <section class="card p-6" aria-labelledby="signin-h">
                        <h2 id="signin-h" class="mb-4 font-display text-xl font-semibold">
                            "Sign in"
                        </h2>
                        <ActionForm action=sign_in attr:class="flex flex-col gap-4">
                            <input type="hidden" name="next" value=next />
                            <div>
                                <label class="label" for="signin-login">
                                    "Email or username"
                                </label>
                                <input
                                    id="signin-login"
                                    class="field"
                                    name="login"
                                    autocomplete="username"
                                    required
                                />
                            </div>
                            <div>
                                <label class="label" for="signin-password">
                                    "Password"
                                </label>
                                <input
                                    id="signin-password"
                                    class="field"
                                    type="password"
                                    name="password"
                                    autocomplete="current-password"
                                    required
                                />
                            </div>
                            <ActionError value=move || sign_in.value().get() />
                            <button type="submit" class="btn-primary" disabled=move || sign_in.pending().get()>
                                "Sign in"
                            </button>
                            <A href="/forgot-password" attr:class="link text-sm">
                                "Forgot your password?"
                            </A>
                        </ActionForm>
                        <Transition fallback=|| ()>
                            {move || Suspend::new(async move {
                                (mock.await == Ok(true))
                                    .then(|| {
                                        view! {
                                            <p class="hint mt-4">
                                                "Running on sample data. Try "
                                                <code class="text-ink">{crate::DEMO_EMAIL}</code>
                                                " with password "
                                                <code class="text-ink">{crate::DEMO_PASSWORD}</code>
                                                "."
                                            </p>
                                        }
                                    })
                            })}
                        </Transition>
                    </section>
                    <section class="card p-6" aria-labelledby="register-h">
                        <h2 id="register-h" class="mb-4 font-display text-xl font-semibold">
                            "Create an account"
                        </h2>
                        <ActionForm action=register attr:class="flex flex-col gap-4">
                            <input type="hidden" name="next" value=next />
                            <div>
                                <label class="label" for="reg-username">
                                    "Username"
                                </label>
                                <input
                                    id="reg-username"
                                    class="field"
                                    name="username"
                                    autocomplete="username"
                                    required
                                />
                                <p class="hint">"3 to 32 lowercase letters, digits, - or _."</p>
                            </div>
                            <div>
                                <label class="label" for="reg-email">
                                    "Email"
                                </label>
                                <input
                                    id="reg-email"
                                    class="field"
                                    type="email"
                                    name="email"
                                    autocomplete="email"
                                    required
                                />
                            </div>
                            <div>
                                <label class="label" for="reg-password">
                                    "Password"
                                </label>
                                <input
                                    id="reg-password"
                                    class="field"
                                    type="password"
                                    name="password"
                                    autocomplete="new-password"
                                    minlength="10"
                                    required
                                />
                                <p class="hint">"At least 10 characters."</p>
                            </div>
                            <ActionError value=move || register.value().get() />
                            <button
                                type="submit"
                                class="btn-secondary"
                                disabled=move || register.pending().get()
                            >
                                "Create account"
                            </button>
                        </ActionForm>
                    </section>
                </div>
            </div>
        </PageShell>
    }
}

#[component]
pub fn ForgotPasswordPage() -> impl IntoView {
    let action = ServerAction::<RequestPasswordReset>::new();
    let sent = move || matches!(action.value().get(), Some(Ok(())));

    view! {
        <Title text="Reset your password · Foyer" />
        <PageShell>
            <div class="card mx-auto max-w-md p-8">
                <h1 class="font-display text-2xl font-semibold">"Reset your password"</h1>
                <Show
                    when=sent
                    fallback=move || {
                        view! {
                            <p class="mt-2 text-muted">
                                "Enter the email of your account and we'll send you a link to choose a new password."
                            </p>
                            <ActionForm action=action attr:class="mt-6 flex flex-col gap-4">
                                <div>
                                    <label class="label" for="reset-email">
                                        "Email"
                                    </label>
                                    <input
                                        id="reset-email"
                                        class="field"
                                        type="email"
                                        name="email"
                                        autocomplete="email"
                                        required
                                    />
                                </div>
                                <ActionError value=move || action.value().get() />
                                <button
                                    type="submit"
                                    class="btn-primary"
                                    disabled=move || action.pending().get()
                                >
                                    "Send link"
                                </button>
                            </ActionForm>
                        }
                    }
                >
                    <p role="status" class="mt-2 text-muted">
                        "If an account uses that email, a link is on its way. It works for one hour."
                    </p>
                </Show>
            </div>
        </PageShell>
    }
}

#[component]
pub fn ResetPasswordPage() -> impl IntoView {
    let query = use_query_map();
    let token = move || query.read().get("token").unwrap_or_default();
    let action = ServerAction::<ResetPassword>::new();

    view! {
        <Title text="Choose a new password · Foyer" />
        <PageShell>
            <div class="card mx-auto max-w-md p-8">
                <h1 class="font-display text-2xl font-semibold">"Choose a new password"</h1>
                <ActionForm action=action attr:class="mt-6 flex flex-col gap-4">
                    <input type="hidden" name="token" value=token />
                    <div>
                        <label class="label" for="new-password">
                            "New password"
                        </label>
                        <input
                            id="new-password"
                            class="field"
                            type="password"
                            name="password"
                            autocomplete="new-password"
                            minlength="10"
                            required
                        />
                        <p class="hint">"At least 10 characters. This signs you out everywhere."</p>
                    </div>
                    <ActionError value=move || action.value().get() />
                    <button type="submit" class="btn-primary" disabled=move || action.pending().get()>
                        "Change password"
                    </button>
                </ActionForm>
            </div>
        </PageShell>
    }
}

#[component]
pub fn VerifyEmailPage() -> impl IntoView {
    let query = use_query_map();
    let token = move || query.read().get("token").unwrap_or_default();
    let result = Resource::new(token, verify_email);

    view! {
        <Title text="Confirm your email · Foyer" />
        <PageShell>
            <div class="card mx-auto max-w-md p-8 text-center">
                <Transition fallback=Loading>
                    {move || Suspend::new(async move {
                        match result.await {
                            Ok(()) => {
                                view! {
                                    <h1 class="font-display text-2xl font-semibold">
                                        "Email confirmed"
                                    </h1>
                                    <p class="mt-2 text-muted">
                                        "Thanks. You can now send games for review and get release emails."
                                    </p>
                                    <A href="/account" attr:class="btn-primary mt-6">
                                        "Go to your account"
                                    </A>
                                }
                                    .into_any()
                            }
                            Err(e) => {
                                view! {
                                    <h1 class="font-display text-2xl font-semibold">
                                        "That link didn't work"
                                    </h1>
                                    <div class="mt-4">
                                        <ErrorNote message=server_error_text(&e) />
                                    </div>
                                    <p class="mt-4 text-muted">
                                        "Links work once and for 48 hours. Sign in to get a new one from your account."
                                    </p>
                                }
                                    .into_any()
                            }
                        }
                    })}
                </Transition>
            </div>
        </PageShell>
    }
}

/// Asks a signed-in user to confirm their email, with a button to send the
/// link again.
#[component]
pub fn VerifyEmailBanner(email: String) -> impl IntoView {
    let resend = ServerAction::<ResendVerification>::new();
    let sent = move || matches!(resend.value().get(), Some(Ok(())));
    view! {
        <div role="status" class="card mb-8 flex flex-col gap-3 border border-warn/40 p-4 sm:flex-row sm:items-center">
            <p class="flex-1 text-sm">
                "Confirm your email to send games for review and get release emails. We sent a link to "
                <strong>{email}</strong>
                "."
            </p>
            <Show
                when=sent
                fallback=move || {
                    view! {
                        <ActionForm action=resend>
                            <button
                                type="submit"
                                class="btn-secondary py-1.5 text-sm"
                                disabled=move || resend.pending().get()
                            >
                                "Send it again"
                            </button>
                        </ActionForm>
                    }
                }
            >
                <p class="text-sm text-good">"Sent."</p>
            </Show>
            <ActionError value=move || resend.value().get() />
        </div>
    }
}
