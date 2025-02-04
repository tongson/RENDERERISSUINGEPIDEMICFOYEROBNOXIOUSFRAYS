use std::sync::Arc;

use funnel::instructions::become_receiver::BecomeReceiverAccounts;
use funnel::{
    Funnel, FunnelConfig, FunnelInstruction, JITO_TIP_ACCOUNT_0, JITO_TIP_PAYMENT_CONFIG,
};
use solana_sdk::account::Account;
use solana_sdk::compute_budget::ComputeBudgetInstruction;
use solana_sdk::instruction::{AccountMeta, Instruction};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::rent::Rent;
use solana_sdk::signature::Keypair;
use solana_sdk::signer::{SeedDerivable, Signer};
use solana_sdk::system_program;
use solana_sdk::transaction::Transaction;
use svm_test::utils::{test_payer_keypair, TEST_PAYER};
use svm_test::{AccountLoader, Harness, Scenario, Svm};

struct BaseState {
    svm: Svm<Arc<Scenario>>,
    funnel: Keypair,
    funnel_config: FunnelConfig,
    paladin_receiver_new: Keypair,
    paladin_receiver_new_state: Pubkey,
    tip_receiver_old: Pubkey,
    block_builder_old: Pubkey,
}

fn setup() -> BaseState {
    let mut svm = Svm::new(Harness::get().get_scenario("become_receiver"));

    // Some accounts.
    let funnel = Keypair::from_seed(&[20; 32]).unwrap();
    let stakers_receiver = Pubkey::new_from_array([10; 32]);
    let holders_receiver = Pubkey::new_from_array([11; 32]);
    let paladin_receiver_new = Keypair::new();
    svm.set(funnel.pubkey(), Account::default());
    svm.set(stakers_receiver, Account::default());
    svm.set(holders_receiver, Account::default());
    svm.set(paladin_receiver_new.pubkey(), Account::default());
    let (paladin_receiver_new_state, _) = funnel::find_leader_state(&paladin_receiver_new.pubkey());
    svm.set(paladin_receiver_new_state, Account { lamports: 10u64.pow(9), ..Account::default() });

    // Setup our test payer.
    svm.set(TEST_PAYER, Account { lamports: 10u64.pow(9), ..Default::default() });

    // Load local funnel program.
    svm.load_program(funnel::ID, "funnel");

    // Initialize the funnel.
    let funnel_config = FunnelConfig { stakers_receiver, holders_receiver };
    let initialize = Instruction::new_with_borsh(
        funnel::ID,
        &FunnelInstruction::InitializeFunnel { config: funnel_config },
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

    // Check initial state.
    let funnel_s = svm.get(&funnel.pubkey()).unwrap();
    let funnel_s = bytemuck::from_bytes::<Funnel>(&funnel_s.data);
    assert_eq!(funnel_s.receiver, TEST_PAYER);

    // Initial load must come from loader.
    let jito_config = svm.loader.load(&JITO_TIP_PAYMENT_CONFIG);
    let tip_receiver_old = Pubkey::new_from_array(*arrayref::array_ref![jito_config.data, 8, 32]);
    let block_builder_old = Pubkey::new_from_array(*arrayref::array_ref![jito_config.data, 40, 32]);
    assert_ne!(tip_receiver_old, funnel.pubkey());
    assert_ne!(block_builder_old, Pubkey::default());

    BaseState {
        svm,
        funnel,
        funnel_config,
        paladin_receiver_new,
        paladin_receiver_new_state,
        tip_receiver_old,
        block_builder_old,
    }
}

#[test]
fn base_case() {
    let BaseState {
        mut svm,
        funnel,
        funnel_config,
        paladin_receiver_new,
        paladin_receiver_new_state,
        tip_receiver_old,
        block_builder_old,
    } = setup();

    // Become the receiver.
    let become_receiver = funnel::instructions::become_receiver::ix(
        BecomeReceiverAccounts {
            payer: TEST_PAYER,
            funnel_config: funnel.pubkey(),
            paladin_receiver_new_state,
            block_builder_old,
            tip_receiver_old,
            paladin_receiver_old: TEST_PAYER,
            paladin_receiver_new: paladin_receiver_new.pubkey(),
        },
        &funnel_config,
        0,
    );
    let become_receiver = Transaction::new_signed_with_payer(
        &[ComputeBudgetInstruction::set_compute_unit_limit(1_400_000), become_receiver],
        Some(&TEST_PAYER),
        &[test_payer_keypair(), &paladin_receiver_new],
        svm.blockhash(),
    );
    svm.execute_transaction(become_receiver).unwrap();

    // Post execution load must come from SVM.
    let jito_config = svm.get(&JITO_TIP_PAYMENT_CONFIG).unwrap();
    let tip_receiver_new = Pubkey::new_from_array(*arrayref::array_ref![jito_config.data, 8, 32]);

    // Assert - Jito receiver is the paladin funnel.
    assert_ne!(tip_receiver_old, tip_receiver_new);
    assert_eq!(tip_receiver_new, funnel.pubkey());

    // Assert - Paladin receiver is new receiver.
    let funnel = svm.get(&funnel.pubkey()).unwrap();
    let funnel = bytemuck::from_bytes::<Funnel>(&funnel.data);
    assert_eq!(funnel.receiver, paladin_receiver_new.pubkey());
}

#[test]
fn sweep_previous_receiver() {
    let BaseState {
        mut svm,
        funnel,
        funnel_config,
        paladin_receiver_new: paladin_receiver_0,
        paladin_receiver_new_state: paladin_receiver_0_state,
        tip_receiver_old,
        block_builder_old,
    } = setup();

    // Initial receiver.
    let become_receiver = funnel::instructions::become_receiver::ix(
        BecomeReceiverAccounts {
            payer: TEST_PAYER,
            funnel_config: funnel.pubkey(),
            block_builder_old,
            tip_receiver_old,
            paladin_receiver_old: TEST_PAYER,
            paladin_receiver_new: paladin_receiver_0.pubkey(),
            paladin_receiver_new_state: paladin_receiver_0_state,
        },
        &funnel_config,
        0,
    );
    let become_receiver = Transaction::new_signed_with_payer(
        &[ComputeBudgetInstruction::set_compute_unit_limit(1_400_000), become_receiver],
        Some(&TEST_PAYER),
        &[test_payer_keypair(), &paladin_receiver_0],
        svm.blockhash(),
    );
    svm.execute_transaction(become_receiver).unwrap();

    // Add 1 SOL rewards to tip account 0.
    let mut tip_account_0 = svm.get(&JITO_TIP_ACCOUNT_0).unwrap();
    tip_account_0.lamports += 10u64.pow(9);
    svm.set(JITO_TIP_ACCOUNT_0, tip_account_0);

    // New receiver should sweep rewards to the original receiver.
    let paladin_receiver_1 = Keypair::new();
    svm.set(paladin_receiver_1.pubkey(), Account::default());
    let (paladin_receiver_1_state, _) = funnel::find_leader_state(&paladin_receiver_1.pubkey());
    svm.set(paladin_receiver_1_state, Account { lamports: 10u64.pow(9), ..Account::default() });
    let become_receiver = funnel::instructions::become_receiver::ix(
        BecomeReceiverAccounts {
            payer: TEST_PAYER,
            funnel_config: funnel.pubkey(),
            block_builder_old,
            tip_receiver_old: funnel.pubkey(),
            paladin_receiver_old: paladin_receiver_0.pubkey(),
            paladin_receiver_new: paladin_receiver_1.pubkey(),
            paladin_receiver_new_state: paladin_receiver_1_state,
        },
        &funnel_config,
        0,
    );
    let become_receiver = Transaction::new_signed_with_payer(
        &[ComputeBudgetInstruction::set_compute_unit_limit(1_400_000), become_receiver],
        Some(&TEST_PAYER),
        &[test_payer_keypair(), &paladin_receiver_1],
        svm.blockhash(),
    );

    // Act - Set a new receiver, should sweep rewards to the old receiver.
    assert_eq!(svm.get(&paladin_receiver_0.pubkey()).unwrap().lamports, 0);
    assert_eq!(
        svm.get(&funnel.pubkey()).unwrap().lamports,
        Rent::default().minimum_balance(std::mem::size_of::<Funnel>())
    );
    svm.execute_transaction(become_receiver).unwrap();
    assert_eq!(
        svm.get(&funnel.pubkey()).unwrap().lamports,
        Rent::default().minimum_balance(std::mem::size_of::<Funnel>())
    );
    assert_eq!(
        svm.get(&paladin_receiver_0.pubkey()).unwrap().lamports,
        10u64.pow(9) * 95 * 90 / 10_000
    );
    assert_eq!(
        svm.get(&funnel_config.holders_receiver).unwrap().lamports,
        10u64.pow(9) * 95 * 5 / 10_000
    );
    assert_eq!(
        svm.get(&funnel_config.stakers_receiver).unwrap().lamports,
        10u64.pow(9) * 95 * 5 / 10_000
    );
}

#[test]
fn additional_lamports() {
    let BaseState {
        mut svm,
        funnel,
        funnel_config,
        paladin_receiver_new,
        paladin_receiver_new_state,
        tip_receiver_old,
        block_builder_old,
    } = setup();

    // Give our receiver 100 lamports.
    let mut receiver = svm.get(&paladin_receiver_new.pubkey()).unwrap();
    receiver.lamports = 100;
    svm.set(paladin_receiver_new.pubkey(), receiver);

    // Become the receiver.
    let become_receiver = funnel::instructions::become_receiver::ix(
        BecomeReceiverAccounts {
            payer: TEST_PAYER,
            funnel_config: funnel.pubkey(),
            block_builder_old,
            tip_receiver_old,
            paladin_receiver_old: TEST_PAYER,
            paladin_receiver_new: paladin_receiver_new.pubkey(),
            paladin_receiver_new_state,
        },
        &funnel_config,
        100,
    );
    let become_receiver = Transaction::new_signed_with_payer(
        &[ComputeBudgetInstruction::set_compute_unit_limit(1_400_000), become_receiver],
        Some(&TEST_PAYER),
        &[test_payer_keypair(), &paladin_receiver_new],
        svm.blockhash(),
    );
    svm.execute_transaction(become_receiver).unwrap();

    // Assert - Staker receiver has 50 lamports.
    assert_eq!(svm.get(&funnel_config.stakers_receiver).unwrap().lamports, 50);

    // Assert - Holder receiver has 50 lamports.
    assert_eq!(svm.get(&funnel_config.holders_receiver).unwrap().lamports, 50);
}
