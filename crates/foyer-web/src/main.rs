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

    let app = Router::new()
        .merge(foyer_web::routes::router())
        .leptos_routes(&leptos_options, routes, {
            let leptos_options = leptos_options.clone();
            move || shell(leptos_options.clone())
        })
        .fallback(leptos_axum::file_and_error_handler(shell))
        .with_state(leptos_options)
        // Pages, the wasm bundle and the stylesheet all shrink a lot; fonts
        // are already compressed and are skipped by content type.
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
