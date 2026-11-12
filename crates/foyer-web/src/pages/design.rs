//! The "Page design" tab: how a developer makes their game page look like
//! their game. Colours, hero and logo art, and the order and content of the
//! page's sections.
//!
//! Everything works as plain forms, so it renders on the server and needs no
//! client-side loading. Colours are checked for contrast on save (the rules
//! are in `foyer_types::page`), and the readability panel shows the ratios
//! of the saved design.

use leptos::prelude::*;
use leptos_router::components::A;
use leptos_meta::Title;
use leptos_router::hooks::{use_params_map, use_query_map};

use crate::api::{
    ArrangePageBlock, DeletePageImage, ResetPageDesign, SavePageBlock, SavePageLook,
    get_page_preview, my_game,
};
use crate::components::*;
use crate::model::*;
use crate::pages::catalog::GameView;
use crate::pages::dashboard::RequireSession;
use crate::pages::tools::GameTabs;

#[component]
pub fn DesignGame() -> impl IntoView {
    view! {
        <Title text="Page design · Foyer" />
        <RequireSession children=|_| view! { <DesignInner /> } />
    }
}

#[component]
fn DesignInner() -> impl IntoView {
    let params = use_params_map();
    let query = use_query_map();
    let slug = move || params.read().get("slug").unwrap_or_default();
    // Keyed on the query string too, so it refetches after a save redirects
    // back here.
    let game = Resource::new(
        move || {
            (
                slug(),
                query.read().get("saved"),
                query.read().get("uploaded"),
            )
        },
        |(slug, _, _)| my_game(slug),
    );
    let flash = move || {
        let q = query.read();
        if q.get("uploaded").is_some() {
            Some(Ok(
                "Image uploaded. Pick it as the hero, logo or an image block below.",
            ))
        } else if q.get("saved").is_some() {
            Some(Ok("Saved."))
        } else {
            q.get("error").map(|e| {
                Err(match e.as_str() {
                    "too_big" => "That image is too big. Images can be up to 8 MB.",
                    "type" => "That file isn't a PNG, JPEG, WebP or AVIF image.",
                    "no_file" => "Choose an image to upload first.",
                    "limit" => "This game has as many images of that kind as it can hold. Delete one first.",
                    _ => "The image couldn't be uploaded. Try again in a moment.",
                })
            })
        }
    };

    view! {
        <PageShell>
            <A href="/dashboard" attr:class="link text-sm">
                "← Dashboard"
            </A>
            {move || {
                flash()
                    .map(|m| match m {
                        Ok(text) => view! {
                            <p
                                role="status"
                                class="mt-4 rounded-control border border-good/40 bg-good/10 px-4 py-3 text-sm text-good"
                            >
                                {text}
                            </p>
                        }
                        .into_any(),
                        Err(text) => view! {
                            <div class="mt-4">
                                <ErrorNote message=text />
                            </div>
                        }
                        .into_any(),
                    })
            }}
            {move || Suspend::new(async move {
                match game.await {
                    Ok(item) => view! { <DesignView game=item.game /> }.into_any(),
                    Err(e) => view! {
                        <div class="mt-6">
                            <ErrorNote message=server_error_text(&e) />
                        </div>
                    }
                    .into_any(),
                }
            })}
        </PageShell>
    }
}

#[component]
fn DesignView(game: Game) -> impl IntoView {
    let editable = is_editable(game.status);
    let preview_href = format!("/dashboard/games/{}/preview", game.slug);

    view! {
        <div class="mt-2 flex flex-wrap items-center gap-3">
            <h1 class="font-display text-3xl font-semibold">{game.title.clone()}</h1>
            <StatusBadge status=game.status />
            <A href=preview_href attr:class="link text-sm">
                "Preview the page"
            </A>
        </div>
        <GameTabs slug=game.slug.clone() active="design" />
        <p class="mt-6 max-w-prose text-muted">
            "Make the page feel like your game. Foyer works out the text and panel colours from your choices so the page stays readable for everyone, and refuses colours that can't be."
        </p>
        <div class="mt-8 grid gap-10 lg:grid-cols-[1fr_20rem]">
            <div class="flex flex-col gap-8">
                {if editable {
                    view! {
                        <LookEditor game=game.clone() />
                        <ImagesEditor game=game.clone() />
                        <LayoutEditor game=game.clone() />
                        <ResetDesign slug=game.slug.clone() />
                    }
                        .into_any()
                } else {
                    view! {
                        <div class="card p-6 text-muted">
                            "The page can't be changed right now: it's in review, rejected or delisted."
                        </div>
                    }
                        .into_any()
                }}
            </div>
            <aside class="lg:sticky lg:top-6 lg:self-start">
                <Readability page=game.page.clone() title=game.title.clone() />
            </aside>
        </div>
    }
}

