//! End-to-end test.

#![cfg(feature = "test-sbf")]

mod setup;

use {
    paladin_lockup_program::{
        error::PaladinLockupError,
        state::{get_escrow_authority_address, Lockup, LockupPool, LockupPoolEntry},
        LOCKUP_COOLDOWN_SECONDS,
    },
    setup::{add_seconds_to_clock, setup, setup_mint, setup_token_account},
    solana_program_test::*,
    solana_sdk::{
        clock::Clock,
        compute_budget::ComputeBudgetInstruction,
        instruction::{Instruction, InstructionError},
        pubkey::Pubkey,
        signature::Keypair,
        signer::Signer,
        system_instruction,
        transaction::{Transaction, TransactionError},
    },
    spl_associated_token_account::get_associated_token_address_with_program_id,
    spl_discriminator::SplDiscriminate,
    spl_token_2022::{extension::StateWithExtensions, state::Account as TokenAccount},
};

async fn check_token_account_balance(
    context: &mut ProgramTestContext,
    token_account_address: &Pubkey,
    check_amount: u64,
) {
    let account = context
        .banks_client
        .get_account(*token_account_address)
        .await
        .expect("get_account")
        .expect("account not found");
    let actual_amount = StateWithExtensions::<TokenAccount>::unpack(&account.data)
        .unwrap()
        .base
        .amount;
    assert_eq!(actual_amount, check_amount);
}

async fn check_lockup_state(
    context: &mut ProgramTestContext,
    lockup_address: &Pubkey,
    check_lockup: &Lockup,
) {
    let account = context
        .banks_client
        .get_account(*lockup_address)
        .await
        .expect("get_account")
        .expect("account not found");
    let actual_lockup = bytemuck::from_bytes::<Lockup>(&account.data);
    assert_eq!(actual_lockup, check_lockup);
}

async fn send_transaction(
    context: &mut ProgramTestContext,
    instructions: &[Instruction],
    signers: &[&Keypair],
) {
    let blockhash = context.banks_client.get_latest_blockhash().await.unwrap();
    let transaction = Transaction::new_signed_with_payer(
        instructions,
        Some(&context.payer.pubkey()),
        signers,
        blockhash,
    );
    context
        .banks_client
        .process_transaction(transaction)
        .await
        .unwrap();
}

async fn send_transaction_with_expected_err(
    context: &mut ProgramTestContext,
    instructions: &[Instruction],
    signers: &[&Keypair],
    expected_err: TransactionError,
) {
    let transaction = Transaction::new_signed_with_payer(
        instructions,
        Some(&context.payer.pubkey()),
        signers,
        context.last_blockhash,
    );
    let err = context
        .banks_client
        .process_transaction(transaction)
        .await
        .unwrap_err()
        .unwrap();
    assert_eq!(err, expected_err);
}

