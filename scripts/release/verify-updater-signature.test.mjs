import { createHash, generateKeyPairSync, randomBytes, sign } from "node:crypto";
import { describe, expect, it } from "vitest";

import { SignatureError, verifyUpdaterSignature } from "./verify-updater-signature.mjs";

const FILE = "Maintafox_1.2.0_x64-setup.exe";

/** A minisign key pair in the Tauri file formats (base64 of the minisign text files). */
function keyPair() {
  const { publicKey, privateKey } = generateKeyPairSync("ed25519");
  const raw = Buffer.from(publicKey.export({ format: "jwk" }).x, "base64url");
  const keyId = randomBytes(8);
  const pub = Buffer.concat([Buffer.from("Ed", "latin1"), keyId, raw]).toString("base64");
  const pubFile = Buffer.from(`untrusted comment: minisign public key: TEST\n${pub}\n`).toString(
    "base64",
  );
  return { privateKey, keyId, pubFile };
}

function signatureFile({ privateKey, keyId }, data, { file = FILE, algorithm = "ED" } = {}) {
  const signed = algorithm === "ED" ? createHash("blake2b512").update(data).digest() : data;
  const signature = sign(null, signed, privateKey);
  const trusted = `timestamp:1760000000\tfile:${file}`;
  const global = sign(null, Buffer.concat([signature, Buffer.from(trusted)]), privateKey);
  const sigLine = Buffer.concat([Buffer.from(algorithm, "latin1"), keyId, signature]).toString(
    "base64",
  );
  const text = `untrusted comment: signature from tauri secret key\n${sigLine}\ntrusted comment: ${trusted}\n${global.toString("base64")}\n`;
  return Buffer.from(text).toString("base64");
}

describe("verifyUpdaterSignature", () => {
  const installer = Buffer.from("the signed installer bytes");

  it("accepts a signature by the committed key naming the expected installer", () => {
    const keys = keyPair();
    for (const algorithm of ["ED", "Ed"]) {
      expect(() =>
        verifyUpdaterSignature({
          data: installer,
          sigFileContent: signatureFile(keys, installer, { algorithm }),
          pubFileContent: keys.pubFile,
          expectedFileName: FILE,
        }),
      ).not.toThrow();
    }
  });

  it("refuses a signature made with another key (secret and committed .pub differ)", () => {
    const committed = keyPair();
    const other = keyPair();
    expect(() =>
      verifyUpdaterSignature({
        data: installer,
        sigFileContent: signatureFile(other, installer),
        pubFileContent: committed.pubFile,
        expectedFileName: FILE,
      }),
    ).toThrow(/not the owner's updater key/);
    // Same key id, different key: the cryptographic check refuses it.
    const forged = signatureFile({ ...other, keyId: committed.keyId }, installer);
    expect(() =>
      verifyUpdaterSignature({
        data: installer,
        sigFileContent: forged,
        pubFileContent: committed.pubFile,
        expectedFileName: FILE,
      }),
    ).toThrow(SignatureError);
  });

  it("refuses changed bytes and a signature for another file name", () => {
    const keys = keyPair();
    expect(() =>
      verifyUpdaterSignature({
        data: Buffer.from("tampered"),
        sigFileContent: signatureFile(keys, installer),
        pubFileContent: keys.pubFile,
        expectedFileName: FILE,
      }),
    ).toThrow(/does not match/);
    expect(() =>
      verifyUpdaterSignature({
        data: installer,
        sigFileContent: signatureFile(keys, installer, { file: "Maintafox_1.0.0_x64-setup.exe" }),
        pubFileContent: keys.pubFile,
        expectedFileName: FILE,
      }),
    ).toThrow(/made for file/);
  });

  it("refuses files that are not minisign base64", () => {
    const keys = keyPair();
    expect(() =>
      verifyUpdaterSignature({
        data: installer,
        sigFileContent: "not a signature",
        pubFileContent: keys.pubFile,
        expectedFileName: FILE,
      }),
    ).toThrow(SignatureError);
  });
});