fn section_heading(title: &'static str, hint: &'static str) -> impl IntoView {
    view! {
        <div class="mb-4">
            <h2 class="font-display text-xl font-semibold">{title}</h2>
            <p class="hint">{hint}</p>
        </div>
    }
}

/// `<option>`s for picking one of the game's images.
fn image_options(images: &[Media], selected: Option<uuid::Uuid>) -> AnyView {
    images
        .iter()
        .map(|m| {
            view! {
                <option value=m.id.to_string() selected=Some(m.id) == selected>
                    {media_label(m)}
                </option>
            }
        })
        .collect_view()
        .into_any()
}

/// Images that can appear in the design: artwork first, then screenshots.
fn design_images(game: &Game) -> Vec<Media> {
    artwork(game).chain(screenshots(game)).cloned().collect()
}

/// A colour picker with a checkbox that says whether to use it.
fn colour_field(
    name: &'static str,
    check: &'static str,
    label: &'static str,
    hint: &'static str,
    value: Option<String>,
    fallback: Rgb,
) -> impl IntoView {
    let on = value.is_some();
    let value = value.unwrap_or_else(|| fallback.hex());
    view! {
        <div>
            <label class="flex items-center gap-2 text-sm font-medium">
                <input type="checkbox" name=check checked=on class="size-4 accent-accent" />
                {label}
            </label>
            <div class="mt-2 flex items-center gap-3">
                <input
                    type="color"
                    name=name
                    value=value
                    class="h-9 w-14 cursor-pointer rounded-control border border-line bg-canvas"
                    aria-label=format!("{label}: colour")
                />
                <p class="text-sm text-faint">{hint}</p>
            </div>
        </div>
    }
}

#[component]
fn LookEditor(game: Game) -> impl IntoView {
    let action = ServerAction::<SavePageLook>::new();
    let page = game.page.clone();
    let palette = Palette::for_style(&page);
    let art: Vec<Media> = artwork(&game).cloned().collect();
    let images = design_images(&game);
    view! {
        <ActionForm action attr:class="card flex flex-col gap-6 p-6">
            {section_heading(
                "Colours and title",
                "Untick a colour to use Foyer's default. The accent colours the download button, focus outlines and highlights.",
            )}
            <input type="hidden" name="slug" value=game.slug.clone() />
            <div class="grid gap-6 sm:grid-cols-2">
                {colour_field(
                    "background",
                    "custom_background",
                    "Custom background",
                    "Dark or light both work.",
                    page.background.clone(),
                    palette.canvas,
                )}
                {colour_field(
                    "accent",
                    "custom_accent",
                    "Custom accent",
                    "Must stand out from the background.",
                    page.accent.clone(),
                    palette.accent,
                )}
            </div>
            <div class="grid gap-6 sm:grid-cols-3">
                <div>
                    <label class="label" for="d-hero">
                        "Hero image"
                    </label>
                    <select id="d-hero" name="hero" class="field">
                        <option value="">"None"</option>
                        {image_options(&images, page.hero)}
                    </select>
                    <p class="hint">"Wide art across the top. 1920×640 or larger works best."</p>
                </div>
                <div>
                    <label class="label" for="d-logo">
                        "Logo"
                    </label>
                    <select id="d-logo" name="logo" class="field">
                        <option value="">"None: show the title"</option>
                        {image_options(&art, page.logo)}
                    </select>
                    <p class="hint">"A transparent PNG reads best on any background."</p>
                </div>
                <div>
                    <label class="label" for="d-title">
                        "Title style"
                    </label>
                    <select id="d-title" name="title_style" class="field">
                        <option value="standard" selected=page.title_style == TitleStyle::Standard>
                            "Standard"
                        </option>
                        <option value="uppercase" selected=page.title_style == TitleStyle::Uppercase>
                            "Big uppercase"
                        </option>
                    </select>
                    <p class="hint">"Used when there's no logo."</p>
                </div>
            </div>
            <ActionError value=move || action.value().get() />
            <div>
                <button type="submit" class="btn-primary" disabled=move || action.pending().get()>
                    "Save colours and title"
                </button>
            </div>
        </ActionForm>
    }
}

