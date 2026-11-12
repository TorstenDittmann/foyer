#[cfg(feature = "ssr")]
#[tokio::main]
async fn main() {
    use axum::Router;
    use foyer_web::app::{App, shell};
    use leptos::prelude::*;
    use leptos_axum::{LeptosRoutes, generate_route_list};

    let conf = get_configuration(None).expect("failed to read Leptos configuration");
    let addr = conf.leptos_options.site_addr;
    let leptos_options = conf.leptos_options;
    let routes = generate_route_list(App);
    let immutable_pkg = leptos_options
        .hash_files
        .then(|| format!("/{}/", leptos_options.site_pkg_dir));

    let app = Router::new()
        .merge(foyer_web::routes::router())
        .leptos_routes(&leptos_options, routes, {
            let leptos_options = leptos_options.clone();
            move || shell(leptos_options.clone())
        })
        .fallback(leptos_axum::file_and_error_handler(shell))
        .with_state(leptos_options)
        // With hashed file names a new build gets new URLs, so browsers and
        // the CDN can keep the bundle for a year instead of revalidating it.
        .layer(axum::middleware::from_fn(
            move |req: axum::extract::Request, next: axum::middleware::Next| {
                let immutable = immutable_pkg
                    .as_deref()
                    .is_some_and(|pkg| req.uri().path().starts_with(pkg));
                async move {
                    let mut resp = next.run(req).await;
                    if immutable && resp.status().is_success() {
                        resp.headers_mut().insert(
                            axum::http::header::CACHE_CONTROL,
                            axum::http::HeaderValue::from_static(
                                "public, max-age=31536000, immutable",
                            ),
                        );
                    }
                    resp
                }
            },
        ))
        // Pages shrink a lot. Release builds also ship .br/.gz copies of the
        // bundle, which are sent as they are and not compressed again.
        .layer(tower_http::compression::CompressionLayer::new());

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("failed to bind");
    leptos::logging::log!("listening on http://{addr}");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await
    .expect("server error");
}

#[cfg(not(feature = "ssr"))]
fn main() {
    // The binary only exists for the `ssr` build; the browser build is the
    // `hydrate` library. Run the site with `cargo leptos watch`.
}
