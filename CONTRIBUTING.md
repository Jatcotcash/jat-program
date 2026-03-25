# Contributing to jat-program

Thanks for taking the time to look at the on-chain side of Jat. This repo holds the
Anchor workspace: the shielded pool and the stealth announcer. The circuits live in
`jat-circuits` and the client SDK in `jat-sdk`, so a change that crosses the wire format
usually touches more than one repo.

## Ground rules

- One logical change per pull request. A verifier tweak, a new account field, and a docs
  pass are three PRs, not one.
- Keep the wire format pinned. The instruction and account discriminators are asserted from
  both ends (`programs/announcer/tests/announce.rs` here, `sdk/contract.test.mjs` in the
  SDK). If you change a struct, update both and say so in the PR body.
- Never commit a keypair, a wallet, or a `.env`. The `.gitignore` already blocks the common
  shapes; if you add a new secret path, extend it in the same commit.

## Building

```
anchor build
cargo test            # host tests: real proof verifies, on-chain tree root matches
```

`cargo test` runs without a validator. It checks that a real circom/snarkjs proof verifies
through `groth16-solana` and that the on-chain Poseidon insert reproduces the proof's root.
If either fails, the program and the circuits have drifted apart.

## Style

- `cargo fmt --all` before you push. CI runs `rustfmt --check`.
- Custom errors over `require!` string messages, immutable reason codes, fail closed.
- Short, domain-dense comments. Explain the privacy invariant, not the syntax.

## Opening a pull request

Fill in the template. Link the issue if there is one. Note whether the change affects the
deployed devnet programs, the IDL, or the SDK wire format, and whether you ran `cargo test`.