#[component]
fn ImagesEditor(game: Game) -> impl IntoView {
    let images = design_images(&game);
    let action = format!("/dashboard/games/{}/images", game.slug);
    let slug = game.slug.clone();
    let has_images = !images.is_empty();

    view! {
        <section class="card p-6">
            {section_heading(
                "Images",
                "Artwork is for the hero, the logo and image blocks. Screenshots also appear in the screenshot gallery. PNG, JPEG, WebP or AVIF, up to 8 MB.",
            )}
            // A plain form: the website server sends the file on to storage.
            <form
                method="post"
                action=action
                enctype="multipart/form-data"
                class="flex flex-wrap items-end gap-3"
            >
                <div class="min-w-48 flex-1">
                    <label class="label" for="d-file">
                        "Image"
                    </label>
                    <input
                        id="d-file"
                        type="file"
                        name="image"
                        accept="image/png,image/jpeg,image/webp,image/avif"
                        required
                        class="field file:mr-3 file:rounded-control file:border-0 file:bg-raised file:px-3 file:py-1 file:text-ink"
                    />
                </div>
                <div>
                    <label class="label" for="d-kind">
                        "Use as"
                    </label>
                    <select id="d-kind" name="kind" class="field">
                        <option value="artwork">"Artwork"</option>
                        <option value="screenshot">"Screenshot"</option>
                    </select>
                </div>
                <button type="submit" class="btn-secondary">
                    "Upload"
                </button>
            </form>
            <Show when=move || has_images>
                <ul class="mt-6 grid grid-cols-2 gap-4 sm:grid-cols-3">
                    {images
                        .clone()
                        .into_iter()
                        .map(|m| view! { <ImageTile media=m slug=slug.clone() /> })
                        .collect_view()}
                </ul>
            </Show>
        </section>
    }
}

#[component]
fn ImageTile(media: Media, slug: String) -> impl IntoView {
    let action = ServerAction::<DeletePageImage>::new();
    let label = media_label(&media);
    view! {
        <li class="flex flex-col gap-2">
            <GameImage
                media=Some(media.clone())
                seed=media.id.to_string()
                alt=label.clone()
                class="aspect-video w-full rounded-control bg-raised"
            />
            <div class="flex items-center justify-between gap-2 text-sm">
                <span class="text-muted">{label.clone()}</span>
                <ActionForm action>
                    <input type="hidden" name="slug" value=slug />
                    <input type="hidden" name="id" value=media.id.to_string() />
                    <button
                        type="submit"
                        class="text-sm text-faint underline underline-offset-4 hover:text-bad"
                        aria-label=format!("Delete {label}")
                    >
                        "Delete"
                    </button>
                </ActionForm>
            </div>
            <ActionError value=move || action.value().get() />
        </li>
    }
}

const BLOCK_KINDS: [(&str, &str, &str); 7] = [
    (
        "text",
        "Text section",
        "A heading and paragraphs: story, controls, credits.",
    ),
    ("features", "Feature list", "Short bullets, one per line."),
    ("quote", "Quote", "Something a reviewer or player said."),
    (
        "image",
        "Image",
        "One of your images, full width, with a caption.",
    ),
    ("trailer", "Trailer", "A YouTube or Vimeo video."),
    ("screenshots", "Screenshots", "The screenshot gallery."),
    ("about", "About this game", "Your game's description."),
];

fn kind_label(kind: &str) -> &'static str {
    BLOCK_KINDS
        .iter()
        .find(|(k, _, _)| *k == kind)
        .map(|(_, label, _)| *label)
        .unwrap_or("Block")
}

