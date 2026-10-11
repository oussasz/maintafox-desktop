#!/usr/bin/env node
/**
 * Multi-device sync harness (Phase 2.5): the real control API on a real Postgres, and three
 * headless desktop databases syncing through the real Rust sync worker.
 *
 *   pnpm sync:harness            (or: node scripts/sync-harness.mjs)
 *
 * What it does:
 *   1. builds the control API of the sibling repository `maintafox-vendor-console`
 *      (`MAINTAFOX_VENDOR_CONSOLE_DIR` overrides the location; `--skip-api-build` reuses dist/);
 *   2. starts a throw-away PostgreSQL (embedded PostgreSQL 16 from the vendor console's dev
 *      dependencies, or `TEST_DATABASE_ADMIN_URL`) and the API on 127.0.0.1 only;
 *   3. creates a tenant and a licence, activates three machines (devices A, B and C);
 *   4. serves a small control endpoint on 127.0.0.1 for actions only an operator can do
 *      (revoking a machine needs a step-up token) and for the object store below;
 *   4b. runs a minimal S3-compatible object store on 127.0.0.1 (path-style bucket, presigned PUT
 *      and GET: the URL must carry the AWS SigV4 query, the access key and an unexpired date; the
 *      signature itself is not recomputed). The API presigns against it like against MinIO. It
 *      can refuse uploads on request, to test a file whose upload is late;
 *   5. runs the Rust scenarios: `cargo test --lib -- --ignored sync::harness_tests`
 *      (src-tauri/src/sync/harness_tests.rs), which reads the connection details from the file
 *      named by `MAINTAFOX_SYNC_HARNESS_CONFIG`;
 *   6. stops the API and the database and exits with the test result.
 *
 * Nothing leaves this machine: the API, the database and the control endpoint listen on
 * 127.0.0.1, and the Rust side refuses any other API host.
 */
