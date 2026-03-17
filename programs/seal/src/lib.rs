//! SEAL v2: trustless private deposit pool with a proof-of-receipt range gate.
//!
//! deposit: the program receives real lamports and mints a commitment leaf
//!   leaf = Poseidon(value, label, precommit), precommit = Poseidon(nullifier, secret),
//! appending it to its OWN on-chain incremental Poseidon-BN254 Merkle tree
//! (sol_poseidon syscall, byte-identical to circomlib). value is pinned to the
//! lamports actually moved in, so amount >= threshold is real. No authority sets
//! the root; the root is a deterministic function of deposits.
//!
//! seal_verify: a Groth16 proof of "I hold a leaf under a recent root with
//! value >= threshold, here is a context-scoped nullifier" is verified on-chain
//! (groth16-solana / alt_bn128), and the nullifier PDA is consumed (single-use
//! per context). Any other program CPIs seal_verify.
//!
//! Circuit public inputs (order MUST match circuits/seal.circom main):
//!   [ merkleRoot, threshold, contextHash, nullifierHash ]

use anchor_lang::prelude::*;
use anchor_lang::solana_program::{program::invoke_signed, system_instruction};
use anchor_lang::system_program::{transfer, Transfer};
use groth16_solana::groth16::Groth16Verifier;
use solana_poseidon::{hashv, Endianness, Parameters};

declare_id!("seuH78RmBPVzoKToLQVEZrDvuL5jDNBSbptozWK9PEm");

pub mod vk;
pub mod vk_withdraw;

pub const DEPTH: usize = 20;
pub const ROOT_HISTORY: usize = 30;

/// Fixed deposit denominations (lamports). All deposits of the same denomination
/// are indistinguishable, so the withdraw's revealed value only narrows the
/// anonymity set to "deposits of this denomination", not to a unique amount.
/// (devnet test set: a tiny tier plus 0.05 / 0.1 / 1 SOL.)
pub const DENOMS: [u64; 4] = [5_000, 50_000_000, 100_000_000, 1_000_000_000];

#[program]
pub mod seal {
    use super::*;

    /// Initialize the pool's incremental Merkle tree. Computes the empty-subtree
    /// zeros on-chain and seeds an empty root. No authority field: nobody can
    /// post a root; only deposits move it.
    pub fn init_tree(ctx: Context<InitTree>) -> Result<()> {
        let z = compute_zeros()?; // z[0..=DEPTH]
        let tree = &mut ctx.accounts.tree_state;
        tree.next_leaf_index = 0;
        for i in 0..DEPTH {
            tree.filled_subtrees[i] = z[i];
            tree.zeros[i] = z[i];
        }
        tree.current_root = z[DEPTH];
        tree.roots = [[0u8; 32]; ROOT_HISTORY];
        tree.roots[0] = z[DEPTH];
        tree.roots_head = 0;
        tree.bump = ctx.bumps.tree_state;
        Ok(())
    }

    /// Deposit real lamports into the pool against a recipient precommitment.
    /// The program pins the leaf's value to the amount actually transferred and
    /// inserts leaf = Poseidon(value, label=leaf_index, precommit) into the tree.
    pub fn deposit(ctx: Context<Deposit>, amount: u64, precommit: [u8; 32]) -> Result<()> {
        // fixed denominations only: same-denom deposits are indistinguishable
        require!(DENOMS.contains(&amount), SealError::BadDenom);

        // move real value in (the binding: value is what actually moved)
        transfer(
            CpiContext::new(
                ctx.accounts.system_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.depositor.to_account_info(),
                    to: ctx.accounts.vault.to_account_info(),
                },
            ),
            amount,
        )?;

        let tree = &mut ctx.accounts.tree_state;
        let label = tree.next_leaf_index;
        let leaf = ph3(&fe(amount), &fe(label), &precommit)?;

        // incremental insert (Tornado filledSubtrees + zeros)
        let mut idx = tree.next_leaf_index;
        let mut cur = leaf;
        for i in 0..DEPTH {
            let (l, r) = if idx & 1 == 0 {
                tree.filled_subtrees[i] = cur;
                (cur, tree.zeros[i])
            } else {
                (tree.filled_subtrees[i], cur)
            };
            cur = ph2(&l, &r)?;
            idx >>= 1;
        }
        tree.next_leaf_index += 1;
        tree.current_root = cur;
        let head = (tree.roots_head as usize + 1) % ROOT_HISTORY;
        tree.roots_head = head as u8;
        tree.roots[head] = cur;

