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
 * Large-tenant measurement (Phase 2 leftover 4):
 *
 *   pnpm sync:harness --large 100000      (work orders; about 10 synchronised rows each)
 *
 * runs `sync::harness_tests::large_tenant` instead of the scenarios: device A creates one work
 * order with its request, tasks, labour, part and history, syncs it, then the control endpoint
 * clones those rows N times in the server's current-state table (new sync ids and numbers, the
 * same shared parents), and a fresh device bootstraps from the snapshot while time, bytes,
 * memory and the server's query time are measured.
 *
 * Mobile sync proof (Android field app, step 0):
 *
 *   pnpm sync:harness --mobile-proof
 *
 * issues a licence with two desktops and one phone (`max_mobile_devices: 1`), starts the API on a
 * fixed port (47831, `MAINTAFOX_MOBILE_PROOF_PORT` overrides it) so a phone on USB reaches it as
 * http://127.0.0.1:47831 through `adb reverse tcp:47831 tcp:47831`, activates no device itself and
 * runs `sync::mobile_proof_tests::mobile_proof`: a real desktop database activates, joins the plant,
 * creates a technician, an equipment and a work order, prints `MOBILE_PROOF_READY` (what the phone
 * operator enters), waits for a request reported on the phone (`MAINTAFOX_MOBILE_PROOF_WAIT_SECS`,
 * default 1200), then sends a change of the work order (`MOBILE_PROOF_DOWNLINK`) and keeps the API
 * up `MAINTAFOX_MOBILE_PROOF_HOLD_SECS` (default 300) for the phone to pull it.
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
import { createRequire } from "node:module";
import { fileURLToPath, pathToFileURL } from "node:url";

const desktopRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const vendorRoot = resolve(
  process.env.MAINTAFOX_VENDOR_CONSOLE_DIR?.trim() || join(desktopRoot, "..", "maintafox-vendor-console"),
);
const apiDir = join(vendorRoot, "api");
const skipApiBuild = process.argv.includes("--skip-api-build");
const largeIndex = process.argv.indexOf("--large");
const largeCopies = largeIndex >= 0 ? Number(process.argv[largeIndex + 1]) : 0;
if (largeIndex >= 0 && !(Number.isInteger(largeCopies) && largeCopies > 0)) {
  throw new Error("--large needs a positive number of work orders, e.g. --large 100000");
}
const mobileProof = process.argv.includes("--mobile-proof");
if (mobileProof && largeIndex >= 0) throw new Error("--mobile-proof and --large cannot be combined");
const mobileProofPort = Number(process.env.MAINTAFOX_MOBILE_PROOF_PORT?.trim() || "47831");
if (mobileProof && !(Number.isInteger(mobileProofPort) && mobileProofPort > 0 && mobileProofPort < 65536)) {
  throw new Error(`MAINTAFOX_MOBILE_PROOF_PORT must be a TCP port, not ${process.env.MAINTAFOX_MOBILE_PROOF_PORT}`);
}

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

