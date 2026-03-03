//! SEAL pool: an on-chain incremental Poseidon-BN254 Merkle tree.
//! init_tree seeds the empty tree; deposits move the root. No authority sets the
//! root; the root is a deterministic function of deposits.

use anchor_lang::prelude::*;
use anchor_lang::system_program::{transfer, Transfer};
use groth16_solana::groth16::Groth16Verifier;
use solana_poseidon::{hashv, Endianness, Parameters};

declare_id!("seuH78RmBPVzoKToLQVEZrDvuL5jDNBSbptozWK9PEm");

pub mod vk;

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
}

// ---- Poseidon helpers (sol_poseidon syscall; BN254 circom params, big-endian). ----

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
}
