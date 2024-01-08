use axum::{routing::post, Router};
use ddclient_rs::Client;
use std::time::Duration;
use tokio::time;
use tracing_panic::panic_hook;
use tracing_subscriber::{filter::EnvFilter, fmt::Subscriber};
use twilight_http::Client as DiscordClient;

const MAX_CHOICES: usize = 32;

#[tokio::main]
async fn main() {
    let subscriber = Subscriber::builder()
        .with_env_filter(EnvFilter::from_default_env())
        .json()
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    std::panic::set_hook(Box::new(panic_hook));

    let db = dd_discord::db::new();
    let bot_token = std::env::var("BOT_TOKEN").expect("BOT_TOKEN env variable not set");
    let dd_token = std::env::var("DD_TOKEN").expect("DD_TOKEN env variable not set");
    let dd_api_url = std::env::var("DD_API_URL").expect("API_URL env variable not set");
    let discord_register_url =
        std::env::var("DISCORD_REGISTER_URL").expect("DISCORD_REGISTER_URL env variable not set");
    let discord_public_key =
        std::env::var("DISCORD_PUBLIC_KEY").expect("DISCORD_PUBLIC_KEY env variable not set");

    let discord_client = DiscordClient::new(bot_token.clone());
    let dd_client = Client::builder(dd_token).api_url(dd_api_url).build();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:8080")
        .await
        .unwrap();
    tracing::info!("listening on {}", listener.local_addr().unwrap());

    let app_state = dd_discord::new_app_state(db, discord_client, dd_client, discord_public_key);

    let app = Router::new()
        .route("/", post(dd_discord::handle_interaction))
        .with_state(app_state.clone());

    dd_discord::util::register_voting_command(&bot_token, &discord_register_url, MAX_CHOICES).await;

    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            tokio::signal::ctrl_c()
                .await
                .expect("failed to install ctrl+c signal handler");
            tracing::info!("received ctrl+c signal, starting graceful shutdown");

            app_state.task_tracker.close();

            match time::timeout(Duration::from_secs(10), app_state.task_tracker.wait()).await {
                Ok(_) => tracing::info!("All tasks finished cleanly."),
                Err(_) => tracing::info!("Timed out waiting for tasks to finish."),
            }
        })
        .await
        .unwrap();
}