/** `port` when it is free on 127.0.0.1 (the fixed port of the mobile proof), else a clear error. */
function requirePortFree(port) {
  return new Promise((resolvePort, reject) => {
    const srv = createNetServer();
    srv.once("error", (e) =>
      reject(new Error(`port ${port} on 127.0.0.1 is in use (${e.code}): set MAINTAFOX_MOBILE_PROOF_PORT`)),
    );
    srv.listen(port, "127.0.0.1", () => srv.close(() => resolvePort(port)));
  });
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
  // Operator sign-in: a console with vendor MFA (Phase 4.1, `vendor-session.mjs`: password then
  // TOTP, step-up per action kind), else the earlier password-only login (`step-up.mjs`).
  const withVendorMfa = existsSync(join(apiDir, "scripts", "lib", "vendor-session.mjs"));
  const { loginOperator } = withVendorMfa ? await import(lib("vendor-session.mjs")) : {};
  const { obtainStepUpToken } = withVendorMfa ? {} : await import(lib("step-up.mjs"));

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

    const apiPort = mobileProof ? await requirePortFree(mobileProofPort) : await freePort();
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
        // Required together by consoles with the split public endpoint (storage/object-store.ts).
        S3_PUBLIC_ENDPOINT: `http://127.0.0.1:${storePort}`,
        S3_REGION: "us-east-1",
        DEPLOYMENT_ENV: "sync-harness",
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

    const session = withVendorMfa ? await loginOperator(baseUrl, ADMIN_EMAIL, ADMIN_PASSWORD) : null;
    const login = session
      ? { access_token: session.accessToken }
      : await json(
          await fetch(`${baseUrl}/api/v1/auth/login`, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ email: ADMIN_EMAIL, password: ADMIN_PASSWORD }),
          }),
          "operator login",
        );
    /** Step-up token for revoking / restoring a machine. */
    const machineStepUp = () =>
      session
        ? session.stepUp("machine_trust_change")
        : obtainStepUpToken(baseUrl, login.access_token, ADMIN_PASSWORD);
    const admin = { Authorization: `Bearer ${login.access_token}`, "Content-Type": "application/json" };
    const tenant = await json(
      await fetch(`${baseUrl}/api/v1/tenants`, {
        method: "POST",
        headers: admin,
        // The mobile proof syncs through the production access rule (the signed licence must include
        // `sync`, an Enterprise module); the scenarios grant access with the activation token alone.
        body: JSON.stringify(
          mobileProof
            ? { slug: "sync-harness", display_name: "Sync harness", edition: "enterprise" }
            : { slug: "sync-harness", display_name: "Sync harness" },
        ),
      }),
      "tenant creation",
    );
    const licence = await json(
      await fetch(`${baseUrl}/api/v1/licenses`, {
        method: "POST",
        headers: admin,
        body: JSON.stringify(
          mobileProof
            ? { tenant_id: tenant.id, max_machines: 2, max_mobile_devices: 1, expires_at: "2099-01-01T00:00:00.000Z" }
            : { tenant_id: tenant.id, max_machines: 5, expires_at: "2099-01-01T00:00:00.000Z" },
        ),
      }),
      "licence issue",
    );
    if (mobileProof && licence.max_mobile_devices !== 1) {
      throw new Error(
        `the control API at ${vendorRoot} did not take max_mobile_devices (answered ${licence.max_mobile_devices}): ` +
          "use the vendor console with the mobile device class (MAINTAFOX_VENDOR_CONSOLE_DIR)",
      );
    }
    const devices = [];
    // The mobile proof activates no device here: the desktop test claims its slot, the phone its own.
    for (const d of mobileProof ? [] : DEVICES) {
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
    if (mobileProof) {
      log(`READY api=${baseUrl} port=${apiPort} tenant=${tenant.id} licence_key=${licence.license_key} max_machines=2 max_mobile_devices=1`);
      log(`READY phone: connect it by USB, then run: adb reverse tcp:${apiPort} tcp:${apiPort}`);
      log("READY do not activate the phone before the desktop prints MOBILE_PROOF_READY (the desktop joins the plant first)");
    } else {
      log(`tenant ${tenant.id}: devices ${devices.map((d) => d.name).join(", ")} activated`);
    }

    // Large-tenant measurement: direct access to the throw-away database.
    const { Pool } = createRequire(join(apiDir, "package.json"))("pg");
    const pool = new Pool({ connectionString: databaseUrl, max: 2 });
    cleanups.push(() => pool.end());

    async function bulkClone(entityTypes, copies) {
      const started = Date.now();
      const { rows: templates } = await pool.query(
        "SELECT entity_type, entity_sync_id, payload_raw FROM sync_entity_state WHERE tenant_id = $1 AND entity_type = ANY($2::text[]) AND NOT deleted",
        [tenant.id, entityTypes],
      );
      if (templates.length === 0) throw new Error("no template rows to clone");
      // Every template sync id and every unique number is replaced per copy.
      let expr = "s.payload_raw";
      const params = [tenant.id, entityTypes, copies];
      for (const t of templates) {
        params.push(t.entity_sync_id);
        const i = params.length;
        expr = `replace(${expr}, $${i}::text, md5($${i}::text || ':' || n)::uuid::text)`;
        const payload = JSON.parse(t.payload_raw);
        for (const key of ["code", "employee_code"]) {
          if (typeof payload[key] === "string" && ["work_orders", "intervention_requests"].includes(t.entity_type)) {
            params.push(`"${key}":${JSON.stringify(payload[key])}`);
            const j = params.length;
            expr = `replace(${expr}, $${j}::text, left($${j}::text, -1) || '-' || n || '"')`;
          }
        }
      }
      const { rowCount } = await pool.query(
        `INSERT INTO sync_entity_state (tenant_id, entity_type, entity_sync_id, row_version, operation, payload_raw,
                                        payload_json, payload_hash, deleted, owner_user_sync_id,
                                        origin_machine_fingerprint, last_ingress_id, updated_at)
         SELECT s.tenant_id, s.entity_type, md5(s.entity_sync_id || ':' || n)::uuid::text, s.row_version, s.operation,
                c.raw, mfx_try_jsonb(c.raw), encode(sha256(convert_to(c.raw, 'UTF8')), 'hex'), false,
                s.owner_user_sync_id, s.origin_machine_fingerprint, s.last_ingress_id, s.updated_at
         FROM sync_entity_state s
         CROSS JOIN generate_series(1, $3::int) AS n
         CROSS JOIN LATERAL (SELECT ${expr} AS raw) c
         WHERE s.tenant_id = $1 AND s.entity_type = ANY($2::text[]) AND NOT s.deleted`,
        params,
      );
      await pool.query("ANALYZE sync_entity_state");
      const { rows: size } = await pool.query(
        "SELECT count(*)::bigint AS rows, coalesce(sum(length(payload_raw)), 0)::bigint AS payload_bytes, pg_total_relation_size('sync_entity_state')::bigint AS table_bytes FROM sync_entity_state WHERE tenant_id = $1",
        [tenant.id],
      );
      return {
        templates: templates.length,
        inserted: rowCount,
        ms: Date.now() - started,
        tenant_rows: Number(size[0].rows),
        tenant_payload_bytes: Number(size[0].payload_bytes),
        table_bytes: Number(size[0].table_bytes),
      };
    }

    /** Server time of the snapshot page query (same SQL as `listSnapshotPage`), first and middle page. */
    async function explainSnapshot(pageSize) {
      const { rows: hw } = await pool.query(
        "SELECT max(last_ingress_id) AS hw, count(*)::bigint AS n FROM sync_entity_state WHERE tenant_id = $1",
        [tenant.id],
      );
      const { rows: middle } = await pool.query(
        "SELECT entity_type, entity_sync_id FROM sync_entity_state WHERE tenant_id = $1 ORDER BY entity_type, entity_sync_id OFFSET $2 LIMIT 1",
        [tenant.id, Math.floor(Number(hw[0].n) / 2)],
      );
      const sql = `EXPLAIN (ANALYZE, FORMAT JSON) SELECT last_ingress_id, entity_type, entity_sync_id, operation, row_version,
                     payload_raw, payload_hash, owner_user_sync_id, deleted
                   FROM sync_entity_state
                   WHERE tenant_id = $1 AND NOT deleted AND last_ingress_id <= $2
                     AND (owner_user_sync_id IS NULL OR owner_user_sync_id = ANY($3::text[]))
                     AND (entity_type, entity_sync_id) > ($4, $5)
                   ORDER BY entity_type ASC, entity_sync_id ASC LIMIT $6`;
      const timing = async (after) => {
        const { rows } = await pool.query(sql, [tenant.id, hw[0].hw, [], after[0], after[1], pageSize + 1]);
        const plan = rows[0]["QUERY PLAN"][0];
        return { ms: plan["Execution Time"], node: plan.Plan["Node Type"] };
      };
      return {
        first_page: await timing(["", ""]),
        middle_page: await timing([middle[0].entity_type, middle[0].entity_sync_id]),
      };
    }

    // Operator actions the scenarios need (POST /machines/<fingerprint>/revoke) and object store
    // switches (POST /object-store/block-puts | allow-puts).
    let opCounter = 0;
    const control = createServer(async (req, res) => {
      if (req.method === "POST" && (req.url ?? "").startsWith("/bulk-clone")) {
        try {
          const chunks = [];
          for await (const c of req) chunks.push(c);
          const body = JSON.parse(Buffer.concat(chunks).toString("utf8"));
          const result = await bulkClone(body.entity_types, body.copies);
          log(`bulk clone: ${JSON.stringify(result)}`);
          res.writeHead(200, { "Content-Type": "application/json" }).end(JSON.stringify(result));
        } catch (e) {
          res.writeHead(500, { "Content-Type": "text/plain" }).end(String(e?.stack ?? e));
        }
        return;
      }
      if (req.method === "POST" && (req.url ?? "").startsWith("/explain-snapshot")) {
        try {
          const pageSize = Number(new URL(req.url, "http://x").searchParams.get("page_size") ?? "500");
          const result = await explainSnapshot(pageSize);
          res.writeHead(200, { "Content-Type": "application/json" }).end(JSON.stringify(result));
        } catch (e) {
          res.writeHead(500, { "Content-Type": "text/plain" }).end(String(e?.stack ?? e));
        }
        return;
      }
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
              "X-Step-Up-Token": await machineStepUp(),
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
          license_key: licence.license_key,
          devices,
          large_copies: largeCopies || null,
        },
        null,
        2,
      ),
    );

    const testFilter = mobileProof
      ? "sync::mobile_proof_tests::mobile_proof"
      : largeCopies
        ? "sync::harness_tests::large_tenant"
        : "sync::harness_tests::multi_device_harness";
    log(`running the Rust scenarios (cargo test --lib -- --ignored ${testFilter})`);
    const code = await run(
      "cargo",
      ["test", "--lib", "--", "--ignored", testFilter, "--exact", "--test-threads=1", "--nocapture"],
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
