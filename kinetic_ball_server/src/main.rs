mod api;
mod auth;
mod state;
mod ws;

use axum::{middleware, response::IntoResponse, routing::get, Json, Router};
use clap::Parser;
use std::net::SocketAddr;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use utoipa::OpenApi;

use crate::state::AppState;

#[derive(Parser, Debug)]
#[command(name = "proxy")]
#[command(about = "Matchbox proxy server with room management")]
struct Args {
    /// Port to listen on
    #[arg(short, long, default_value = "3537")]
    port: u16,

    /// Matchbox server URL
    #[arg(short, long, default_value = "ws://127.0.0.1:3536")]
    matchbox_url: String,

    /// Domain for TLS certificate (enables HTTPS)
    #[arg(long)]
    domain: Option<String>,

    /// Email for ACME/Let's Encrypt registration
    #[arg(long)]
    acme_email: Option<String>,

    /// Directory to cache ACME certificates
    #[arg(long, default_value = "./acme-cache")]
    acme_cache: String,

    /// Use Let's Encrypt staging environment (for testing)
    #[arg(long)]
    acme_staging: bool,

    /// Minimum client version required (semver, e.g. "0.7.1")
    #[arg(long, default_value = env!("CARGO_PKG_VERSION"))]
    min_version: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "proxy=info,tower_http=info".into()),
        )
        .init();

    let args = Args::parse();

    // Create shared state
    let state = AppState::new(args.matchbox_url.clone(), args.min_version);

    // CORS configuration
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // Build router
    let app = Router::new()
        // Landing page
        .route_service("/", ServeFile::new("kinetic_ball_server/static/index.html"))
        // Swagger UI + auto-generated OpenAPI spec
        .route_service("/swagger", ServeFile::new("kinetic_ball_server/static/swagger.html"))
        .route("/openapi.json", get(serve_openapi))
        // Static images
        .nest_service("/images", ServeDir::new("kinetic_ball_server/static/images"))
        // REST API (protected by HMAC + version middleware)
        .nest(
            "/api",
            api::rooms_router()
                .layer(middleware::from_fn_with_state(state.clone(), auth::version_middleware)),
        )
        // WebSocket endpoints
        .route("/connect", get(ws::handle_server_ws))
        .route("/:room_id", get(ws::handle_client_ws))
        .layer(cors)
        .with_state(state);

    // Determine bind address
    let addr = SocketAddr::from(([0, 0, 0, 0], args.port));

    // Check if TLS is enabled
    if let (Some(domain), Some(email)) = (args.domain, args.acme_email) {
        run_with_tls(app, addr, domain, email, args.acme_cache, args.acme_staging).await
    } else {
        run_without_tls(app, addr, &args.matchbox_url).await
    }
}

async fn serve_openapi() -> impl IntoResponse {
    Json(api::ApiDoc::openapi())
}

async fn run_without_tls(
    app: Router,
    addr: SocketAddr,
    matchbox_url: &str,
) -> anyhow::Result<()> {
    tracing::info!("Starting proxy server on http://{}", addr);
    tracing::info!("Proxying to matchbox at {}", matchbox_url);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn run_with_tls(
    app: Router,
    addr: SocketAddr,
    domain: String,
    email: String,
    cache_dir: String,
    staging: bool,
) -> anyhow::Result<()> {
    use rustls_acme::{caches::DirCache, AcmeConfig};
    use tokio_stream::StreamExt;

    tracing::info!("Starting proxy server with TLS on https://{}", addr);
    tracing::info!("Domain: {}", domain);
    tracing::info!(
        "Using Let's Encrypt {}",
        if staging { "staging" } else { "production" }
    );

    // Create cache directory if it doesn't exist
    tokio::fs::create_dir_all(&cache_dir).await?;

    // Configure ACME
    let cache_path: &'static str = Box::leak(cache_dir.into_boxed_str());
    let mut acme_state = AcmeConfig::new([domain])
        .contact([format!("mailto:{}", email)])
        .cache(DirCache::new(cache_path))
        .directory_lets_encrypt(staging)
        .state();

    let acceptor = acme_state.axum_acceptor(acme_state.default_rustls_config());

    // Spawn ACME event handler
    tokio::spawn(async move {
        loop {
            match acme_state.next().await {
                Some(Ok(ok)) => tracing::info!("ACME event: {:?}", ok),
                Some(Err(err)) => tracing::error!("ACME error: {:?}", err),
                None => break,
            }
        }
    });

    // Run server with TLS
    axum_server::bind(addr)
        .acceptor(acceptor)
        .serve(app.into_make_service())
        .await?;

    Ok(())
}
