# SEAL security posture

What is hardened, what is trusted, and the roadmap to mainnet.

## Threat model

- **Stealth layer (graph privacy).** Pure Ed25519 ECDH, no ZK. A payer funds a
  one-time address; only the holder of the scan key can detect it and only the
  holder of the spend key can move it. The announcer stores `R` + a view tag,
  both public by construction; they reveal nothing without the scan key. Stealth
  covers the payer-recipient link; the default path then routes through the
  relayer and the pool so the funding and fee trail is closed too.
- **Pool layer (value privacy).** Tornado/Semaphore-style: deposit a fixed
  denomination against `precommit = Poseidon(nullifier, secret)`; withdraw with a
  Groth16 proof of membership, binding the recipient and a single-use nullifier.
  The payout comes from the vault PDA via `invoke_signed`; there is no operator
  or authority key. Anonymity set = the count of same-denomination deposits.
- **Relayer.** Trusted for liveness/censorship only, never for custody or
  privacy. It pays fees so a fresh account never originates one. It cannot move
  user funds (the pool pays out of the vault, not the relayer).

## Hardened

- **Relayer cannot be drained.** It refuses any transaction that is not feePayer
  = relayer, references an unlisted program, references the relayer inside a
  System instruction (no relayer-funded transfer/create), or whose simulated cost
  to the relayer exceeds `MAX_RELAYER_COST_LAMPORTS`. Plus a minimum-balance
  floor, per-IP and global rate limits, and tx size / instruction-count caps.
  Verified: a "transfer the relayer's SOL to me" request is rejected; legitimate
  sweep / pool-deposit / withdraw relays pass.
- **Indexer is untrusted.** It serves only public data (announcements, pool
  leaves). The client rebuilds its own Merkle path locally, so the indexer never
  learns which leaf a recipient withdraws.
- **Single-use enforcement on-chain.** Gate nullifier, withdraw nullifier, and
  announcement PDAs are `init` (not `init_if_needed`), so replays fail.
- **Value is bound to lamports moved.** The deposit pins the leaf value to the
  amount actually transferred, and denominations are fixed, so same-denom
  deposits are indistinguishable and the amount cannot be forged.

## Roadmap to mainnet

SEAL runs on devnet today. The path to mainnet:

1. **Multi-party trusted-setup ceremony.** The current proving keys come from an
   initial setup; a multi-party ceremony with independent contributors and a
   public randomness beacon (`CEREMONY.md`) produces the keys baked into the
   mainnet program.
2. **External review** of the circuits, the Anchor programs, and the SDK.
3. **Anonymity set.** A withdrawal's privacy scales with the same-denomination
   deposits in the pool; mainnet liquidity widens it.
4. **Operational.** Burn or multisig the program upgrade authority, and rebuild
   the pool program for the current SBPF target on the mainnet deploy.

## Reporting

SEAL is on devnet. Security issues: open a private report to the maintainer
before public disclosure.
