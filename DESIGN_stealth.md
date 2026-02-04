# SEAL stealth layer: design and evidence

The pool (see `DESIGN_v2_pool.md`) hides **how much** moved. The stealth layer
hides **who received it**. They are orthogonal axes and compose. This document
covers the stealth layer only: the cryptography, the on-chain announcer, the
honest privacy limits, and the devnet evidence.

## 1. Scheme: dual-key stealth addresses on Ed25519

A recipient holds two raw scalars (not seed-derived, see Fix B below):

- `p_scan`  scan key, may be delegated to an untrusted indexer
- `p_spend` spend key, never leaves the device

Meta-address (published, e.g. as a link or QR): `(P_spend = p_spend·B, P_scan = p_scan·B)`.

Payer, per payment:

```
r          random scalar
R          = r·B                         (ephemeral pubkey, published in the announcement)
S          = r·P_scan                    (ECDH shared secret)
s_h        = H(S) mod L                  (H = SHA-512)
view_tag   = H(S)[0]                     (1 byte, scan accelerator)
P_stealth  = P_spend + s_h·B             (the one-time address to fund)
```

Recipient, scanning:

```
S'         = p_scan·R = r·p_scan·B = S   (same shared secret)
view_tag'  = H(S')[0]                    (reject ~255/256 of foreign announcements with no point math)
P_stealth' = P_spend + H(S')·B = P_stealth
```

Recipient, spending: the one-time secret is the additive scalar

```
p_stealth  = (p_spend + s_h) mod L       and   p_stealth·B = P_stealth   ✓
```

This is the EIP-5564 / Monero-lineage construction ported to the prime-order
Ed25519 group. No new cryptography is claimed.

## 2. Fix B: signing with a raw additive scalar

`p_stealth` is an additive combination of scalars, **not** a 32-byte seed. The
standard Ed25519 API (`SigningKey::from_seed`) hashes-and-clamps the seed to
derive the scalar, which would produce a different key entirely. So we sign with
a hand-rolled RFC-8032 signer that takes the scalar directly:

```
a          = p_stealth
nonce r    = SHA512( SHA256("SEAL-nonce" || aLE) || M ) mod L     (deterministic, message-bound)
R          = r·B
k          = SHA512( R || P_stealth || M ) mod L
S          = (r + k·a) mod L
signature  = R || S        (64 bytes, verifies under ed25519_dalek verify_strict)
```

The nonce is **deterministic and binds the whole message** `M`. A random nonce
(used in the first spike) risks catastrophic scalar disclosure on nonce reuse;
binding `M` removes that class of bug. The clamp trap (feeding `p_stealth` to a
seed API) is covered by a unit test that asserts the seed API does **not**
reproduce `P_stealth`.

## 3. On-chain announcer

Solana does not retain transaction logs, so the ephemeral `R` cannot be emitted
and forgotten: it must live in durable account state. The `announcer` program
(`program/programs/announcer`) writes each announcement to its **own PDA seeded
by `R`**:

```
seeds = ["ann", R]     →     { r: [u8;32], view_tag: u8, scheme: u8, slot: u64 }
```

`R` is unique per payment, so concurrent payers never write the same account:
no serialization, no timing side-channel from a shared ring buffer. The payer
funds `P_stealth` and calls `announce(R, view_tag, scheme)` in the **same
transaction**, so funding and announcement commit atomically.

A recipient (or an untrusted indexer holding only `p_scan`) enumerates
announcements with `getProgramAccounts` filtered by data size, then runs the
1-byte view-tag prefilter locally. The announcer learns nothing it could not
already see: `R` and `view_tag` are public by construction and carry no link to
the recipient without `p_scan`.

`init` (not `init_if_needed`) makes a duplicate `R` a hard error rather than an
overwrite. The program holds no funds and has no authority; `announce` is
permissionless.

## 4. Composition with the pool, and the honest privacy limit

