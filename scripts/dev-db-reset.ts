import * as fs from "fs";
import * as path from "path";
import * as readline from "readline";
import * as os from "os";
import * as process from "process";

// Resolve the database path from .env or default
function getDbPath(): string {
  const envFile = path.join(process.cwd(), ".env");
  if (fs.existsSync(envFile)) {
    const content = fs.readFileSync(envFile, "utf8");
    const match = content.match(/^DATABASE_URL\s*=\s*(.+)$/m);
    const dbUrl = match?.[1];
    if (dbUrl) {
      return dbUrl.trim().replace("sqlite://", "").replace(/\?.*/, "");
    }
  }
  // Dev-data path (matches existing convention from Sub-phase 01)
  const devDataPath = path.resolve(process.cwd(), "dev-data", "maintafox_dev.db");
  if (fs.existsSync(devDataPath) || fs.existsSync(path.dirname(devDataPath))) {
    return devDataPath;
  }
  // Tauri default: %APPDATA%/maintafox/maintafox.db on Windows
  const appData = process.env["APPDATA"] ?? path.join(os.homedir(), ".local", "share");
  return path.join(appData, "maintafox", "maintafox.db");
}

function getBackupsDir(dbPath: string): string {
  return path.join(path.dirname(dbPath), "backups");
}

async function confirm(message: string): Promise<boolean> {
  // In CI, auto-confirm
  if (process.env["CI"] === "true" || process.argv.includes("--yes")) {
    return true;
  }
  const rl = readline.createInterface({
    input: process.stdin,
    output: process.stdout,
  });
  return new Promise((resolve) => {
    rl.question(`${message} [y/N]: `, (answer) => {
      rl.close();
      resolve(answer.trim().toLowerCase() === "y");
    });
  });
}

function cleanOldBackups(backupsDir: string, maxAgeDays = 30): void {
  if (!fs.existsSync(backupsDir)) return;
  const now = Date.now();
  const maxAgeMs = maxAgeDays * 24 * 60 * 60 * 1000;
  const entries = fs.readdirSync(backupsDir);
  let deleted = 0;
  for (const entry of entries) {
    if (!entry.startsWith("pre_migration_")) continue;
    const fullPath = path.join(backupsDir, entry);
    const stat = fs.statSync(fullPath);
    if (now - stat.mtimeMs > maxAgeMs) {
      fs.unlinkSync(fullPath);
      deleted++;
    }
  }
  if (deleted > 0) {
    console.log(`Cleaned ${deleted} backup(s) older than ${maxAgeDays} days.`);
  }
}

async function main(): Promise<void> {
  const dbPath = getDbPath();
  const backupsDir = getBackupsDir(dbPath);

  console.log("=== Maintafox Dev DB Reset ===");
  console.log(`Database: ${path.resolve(dbPath)}`);

  if (!fs.existsSync(dbPath)) {
    console.log("Database file does not exist. Nothing to delete.");
    console.log("Run 'pnpm run dev' to create and migrate the database.");
    process.exit(0);
  }

  const ok = await confirm("This will DELETE the local database and all its data. Continue?");
  if (!ok) {
    console.log("Reset cancelled.");
    process.exit(0);
  }

  // Also delete WAL and SHM companion files
  for (const ext of ["", "-wal", "-shm"]) {
    const p = `${dbPath}${ext}`;
    if (fs.existsSync(p)) {
      fs.unlinkSync(p);
      console.log(`Deleted: ${p}`);
    }
  }

  cleanOldBackups(backupsDir);

  console.log("\nDatabase reset complete.");
  console.log("Run 'pnpm run dev' to recreate and run all migrations.");
  console.log("Run 'pnpm run db:seed' after startup to restore development seed data.");
}

main().catch((e) => {
  console.error("Reset failed:", e);
  process.exit(1);
});