/// A short reminder of what a block holds, for its row.
fn block_summary(block: &PageBlock) -> String {
    let cut = |s: &str| {
        let s = s.lines().next().unwrap_or_default();
        if s.chars().count() > 60 {
            format!("{}…", s.chars().take(60).collect::<String>())
        } else {
            s.to_string()
        }
    };
    match block {
        PageBlock::Screenshots => "Your screenshots".into(),
        PageBlock::About => "From the Game page tab".into(),
        PageBlock::Text { heading, body } => cut(if heading.is_empty() { body } else { heading }),
        PageBlock::Features { heading, items } => {
            format!("{} · {} items", cut(heading), items.len())
        }
        PageBlock::Quote { text, .. } => cut(text),
        PageBlock::Image { caption, .. } => cut(caption),
        PageBlock::Trailer { url } => cut(url),
    }
}

/// The inputs for one kind of block, filled from `block` when editing.
fn block_fields(kind: &'static str, block: Option<&PageBlock>, images: &[Media]) -> AnyView {
    let id = |name: &str| format!("b-{kind}-{name}-{}", block.map_or(0, |_| 1));
    match (kind, block) {
        ("text", b) => {
            let (heading, body) = match b {
                Some(PageBlock::Text { heading, body }) => (heading.clone(), body.clone()),
                _ => Default::default(),
            };
            view! {
                <div>
                    <label class="label" for=id("heading")>"Heading"</label>
                    <input id=id("heading") class="field" name="heading" value=heading maxlength="80" placeholder="Story" />
                </div>
                <div>
                    <label class="label" for=id("body")>"Text"</label>
                    <textarea id=id("body") class="field" name="body" rows="5" required>{body}</textarea>
                    <p class="hint">"Plain text. Leave a blank line between paragraphs."</p>
                </div>
            }
            .into_any()
        }
        ("features", b) => {
            let (heading, items) = match b {
                Some(PageBlock::Features { heading, items }) => (heading.clone(), items.join("\n")),
                _ => ("Features".to_string(), String::new()),
            };
            view! {
                <div>
                    <label class="label" for=id("heading")>"Heading"</label>
                    <input id=id("heading") class="field" name="heading" value=heading maxlength="80" />
                </div>
                <div>
                    <label class="label" for=id("items")>"Features"</label>
                    <textarea id=id("items") class="field" name="items" rows="5" required>{items}</textarea>
                    <p class="hint">"One per line, up to 12."</p>
                </div>
            }
            .into_any()
        }
        ("quote", b) => {
            let (text, source) = match b {
                Some(PageBlock::Quote { text, source }) => (text.clone(), source.clone()),
                _ => Default::default(),
            };
            view! {
                <div>
                    <label class="label" for=id("text")>"Quote"</label>
                    <textarea id=id("text") class="field" name="text" rows="3" maxlength="400" required>{text}</textarea>
                </div>
                <div>
                    <label class="label" for=id("source")>"Who said it"</label>
                    <input id=id("source") class="field" name="source" value=source maxlength="80" placeholder="A playtester" />
                </div>
            }
            .into_any()
        }
        ("image", b) => {
            let (media, caption) = match b {
                Some(PageBlock::Image { media, caption }) => (Some(*media), caption.clone()),
                _ => (None, String::new()),
            };
            view! {
                <div>
                    <label class="label" for=id("media")>"Image"</label>
                    <select id=id("media") class="field" name="media" required>
                        <option value="">"Pick an image"</option>
                        {image_options(images, media)}
                    </select>
                </div>
                <div>
                    <label class="label" for=id("caption")>"Caption"</label>
                    <input id=id("caption") class="field" name="caption" value=caption maxlength="160" />
                    <p class="hint">"Also read out to screen reader users, so describe what's shown."</p>
                </div>
            }
            .into_any()
        }
        ("trailer", b) => {
            let url = match b {
                Some(PageBlock::Trailer { url }) => url.clone(),
                _ => String::new(),
            };
            view! {
                <div>
                    <label class="label" for=id("url")>"Video link"</label>
                    <input id=id("url") class="field" name="url" type="url" value=url required placeholder="https://www.youtube.com/watch?v=…" />
                    <p class="hint">"YouTube or Vimeo. It plays from YouTube's no-cookie domain."</p>
                </div>
            }
            .into_any()
        }
        _ => ().into_any(),
    }
}

