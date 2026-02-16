# SEAL v2: trustless shielded deposit pool (design + spike log)

*This is the working design and de-risking log kept during development: the rationale, the spikes that validated each risky step, the devnet evidence, and the honest limitations. Some sections are historical planning notes, including checklist items from the original build sequence.*

Status: SHIPPED + verified live on devnet (2026-06-17). Real deposit -> on-chain Poseidon leaf -> on-chain incremental Merkle tree -> root == the off-chain circuit's merkleRoot -> Groth16 verify -> single-use nullifier, all confirmed on devnet.

## DEVNET EVIDENCE (program seuH78RmBPVzoKToLQVEZrDvuL5jDNBSbptozWK9PEm)

- Upgrade to v2: tx 5ygewx8qS5ErXawP5ujfeFo7nZMxw9ArGZcJkT8qVg8FShbEbArmk5yb8ht9Jecn32svHNgbtv7ocaPYJNMQ3FK1
- deposit (real 5000 lamports -> leaf -> tree): tx 4heNX3f2gES8EuYCMGfT8FioHLVqAQMXW8LGvuMCnxR5e4n7U4icaX6nbh6YMVPkwCMsUwQwMEUujgUMfPuXhE5q
- seal_verify (Groth16 verify, gate opened): tx 5hGuppz6jS2c9tVUU1D5anewQjUP47KFv9RcpbMEasJgV4uojkUA4cd9JqxxqLv6kA9PYMWWZFzp23igBiZ8J5U6 (109,114 CU; program 108,964)
- double-use of the same nullifier: rejected (nullifier PDA already in use).
- WITHDRAW (added 2026-06-17, upgrade tx 4Qubpzmqe7UvEyUktmoBbB3YK54FoKyvN1AKknKR5ixuYte1wtAvhxs7DQrUmQcavRRidHAtYrjw8mY5Nq79XgTx): withdraw circuit (value public + global Poseidon(nullifier) nullifier + recipient binding) + withdraw instruction (vk_withdraw, recipientHash == Poseidon(hi16,lo16) of payout pubkey, trustless payout from the vault PDA via invoke_signed). Devnet: withdraw tx 3ABt3E8MnTMWJgXJTf3RNifF4yz7mEwPVDa1116Vw6gfEvAj1xt6q1UEiAvCpvihBGTbLbcs3wVCvk57bvj1BvCD (117,074 CU), recipient balance delta = exactly 5000 lamports paid out of the vault; a second withdraw with the same nullifier is rejected (wnf PDA already in use). So the full cycle deposit -> zk receipt gate -> trustless withdraw is live, single-use enforced on both the gate and the withdraw, no operator/authority/auditor anywhere.
- FIXED DENOMINATIONS (added 2026-06-17, upgrade tx 4zWgaTcE7Xip1TFtpgupxKMRbJgbWU83p1CPBkYDrZmQTa85Y7ARjGVxjoiPQc7K8ugKYJSt3aiTauL7TCuPMKMt): deposit now requires amount in a fixed DENOMS set (devnet test set [5_000, 0.05, 0.1, 1 SOL]), so same-denomination deposits are indistinguishable and the withdraw's revealed value only narrows the set to "deposits of that denomination". This closes the v1 amount-correlation leak. A non-denomination deposit is rejected (BadDenom).
- REPEATABLE FULL e2e (scripts/e2e_devnet.mjs): one run, fresh random secrets, picks a valid denomination (0.1 SOL), deposits, rebuilds the Merkle path for the appended leaf from on-chain filled_subtrees + zeros, proves the gate and the withdraw with snarkjs, and asserts all of: deposit committed, off-chain root == on-chain root, gate opens, gate replay rejected, withdraw pays exactly the denom (recipient delta == value), withdraw replay rejected, non-denom deposit rejected. Latest pass: deposit tx 5KvdK2jHMyCaEu4ABNK6fmMzijmgPZHQz7a8YfMPq1HMyHv3F94ZGcNktrtda4FQZ8DDo3ezX2tycZWGkCEBLX4i, gate 3C1oyRXxeA2Y4fyotnNAAPgtLYJfXr83NVLVpuabEckrphtbjAEBdJjGwYqfYjRENV7zeFouYoFRxUi5APHuTeLr, withdraw 56fSR8SKfFngP1qmUa9cXKnrzU4myMZjwDz7iE24MpG4JfJyg3am9ctTZ7ap4hSN58CsRxVCdcokU79Fojjz3F2o. Because real denominations exceed rent-exemption, the earlier rent pre-funding step is no longer needed for realistic amounts.

