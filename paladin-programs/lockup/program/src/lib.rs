#![allow(unexpected_cfgs)]
//! Paladin Lockup program.
//!
//! Allows for the creation of lockups that can be used to restrict the
//! transfer of tokens.
//!
//! Lockups are created with a duration of 30 minutes and will not allow
//! withdrawal of the locked tokens until the duration has passed.

#[cfg(all(target_os = "solana", feature = "bpf-entrypoint"))]
mod entrypoint;
pub mod error;
pub mod instruction;
pub mod processor;
pub mod state;

solana_program::declare_id!("2fzrtg7ckXTFxjgsXbAWLLvXr4wJK9xfUjJJ69uZ7chb");

pub const LOCKUP_COOLDOWN_SECONDS: u64 = 30 * 60; // 30 minutes
