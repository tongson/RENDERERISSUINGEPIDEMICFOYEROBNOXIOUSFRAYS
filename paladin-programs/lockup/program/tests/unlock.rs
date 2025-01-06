#![cfg(feature = "test-sbf")]

mod setup;

use {
    paladin_lockup_program::{error::PaladinLockupError, state::Lockup},
    setup::{setup, setup_lockup, setup_lockup_pool},
    solana_program_test::*,
    solana_sdk::{
        account::AccountSharedData,
        clock::Clock,
        instruction::InstructionError,
        pubkey::Pubkey,
        signature::Keypair,
        signer::Signer,
        transaction::{Transaction, TransactionError},
    },
    spl_discriminator::SplDiscriminate,
    std::num::NonZeroU64,
};

#[tokio::test]
async fn fail_unlock_authority_not_signer() {
    let mut context = setup().start_with_context().await;

    let authority = Keypair::new();
    let lockup = Pubkey::new_unique();

    // Create the lockup pool account.
    let pool = Pubkey::new_unique();
    setup_lockup_pool(&mut context, &pool).await;

    let mut instruction =
        paladin_lockup_program::instruction::unlock(&authority.pubkey(), pool, &lockup);
    instruction.accounts[0].is_signer = false;

    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&context.payer.pubkey()),
        &[&context.payer], // authority not signer.
        context.last_blockhash,
    );

    let err = context
        .banks_client
        .process_transaction(transaction)
        .await
        .unwrap_err()
        .unwrap();

    assert_eq!(
        err,
        TransactionError::InstructionError(0, InstructionError::MissingRequiredSignature)
    );
}

#[tokio::test]
async fn fail_unlock_not_enough_space() {
    let mut context = setup().start_with_context().await;

    let authority = Keypair::new();
    let lockup = Pubkey::new_unique();

    // Create the lockup pool account.
    let pool = Pubkey::new_unique();
    setup_lockup_pool(&mut context, &pool).await;

    // Create the lockup account with not enough space.
    {
        let rent = context.banks_client.get_rent().await.unwrap();
        let space = std::mem::size_of::<Lockup>().saturating_sub(6); // Not enough space.
        let lamports = rent.minimum_balance(space);
        context.set_account(
            &lockup,
            &AccountSharedData::new(lamports, space, &paladin_lockup_program::id()),
        );
    }

    let instruction =
        paladin_lockup_program::instruction::unlock(&authority.pubkey(), pool, &lockup);

    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&context.payer.pubkey()),
        &[&context.payer, &authority],
        context.last_blockhash,
    );

    let err = context
        .banks_client
        .process_transaction(transaction)
        .await
        .unwrap_err()
        .unwrap();

    assert_eq!(
        err,
        TransactionError::InstructionError(0, InstructionError::UninitializedAccount)
    );
}

#[tokio::test]
async fn fail_unlock_uninitialized() {
    let mut context = setup().start_with_context().await;

    let authority = Keypair::new();
    let lockup = Pubkey::new_unique();

    // Create the lockup pool account.
    let pool = Pubkey::new_unique();
    setup_lockup_pool(&mut context, &pool).await;

    // Create the lockup account with uninitialized state.
    {
        let rent = context.banks_client.get_rent().await.unwrap();
        let space = std::mem::size_of::<Lockup>(); // Not enough space.
        let lamports = rent.minimum_balance(space);
        context.set_account(
            &lockup,
            &AccountSharedData::new(lamports, space, &paladin_lockup_program::id()),
        );
    }

    let instruction =
        paladin_lockup_program::instruction::unlock(&authority.pubkey(), pool, &lockup);

    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&context.payer.pubkey()),
        &[&context.payer, &authority],
        context.last_blockhash,
    );

    let err = context
        .banks_client
        .process_transaction(transaction)
        .await
        .unwrap_err()
        .unwrap();

    assert_eq!(
        err,
        TransactionError::InstructionError(0, InstructionError::UninitializedAccount)
    );
}

#[tokio::test]
async fn fail_incorrect_lockup_authority() {
    let mut context = setup().start_with_context().await;

    let authority = Keypair::new();
    let lockup = Pubkey::new_unique();

    // Create the lockup pool account.
    let pool = Pubkey::new_unique();
    setup_lockup_pool(&mut context, &pool).await;

    setup_lockup(
        &mut context,
        &lockup,
        Lockup {
            discriminator: Lockup::SPL_DISCRIMINATOR.into(),
            amount: 10_000,
            authority: Pubkey::new_unique(), // Incorrect authority.
            lockup_start_timestamp: 10_000,
            lockup_end_timestamp: None,
            mint: Pubkey::new_unique(),
            pool,
            metadata: Pubkey::new_unique().to_bytes(),
        },
    )
    .await;

    let instruction =
        paladin_lockup_program::instruction::unlock(&authority.pubkey(), pool, &lockup);

    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&context.payer.pubkey()),
        &[&context.payer, &authority],
        context.last_blockhash,
    );

    let err = context
        .banks_client
        .process_transaction(transaction)
        .await
        .unwrap_err()
        .unwrap();

    assert_eq!(
        err,
        TransactionError::InstructionError(0, InstructionError::IncorrectAuthority)
    );
}