        emit!(Deposit_ {
            leaf,
            leaf_index: label,
            root: cur,
            amount,
        });
        Ok(())
    }

    /// Verify a proof-of-receipt against a recent root and consume the nullifier.
    /// public_inputs order: [merkle_root, threshold, context_hash, nullifier]
    pub fn seal_verify(
        ctx: Context<SealVerify>,
        proof_a: [u8; 64],
        proof_b: [u8; 128],
        proof_c: [u8; 64],
        merkle_root: [u8; 32],
        threshold: [u8; 32],
        context_hash: [u8; 32],
        nullifier: [u8; 32],
    ) -> Result<()> {
        // the proof's root must be one the pool actually produced (recent history)
        require!(
            ctx.accounts.tree_state.roots.contains(&merkle_root),
            SealError::StaleRoot
        );

        let public_inputs: [[u8; 32]; 4] = [merkle_root, threshold, context_hash, nullifier];

        let mut verifier = Groth16Verifier::new(
            &proof_a,
            &proof_b,
            &proof_c,
            &public_inputs,
            &vk::VERIFYINGKEY,
        )
        .map_err(|_| error!(SealError::ProofMalformed))?;

        verifier
            .verify()
            .map_err(|_| error!(SealError::ProofInvalid))?;

        // nullifier PDA is init'd here; if it already exists the tx fails =
        // double-use within this context is rejected.
        let nf = &mut ctx.accounts.nullifier_record;
        nf.used = true;
        nf.bump = ctx.bumps.nullifier_record;

        emit!(GateOpened {
            context_hash,
            nullifier
        });
        Ok(())
    }

    /// Withdraw: prove ownership of a pool leaf and claim its exact value to a
    /// bound recipient, consuming a global single-use nullifier. Trustless payout
    /// from the vault PDA; no operator, no authority.
    /// public_inputs order: [merkle_root, value, recipient_hash, nullifier_hash]
    pub fn withdraw(
        ctx: Context<Withdraw>,
        proof_a: [u8; 64],
        proof_b: [u8; 128],
        proof_c: [u8; 64],
        merkle_root: [u8; 32],
        value: [u8; 32],
        recipient_hash: [u8; 32],
        nullifier_hash: [u8; 32],
    ) -> Result<()> {
        // root must be a real pool root
        require!(
            ctx.accounts.tree_state.roots.contains(&merkle_root),
            SealError::StaleRoot
        );

        // bind the payout to the recipient: recipient_hash must equal
        // Poseidon(hi16, lo16) of the actual recipient pubkey. A front-runner
        // cannot redirect the payout without a fresh proof.
        let rk = ctx.accounts.recipient.key().to_bytes();
        let mut hi = [0u8; 32];
        hi[16..32].copy_from_slice(&rk[0..16]);
        let mut lo = [0u8; 32];
        lo[16..32].copy_from_slice(&rk[16..32]);
        require!(
            ph2(&hi, &lo)? == recipient_hash,
            SealError::RecipientMismatch
        );

        // value field element -> u64 lamports (must fit in 64 bits)
        require!(
            value[0..24].iter().all(|&b| b == 0),
            SealError::ValueTooLarge
        );
        let amount = u64::from_be_bytes(value[24..32].try_into().unwrap());

        let public_inputs: [[u8; 32]; 4] = [merkle_root, value, recipient_hash, nullifier_hash];
        let mut verifier = Groth16Verifier::new(
            &proof_a,
            &proof_b,
            &proof_c,
            &public_inputs,
            &vk_withdraw::VERIFYINGKEY_WITHDRAW,
        )
        .map_err(|_| error!(SealError::ProofMalformed))?;
        verifier
            .verify()
            .map_err(|_| error!(SealError::ProofInvalid))?;

        // consume the global withdraw nullifier (init = single withdraw per leaf)
        let nf = &mut ctx.accounts.withdraw_nullifier;
        nf.used = true;
        nf.bump = ctx.bumps.withdraw_nullifier;

        // trustless payout from the vault PDA
        let seeds: &[&[u8]] = &[b"vault", &[ctx.bumps.vault]];
        invoke_signed(
            &system_instruction::transfer(
                &ctx.accounts.vault.key(),
                &ctx.accounts.recipient.key(),
                amount,
            ),
            &[
                ctx.accounts.vault.to_account_info(),
                ctx.accounts.recipient.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
            ],
            &[seeds],
        )?;

        emit!(Withdrawn {
            nullifier_hash,
            amount,
            recipient: ctx.accounts.recipient.key()
        });
        Ok(())
    }
}

