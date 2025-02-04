use std::collections::HashMap;

use funnel::{FunnelConfig, FunnelInstruction};
use solana_sdk::account::Account;
use solana_sdk::instruction::{AccountMeta, Instruction};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Keypair;
use solana_sdk::signer::Signer;
use solana_sdk::system_program;
use solana_sdk::transaction::Transaction;
use svm_test::svm::DefaultLoader;
use svm_test::utils::{test_payer_keypair, TEST_PAYER};
use svm_test::Svm;

#[test]
fn initialize_funnel() {
    // Setup our test payer.
    let mut svm: Svm<DefaultLoader> = Svm::new(HashMap::default());
    svm.set(TEST_PAYER, Account { lamports: 10u64.pow(9), ..Default::default() });

    // Load local funnel program.
    svm.load_program(funnel::ID, "funnel");

    // Initialize the funnel.
    let funnel = Keypair::new();
    let initialize = Instruction::new_with_borsh(
        funnel::ID,
        &FunnelInstruction::InitializeFunnel {
            config: FunnelConfig {
                stakers_receiver: Pubkey::default(),
                holders_receiver: Pubkey::default(),
            },
        },
        vec![
            AccountMeta { pubkey: system_program::ID, is_signer: false, is_writable: false },
            AccountMeta { pubkey: TEST_PAYER, is_signer: true, is_writable: true },
            AccountMeta { pubkey: funnel.pubkey(), is_signer: true, is_writable: true },
        ],
    );
    let initialize = Transaction::new_signed_with_payer(
        &[initialize],
        Some(&TEST_PAYER),
        &[test_payer_keypair(), &funnel],
        svm.blockhash(),
    );
    svm.execute_transaction(initialize).unwrap();
}