#[tokio::test]
async fn fail_unlock_already_unlocked() {
    let mut context = setup().start_with_context().await;

    let authority = Keypair::new();
    let lockup = Pubkey::new_unique();

    // Create the lockup pool account.
    let pool = Pubkey::new_unique();
    setup_lockup_pool(&mut context, &pool).await;

    // Actual timestamp doesn't matter for this test.
    let start = 100_000u64;
    let end = 200_000u64;

    setup_lockup(
        &mut context,
        &lockup,
        Lockup {
            discriminator: Lockup::SPL_DISCRIMINATOR.into(),
            amount: 10_000,
            authority: authority.pubkey(),
            lockup_start_timestamp: start,
            lockup_end_timestamp: NonZeroU64::new(end),
            mint: Pubkey::new_unique(),
            pool,
            metadata: Pubkey::new_unique().to_bytes(),
        },
    )
    .await;

    let instruction =
        paladin_lockup_program::instruction::unlock(&authority.pubkey(), pool, &lockup);

    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&context.payer.pubkey()),
        &[&context.payer, &authority],
        context.last_blockhash,
    );

    let err = context
        .banks_client
        .process_transaction(transaction)
        .await
        .unwrap_err()
        .unwrap();

    assert_eq!(
        err,
        TransactionError::InstructionError(
            0,
            InstructionError::Custom(PaladinLockupError::LockupAlreadyUnlocked as u32)
        )
    );
}

#[tokio::test]
async fn fail_unlock_incorrect_pool() {
    let mut context = setup().start_with_context().await;

    let authority = Keypair::new();
    let lockup = Pubkey::new_unique();

    // Create the lockup pool account.
    let pool1 = Pubkey::new_unique();
    setup_lockup_pool(&mut context, &pool1).await;
    let pool2 = Pubkey::new_unique();
    setup_lockup_pool(&mut context, &pool2).await;

    setup_lockup(
        &mut context,
        &lockup,
        Lockup {
            discriminator: Lockup::SPL_DISCRIMINATOR.into(),
            amount: 10_000,
            authority: authority.pubkey(),
            lockup_start_timestamp: 10,
            lockup_end_timestamp: None,
            mint: Pubkey::new_unique(),
            pool: pool1,
            metadata: Pubkey::new_unique().to_bytes(),
        },
    )
    .await;

    let instruction =
        paladin_lockup_program::instruction::unlock(&authority.pubkey(), pool2, &lockup);

    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&context.payer.pubkey()),
        &[&context.payer, &authority],
        context.last_blockhash,
    );

    let err = context
        .banks_client
        .process_transaction(transaction)
        .await
        .unwrap_err()
        .unwrap();

    assert_eq!(
        err,
        TransactionError::InstructionError(
            0,
            InstructionError::Custom(PaladinLockupError::IncorrectPool as u32)
        )
    );
}

#[tokio::test]
async fn success() {
    let mut context = setup().start_with_context().await;

    let authority = Keypair::new();
    let lockup = Pubkey::new_unique();
    let clock = context.banks_client.get_sysvar::<Clock>().await.unwrap();
    let start = clock.unix_timestamp as u64;

    // Create the lockup pool account.
    let pool = Pubkey::new_unique();
    setup_lockup_pool(&mut context, &pool).await;

    setup_lockup(
        &mut context,
        &lockup,
        Lockup {
            discriminator: Lockup::SPL_DISCRIMINATOR.into(),
            amount: 10_000,
            authority: authority.pubkey(),
            lockup_start_timestamp: start,
            lockup_end_timestamp: None,
            mint: Pubkey::new_unique(),
            pool,
            metadata: Pubkey::new_unique().to_bytes(),
        },
    )
    .await;

    let instruction =
        paladin_lockup_program::instruction::unlock(&authority.pubkey(), pool, &lockup);

    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&context.payer.pubkey()),
        &[&context.payer, &authority],
        context.last_blockhash,
    );

    context
        .banks_client
        .process_transaction(transaction)
        .await
        .unwrap();

    // Check the lockup account.
    let lockup_account = context
        .banks_client
        .get_account(lockup)
        .await
        .unwrap()
        .unwrap();
    let state = bytemuck::from_bytes::<Lockup>(&lockup_account.data);
    assert_eq!(state.lockup_end_timestamp.unwrap().get(), start);
}
