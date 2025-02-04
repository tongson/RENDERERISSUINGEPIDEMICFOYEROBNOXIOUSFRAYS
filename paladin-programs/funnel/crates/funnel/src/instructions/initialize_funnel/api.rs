use solana_program::instruction::{AccountMeta, Instruction};
use solana_program::pubkey::Pubkey;
use solana_program::system_program;

use crate::FunnelConfig;

pub struct InitializeFunnelAccounts {
    pub payer: Pubkey,
    pub funnel_config: Pubkey,
}

pub fn ix(accounts: InitializeFunnelAccounts, config: FunnelConfig) -> Instruction {
    Instruction::new_with_borsh(
        crate::ID,
        &crate::instructions::FunnelInstruction::InitializeFunnel { config },
        account_metas(accounts),
    )
}

pub fn account_metas(accounts: InitializeFunnelAccounts) -> Vec<AccountMeta> {
    vec![
        AccountMeta { pubkey: system_program::ID, is_signer: false, is_writable: false },
        AccountMeta { pubkey: accounts.payer, is_signer: true, is_writable: true },
        AccountMeta { pubkey: accounts.funnel_config, is_signer: true, is_writable: true },
    ]
}
