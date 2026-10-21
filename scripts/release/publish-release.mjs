#!/usr/bin/env node
/**
 * Uploads a built installer to the Maintafox control API and registers the release (master plan
 * Phase 5.2). Used by .github/workflows/release.yml; works the same from a terminal.
 *
 *   node scripts/release/publish-release.mjs --version 1.2.0 --installer <setup.exe> \
 *     --signature <setup.exe.sig> --notes-en <file> --notes-fr <file> --git-commit <sha> \
 *     --code-signed true|false [--target windows] [--arch x86_64]
 *
 * Environment: MAINTAFOX_RELEASE_API_BASE_URL (the control API origin) and
 * MAINTAFOX_RELEASE_PUBLISHER_TOKEN (a release workflow key created in the console).
 *
 * Steps: declare (size, SHA-256, updater signature) -> PUT the file to the presigned URL ->
 * complete (the server re-reads the file and verifies the updater signature). The release then
 * appears as a draft in Console > Updates > Releases; publishing it is an operator decision.
 * Any refusal stops with the server's message; nothing is reported as done unless the server
 * answered that the release is registered.
 */
import { createHash } from "node:crypto";
import { readFileSync, statSync } from "node:fs";
import { basename } from "node:path";
import { pathToFileURL } from "node:url";

/** A refusal already reported: unwinds to main() (no process.exit while sockets are open). */
class Stop extends Error {}

function fail(message) {
  console.error(`::error::release publish: ${message}`);
  throw new Stop(message);
}

/**
 * The control API origin the publisher token is sent to: https only (the token would otherwise
 * travel in clear), except a local test server on localhost / 127.0.0.1 / [::1]. Returns the
 * origin without a trailing slash, or the reason it is refused.
 */
export function checkApiBase(value) {
  const trimmed = value?.trim().replace(/\/+$/, "");
  if (!trimmed) return { error: "MAINTAFOX_RELEASE_API_BASE_URL is not set" };
  let url;
  try {
    url = new URL(trimmed);
  } catch {
    return { error: `MAINTAFOX_RELEASE_API_BASE_URL is not a URL: ${trimmed}` };
  }
  const local = ["localhost", "127.0.0.1", "[::1]"].includes(url.hostname);
  if (url.protocol === "https:" || (url.protocol === "http:" && local)) return { base: trimmed };
  return {
    error: `MAINTAFOX_RELEASE_API_BASE_URL must be an https:// URL (the release token must not travel in clear): ${trimmed}`,
  };
}

function args(argv) {
  const out = {};
  for (let i = 0; i < argv.length; i += 1) {
    const key = argv[i];
    if (!key.startsWith("--")) fail(`unexpected argument ${key}`);
    const value = argv[i + 1];
    if (value === undefined || value.startsWith("--")) fail(`${key} needs a value`);
    out[key.slice(2)] = value;
    i += 1;
  }
  return out;
}

async function main() {
  const a = args(process.argv.slice(2));
  for (const required of [
    "version",
    "installer",
    "signature",
    "notes-en",
    "notes-fr",
    "git-commit",
    "code-signed",
  ]) {
    if (!a[required]) fail(`--${required} is required`);
  }
  if (!/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(a.version))
    fail(`version ${a.version} is not x.y.z`);
  if (!["true", "false"].includes(a["code-signed"])) fail("--code-signed must be true or false");
  const checked = checkApiBase(process.env.MAINTAFOX_RELEASE_API_BASE_URL);
  if (checked.error) fail(checked.error);
  const apiBase = checked.base;
  const token = process.env.MAINTAFOX_RELEASE_PUBLISHER_TOKEN?.trim();
  if (!token)
    fail(
      "MAINTAFOX_RELEASE_PUBLISHER_TOKEN is not set (create a key in Console > Updates > Releases)",
    );

  const installer = readFileSync(a.installer);
  const signature = readFileSync(a.signature, "utf8").trim();
  const notesEn = readFileSync(a["notes-en"], "utf8").trim();
  const notesFr = readFileSync(a["notes-fr"], "utf8").trim();
  if (!notesEn || !notesFr) fail("release notes must not be empty (EN and FR)");
  const sha256 = createHash("sha256").update(installer).digest("hex");
  const fileName = basename(a.installer);

  async function api(method, path, body) {
    let res;
    try {
      res = await fetch(`${apiBase}${path}`, {
        method,
        headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/json" },
        body: body === undefined ? undefined : JSON.stringify(body),
      });
    } catch (err) {
      fail(`${method} ${path}: the API could not be reached (${err.message})`);
    }
    const text = await res.text();
    let json;
    try {
      json = JSON.parse(text);
    } catch {
      json = undefined;
    }
    return { status: res.status, json, text };
  }

  const declared = await api("POST", "/api/v1/release-uploads", {
    version: a.version,
    notes_en: notesEn,
    notes_fr: notesFr,
    git_commit: a["git-commit"],
    code_signed: a["code-signed"] === "true",
    artifacts: [
      {
        target: a.target ?? "windows",
        arch: a.arch ?? "x86_64",
        file_name: fileName,
        byte_size: statSync(a.installer).size,
        sha256,
        updater_signature: signature,
      },
    ],
  });
  if (declared.status !== 201) {
    fail(
      `declare refused (${declared.status}): ${declared.json?.message ?? declared.json?.error ?? declared.text}`,
    );
  }
  console.log(
    `Declared ${a.version} as ${declared.json.release_id}; uploading ${fileName} (${installer.length} bytes, sha256 ${sha256})`,
  );

  for (const upload of declared.json.uploads) {
    let put;
    try {
      put = await fetch(upload.url, { method: upload.method, body: installer });
    } catch (err) {
      fail(
        `upload of ${fileName} failed: ${err.message} (is the object-storage public endpoint deployed?)`,
      );
    }
    if (!put.ok) fail(`upload of ${fileName} refused (${put.status}): ${await put.text()}`);
  }

  const completed = await api(
    "POST",
    `/api/v1/release-uploads/${declared.json.release_id}/complete`,
    {},
  );
  if (completed.status !== 200 || completed.json?.state !== "registered") {
    fail(
      `registration refused (${completed.status}): ${completed.json?.message ?? completed.json?.error ?? completed.text}`,
    );
  }
  console.log(
    `Registered ${a.version} (${completed.json.release_id}) as a draft: publish it in Console > Updates > Releases.`,
  );
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((err) => {
    if (!(err instanceof Stop)) console.error(`::error::release publish: ${err.stack ?? err}`);
    process.exitCode = 1;
  });
}
