pub mod delivery;
pub mod digest;
pub mod email;
pub mod emitter;
pub mod os_toast;
pub mod router;
pub mod scheduler;

pub type SqlitePool = sea_orm::DatabaseConnection;
pub type Result<T> = crate::errors::AppResult<T>;

#[cfg(test)]
mod channel_tests;
