import {
  X509Certificate,
  createHash,
  generateKeyPairSync,
  randomBytes,
  sign,
} from "node:crypto";

// Minimal fixed-template DER for one self-signed host certificate. Every field
// is constant except the key, serial and validity, so no caller input reaches
// the encoder. The result is parsed and its signature verified before use.
const tag = (type, ...parts) => {
  const body = Buffer.concat(parts);
  const n = body.length;
  const length =
    n < 0x80
      ? Buffer.from([n])
      : n < 0x100
        ? Buffer.from([0x81, n])
        : Buffer.from([0x82, n >> 8, n & 0xff]);
  if (n > 0xffff) throw new Error("Certificate field too large.");
  return Buffer.concat([Buffer.from([type]), length, body]);
};
const sequence = (...parts) => tag(0x30, ...parts);
const oid = (dotted) => {
  const [a, b, ...rest] = dotted.split(".").map(Number);
  const bytes = [40 * a + b];
  for (const value of rest) {
    const chunk = [value & 0x7f];
    for (let v = value >> 7; v > 0; v >>= 7) chunk.unshift(0x80 | (v & 0x7f));
    bytes.push(...chunk);
  }
  return tag(0x06, Buffer.from(bytes));
};
const integer = (bytes) =>
  tag(0x02, bytes[0] & 0x80 ? Buffer.concat([Buffer.from([0]), bytes]) : bytes);
const bitString = (bytes) => tag(0x03, Buffer.from([0]), bytes);
const octetString = (bytes) => tag(0x04, bytes);
const time = (date) => {
  const iso = date.toISOString().replace(/[-:T]/g, "").slice(0, 14) + "Z";
  // RFC 5280: UTCTime through 2049, GeneralizedTime from 2050.
  return date.getUTCFullYear() < 2050
    ? tag(0x17, Buffer.from(iso.slice(2)))
    : tag(0x18, Buffer.from(iso));
};
const ecdsaWithSHA256 = sequence(oid("1.2.840.10045.4.3.2"));
const name = sequence(
  tag(0x31, sequence(oid("2.5.4.3"), tag(0x0c, Buffer.from("Point Guard")))),
);

/** Mint a self-signed P-256 identity valid for `days`. Returns PEM strings. */
export function mintHostIdentity({ days = 3650, now = new Date() } = {}) {
  const { privateKey, publicKey } = generateKeyPairSync("ec", {
    namedCurve: "P-256",
  });
  const spki = publicKey.export({ type: "spki", format: "der" });
  const keyId = createHash("sha1").update(spki.subarray(-65)).digest();
  const serial = randomBytes(16);
  serial[0] &= 0x7f;
  serial[0] |= 0x01;
  const notBefore = new Date(Math.floor(now.getTime() / 1000) * 1000 - 60000);
  const notAfter = new Date(notBefore.getTime() + days * 86400000);
  const extensions = tag(
    0xa3,
    sequence(
      sequence(
        oid("2.5.29.19"),
        tag(0x01, Buffer.from([0xff])),
        octetString(sequence(tag(0x01, Buffer.from([0xff])))),
      ),
      sequence(oid("2.5.29.14"), octetString(octetString(keyId))),
      sequence(
        oid("2.5.29.35"),
        octetString(sequence(tag(0x80, keyId))),
      ),
    ),
  );
  const tbs = sequence(
    tag(0xa0, integer(Buffer.from([2]))),
    integer(serial),
    ecdsaWithSHA256,
    name,
    sequence(time(notBefore), time(notAfter)),
    name,
    spki,
    extensions,
  );
  const der = sequence(tbs, ecdsaWithSHA256, bitString(sign("sha256", tbs, privateKey)));
  const certificate = new X509Certificate(der);
  if (!certificate.verify(publicKey) || !certificate.checkIssued(certificate))
    throw new Error("Minted identity failed self-verification.");
  return {
    cert: certificate.toString(),
    key: privateKey.export({ type: "pkcs8", format: "pem" }).toString(),
  };
}
