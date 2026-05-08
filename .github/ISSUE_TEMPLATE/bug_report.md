---
name: Bug report
about: A proof that should verify and does not, a deposit/withdraw that misbehaves, or any on-chain incorrectness
title: "[bug] "
labels: bug
---

## What happened

A clear description of the behavior you saw on-chain or in the host tests.

## What you expected

What the program should have done instead.

## Reproduction

- Cluster: devnet / localnet
- Program: pool / announcer
- Instruction: init_tree / deposit / seal_verify / withdraw / announce
- Steps, transaction signature, or a failing `cargo test` name

## Environment

- anchor --version:
- solana --version:
- rustc --version:

## Notes

If the issue is a proof that fails to verify, include the public inputs and whether the
on-chain Merkle root matched the proof's root. A wire-format change in the SDK or circuits
can surface here.
