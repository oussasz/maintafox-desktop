//! Test databases without paying for ~170 migrations per test.
//!
//! Running every migration (and the system seed) costs seconds per database in a debug build; with
//! ~1,200 tests that was most of `cargo test`. Each kind of database is now built ONCE per test
//! process, from scratch, exactly as before (`Migrator::up`, then the seeders), and serialized with
//! `sqlite3_serialize`. Every test then gets its own private in-memory copy
//! (`sqlite3_deserialize`): same schema, same rows, same `seaql_migrations` bookkeeping, nothing
//! shared with any other test. `template_copies_match_a_from_scratch_build` proves the copy is
//! identical to a database built from scratch.
//!
//! Copies share every value generated while the template was built: the sync replica identity
//! (so the device code), the random `entity_sync_id`s and the seed timestamps. A test that needs
//! databases which differ the way separately installed devices do (two devices exchanging
//! changes, "the same on every database" checks) builds each one with [`Template::fresh`].
//! Tests that exercise migrations themselves, files or SQLCipher keep building their own database.

use std::future::Future;
use std::pin::Pin;

use sea_orm::{ConnectionTrait, DatabaseConnection};
use sea_orm_migration::MigratorTrait;
use sqlx::sqlite::{SqliteConnectOptions, SqliteOwnedBuf, SqlitePoolOptions};
use tokio::sync::OnceCell;

type BuildFn = for<'a> fn(&'a DatabaseConnection) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>>;

/// One kind of test database: how to build it, and its serialized image once built.
pub(crate) struct Template {
    image: OnceCell<Vec<u8>>,
    build: BuildFn,
}

impl Template {
    pub(crate) const fn new(build: BuildFn) -> Self {
        Self {
            image: OnceCell::const_new(),
            build,
        }
    }

    /// A new private in-memory database holding a copy of this template.
    pub(crate) async fn copy(&'static self) -> DatabaseConnection {
        let image = self
            .image
            .get_or_init(|| async {
                let db = empty_memory_db().await;
                (self.build)(&db).await;
                let mut conn = db
                    .get_sqlite_connection_pool()
                    .acquire()
                    .await
                    .expect("acquire template");
                let image = conn.serialize(None).await.expect("serialize template database");
                image.to_vec()
            })
            .await;

        let db = empty_memory_db().await;
        {
            let mut conn = db.get_sqlite_connection_pool().acquire().await.expect("acquire copy");
            let buf = SqliteOwnedBuf::try_from(image.as_slice()).expect("copy template image");
            conn.deserialize(None, buf, false)
                .await
                .expect("deserialize template database");
        }
        db
    }

    /// A new database built from scratch (no copy): its own replica identity and random ids, as
    /// on a separately installed device. Seconds per call; only for multi-device tests.
    pub(crate) async fn fresh(&self) -> DatabaseConnection {
        let db = empty_memory_db().await;
        (self.build)(&db).await;
        db
    }
}

/// A private in-memory database on a single connection that lives as long as the pool, opened
/// like `Database::connect("sqlite::memory:")` does (sqlx defaults, so `foreign_keys = ON`).
async fn empty_memory_db() -> DatabaseConnection {
    let opts = SqliteConnectOptions::new().in_memory(true).shared_cache(false);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .min_connections(1)
        .idle_timeout(None)
        .max_lifetime(None)
        .connect_with(opts)
        .await
        .expect("in-memory SQLite");
    let db = sea_orm::SqlxSqliteConnector::from_sqlx_sqlite_pool(pool);
    db.execute_unprepared("PRAGMA foreign_keys = ON")
        .await
        .expect("PRAGMA foreign_keys");
    db
}

static MIGRATED: Template = Template::new(|db| {
    Box::pin(async move {
        crate::migrations::Migrator::up(db, None)
            .await
            .expect("migrations should apply cleanly");
    })
});

static SEEDED: Template = Template::new(|db| {
    Box::pin(async move {
        crate::migrations::Migrator::up(db, None)
            .await
            .expect("migrations should apply cleanly");
        crate::db::seeder::seed_system_data(db)
            .await
            .expect("seeder should run cleanly");
    })
});

/// In-memory database with every migration applied (no seed data). Replaces
/// `Database::connect("sqlite::memory:")` + `Migrator::up(&db, None)`.
pub(crate) async fn migrated() -> DatabaseConnection {
    MIGRATED.copy().await
}

/// In-memory database with every migration applied and the system seed (`seed_system_data`).
/// Replaces `Database::connect("sqlite::memory:")` + `Migrator::up` + `seed_system_data`.
pub(crate) async fn seeded() -> DatabaseConnection {
    SEEDED.copy().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{Database, DbBackend, Statement};

    async fn rows(db: &DatabaseConnection, sql: &str) -> Vec<String> {
        db.query_all(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e}"))
            .iter()
            .map(|r| r.try_get_by_index::<String>(0).expect("text column"))
            .collect()
    }

    async fn scalar(db: &DatabaseConnection, sql: &str) -> i64 {
        db.query_one(Statement::from_string(DbBackend::Sqlite, sql.to_string()))
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e}"))
            .expect("row")
            .try_get_by_index::<i64>(0)
            .expect("integer")
    }

    /// Schema objects, per-table row counts and migration bookkeeping: what a test can observe of
    /// a fresh database (timestamps written by the seed differ by construction and are not compared).
    async fn fingerprint(db: &DatabaseConnection) -> Vec<String> {
        let mut out = rows(
            db,
            "SELECT type || '|' || name || '|' || tbl_name || '|' || COALESCE(sql, '') \
             FROM sqlite_master ORDER BY type, name",
        )
        .await;
        for table in rows(db, "SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name").await {
            let n = scalar(db, &format!("SELECT COUNT(*) FROM \"{table}\"")).await;
            out.push(format!("rows|{table}|{n}"));
        }
        out.extend(rows(db, "SELECT version FROM seaql_migrations ORDER BY version").await);
        out.push(format!("user_version|{}", scalar(db, "PRAGMA user_version").await));
        out
    }

    #[tokio::test]
    async fn template_copies_match_a_from_scratch_build() {
        let scratch = Database::connect("sqlite::memory:").await.expect("connect");
        crate::migrations::Migrator::up(&scratch, None).await.expect("migrate");
        assert_eq!(fingerprint(&migrated().await).await, fingerprint(&scratch).await);

        crate::db::seeder::seed_system_data(&scratch).await.expect("seed");
        assert_eq!(fingerprint(&seeded().await).await, fingerprint(&scratch).await);
    }

    #[tokio::test]
    async fn copies_are_private_and_enforce_foreign_keys() {
        let a = seeded().await;
        let b = seeded().await;
        assert_eq!(scalar(&a, "PRAGMA foreign_keys").await, 1);
        assert_eq!(scalar(&b, "PRAGMA foreign_keys").await, 1);

        a.execute_unprepared("CREATE TABLE copy_probe (x INTEGER); INSERT INTO copy_probe VALUES (1);")
            .await
            .expect("write in copy a");
        let probe = "SELECT COUNT(*) FROM sqlite_master WHERE name = 'copy_probe'";
        assert_eq!(scalar(&a, probe).await, 1);
        assert_eq!(scalar(&b, probe).await, 0, "copy b must not see a's writes");
        assert_eq!(
            scalar(&seeded().await, probe).await,
            0,
            "the template must not see a's writes"
        );
    }
}
