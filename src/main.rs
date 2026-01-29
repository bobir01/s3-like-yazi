mod app;
mod credentials;
mod s3_client;
mod ui;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = credentials::McConfig::load()?;
    let mut app = app::App::new(config);
    ui::run(&mut app).await
}
