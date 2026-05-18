# Roadmap

## Shipped

- [x] Announcer program: write-once stealth announcement PDAs keyed by `R`
- [x] Wire-format pins against the generated IDL (both ends)
- [x] Shielded pool: incremental Poseidon (BN254) Merkle tree, depth twenty
- [x] Fixed deposit denominations, value pinned to lamports moved
- [x] Proof-of-receipt gate `seal_verify`, CPI-able, per-context nullifier
- [x] Groth16 verifier over `alt_bn128` syscalls
- [x] ZK withdraw with recipient binding and a global single-use nullifier
- [x] Trustless vault payout via `invoke_signed`, no operator key
- [x] Thirty-root history so slightly stale proofs still verify
- [x] Devnet deployment of both programs
- [x] Trusted-setup ceremony documented for the mainnet keys
