//! The player's account: wishlist, followed developers, release emails and
//! passkeys. Also the page behind the unsubscribe link in release emails.

use leptos::prelude::*;
use leptos_meta::Title;
use leptos_router::hooks::use_query_map;

use crate::api::{RemovePasskey, SetFollowing, SetReleaseEmails, Unsubscribe, get_account};
use crate::components::*;
use crate::model::*;
use crate::pages::auth::VerifyEmailBanner;
use crate::pages::dashboard::RequireSession;

#[component]
pub fn AccountPage() -> impl IntoView {
    view! {
        <Title text="Your account · Foyer" />
        <RequireSession
            reason="Your account keeps your wishlist, the developers you follow and your passkeys."
            children=|_| view! { <AccountInner /> }
        />
    }
}

#[component]
fn AccountInner() -> impl IntoView {
    let unfollow = ServerAction::<SetFollowing>::new();
    let emails = ServerAction::<SetReleaseEmails>::new();
    let remove_key = ServerAction::<RemovePasskey>::new();
    let account = Resource::new(
        move || {
            (
                unfollow.version().get(),
                emails.version().get(),
                remove_key.version().get(),
            )
        },
        |_| get_account(),
    );

    view! {
        <Suspense fallback=Loading>
            {move || Suspend::new(async move {
                match account.await {
                    Ok(page) => {
                        view! { <AccountView page unfollow emails remove_key /> }.into_any()
                    }
                    Err(e) => {
                        view! {
                            <PageShell>
                                <ErrorNote message=server_error_text(&e) />
                            </PageShell>
                        }
                            .into_any()
                    }
                }
            })}
        </Suspense>
    }
}

