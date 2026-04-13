pub mod emitter;

pub type SqlitePool = sea_orm::DatabaseConnection;
pub type Result<T> = crate::errors::AppResult<T>;