#[component]
fn LayoutEditor(game: Game) -> impl IntoView {
    let images = design_images(&game);
    let blocks = game.page.layout();
    let count = blocks.len();
    let slug = game.slug.clone();
    let present: Vec<&'static str> = blocks.iter().map(|b| b.kind()).collect();

    view! {
        <section class="card p-6">
            {section_heading(
                "Layout",
                "The main column of your page, top to bottom. The download box and details stay in the side column.",
            )}
            <ol class="flex flex-col gap-3">
                {blocks
                    .into_iter()
                    .enumerate()
                    .map(|(index, block)| {
                        view! {
                            <BlockRow
                                slug=slug.clone()
                                index
                                count
                                block
                                images=images.clone()
                            />
                        }
                    })
                    .collect_view()}
            </ol>
            <h3 class="mt-8 text-sm font-semibold">"Add a block"</h3>
            <div class="mt-3 flex flex-col gap-2">
                {BLOCK_KINDS
                    .into_iter()
                    .filter(|(kind, _, _)| !matches!(*kind, "screenshots" | "about") || !present.contains(kind))
                    .map(|(kind, label, hint)| {
                        view! { <AddBlock slug=slug.clone() kind label hint images=images.clone() /> }
                    })
                    .collect_view()}
            </div>
        </section>
    }
}

#[component]
fn BlockRow(
    slug: String,
    index: usize,
    count: usize,
    block: PageBlock,
    images: Vec<Media>,
) -> impl IntoView {
    let save = ServerAction::<SavePageBlock>::new();
    let arrange = ServerAction::<ArrangePageBlock>::new();
    let kind = block.kind();
    let label = kind_label(kind);
    let summary = block_summary(&block);
    let editable = !matches!(block, PageBlock::Screenshots | PageBlock::About);
    let arrange_slug = slug.clone();
    let arrange_button = move |op: &'static str,
                               text: &'static str,
                               aria: String,
                               disabled: bool| {
        let slug = arrange_slug.clone();
        view! {
            <ActionForm action=arrange>
                <input type="hidden" name="slug" value=slug />
                <input type="hidden" name="index" value=index.to_string() />
                <input type="hidden" name="op" value=op />
                <button type="submit" class="btn-secondary px-2.5 py-1.5" aria-label=aria disabled=disabled>
                    {text}
                </button>
            </ActionForm>
        }
    };
    let position = format!("{label}, block {} of {count}", index + 1);

    view! {
        <li class="rounded-control bg-raised p-3">
            <div class="flex items-center gap-3">
                <span class="w-6 text-center font-mono text-sm text-faint">{index + 1}</span>
                <div class="min-w-0 flex-1">
                    <p class="text-sm font-semibold">{label}</p>
                    <p class="truncate text-sm text-muted">{summary}</p>
                </div>
                <div class="flex gap-1">
                    {arrange_button("up", "↑", format!("Move up: {position}"), index == 0)}
                    {arrange_button("down", "↓", format!("Move down: {position}"), index + 1 == count)}
                    {arrange_button("remove", "Remove", format!("Remove: {position}"), false)}
                </div>
            </div>
            {editable
                .then(|| {
                    view! {
                        <details class="mt-3">
                            <summary class="cursor-pointer text-sm text-muted hover:text-ink">"Edit"</summary>
                            <ActionForm action=save attr:class="mt-3 flex flex-col gap-4">
                                <input type="hidden" name="slug" value=slug.clone() />
                                <input type="hidden" name="index" value=index.to_string() />
                                <input type="hidden" name="kind" value=kind />
                                {block_fields(kind, Some(&block), &images)}
                                <ActionError value=move || save.value().get() />
                                <div>
                                    <button type="submit" class="btn-secondary" disabled=move || save.pending().get()>
                                        "Save block"
                                    </button>
                                </div>
                            </ActionForm>
                        </details>
                    }
                })}
            <ActionError value=move || arrange.value().get() />
        </li>
    }
}