#[tokio::test]
async fn test_e2e() {
    let mint = Pubkey::new_unique();

    let alice = Keypair::new();
    let alice_token_account =
        get_associated_token_address_with_program_id(&alice.pubkey(), &mint, &spl_token_2022::id());
    let alice_token_account_starting_token_balance = 10_000;

    let bob = Keypair::new();
    let bob_token_account =
        get_associated_token_address_with_program_id(&bob.pubkey(), &mint, &spl_token_2022::id());
    let bob_token_account_starting_token_balance = 10_000;

    let escrow_authority = get_escrow_authority_address(&paladin_lockup_program::id());
    let escrow_token_account = get_associated_token_address_with_program_id(
        &escrow_authority,
        &mint,
        &spl_token_2022::id(),
    );

    let mut context = setup().start_with_context().await;
    let payer = context.payer.insecure_clone();

    // Setup.
    {
        setup_token_account(
            &mut context,
            &alice_token_account,
            &alice.pubkey(),
            &mint,
            alice_token_account_starting_token_balance,
        )
        .await;
        setup_token_account(
            &mut context,
            &bob_token_account,
            &bob.pubkey(),
            &mint,
            bob_token_account_starting_token_balance,
        )
        .await;
        setup_token_account(
            &mut context,
            &escrow_token_account,
            &escrow_authority,
            &mint,
            0,
        )
        .await;
        setup_mint(&mut context, &mint, &Pubkey::new_unique(), 1_000_000).await;
    }

    // Create a lockup pool
    let clock = context
        .banks_client
        .get_sysvar::<Clock>()
        .await
        .expect("get_sysvar");
    let rent = context.banks_client.get_rent().await.expect("get_rent");
    let pool = Keypair::new();
    {
        let space = std::mem::size_of::<LockupPool>();

        send_transaction(
            &mut context,
            &[
                system_instruction::transfer(
                    &payer.pubkey(),
                    &pool.pubkey(),
                    rent.minimum_balance(space),
                ),
                system_instruction::allocate(&pool.pubkey(), space as u64),
                system_instruction::assign(&pool.pubkey(), &paladin_lockup_program::id()),
                paladin_lockup_program::instruction::initialize_lockup_pool(pool.pubkey()),
            ],
            &[&payer, &pool],
        )
        .await;
    }

    // Create a lockup for Alice.
    let metadata = Pubkey::new_unique();
    let alice_lockup = Keypair::new();
    let alice_lockup_amount = 1_000;
    {
        let space = std::mem::size_of::<Lockup>();

        send_transaction(
            &mut context,
            &[
                system_instruction::transfer(
                    &payer.pubkey(),
                    &alice_lockup.pubkey(),
                    rent.minimum_balance(space),
                ),
                system_instruction::allocate(&alice_lockup.pubkey(), space as u64),
                system_instruction::assign(&alice_lockup.pubkey(), &paladin_lockup_program::id()),
                paladin_lockup_program::instruction::lockup(
                    &alice.pubkey(),
                    &alice.pubkey(),
                    &alice_token_account,
                    pool.pubkey(),
                    &alice_lockup.pubkey(),
                    &mint,
                    metadata.to_bytes(),
                    alice_lockup_amount,
                    &spl_token_2022::id(),
                ),
            ],
            &[&payer, &alice, &alice_lockup],
        )
        .await;

        let expected_lockup_start = clock.unix_timestamp as u64;

        // Validate the lockup was created and tokens were transferred to the escrow.
        check_lockup_state(
            &mut context,
            &alice_lockup.pubkey(),
            &Lockup {
                discriminator: Lockup::SPL_DISCRIMINATOR.into(),
                amount: alice_lockup_amount,
                authority: alice.pubkey(),
                lockup_start_timestamp: expected_lockup_start,
                lockup_end_timestamp: None,
                mint,
                pool: pool.pubkey(),
                metadata: metadata.to_bytes(),
            },
        )
        .await;
        check_token_account_balance(
            &mut context,
            &alice_token_account,
            alice_token_account_starting_token_balance.saturating_sub(alice_lockup_amount),
        )
        .await;
        check_token_account_balance(&mut context, &escrow_token_account, alice_lockup_amount).await;

        // Assert - Lockup pool includes the new lockup.
        let lockup_pool = context
            .banks_client
            .get_account(pool.pubkey())
            .await
            .unwrap()
            .unwrap();
        let actual_lockup = bytemuck::from_bytes::<LockupPool>(&lockup_pool.data);
        assert_eq!(actual_lockup.entries_len, 1);
        assert_eq!(
            actual_lockup.entries[0],
            LockupPoolEntry {
                amount: alice_lockup_amount,
                lockup: alice_lockup.pubkey(),
                metadata: metadata.to_bytes(),
            }
        );
    }

    // Warp the clock 30 seconds.
    // Alice can't withdraw until the period ends.
    {
        add_seconds_to_clock(&mut context, 30).await;

        send_transaction_with_expected_err(
            &mut context,
            &[paladin_lockup_program::instruction::withdraw(
                &alice.pubkey(),
                &alice.pubkey(),
                &alice_token_account,
                &alice_lockup.pubkey(),
                &mint,
                &spl_token_2022::id(),
            )],
            &[&payer, &alice],
            TransactionError::InstructionError(
                0,
                InstructionError::Custom(PaladinLockupError::LockupActive as u32),
            ),
        )
        .await;
    }

    // Unlock the lockup
    {
        send_transaction(
            &mut context,
            &[
                ComputeBudgetInstruction::set_compute_unit_limit(400_000),
                paladin_lockup_program::instruction::unlock(
                    &alice.pubkey(),
                    pool.pubkey(),
                    &alice_lockup.pubkey(),
                ),
            ],
            &[&payer, &alice],
        )
        .await;
    }

    // Warp the clock 30 more minutes.
    // Alice can now withdraw.
    {
        add_seconds_to_clock(&mut context, LOCKUP_COOLDOWN_SECONDS).await;

        send_transaction(
            &mut context,
            &[paladin_lockup_program::instruction::withdraw(
                &alice.pubkey(),
                &alice.pubkey(),
                &alice_token_account,
                &alice_lockup.pubkey(),
                &mint,
                &spl_token_2022::id(),
            )],
            &[&payer, &alice],
        )
        .await;

        // Validate the lockup was closed and tokens were transferred back to Alice.
        assert!(context
            .banks_client
            .get_account(alice_lockup.pubkey())
            .await
            .expect("get_account")
            .is_none());
        check_token_account_balance(
            &mut context,
            &alice_token_account,
            alice_token_account_starting_token_balance,
        )
        .await;
        check_token_account_balance(&mut context, &escrow_token_account, 0).await;

        // Assert - Lockup pool no longer includes the lockup.
        let lockup_pool = context
            .banks_client
            .get_account(pool.pubkey())
            .await
            .unwrap()
            .unwrap();
        let actual_lockup = bytemuck::from_bytes::<LockupPool>(&lockup_pool.data);
        assert_eq!(actual_lockup.entries_len, 0);
        assert_eq!(actual_lockup.entries[0], LockupPoolEntry::default(),);
    }
}