#[component]
fn AccountView(
    page: AccountPage,
    unfollow: ServerAction<SetFollowing>,
    emails: ServerAction<SetReleaseEmails>,
    remove_key: ServerAction<RemovePasskey>,
) -> impl IntoView {
    let AccountPage {
        user,
        has_developer,
        wishlist,
        following,
        settings,
        passkeys,
    } = page;
    let banner = (!user.email_verified).then(|| {
        view! { <VerifyEmailBanner email=user.email.clone() /> }
    });
    let wishlist_games: Vec<Game> = wishlist.into_iter().map(|e| e.game).collect();
    let releases_on = settings.release_emails;
    let verified = user.email_verified;

    view! {
        <PageShell>
            {banner}
            <header class="flex flex-col gap-2 border-b border-line pb-8 sm:flex-row sm:items-end sm:justify-between">
                <div>
                    <h1 class="font-display text-3xl font-semibold">"Your account"</h1>
                    <p class="mt-2 text-muted">
                        "Signed in as " <strong class="text-ink">{user.username.clone()}</strong>
                        " (" {user.email.clone()} ")"
                    </p>
                </div>
                <a href="/dashboard" class="btn-secondary self-start sm:self-auto">
                    {if has_developer { "Developer dashboard" } else { "Publish a game" }}
                </a>
            </header>

            <section class="mt-10" aria-labelledby="wishlist-h">
                <h2 id="wishlist-h" class="mb-4 font-display text-2xl font-semibold">
                    "Wishlist"
                </h2>
                {if wishlist_games.is_empty() {
                    view! {
                        <div class="card p-8 text-center">
                            <p class="text-muted">
                                "Games you add to your wishlist show up here."
                            </p>
                            <a href="/games" class="link mt-2 inline-block text-sm">
                                "Browse games"
                            </a>
                        </div>
                    }
                        .into_any()
                } else {
                    view! { <GameGrid games=wishlist_games /> }.into_any()
                }}
            </section>

            <div class="mt-12 grid gap-8 lg:grid-cols-2">
                <section class="card p-6" aria-labelledby="following-h">
                    <h2 id="following-h" class="font-display text-xl font-semibold">
                        "Following"
                    </h2>
                    {if following.is_empty() {
                        view! {
                            <p class="mt-2 text-sm text-muted">
                                "Follow developers from their page to hear when they release something new."
                            </p>
                        }
                            .into_any()
                    } else {
                        view! {
                            <ul class="mt-4 flex flex-col divide-y divide-line">
                                {following
                                    .into_iter()
                                    .map(|f| {
                                        let slug = f.developer.slug.clone();
                                        view! {
                                            <li class="flex items-center justify-between gap-4 py-2">
                                                <a href=format!("/developers/{slug}") class="link">
                                                    {f.developer.display_name}
                                                </a>
                                                <ActionForm action=unfollow>
                                                    <input type="hidden" name="slug" value=slug />
                                                    <input type="hidden" name="on" value="false" />
                                                    <button type="submit" class="text-sm text-muted hover:text-ink">
                                                        "Unfollow"
                                                    </button>
                                                </ActionForm>
                                            </li>
                                        }
                                    })
                                    .collect_view()}
                            </ul>
                        }
                            .into_any()
                    }}
                    <ActionError value=move || unfollow.value().get() />
                </section>

                <section class="card p-6" aria-labelledby="emails-h">
                    <h2 id="emails-h" class="font-display text-xl font-semibold">
                        "Release emails"
                    </h2>
                    <p class="mt-2 text-sm text-muted">
                        {if releases_on {
                            "On: we email you when a developer you follow releases a new game. Nothing else."
                        } else {
                            "Off. Turn them on to get an email when a developer you follow releases a new game. Nothing else."
                        }}
                    </p>
                    <Show when=move || !verified>
                        <p class="hint">"Emails only go to confirmed addresses, so confirm yours first."</p>
                    </Show>
                    <ActionForm action=emails attr:class="mt-4">
                        <input type="hidden" name="on" value=(!releases_on).to_string() />
                        <button
                            type="submit"
                            class=if releases_on { "btn-secondary" } else { "btn-primary" }
                            disabled=move || emails.pending().get()
                        >
                            {if releases_on { "Turn off" } else { "Turn on" }}
                        </button>
                    </ActionForm>
                    <ActionError value=move || emails.value().get() />
                </section>
            </div>

            <section class="card mt-8 p-6" aria-labelledby="passkeys-h">
                <h2 id="passkeys-h" class="font-display text-xl font-semibold">
                    "Passkeys"
                </h2>
                <p class="mt-2 max-w-prose text-sm text-muted">
                    "Sign in with your fingerprint, face or device PIN instead of a password. "
                    "Your password keeps working too."
                </p>
                {(!passkeys.is_empty())
                    .then(|| {
                        view! {
                            <ul class="mt-4 flex flex-col divide-y divide-line">
                                {passkeys
                                    .into_iter()
                                    .map(|k| {
                                        let used = match &k.last_used_at {
                                            Some(t) => format!("last used {}", date_only(t)),
                                            None => "never used".to_string(),
                                        };
                                        view! {
                                            <li class="flex items-center justify-between gap-4 py-2">
                                                <div>
                                                    <p>{k.name}</p>
                                                    <p class="text-xs text-faint">
                                                        {format!("Added {} · {used}", date_only(&k.created_at))}
                                                    </p>
                                                </div>
                                                <ActionForm action=remove_key>
                                                    <input type="hidden" name="id" value=k.id.to_string() />
                                                    <button type="submit" class="text-sm text-muted hover:text-ink">
                                                        "Remove"
                                                    </button>
                                                </ActionForm>
                                            </li>
                                        }
                                    })
                                    .collect_view()}
                            </ul>
                        }
                    })}
                <ActionError value=move || remove_key.value().get() />
                <div class="mt-4 flex max-w-md flex-col gap-2 sm:flex-row sm:items-end">
                    <div class="flex-1">
                        <label class="label" for="passkey-name">
                            "Name"
                        </label>
                        <input
                            id="passkey-name"
                            class="field"
                            maxlength="60"
                            placeholder="e.g. Laptop or Phone"
                        />
                    </div>
                    <button
                        type="button"
                        class="btn-primary"
                        data-passkey="register"
                        data-name-input="passkey-name"
                        data-status="passkey-status"
                    >
                        "Add a passkey"
                    </button>
                </div>
                <p id="passkey-status" class="hint" role="status" hidden></p>
            </section>
        </PageShell>
    }
}

/// The page behind the unsubscribe link in release emails. It asks before
/// unsubscribing, so link scanners that open every URL don't do it.
#[component]
pub fn UnsubscribePage() -> impl IntoView {
    let query = use_query_map();
    let token = move || query.read().get("token").unwrap_or_default();
    let action = ServerAction::<Unsubscribe>::new();
    let done = move || matches!(action.value().get(), Some(Ok(())));

    view! {
        <Title text="Unsubscribe · Foyer" />
        <PageShell>
            <div class="card mx-auto max-w-md p-8">
                <h1 class="font-display text-2xl font-semibold">"Stop release emails"</h1>
                <Show
                    when=done
                    fallback=move || {
                        view! {
                            <p class="mt-2 text-muted">
                                "You won't get emails when developers you follow release games. You still follow them."
                            </p>
                            <ActionForm action attr:class="mt-6 flex flex-col gap-4">
                                <input type="hidden" name="token" value=token />
                                <ActionError value=move || action.value().get() />
                                <button
                                    type="submit"
                                    class="btn-primary"
                                    disabled=move || action.pending().get()
                                >
                                    "Unsubscribe"
                                </button>
                            </ActionForm>
                        }
                    }
                >
                    <p role="status" class="mt-2 text-muted">
                        "Done. You can turn release emails back on from your "
                        <a href="/account" class="link">
                            "account"
                        </a>
                        "."
                    </p>
                </Show>
            </div>
        </PageShell>
    }
}
