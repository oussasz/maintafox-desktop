#!/usr/bin/env node
/**
 * Verifies a Tauri updater signature (`<installer>.sig`) against the committed public key
 * (`src-tauri/updater/maintafox-updater.pub`) before a release is registered (security review S7).
 * A signing secret that does not match the key built into the app would make every installed copy
 * refuse the update; this check finds it in the release workflow instead of on customers' PCs.
 *
 *   node scripts/release/verify-updater-signature.mjs --file <setup.exe> --signature <setup.exe.sig> \
 *     --public-key src-tauri/updater/maintafox-updater.pub --expected-file-name Maintafox_1.2.0_x64-setup.exe
 *
 * Format (minisign, as written by `tauri signer sign`): the .pub and .sig files are base64 of the
 * minisign text files. Public key: "Ed" + key id (8 bytes) + Ed25519 key (32 bytes). Signature:
 * algorithm ("ED": Ed25519 over BLAKE2b-512 of the file; "Ed": over the file itself) + key id +
 * signature (64 bytes), then a trusted comment ("timestamp:<n>\tfile:<name>") whose global
 * signature covers the signature bytes followed by the comment. All of it is checked here.
 */
import { createHash, createPublicKey, verify } from "node:crypto";
import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

/** A verification failure, with a message for the release log. */
export class SignatureError extends Error {}

function fromBase64Text(value, what) {
  const text = Buffer.from(value.trim(), "base64").toString("utf8");
  if (!text.includes("\n"))
    throw new SignatureError(`${what} is not a base64-encoded minisign file`);
  return text.split("\n").map((line) => line.replace(/\r$/, ""));
}

function decodeLine(line, length, what) {
  const bytes = Buffer.from((line ?? "").trim(), "base64");
  if (bytes.length !== length)
    throw new SignatureError(`${what} has ${bytes.length} bytes, expected ${length}`);
  return bytes;
}

/** Reads a Tauri public key file's content: { keyId, key } (raw 32-byte Ed25519 key). */
export function parsePublicKey(pubFileContent) {
  const lines = fromBase64Text(pubFileContent, "the public key");
  const raw = decodeLine(
    lines.find((l) => l && !l.startsWith("untrusted comment:")),
    42,
    "the public key",
  );
  if (raw.subarray(0, 2).toString("latin1") !== "Ed")
    throw new SignatureError("the public key is not an Ed25519 minisign key");
  return { keyId: raw.subarray(2, 10), key: raw.subarray(10) };
}

/** Reads a Tauri signature file's content: { algorithm, keyId, signature, trustedComment, globalSignature }. */
export function parseSignature(sigFileContent) {
  const lines = fromBase64Text(sigFileContent, "the signature");
  const trustedIndex = lines.findIndex((l) => l.startsWith("trusted comment: "));
  if (trustedIndex < 2) throw new SignatureError("the signature has no trusted comment");
  const raw = decodeLine(lines[trustedIndex - 1], 74, "the signature");
  return {
    algorithm: raw.subarray(0, 2).toString("latin1"),
    keyId: raw.subarray(2, 10),
    signature: raw.subarray(10),
    trustedComment: lines[trustedIndex].slice("trusted comment: ".length),
    globalSignature: decodeLine(lines[trustedIndex + 1], 64, "the global signature"),
  };
}

function ed25519Verify(key, message, signature) {
  const publicKey = createPublicKey({
    key: { kty: "OKP", crv: "Ed25519", x: key.toString("base64url") },
    format: "jwk",
  });
  return verify(null, message, publicKey, signature);
}

/**
 * Throws a SignatureError unless `sigFileContent` is a valid signature of `data` by the key in
 * `pubFileContent`, whose trusted comment names `expectedFileName` (exact, case-sensitive).
 */
export function verifyUpdaterSignature({ data, sigFileContent, pubFileContent, expectedFileName }) {
  const pub = parsePublicKey(pubFileContent);
  const sig = parseSignature(sigFileContent);
  if (!sig.keyId.equals(pub.keyId)) {
    throw new SignatureError(
      `the installer was signed with key ${Buffer.from(sig.keyId).reverse().toString("hex").toUpperCase()}, ` +
        `but the app trusts key ${Buffer.from(pub.keyId).reverse().toString("hex").toUpperCase()} ` +
        "(src-tauri/updater/maintafox-updater.pub): TAURI_SIGNING_PRIVATE_KEY is not the owner's updater key",
    );
  }
  let signed;
  if (sig.algorithm === "ED") signed = createHash("blake2b512").update(data).digest();
  else if (sig.algorithm === "Ed") signed = data;
  else throw new SignatureError(`unknown signature algorithm ${JSON.stringify(sig.algorithm)}`);
  if (!ed25519Verify(pub.key, signed, sig.signature)) {
    throw new SignatureError(
      "the signature does not match the installer and the committed public key",
    );
  }
  const global = Buffer.concat([sig.signature, Buffer.from(sig.trustedComment, "utf8")]);
  if (!ed25519Verify(pub.key, global, sig.globalSignature)) {
    throw new SignatureError("the trusted comment's signature is invalid");
  }
  const file = sig.trustedComment
    .split("\t")
    .find((field) => field.startsWith("file:"))
    ?.slice("file:".length);
  if (file !== expectedFileName) {
    throw new SignatureError(
      `the signature was made for file '${file ?? ""}', not '${expectedFileName}'`,
    );
  }
}

function args(argv) {
  const out = {};
  for (let i = 0; i < argv.length; i += 2) {
    if (!argv[i].startsWith("--") || argv[i + 1] === undefined)
      throw new SignatureError(`bad argument ${argv[i]}`);
    out[argv[i].slice(2)] = argv[i + 1];
  }
  for (const required of ["file", "signature", "public-key", "expected-file-name"]) {
    if (!out[required]) throw new SignatureError(`--${required} is required`);
  }
  return out;
}

function main() {
  try {
    const a = args(process.argv.slice(2));
    verifyUpdaterSignature({
      data: readFileSync(a.file),
      sigFileContent: readFileSync(a.signature, "utf8"),
      pubFileContent: readFileSync(a["public-key"], "utf8"),
      expectedFileName: a["expected-file-name"],
    });
    console.log(
      `Updater signature verified against ${a["public-key"]} for ${a["expected-file-name"]}.`,
    );
  } catch (err) {
    console.error(
      `::error::updater signature: ${err instanceof SignatureError ? err.message : (err.stack ?? err)}`,
    );
    process.exitCode = 1;
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) main();