Stealth alone is **graph privacy with a known failure mode**. The Umbra
deanonymization study (arXiv 2308.01703) shows recipients are linkable a large
fraction of the time through funding and consolidation behavior. On Solana this
is worse: a freshly funded stealth account holds zero SOL, so it cannot pay its
own first transaction fee, which pushes users toward fee-payer or consolidation
patterns that leak the link.

Therefore SEAL does **not** ship "stealth-only payment links" as a privacy
claim. The shipped privacy path is:

```
payer → stealth address (graph privacy) → relayer pays the first fee → pool deposit (value privacy) → withdraw
```

The relayer is mandatory in v1 precisely because it removes the
self-pay-first-fee leak. Marketing copy must not claim stealth-only anonymity.

## 5. Evidence (devnet)

- Raw-scalar stealth spend accepted by Solana (random-nonce spike):
  tx `5AAxYodvT49bDTLQCW86LfHD66qVV1XgP6iAShCdineG5wGJzFCf85mgpCZ1n1uAX6aCfpMWZwrdtxreMyS9PfTv`
- Production deterministic-nonce signer, full SDK path (derive → fund → scan →
  sweep): tx `58nBof8EzAm3bNE98eCgXhMtc3GUmjsBAFYta8iRZHiekJKTQ6rSShG4X4QL5s3GsNrUJQYWu321zdU8w8KezBLa`
- SDK crypto core: 550/550 unit assertions (`sdk/stealth.test.mjs`) covering
  additive consistency, ECDH symmetry, the clamp trap, seed-API non-reproduction,
  deterministic nonce, and signature verification.
- Announcer wire contract pinned from both ends against the Anchor-generated
  IDL: the SDK client (`sdk/contract.test.mjs`, 12/12) and the program's own
  constants (`program/programs/announcer/tests/announce.rs`, host test) agree on
  one discriminator, account order, PDA seed set, and arg layout.
- Announcer deployed and exercised live on devnet (SBPFv3): the full flow
  (fund + announce, scan, sweep) passes end-to-end, `sdk/e2e_stealth.mjs` 6/6
  (see section 6 for the program id and transaction signatures).

## 6. Building and deploying the announcer (SBPFv3)

devnet now **requires SBPFv3** for new program deployments (SIMD-0377, active
since epoch 1069); v0/v1/v2 deployments are refused with "sbpf_version required
by the executable which are not enabled". The announcer must therefore be built
for v3:

```
cargo-build-sbf --arch v3 --manifest-path programs/announcer/Cargo.toml
solana program deploy target/deploy/announcer.so \
  --program-id <announcer-keypair>.json -k <fee-payer>.json -u devnet
```

This needs a toolchain whose platform-tools ships the v3 sysroot (v1.53+, e.g.
Anza solana-cli 4.0.x). On a host where installing that platform-tools is
blocked, build and deploy from Linux/WSL. The announcer is live on devnet at
`seaWHA64tVzN8yfa33bE6cvqKRSxVp3R6c7Ts5NXPM9` (deploy tx `3r2neDFe32soRAEHaTn9xgCvFQt5aTyqvBJwGuRfPKPFxf75Ccuoyf65TqhFUPjTnmR8bgpAxbBGKaLA5Fm5gmLy`).

The full flow is verified end-to-end on devnet (`sdk/e2e_stealth.js`, 6/6): a
payment funds a derived stealth address and announces atomically, the recipient
finds it by scanning announcements with the view tag, and sweeps it. pay tx
`4Q9frmFEtWVKuc7cJFU7TzD8KkwC6uTuBCz7xKjYu8K5BSezaXdV8fv6HpkyjhJ45CAEyTS5FkRAVQMEVgVHRxi9`,
sweep tx `3xmsekG3nFmJDBt7gvTtWXsMgVxHjSvB6P9N3y6EX2RkNagtzWKfgBGmKd4z1XBH19FyDETm2R4RzK1FsSeUKtE5`.