// ---- Poseidon helpers (sol_poseidon syscall; BN254 circom params, big-endian).
// Byte-identical to circomlibjs 0.1.7 (verified in spikes). ----------------------

/// u64 -> 32-byte big-endian field element (matches circom number encoding).
fn fe(x: u64) -> [u8; 32] {
    let mut b = [0u8; 32];
    b[24..32].copy_from_slice(&x.to_be_bytes());
    b
}

fn ph2(a: &[u8; 32], b: &[u8; 32]) -> Result<[u8; 32]> {
    Ok(hashv(Parameters::Bn254X5, Endianness::BigEndian, &[a, b])
        .map_err(|_| error!(SealError::HashError))?
        .to_bytes())
}

fn ph3(a: &[u8; 32], b: &[u8; 32], c: &[u8; 32]) -> Result<[u8; 32]> {
    Ok(
        hashv(Parameters::Bn254X5, Endianness::BigEndian, &[a, b, c])
            .map_err(|_| error!(SealError::HashError))?
            .to_bytes(),
    )
}

/// Empty-subtree zeros z[0..=DEPTH]; z[0]=0, z[i]=Poseidon(z[i-1],z[i-1]).
fn compute_zeros() -> Result<[[u8; 32]; DEPTH + 1]> {
    let mut z = [[0u8; 32]; DEPTH + 1];
    for i in 1..=DEPTH {
        z[i] = ph2(&z[i - 1], &z[i - 1])?;
    }
    Ok(z)
}

#[derive(Accounts)]
pub struct InitTree<'info> {
    #[account(
        init, payer = payer, space = 8 + TreeState::SIZE,
        seeds = [b"tree"], bump
    )]
    pub tree_state: Box<Account<'info, TreeState>>,
    #[account(mut)]
    pub payer: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Deposit<'info> {
    #[account(mut, seeds = [b"tree"], bump = tree_state.bump)]
    pub tree_state: Box<Account<'info, TreeState>>,
    /// CHECK: pool vault PDA, holds pooled lamports (system-owned).
    #[account(mut, seeds = [b"vault"], bump)]
    pub vault: UncheckedAccount<'info>,
    #[account(mut)]
    pub depositor: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(proof_a: [u8;64], proof_b: [u8;128], proof_c: [u8;64], merkle_root: [u8;32], threshold: [u8;32], context_hash: [u8;32], nullifier: [u8;32])]
pub struct SealVerify<'info> {
    #[account(seeds = [b"tree"], bump = tree_state.bump)]
    pub tree_state: Box<Account<'info, TreeState>>,
    /// one nullifier record per (context_hash, nullifier): init = single-use gate
    #[account(
        init, payer = payer, space = 8 + NullifierRecord::SIZE,
        seeds = [b"nf", context_hash.as_ref(), nullifier.as_ref()], bump
    )]
    pub nullifier_record: Account<'info, NullifierRecord>,
    #[account(mut)]
    pub payer: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(proof_a: [u8;64], proof_b: [u8;128], proof_c: [u8;64], merkle_root: [u8;32], value: [u8;32], recipient_hash: [u8;32], nullifier_hash: [u8;32])]
pub struct Withdraw<'info> {
    #[account(seeds = [b"tree"], bump = tree_state.bump)]
    pub tree_state: Box<Account<'info, TreeState>>,
    /// CHECK: pool vault PDA (system-owned), pays out via invoke_signed.
    #[account(mut, seeds = [b"vault"], bump)]
    pub vault: UncheckedAccount<'info>,
    /// CHECK: payout target, bound by recipient_hash in the instruction.
    #[account(mut)]
    pub recipient: UncheckedAccount<'info>,
    /// global single-use: one withdraw per leaf nullifier
    #[account(
        init, payer = payer, space = 8 + NullifierRecord::SIZE,
        seeds = [b"wnf", nullifier_hash.as_ref()], bump
    )]
    pub withdraw_nullifier: Account<'info, NullifierRecord>,
    #[account(mut)]
    pub payer: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[account]
