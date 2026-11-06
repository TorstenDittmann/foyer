//! Sign in and create account.

use leptos::prelude::*;
use leptos_meta::Title;
use leptos_router::hooks::use_query_map;

use crate::api::{Register, SignIn};
use crate::components::*;

#[component]
pub fn SignInPage() -> impl IntoView {
    let query = use_query_map();
    let next = move || {
        query
            .read()
            .get("next")
            .unwrap_or_else(|| "/dashboard".into())
    };
    let sign_in = ServerAction::<SignIn>::new();
    let register = ServerAction::<Register>::new();

    view! {
        <Title text="Sign in · Foyer" />
        <PageShell>
            <div class="mx-auto max-w-4xl">
                <h1 class="font-display text-3xl font-semibold">"Sign in to Foyer"</h1>
                <p class="mt-2 max-w-prose text-muted">
                    "Developers sign in to create game pages and send them for review. "
                    "Players will be able to keep a library and follow developers."
                </p>
                <div class="mt-10 grid gap-8 md:grid-cols-2">
                    <section class="card p-6" aria-labelledby="signin-h">
                        <h2 id="signin-h" class="mb-4 font-display text-xl font-semibold">
                            "Sign in"
                        </h2>
                        <ActionForm action=sign_in attr:class="flex flex-col gap-4">
                            <input type="hidden" name="next" prop:value=next />
                            <div>
                                <label class="label" for="signin-email">
                                    "Email"
                                </label>
                                <input
                                    id="signin-email"
                                    class="field"
                                    type="email"
                                    name="email"
                                    autocomplete="email"
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
                        </ActionForm>
                        <p class="hint mt-4">
                            "Trying it out? Use "
                            <code class="text-ink">{crate::DEMO_EMAIL}</code>
                            " with password "
                            <code class="text-ink">{crate::DEMO_PASSWORD}</code>
                            "."
                        </p>
                    </section>
                    <section class="card p-6" aria-labelledby="register-h">
                        <h2 id="register-h" class="mb-4 font-display text-xl font-semibold">
                            "Create an account"
                        </h2>
                        <ActionForm action=register attr:class="flex flex-col gap-4">
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
                                    minlength="8"
                                    required
                                />
                                <p class="hint">"At least 8 characters."</p>
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
