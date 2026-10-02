//! PostgreSQL persistence (SQLx, runtime-checked queries).

pub mod models;
pub mod repositories;

use sqlx::{PgPool, postgres::PgPoolOptions};

pub use models::{ClaimRecord, RunStats};
pub use repositories::Repository;

/// Connects and applies embedded migrations.
pub async fn connect(database_url: &str) -> Result<PgPool, sqlx::Error> {
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(std::time::Duration::from_secs(10))
        .connect(database_url)
        .await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}
