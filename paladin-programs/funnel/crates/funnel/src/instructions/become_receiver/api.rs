use solana_program::instruction::{AccountMeta, Instruction};
use solana_program::pubkey::Pubkey;
use solana_program::system_program;

use crate::{
    FunnelConfig, JITO_TIP_ACCOUNT_0, JITO_TIP_ACCOUNT_1, JITO_TIP_ACCOUNT_2, JITO_TIP_ACCOUNT_3,
    JITO_TIP_ACCOUNT_4, JITO_TIP_ACCOUNT_5, JITO_TIP_ACCOUNT_6, JITO_TIP_ACCOUNT_7,
    JITO_TIP_PAYMENT_CONFIG, JITO_TIP_PAYMENT_PROGRAM,
};

pub struct BecomeReceiverAccounts {
    pub payer: Pubkey,
    pub funnel_config: Pubkey,
    pub block_builder_old: Pubkey,
    pub tip_receiver_old: Pubkey,
    pub paladin_receiver_old: Pubkey,
    pub paladin_receiver_new: Pubkey,
    pub paladin_receiver_new_state: Pubkey,
}

pub fn ix(
    accounts: BecomeReceiverAccounts,
    config: &FunnelConfig,
    additional_lamports: u64,
) -> Instruction {
    Instruction::new_with_borsh(
        crate::ID,
        &crate::instructions::FunnelInstruction::BecomeReceiver {
            new_receiver: accounts.paladin_receiver_new,
            prepay_lamports: additional_lamports,
        },
        account_metas(accounts, config),
    )
}

pub fn account_metas(accounts: BecomeReceiverAccounts, config: &FunnelConfig) -> Vec<AccountMeta> {
    vec![
        AccountMeta { pubkey: system_program::ID, is_signer: false, is_writable: false },
        AccountMeta { pubkey: accounts.funnel_config, is_signer: false, is_writable: true },
        AccountMeta { pubkey: config.stakers_receiver, is_signer: false, is_writable: true },
        AccountMeta { pubkey: config.holders_receiver, is_signer: false, is_writable: true },
        AccountMeta { pubkey: accounts.paladin_receiver_old, is_signer: false, is_writable: true },
        AccountMeta { pubkey: accounts.paladin_receiver_new, is_signer: true, is_writable: true },
        AccountMeta {
            pubkey: accounts.paladin_receiver_new_state,
            is_signer: false,
            is_writable: true,
        },
        AccountMeta { pubkey: JITO_TIP_PAYMENT_CONFIG, is_signer: false, is_writable: true },
        AccountMeta { pubkey: accounts.tip_receiver_old, is_signer: false, is_writable: true },
        AccountMeta { pubkey: accounts.funnel_config, is_signer: false, is_writable: true },
        AccountMeta { pubkey: accounts.block_builder_old, is_signer: false, is_writable: true },
        AccountMeta { pubkey: JITO_TIP_ACCOUNT_0, is_signer: false, is_writable: true },
        AccountMeta { pubkey: JITO_TIP_ACCOUNT_1, is_signer: false, is_writable: true },
        AccountMeta { pubkey: JITO_TIP_ACCOUNT_2, is_signer: false, is_writable: true },
        AccountMeta { pubkey: JITO_TIP_ACCOUNT_3, is_signer: false, is_writable: true },
        AccountMeta { pubkey: JITO_TIP_ACCOUNT_4, is_signer: false, is_writable: true },
        AccountMeta { pubkey: JITO_TIP_ACCOUNT_5, is_signer: false, is_writable: true },
        AccountMeta { pubkey: JITO_TIP_ACCOUNT_6, is_signer: false, is_writable: true },
        AccountMeta { pubkey: JITO_TIP_ACCOUNT_7, is_signer: false, is_writable: true },
        AccountMeta { pubkey: accounts.payer, is_signer: true, is_writable: true },
        AccountMeta { pubkey: JITO_TIP_PAYMENT_PROGRAM, is_signer: false, is_writable: false },
    ]
}
