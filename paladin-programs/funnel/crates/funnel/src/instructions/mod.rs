use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::pubkey::Pubkey;

use crate::FunnelConfig;

pub mod become_receiver;
pub mod initialize_funnel;

/// All possible Funnel actions.
#[derive(Debug, Clone, BorshSerialize, BorshDeserialize)]
pub enum FunnelInstruction {
    /// Initializes the [`crate::Funnel`] PDA account.
    InitializeFunnel { config: FunnelConfig },
    /// Sweeps the previous owner's rewards and claims ownership of the funnel.
    BecomeReceiver { new_receiver: Pubkey, prepay_lamports: u64 },
}
