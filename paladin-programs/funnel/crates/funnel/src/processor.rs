use borsh::BorshDeserialize;
use solana_program::account_info::AccountInfo;
use solana_program::entrypoint::ProgramResult;
use solana_program::pubkey::Pubkey;

use crate::FunnelInstruction;

/// Program instruction dispatch.
///
/// # Panics
///
/// Panics if any accounts fail validation or other invariants are broken.
pub fn process(_: &Pubkey, accounts: &[AccountInfo], mut data: &[u8]) -> ProgramResult {
    // Deserialize.
    let instruction = FunnelInstruction::deserialize(&mut data).unwrap();

    // Dispatch.
    match instruction {
        FunnelInstruction::InitializeFunnel { config } => {
            crate::instructions::initialize_funnel::process(accounts, config)
        }
        FunnelInstruction::BecomeReceiver { new_receiver, prepay_lamports } => {
            crate::instructions::become_receiver::process(accounts, new_receiver, prepay_lamports)
        }
    }
}
