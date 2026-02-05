//! SEAL pool: an on-chain incremental Poseidon-BN254 Merkle tree.
//! init_tree seeds the empty tree; deposits move the root. No authority sets the
//! root; the root is a deterministic function of deposits.

use anchor_lang::prelude::*;
use solana_poseidon::{hashv, Endianness, Parameters};

declare_id!("seuH78RmBPVzoKToLQVEZrDvuL5jDNBSbptozWK9PEm");

pub const DEPTH: usize = 20;
pub const ROOT_HISTORY: usize = 30;

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


#[error_code]
pub enum SealError {
    #[msg("poseidon hash error")]
    HashError,
}
