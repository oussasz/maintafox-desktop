//! Developer-only generator of the demo company (feature `demo-company`, never shipped).
//!
//! Usage: `demo-company --db <path-to-existing-db> [--seed N] [--today YYYY-MM-DD]`
//!
//! The database must be an existing Maintafox database, opened once by this version of the app
//! (no pending migration), activated with the licence of the demo company and with no company data
//! yet. Before writing, the file (and its `-wal` / `-shm` companions) is copied next to it.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use chrono::NaiveDate;
use clap::Parser;
use maintafox_lib::clock::simulated::{register_vfs, SimClock};
use maintafox_lib::demo_company::{generate, preflight, GenerationOptions, DEFAULT_SEED, SIM_VFS_NAME};

#[derive(Parser)]
#[command(name = "demo-company")]
#[command(
    about = "Remplit une base Maintafox vide avec un an d'historique de l'entreprise fictive Tellmétal Profilés du Nord"
)]
struct Cli {
    /// Existing database file (opened once by this version of Maintafox, without company data).
    #[arg(long)]
    db: PathBuf,
    /// Seed of the random draws (same seed and same day give the same company).
    #[arg(long, default_value_t = DEFAULT_SEED)]
    seed: u64,
    /// Last day of the history (default: today).
    #[arg(long)]
    today: Option<NaiveDate>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    if !cli.db.is_file() {
        bail!("Base introuvable: {}", cli.db.display());
    }
    let today = cli.today.unwrap_or_else(|| chrono::Local::now().date_naive());
    if today > chrono::Local::now().date_naive() {
        bail!("--today ne peut pas être dans le futur");
    }
    let opts = GenerationOptions::new(cli.seed, today);

    let clock = SimClock::new();
    register_vfs(SIM_VFS_NAME, &clock).map_err(anyhow::Error::msg)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("runtime")?;
    let _guard = clock.install_on_this_thread();
    // Checks first (read only), then the copy of the closed file, then the generation.
    ensure_not_in_use(&cli.db)?;
    runtime.block_on(async {
        let db = maintafox_lib::db::open_existing_with_vfs(&cli.db, SIM_VFS_NAME).await?;
        let checked = preflight(&db).await;
        db.close().await.ok();
        checked
    })?;
    ensure_not_in_use(&cli.db)?;
    let backup = backup_copy(&cli.db)?;
    println!("Copie de sauvegarde avant génération: {}", backup.display());
    let report = runtime.block_on(async {
        let db = maintafox_lib::db::open_existing_with_vfs(&cli.db, SIM_VFS_NAME).await?;
        let report = generate(&db, &clock, &opts).await?;
        db.close().await.ok();
        Ok::<_, maintafox_lib::errors::AppError>(report)
    })?;

    println!(
        "Entreprise générée: {}",
        maintafox_lib::demo_company::catalog::COMPANY_NAME
    );
    for (kind, n) in &report.counts {
        println!("  {kind}: {n}");
    }
    let k = &report.kpis;
    let mtbf = k.mtbf_values();
    let mttr = k.mttr_values();
    let mean = |v: &[f64]| {
        if v.is_empty() {
            0.0
        } else {
            v.iter().sum::<f64>() / v.len() as f64
        }
    };
    println!("Indicateurs:");
    println!("  MTBF moyen: {:.0} h ({} équipements)", mean(&mtbf), mtbf.len());
    println!("  MTTR moyen: {:.1} h", mean(&mttr));
    if let Some(pm) = k.pm_compliance_pct {
        println!("  Respect du préventif: {pm:.1} %");
    }
    if let Some(w) = k.backlog_weeks {
        println!("  Carnet de travaux: {:.0} h ({w:.1} semaines)", k.backlog_hours);
    }
    println!(
        "  OT clôturés: {}, coût total {:.0} DZD",
        k.closed_work_orders, k.total_cost
    );
    if !report.warnings.is_empty() {
        println!("Étapes refusées par l'application ({}):", report.warnings.len());
        for w in &report.warnings {
            println!("  - {w}");
        }
    }
    println!("Les comptes créés n'ont pas de mot de passe: l'administrateur les active par une réinitialisation.");
    println!(
        "Journal d'audit: les événements de l'activation et de la première connexion, datés du jour, précèdent dans la chaîne l'historique daté en arrière (accepté pour une base de démonstration)."
    );
    Ok(())
}

/// Refuses while another program (normally Maintafox itself) has the database open: its background
/// work would write rows dated today among the back-dated history, and the copy could be torn.
/// SQLite opens its files shared, so an open without sharing fails while any other handle exists.
#[cfg(windows)]
fn ensure_not_in_use(db: &Path) -> Result<()> {
    use std::os::windows::fs::OpenOptionsExt;
    let name = db
        .file_name()
        .context("nom de fichier de la base")?
        .to_string_lossy()
        .into_owned();
    for suffix in ["", "-wal", "-shm"] {
        let file = db.with_file_name(format!("{name}{suffix}"));
        if !file.is_file() {
            continue;
        }
        if std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&file)
            .is_err()
        {
            bail!(
                "La base est ouverte par un autre programme ({}). Fermez Maintafox, puis relancez le générateur.",
                file.display()
            );
        }
    }
    Ok(())
}

/// Other systems: the run guide asks for Maintafox to be closed; nothing to check here.
#[cfg(not(windows))]
fn ensure_not_in_use(_db: &Path) -> Result<()> {
    Ok(())
}

/// Copies `db` (and its `-wal` / `-shm` files when present) to
/// `<name>.avant-demo-<YYYYMMDD-HHMMSS>.bak` next to it; returns the copy of the database file.
fn backup_copy(db: &Path) -> Result<PathBuf> {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let name = db
        .file_name()
        .context("nom de fichier de la base")?
        .to_string_lossy()
        .into_owned();
    let target = db.with_file_name(format!("{name}.avant-demo-{stamp}.bak"));
    if target.exists() {
        bail!("La copie {} existe déjà", target.display());
    }
    std::fs::copy(db, &target).with_context(|| format!("copie de {}", db.display()))?;
    for suffix in ["-wal", "-shm"] {
        let side = db.with_file_name(format!("{name}{suffix}"));
        if side.is_file() {
            let side_target = db.with_file_name(format!("{name}.avant-demo-{stamp}.bak{suffix}"));
            std::fs::copy(&side, &side_target).with_context(|| format!("copie de {}", side.display()))?;
        }
    }
    Ok(target)
}