## Honestly NOT done (out of solo reach or mainnet-stage)
- External audit: requires a third-party firm. Not performed; never claim "audited".
- Real multi-party trusted-setup ceremony: the zkey is still a single-party dev ptau. Groth16 soundness for real value needs a real MPC ceremony with independent participants; cannot be done solo.
- Mainnet + burning/multisig the upgrade authority: deferred to mainnet (burning it on devnet would block further upgrades).
- Real anonymity set: depth-20 holds ~1M leaves but practical privacy needs many real same-denom deposits (adoption), which a devnet PoC does not have.
- Withdraw fee-payer/timing linkage: the withdraw tx is signed by a public payer; a relayer for gas-level unlinkability is future work.
- On-chain tree after deposit: next_leaf_index=1, current_root=0cb78fb64afe753c19d19f140bac276badd7a5435b2377643e3ae76dcb054dac == off-chain circuit merkleRoot (5752079060189312976705165946397342993546852137952364424569440771955470192044). Byte-identical: the real sol_poseidon syscall matches circomlib on devnet, not just in host tests.

So the v1 weakness is gone: amount>=threshold is now backed by a real deposited amount, and the root is a function of deposits with no authority key. The remaining roadmap is stealth/unlinkability, a larger anonymity set, a real multi-party MPC ceremony, an external audit, and mainnet.

## What changed vs v1 (implemented)

- circuits/seal.circom: leaf = Poseidon(value, label, Poseidon(nullifier, secret)); range gate over the program-pinned `value`; depth 8 -> 20; public-input order unchanged. Recompiled, new vkey/vk.rs, proof verifies (snarkjs + groth16-solana host test).
- program/programs/seal/src/lib.rs: deleted Registry/init_registry/set_root/authority. Added TreeState PDA + init_tree (computes zeros on-chain) + deposit (CPI value in, sol_poseidon leaf, incremental insert, ROOT_HISTORY ring). seal_verify now requires proof root in the roots ring. solana-poseidon 2.3.13 dep. Large account boxed (Box<Account<TreeState>>) to fit the BPF stack.
- scripts: gen_input.mjs (v2 witness + deposit.json), e2e_devnet.mjs (init_tree -> fund vault -> deposit -> seal_verify -> replay reject).
- vault must be pre-funded to rent-exemption once (a 5000-lamport deposit alone cannot make a 0-data PDA rent-exempt).

---
(original design + spike log below)

## Why v2

v1 (devnet PoC) weakness: the leaf `Poseidon(secret, amount)` is self-chosen (prover invents the amount) and the registry root is posted by a single authority key. So `amount >= threshold` is meaningless and the design is centralized. Structurally it is Semaphore + a range check.

## Chosen design

On-chain SHIELDED DEPOSIT POOL with a custom on-chain incremental Poseidon-BN254 Merkle tree (Tornado / Privacy-Pools pattern ported to Solana).

- The SEAL program receives real value via a System/SPL CPI in the same `deposit` instruction, measures the actual amount, computes the leaf on-chain with the `sol_poseidon` syscall, and appends it to its own incremental Poseidon tree (filledSubtrees + precomputed zeros), pushing the new root into a ROOT_HISTORY ring buffer.
- `set_root` and `Registry.authority` are DELETED. The root becomes a deterministic function of deposits = decentralized.
- Leaf preimage changes to `leaf = Poseidon(value, label, precommitment)` where `precommitment = Poseidon(nullifier, secret)`. `value` is pinned by the program at deposit time, so the range gate finally means something. `label` is a program-set scope/nonce the prover cannot forge.
- Tree depth grows 8 -> 20 (~1M leaves).
- KEPT unchanged: groth16-solana verify (measured 107,838 CU on devnet, tx dXo3cMP5...UAm), the nullifier PDA single-use, the public-inputs order `[merkleRoot, threshold, contextHash, nullifierHash]`, the Merkle fold template.

## Both walls cleared

- VRL proving wall (ZK state inclusion): AVOIDED. Leaf-to-value binding + Merkle append are plain on-chain Rust via `sol_poseidon`. Nothing proves Solana state inside a SNARK. The chain's consensus is the source of truth that value moved, exactly like Tornado/Railgun/Privacy-Pools. depth-20 insert ~= 20 hashes ~= ~15.7k CU; verify is a separate tx at 107,838 CU.
- Operator/auditor wall: AVOIDED iff (1) delete set_root + authority, (2) use the custom Poseidon tree (NOT SPL Account Compression, which is keccak not Poseidon; NOT a watcher), (3) no ASP / viewing key / auditor. Residual trust = Groth16 trusted setup + program upgrade authority (burn/multisig), same class as Tornado.

## Rejected (with reason)