import { spawn } from "node:child_process";
import { once } from "node:events";
import { existsSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { createServer as createNetServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const desktopRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const vendorRoot = resolve(
  process.env.MAINTAFOX_VENDOR_CONSOLE_DIR?.trim() || join(desktopRoot, "..", "maintafox-vendor-console"),
);
const apiDir = join(vendorRoot, "api");
const skipApiBuild = process.argv.includes("--skip-api-build");

const ADMIN_EMAIL = "admin@maintafox.systems";
const S3_ACCESS_KEY = "harness-access-key";
const S3_SECRET_KEY = "harness-secret-key-not-a-real-one";
const S3_BUCKET = "maintafox-tenant-media";
const ADMIN_PASSWORD = "HarnessAdminPassw0rd!";
const DEVICES = [
  { name: "A", fingerprint: "fp-harness-device-a" },
  { name: "B", fingerprint: "fp-harness-device-b" },
  { name: "C", fingerprint: "fp-harness-device-c" },
];

const delay = (ms) => new Promise((r) => setTimeout(r, ms));

function log(message) {
  process.stdout.write(`[sync-harness] ${message}\n`);
}

function freePort() {
  return new Promise((resolvePort, reject) => {
    const srv = createNetServer();
    srv.once("error", reject);
    srv.listen(0, "127.0.0.1", () => {
      const { port } = srv.address();
      srv.close(() => resolvePort(port));
    });
  });
}

/** Runs a fixed command line (no user input in it); a shell resolves `pnpm.cmd` / `cargo.exe` on Windows. */
function run(command, args, options) {
  return new Promise((resolveRun, reject) => {
    const child = spawn([command, ...args].join(" "), { stdio: "inherit", shell: true, ...options });
    child.once("error", reject);
    child.once("exit", (code) => resolveRun(code ?? 1));
  });
}

async function json(res, what) {
  const text = await res.text();
  if (!res.ok) throw new Error(`${what} failed with HTTP ${res.status}: ${text}`);
  return JSON.parse(text);
}

/**
 * Minimal S3-compatible object store for the harness: HEAD/PUT bucket, presigned PUT/GET object.
 * Objects live in memory. `blockPuts` makes uploads answer 503 (a late upload).
 */
function startObjectStore() {
  const objects = new Map();
  const state = { blockPuts: false, puts: 0, gets: 0 };
  const server = createServer((req, res) => {
    const url = new URL(req.url ?? "/", "http://127.0.0.1");
    const [bucket, ...keyParts] = url.pathname.replace(/^\//, "").split("/");
    const key = keyParts.map(decodeURIComponent).join("/");
    const chunks = [];
    req.on("data", (c) => chunks.push(c));
    req.on("end", () => {
      if (bucket !== S3_BUCKET) {
        res.writeHead(404, { "Content-Type": "application/xml" }).end("<Error><Code>NoSuchBucket</Code></Error>");
        return;
      }
      if (!key) {
        // HeadBucket / CreateBucket from the API (signed headers, not a presigned URL).
        res.writeHead(200).end();
        return;
      }
      const q = url.searchParams;
      const credential = q.get("X-Amz-Credential") ?? "";
      const issued = q.get("X-Amz-Date") ?? "";
      const expires = Number(q.get("X-Amz-Expires") ?? "0");
      const issuedAt = Date.parse(
        issued.replace(/^(\d{4})(\d{2})(\d{2})T(\d{2})(\d{2})(\d{2})Z$/, "$1-$2-$3T$4:$5:$6Z"),
      );
      const presigned =
        q.get("X-Amz-Algorithm") === "AWS4-HMAC-SHA256" &&
        q.get("X-Amz-Signature") &&
        credential.startsWith(`${S3_ACCESS_KEY}/`) &&
        Number.isFinite(issuedAt) &&
        Date.now() <= issuedAt + expires * 1000;
      if (!presigned) {
        res.writeHead(403, { "Content-Type": "application/xml" }).end("<Error><Code>AccessDenied</Code></Error>");
        return;
      }
      if (req.method === "PUT") {
        if (state.blockPuts) {
          res.writeHead(503, { "Content-Type": "application/xml" }).end("<Error><Code>SlowDown</Code></Error>");
          return;
        }
        objects.set(key, Buffer.concat(chunks));
        state.puts += 1;
        res.writeHead(200, { ETag: `"${state.puts}"` }).end();
        return;
      }
      if (req.method === "GET") {
        const body = objects.get(key);
        if (!body) {
          res.writeHead(404, { "Content-Type": "application/xml" }).end("<Error><Code>NoSuchKey</Code></Error>");
          return;
        }
        state.gets += 1;
        res.writeHead(200, { "Content-Type": "application/octet-stream", "Content-Length": body.length }).end(body);
        return;
      }
      res.writeHead(405).end();
    });
  });
  return { server, state };
}

async function main() {
  if (!existsSync(join(apiDir, "package.json"))) {
    throw new Error(
      `control API not found at ${apiDir}: clone maintafox-vendor-console next to this repository ` +
        "or set MAINTAFOX_VENDOR_CONSOLE_DIR",
    );
  }
  if (!skipApiBuild) {
    log("building the control API");
    const code = await run("pnpm", ["--filter", "maintafox-control-api", "run", "build"], { cwd: vendorRoot });
    if (code !== 0) throw new Error(`control API build failed (exit ${code})`);
  }

  const lib = (file) => pathToFileURL(join(apiDir, "scripts", "lib", file)).href;
  const { startTestPostgres, TEST_SYNC_ENV } = await import(lib("test-postgres.mjs"));
  const { TEST_ENTITLEMENT_SIGNING_ENV } = await import(lib("test-signing-key.mjs"));
  const { obtainStepUpToken } = await import(lib("step-up.mjs"));

  const cleanups = [];
  const cleanup = async () => {
    while (cleanups.length > 0) {
      try {
        await cleanups.pop()();
      } catch (e) {
        process.stderr.write(`[sync-harness] cleanup: ${e}\n`);
      }
    }
  };

  try {
    log("starting PostgreSQL");
    const pg = await startTestPostgres();
    cleanups.push(() => pg.stop());
    const databaseUrl = await pg.createDatabase();

    const store = startObjectStore();
    const storePort = await freePort();
    await new Promise((r) => store.server.listen(storePort, "127.0.0.1", r));
    cleanups.push(() => new Promise((r) => store.server.close(() => r())));
    log(`object store on http://127.0.0.1:${storePort}`);

    const apiPort = await freePort();
    const baseUrl = `http://127.0.0.1:${apiPort}`;
    log(`starting the control API on ${baseUrl}`);
    let apiOutput = "";
    const api = spawn(process.execPath, ["dist/index.js"], {
      cwd: apiDir,
      env: {
        ...process.env,
        PORT: String(apiPort),
        HOST: "127.0.0.1",
        DATABASE_URL: databaseUrl,
        ...TEST_SYNC_ENV,
        JWT_SECRET: "sync_harness_jwt_secret_value_with_minimum_length_12345",
        SEED_ADMIN_PASSWORD: ADMIN_PASSWORD,
        SEED_ADMIN_EMAIL: ADMIN_EMAIL,
        ...TEST_ENTITLEMENT_SIGNING_ENV,
        SEED_BEHAVIOR: "safe",
        NODE_ENV: "production",
        CORS_ORIGIN: "http://localhost:5174",
        S3_ENDPOINT: `http://127.0.0.1:${storePort}`,
        S3_ACCESS_KEY,
        S3_SECRET_KEY,
        S3_BUCKET,
        S3_FORCE_PATH_STYLE: "true",
      },
      stdio: ["ignore", "pipe", "pipe"],
    });
    api.stdout.on("data", (b) => (apiOutput += String(b)));
    api.stderr.on("data", (b) => (apiOutput += String(b)));
    cleanups.push(async () => {
      if (api.exitCode === null) {
        api.kill("SIGTERM");
        await once(api, "exit").catch(() => undefined);
      }
    });
    let healthy = false;
    for (let i = 0; i < 240 && !healthy; i += 1) {
      if (api.exitCode !== null) break;
      try {
        healthy = (await fetch(`${baseUrl}/health`)).ok;
      } catch {
        // not listening yet
      }
      if (!healthy) await delay(250);
    }
    if (!healthy) throw new Error(`the control API did not become healthy:\n${apiOutput}`);

    const login = await json(
      await fetch(`${baseUrl}/api/v1/auth/login`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ email: ADMIN_EMAIL, password: ADMIN_PASSWORD }),
      }),
      "operator login",
    );
    const admin = { Authorization: `Bearer ${login.access_token}`, "Content-Type": "application/json" };
    const tenant = await json(
      await fetch(`${baseUrl}/api/v1/tenants`, {
        method: "POST",
        headers: admin,
        body: JSON.stringify({ slug: "sync-harness", display_name: "Sync harness" }),
      }),
      "tenant creation",
    );
    const licence = await json(
      await fetch(`${baseUrl}/api/v1/licenses`, {
        method: "POST",
        headers: admin,
        body: JSON.stringify({ tenant_id: tenant.id, max_machines: 5, expires_at: "2099-01-01T00:00:00.000Z" }),
      }),
      "licence issue",
    );
    const devices = [];
    for (const d of DEVICES) {
      const claim = await json(
        await fetch(`${baseUrl}/api/v1/activation/claim`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            license_key: licence.license_key,
            machine_fingerprint: d.fingerprint,
            app_version: "0.5.0",
          }),
        }),
        `activation of device ${d.name}`,
      );
      devices.push({ ...d, token: claim.activation_token });
    }
    log(`tenant ${tenant.id}: devices ${devices.map((d) => d.name).join(", ")} activated`);

    // Operator actions the scenarios need (POST /machines/<fingerprint>/revoke) and object store
    // switches (POST /object-store/block-puts | allow-puts).
    let opCounter = 0;
    const control = createServer(async (req, res) => {
      const storeSwitch = /^\/object-store\/(block|allow)-puts$/.exec(req.url ?? "");
      if (req.method === "POST" && storeSwitch) {
        store.state.blockPuts = storeSwitch[1] === "block";
        res.writeHead(200, { "Content-Type": "application/json" }).end(JSON.stringify(store.state));
        return;
      }
      const match = /^\/machines\/([^/]+)\/(revoke|restore)$/.exec(req.url ?? "");
      if (req.method !== "POST" || !match) {
        res.writeHead(404).end();
        return;
      }
      try {
        const [, fingerprint, action] = match;
        const r = await fetch(
          `${baseUrl}/api/v1/tenants/${tenant.id}/machines/${encodeURIComponent(decodeURIComponent(fingerprint))}/${action}`,
          {
            method: "POST",
            headers: {
              ...admin,
              "Idempotency-Key": `sync-harness-${(opCounter += 1)}`,
              "X-Operator-Reason": `SYNC-HARNESS-${action.toUpperCase()}`,
              "X-Step-Up-Token": await obtainStepUpToken(baseUrl, login.access_token, ADMIN_PASSWORD),
            },
            body: JSON.stringify({ reason: `sync harness ${action}` }),
          },
        );
        res.writeHead(r.status, { "Content-Type": "application/json" }).end(await r.text());
      } catch (e) {
        res.writeHead(500, { "Content-Type": "text/plain" }).end(String(e));
      }
    });
    const controlPort = await freePort();
    await new Promise((r) => control.listen(controlPort, "127.0.0.1", r));
    cleanups.push(() => new Promise((r) => control.close(() => r())));

    const configDir = mkdtempSync(join(tmpdir(), "mfx-sync-harness-"));
    cleanups.push(() => rmSync(configDir, { recursive: true, force: true }));
    const configPath = join(configDir, "harness.json");
    writeFileSync(
      configPath,
      JSON.stringify(
        {
          api_base_url: baseUrl,
          control_url: `http://127.0.0.1:${controlPort}`,
          tenant_id: tenant.id,
          devices,
        },
        null,
        2,
      ),
    );

    log("running the Rust scenarios (cargo test --lib -- --ignored sync::harness_tests)");
    const code = await run(
      "cargo",
      ["test", "--lib", "--", "--ignored", "sync::harness_tests", "--test-threads=1", "--nocapture"],
      {
        cwd: join(desktopRoot, "src-tauri"),
        env: { ...process.env, MAINTAFOX_SYNC_HARNESS_CONFIG: configPath },
      },
    );
    if (code !== 0) {
      process.stderr.write(`[sync-harness] control API output:\n${apiOutput.slice(-8000)}\n`);
    }
    log(code === 0 ? "all scenarios passed" : `scenarios failed (exit ${code})`);
    return code;
  } finally {
    await cleanup();
  }
}

main().then(
  (code) => process.exit(code),
  (e) => {
    process.stderr.write(`[sync-harness] ${e?.stack ?? e}\n`);
    process.exit(1);
  },
);
