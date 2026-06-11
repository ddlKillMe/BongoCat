use std::{env, net::SocketAddr};

use bongo_cat_relay::router;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let addr = env::var("BONGO_RELAY_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:8787".to_string())
        .parse::<SocketAddr>()
        .expect("BONGO_RELAY_ADDR must be a socket address");

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind relay");

    println!("BongoCat relay listening on ws://{addr}/ws");

    axum::serve(listener, router())
        .await
        .expect("relay server failed");
}