#[component]
fn AddBlock(
    slug: String,
    kind: &'static str,
    label: &'static str,
    hint: &'static str,
    images: Vec<Media>,
) -> impl IntoView {
    let action = ServerAction::<SavePageBlock>::new();
    view! {
        <details class="rounded-control border border-line p-3">
            <summary class="cursor-pointer text-sm">
                <span class="font-semibold">{label}</span>
                <span class="text-faint">{format!(" · {hint}")}</span>
            </summary>
            <ActionForm action attr:class="mt-3 flex flex-col gap-4">
                <input type="hidden" name="slug" value=slug />
                <input type="hidden" name="kind" value=kind />
                {block_fields(kind, None, &images)}
                <ActionError value=move || action.value().get() />
                <div>
                    <button type="submit" class="btn-secondary" disabled=move || action.pending().get()>
                        {format!("Add {}", label.to_lowercase())}
                    </button>
                </div>
            </ActionForm>
        </details>
    }
}

#[component]
fn ResetDesign(slug: String) -> impl IntoView {
    let action = ServerAction::<ResetPageDesign>::new();
    view! {
        <ActionForm action attr:class="flex flex-wrap items-center gap-3 text-sm text-faint">
            <input type="hidden" name="slug" value=slug />
            <button type="submit" class="btn-danger">
                "Reset to the standard look"
            </button>
            "Clears colours, art choices and layout. Uploaded images are kept."
            <ActionError value=move || action.value().get() />
        </ActionForm>
    }
}

/// A small sample of the page in the saved colours, with the contrast of
/// each pairing that matters.
#[component]
fn Readability(page: PageStyle, title: String) -> impl IntoView {
    let palette = Palette::for_style(&page);
    let checks = palette.contrast_checks();
    view! {
        <section class="card p-5" aria-labelledby="readability">
            <h2 id="readability" class="font-display text-lg font-semibold">
                "Readability"
            </h2>
            <p class="hint mt-0">"The saved design, checked against WCAG 2.2 AA."</p>
            <div
                class="mt-4 flex flex-col gap-3 rounded-control bg-canvas p-4 text-ink"
                style=palette.css_vars()
                aria-hidden="true"
            >
                <p class=match page.title_style {
                    TitleStyle::Standard => "font-display text-xl font-semibold",
                    TitleStyle::Uppercase => "font-display text-xl font-bold uppercase tracking-wide",
                }>{title}</p>
                <p class="text-sm text-muted">"Secondary text looks like this."</p>
                <div class="rounded-control bg-surface p-3">
                    <p class="text-sm">"A panel"</p>
                    <p class="text-xs text-faint">"with a hint"</p>
                    <span class="btn-primary mt-2 w-full">"Download"</span>
                </div>
                <p class="text-sm text-info underline underline-offset-4">"A link"</p>
            </div>
            <ul class="mt-4 flex flex-col gap-2 text-sm">
                {checks
                    .into_iter()
                    .map(|c| {
                        let pass = c.passes();
                        view! {
                            <li class="flex items-baseline justify-between gap-3">
                                <span class="text-muted">{c.label}</span>
                                <span class=if pass { "font-mono text-good" } else { "font-mono text-bad" }>
                                    {format!("{:.1}:1 {}", c.ratio, if pass { "✓" } else { "✗" })}
                                </span>
                            </li>
                        }
                    })
                    .collect_view()}
            </ul>
        </section>
    }
}

/// The developer's preview of their page, whatever its status.
#[component]
pub fn DesignPreview() -> impl IntoView {
    view! {
        <Title text="Preview · Foyer" />
        <RequireSession children=|_| view! { <PreviewInner /> } />
    }
}

#[component]
fn PreviewInner() -> impl IntoView {
    let params = use_params_map();
    let slug = move || params.read().get("slug").unwrap_or_default();
    let page = Resource::new(slug, get_page_preview);
    view! {
        {move || Suspend::new(async move {
            match page.await {
                Ok(p) => {
                    let back = format!("/dashboard/games/{}/design", p.game.slug);
                    view! {
                        <div class="mx-auto max-w-6xl px-6 pt-6">
                            <A href=back attr:class="link text-sm">"← Back to page design"</A>
                        </div>
                        <GameView page=p owner_preview=true />
                    }
                        .into_any()
                }
                Err(e) => view! {
                    <PageShell>
                        <ErrorNote message=server_error_text(&e) />
                    </PageShell>
                }
                .into_any(),
            }
        })}
    }
}