- Watcher/indexer attestation: reintroduces a trusted seer that sees amount<->leaf links and can mint fake leaves = an auditor key renamed.
- Stealth "pay my normal wallet, get a trustless receipt": requires in-SNARK Solana state inclusion = the VRL wall (OOMs 16GB, needs WSL2).
- SPL Account Compression ConcurrentMerkleTree: hashes with keccak256, not Poseidon, so it cannot back a Poseidon circuit without proving keccak in-circuit (constraint blowup).

## Product reality (honest)

It CANNOT be "someone pays my ordinary wallet and I get a trustless receipt" (that is the VRL wall). The honest product: a payer DEPOSITS >= X into the SEAL pool against the recipient's precommitment, and the recipient later PROVES "I hold a receipt for >= X in this pool" with no link to the deposit and no operator. That is Tornado-class privacy + a native range gate + scoped single-use nullifiers, with no viewing key and no auditor. That is the one survivable claim.

## SPIKE LOG (verified on this 16GB Windows machine, native, no WSL2)

### Spike 0 (GATE): circom <-> on-chain Poseidon byte-identical -- PASS
`circomlibjs 0.1.7` (off-chain, used by gen_input.mjs) vs `light-poseidon 0.2.0` (the crate `sol_poseidon` wraps), via `Poseidon::<Fr>::new_circom(n)` + `into_bigint().to_bytes_be()`:

- P([1,2]) = 7853200120776062878684798364095072458815029376092732009249414926327459813530 (BE 115cc0f5...4417189a) -- identical
- depth-2 root = 10335207501481666499037275020420411903216847211369284971187939504066770527385 -- identical
- 3-input leaf Poseidon(value,label,precommit) = 1021490356871162079490305060965324153498871171979559448248716413162705271789 -- identical

Conclusion: the #1 risk (Poseidon param/endianness mismatch) is eliminated. BE is the consistent byte convention (matches groth16-solana public inputs). 2-input, 3-input, and multi-level fold all match.

### Step 1: incremental tree root == full-recompute -- PASS
Tornado filledSubtrees+zeros incremental insert (the exact algorithm the deposit ix will run on-chain) vs an off-chain full-tree recompute, depth 4, 5 sequential leaves. All 5 per-insert roots match across both, and the Rust (light-poseidon) final root equals the off-chain circomlibjs root:

ROOT5 = 2950969168947213669054309960787367253889194047464357038902518255561662322590 (both sides identical)

Conclusion: the on-chain incremental Poseidon tree will produce roots the Circom membership proof verifies against.

Spike code: `scripts/spike_poseidon.mjs`, `scripts/spike_tree.mjs`, `spikes/poseidon_check/`.

## Remaining build sequence

- [ ] Spike 0b: confirm `sol_poseidon` enable feature gate is active on devnet (low risk, syscall live since v1.17). Verify, do not assume.
- [ ] STEP 2: `deposit` instruction (SOL first): CPI value in, assert measured == declared, leaf = Poseidon(value, label, precommitment) on-chain, incremental insert, push root, emit Deposit event. Replace Registry with TreeState PDA { current_root, next_leaf_index, filled_subtrees[DEPTH], roots[ROOT_HISTORY], roots_head }. Delete set_root + authority. Cargo: add light-poseidon (or sol_poseidon via solana-program); match BE byte order confirmed in Spike 0.
- [ ] STEP 3: edit seal.circom -> 3-input leaf `Poseidon(value,label,Poseidon(nullifier,secret))`, range gate over `value`, keep public-input order, depth Seal(8)->Seal(20). Recompile (check pot14_final.ptau covers depth-20; else download bigger ptau). Regen zkey+vkey, rerun vk_to_rust.mjs.
- [ ] STEP 4: seal_verify -> require proof root is in tree_state.roots[] ring (replace single-root check). Keep groth16 verify + nullifier PDA untouched.
- [ ] STEP 5: full devnet e2e: init tree -> deposit (real value moves, root advances) -> rebuild path off-chain from event -> prove in browser -> seal_verify opens gate -> replay same nullifier rejected.
- [ ] STEP 6 (hardening, not PoC-blocking): fixed deposit tiers (cut amount correlation), optional untrusted withdraw relayer, burn/multisig upgrade authority, real multi-party trusted-setup ceremony for Seal(20).

## Honest risks still open

- Poseidon endianness in the actual `sol_poseidon` syscall call (Spike 0 proved the crate; confirm the syscall's Endianness::BigEndian path matches when wired in STEP 2).
- Amount-correlation leak with arbitrary deposit amounts even at depth-20 (mitigate with fixed tiers).
- Trusted setup is a dev ptau today; needs a real MPC ceremony before real value.
- Single tree_state PDA serializes deposits; ROOT_HISTORY ring covers stale-root races for normal throughput.
- Label binding: the circuit must tie proven `value` to a `label` the prover cannot forge; confirm no high-value+forged-label bypass.