pub struct TreeState {
    pub current_root: [u8; 32],
    pub next_leaf_index: u64,
    pub filled_subtrees: [[u8; 32]; DEPTH],
    pub zeros: [[u8; 32]; DEPTH],
    pub roots: [[u8; 32]; ROOT_HISTORY],
    pub roots_head: u8,
    pub bump: u8,
}
impl TreeState {
    pub const SIZE: usize = 32 + 8 + (32 * DEPTH) + (32 * DEPTH) + (32 * ROOT_HISTORY) + 1 + 1;
}

#[account]
pub struct NullifierRecord {
    pub used: bool,
    pub bump: u8,
}
impl NullifierRecord {
    pub const SIZE: usize = 1 + 1;
}

#[event]
pub struct Deposit_ {
    pub leaf: [u8; 32],
    pub leaf_index: u64,
    pub root: [u8; 32],
    pub amount: u64,
}

#[event]
pub struct GateOpened {
    pub context_hash: [u8; 32],
    pub nullifier: [u8; 32],
}

#[event]
pub struct Withdrawn {
    pub nullifier_hash: [u8; 32],
    pub amount: u64,
    pub recipient: Pubkey,
}

#[error_code]
pub enum SealError {
    #[msg("merkle root in proof is not a recent pool root")]
    StaleRoot,
    #[msg("proof bytes malformed")]
    ProofMalformed,
    #[msg("groth16 proof verification failed")]
    ProofInvalid,
    #[msg("deposit amount is not an allowed denomination")]
    BadDenom,
    #[msg("poseidon hash error")]
    HashError,
    #[msg("recipient does not match the proof's recipient hash")]
    RecipientMismatch,
    #[msg("value does not fit in u64")]
    ValueTooLarge,
}

// Host tests: (1) the real circom/snarkjs proof verifies via groth16-solana, and
// (2) the on-chain Poseidon tree insert reproduces the proof's merkle root. Both
// run without deploying. Run: `cargo test`.
#[cfg(test)]
mod proof_test {
    use super::*;

    const PROOF_JSON: &str = include_str!("../../../../circuits/proof_bytes.json");

    fn arr<const N: usize>(v: &serde_json::Value, key: &str) -> [u8; N] {
        let a = v[key].as_array().unwrap();
        let mut out = [0u8; N];
        for (i, x) in a.iter().enumerate() {
            out[i] = x.as_u64().unwrap() as u8;
        }
        out
    }

    fn pubin(v: &serde_json::Value, i: usize) -> [u8; 32] {
        let a = v["public_inputs"][i].as_array().unwrap();
        let mut out = [0u8; 32];
        for (j, x) in a.iter().enumerate() {
            out[j] = x.as_u64().unwrap() as u8;
        }
        out
    }

    #[test]
    fn real_proof_verifies() {
        let v: serde_json::Value = serde_json::from_str(PROOF_JSON).unwrap();
        let proof_a: [u8; 64] = arr(&v, "proof_a");
        let proof_b: [u8; 128] = arr(&v, "proof_b");
        let proof_c: [u8; 64] = arr(&v, "proof_c");
        let mut public_inputs = [[0u8; 32]; 4];
        for i in 0..4 {
            public_inputs[i] = pubin(&v, i);
        }
        let mut ver = Groth16Verifier::new(
            &proof_a,
            &proof_b,
            &proof_c,
            &public_inputs,
            &vk::VERIFYINGKEY,
        )
        .expect("verifier new");
        ver.verify().expect("REAL PROOF MUST VERIFY");
    }

    // The on-chain deposit (Poseidon leaf + incremental insert at index 0) must
    // reproduce exactly the merkleRoot the proof was generated against. Uses the
    // same gen_input.mjs values (value=5000, label=0, secret=123456789,
    // nullifier=987654321).
    #[test]
    fn tree_root_matches_proof() {
        let precommit = ph2(&fe(987654321), &fe(123456789)).unwrap();
        let leaf = ph3(&fe(5000), &fe(0), &precommit).unwrap();
        let z = compute_zeros().unwrap();
        // leaf at index 0 = fold with empty siblings at every level
        let mut cur = leaf;
        for i in 0..DEPTH {
            cur = ph2(&cur, &z[i]).unwrap();
        }
        let v: serde_json::Value = serde_json::from_str(PROOF_JSON).unwrap();
        let expected_root = pubin(&v, 0); // public_inputs[0] = merkleRoot
        assert_eq!(
            cur, expected_root,
            "on-chain tree root must equal proof merkleRoot"
        );
    }
}
