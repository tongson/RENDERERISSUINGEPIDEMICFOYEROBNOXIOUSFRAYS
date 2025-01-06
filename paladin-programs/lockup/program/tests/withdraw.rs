#![cfg(feature = "test-sbf")]

mod setup;

use {
    paladin_lockup_program::{
        error::PaladinLockupError,
        state::{get_escrow_authority_address, Lockup},
        LOCKUP_COOLDOWN_SECONDS,
    },
    setup::{
        add_seconds_to_clock, setup, setup_lockup, setup_lockup_pool, setup_mint,
        setup_token_account,
    },
    solana_program_test::*,
    solana_sdk::{
        account::{Account, AccountSharedData},
        clock::Clock,
        instruction::InstructionError,
        pubkey::Pubkey,
        signature::Keypair,
        signer::Signer,
        transaction::{Transaction, TransactionError},
    },
    spl_associated_token_account::get_associated_token_address_with_program_id,
    spl_discriminator::SplDiscriminate,
    spl_token_2022::{extension::StateWithExtensions, state::Account as TokenAccount},
    std::num::NonZeroU64,
};

#[tokio::test]
async fn fail_lockup_authority_not_signer() {
    let mint = Pubkey::new_unique();

    let authority = Keypair::new();
    let token_account = get_associated_token_address_with_program_id(
        &authority.pubkey(),
        &mint,
        &spl_token_2022::id(),
    );

    let lockup = Pubkey::new_unique();

    let mut context = setup().start_with_context().await;

    let mut instruction = paladin_lockup_program::instruction::withdraw(
        &authority.pubkey(),
        &token_account,
        &token_account,
        &lockup,
        &mint,
        &spl_token_2022::id(),
    );
    instruction.accounts[0].is_signer = false; // Authority not signer.

    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&context.payer.pubkey()),
        &[&context.payer], // Authority not signer.
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
async fn fail_incorrect_lockup_owner() {
    let mint = Pubkey::new_unique();

    let authority = Keypair::new();
    let token_account = get_associated_token_address_with_program_id(
        &authority.pubkey(),
        &mint,
        &spl_token_2022::id(),
    );

    let lockup = Pubkey::new_unique();

    let mut context = setup().start_with_context().await;
    setup_token_account(
        &mut context,
        &token_account,
        &authority.pubkey(),
        &mint,
        10_000,
    )
    .await;

    // Create the lockup account with the incorrect owner.
    {
        let rent = context.banks_client.get_rent().await.unwrap();
        let space = std::mem::size_of::<Lockup>();
        let lamports = rent.minimum_balance(space);
        context.set_account(
            &lockup,
            &AccountSharedData::new(lamports, space, &Pubkey::new_unique()), // Incorrect owner.
        );
    }

    let instruction = paladin_lockup_program::instruction::withdraw(
        &authority.pubkey(),
        &token_account,
        &token_account,
        &lockup,
        &mint,
        &spl_token_2022::id(),
    );

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
        TransactionError::InstructionError(0, InstructionError::InvalidAccountOwner)
    );
}

#[tokio::test]
async fn fail_lockup_not_enough_space() {
    let mint = Pubkey::new_unique();

    let authority = Keypair::new();
    let token_account = get_associated_token_address_with_program_id(
        &authority.pubkey(),
        &mint,
        &spl_token_2022::id(),
    );

    let lockup = Pubkey::new_unique();

    let mut context = setup().start_with_context().await;
    setup_token_account(
        &mut context,
        &token_account,
        &authority.pubkey(),
        &mint,
        10_000,
    )
    .await;

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

    let instruction = paladin_lockup_program::instruction::withdraw(
        &authority.pubkey(),
        &token_account,
        &token_account,
        &lockup,
        &mint,
        &spl_token_2022::id(),
    );

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
async fn fail_lockup_already_initialized() {
    let mint = Pubkey::new_unique();

    let authority = Keypair::new();
    let token_account = get_associated_token_address_with_program_id(
        &authority.pubkey(),
        &mint,
        &spl_token_2022::id(),
    );

    let lockup = Pubkey::new_unique();

    let mut context = setup().start_with_context().await;
    setup_token_account(
        &mut context,
        &token_account,
        &authority.pubkey(),
        &mint,
        10_000,
    )
    .await;

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

    let instruction = paladin_lockup_program::instruction::withdraw(
        &authority.pubkey(),
        &token_account,
        &token_account,
        &lockup,
        &mint,
        &spl_token_2022::id(),
    );

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
async fn fail_incorrect_escrow_authority_address() {
    let mint = Pubkey::new_unique();

    let authority = Keypair::new();
    let token_account = get_associated_token_address_with_program_id(
        &authority.pubkey(),
        &mint,
        &spl_token_2022::id(),
    );

    let lockup = Pubkey::new_unique();

    let mut context = setup().start_with_context().await;
    let clock = context.banks_client.get_sysvar::<Clock>().await.unwrap();

    // Create the lockup pool account.
    let pool = Pubkey::new_unique();
    setup_lockup_pool(&mut context, &pool).await;

    setup_token_account(
        &mut context,
        &token_account,
        &authority.pubkey(),
        &mint,
        10_000,
    )
    .await;
    setup_lockup(
        &mut context,
        &lockup,
        Lockup {
            discriminator: Lockup::SPL_DISCRIMINATOR.into(),
            amount: 10_000,
            authority: authority.pubkey(),
            lockup_start_timestamp: clock.unix_timestamp as u64,
            lockup_end_timestamp: NonZeroU64::new(clock.unix_timestamp as u64), // Unlocked.
            mint,
            pool,
            metadata: Pubkey::new_unique().to_bytes(),
        },
    )
    .await;
    add_seconds_to_clock(&mut context, LOCKUP_COOLDOWN_SECONDS).await;

    let mut instruction = paladin_lockup_program::instruction::withdraw(
        &authority.pubkey(),
        &token_account,
        &token_account,
        &lockup,
        &mint,
        &spl_token_2022::id(),
    );
    instruction.accounts[4].pubkey = Pubkey::new_unique(); // Incorrect escrow authority address.

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
            InstructionError::Custom(PaladinLockupError::IncorrectEscrowAuthorityAddress as u32)
        )
    );
}

#[tokio::test]
async fn fail_incorrect_escrow_token_account_address() {
    let mint = Pubkey::new_unique();

    let authority = Keypair::new();
    let token_account = get_associated_token_address_with_program_id(
        &authority.pubkey(),
        &mint,
        &spl_token_2022::id(),
    );

    let lockup = Pubkey::new_unique();

    let mut context = setup().start_with_context().await;

    let clock = context.banks_client.get_sysvar::<Clock>().await.unwrap();

    // Create the lockup pool account.
    let pool = Pubkey::new_unique();
    setup_lockup_pool(&mut context, &pool).await;

    setup_token_account(
        &mut context,
        &token_account,
        &authority.pubkey(),
        &mint,
        10_000,
    )
    .await;
    setup_lockup(
        &mut context,
        &lockup,
        Lockup {
            discriminator: Lockup::SPL_DISCRIMINATOR.into(),
            amount: 10_000,
            authority: authority.pubkey(),
            lockup_start_timestamp: clock.unix_timestamp as u64,
            lockup_end_timestamp: NonZeroU64::new(clock.unix_timestamp as u64), // Unlocked.
            mint,
            pool,
            metadata: Pubkey::new_unique().to_bytes(),
        },
    )
    .await;
    add_seconds_to_clock(&mut context, LOCKUP_COOLDOWN_SECONDS).await;

    let mut instruction = paladin_lockup_program::instruction::withdraw(
        &authority.pubkey(),
        &token_account,
        &token_account,
        &lockup,
        &mint,
        &spl_token_2022::id(),
    );
    instruction.accounts[5].pubkey = Pubkey::new_unique(); // Incorrect escrow token account address.

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
            InstructionError::Custom(PaladinLockupError::IncorrectEscrowTokenAccount as u32)
        )
    );
}

#[tokio::test]
async fn fail_lockup_still_active() {
    let mint = Pubkey::new_unique();

    let authority = Keypair::new();
    let token_account = get_associated_token_address_with_program_id(
        &authority.pubkey(),
        &mint,
        &spl_token_2022::id(),
    );

    let lockup = Pubkey::new_unique();

    let mut context = setup().start_with_context().await;

    let clock = context.banks_client.get_sysvar::<Clock>().await.unwrap();

    // Create the lockup pool account.
    let pool = Pubkey::new_unique();
    setup_lockup_pool(&mut context, &pool).await;

    setup_token_account(
        &mut context,
        &token_account,
        &authority.pubkey(),
        &mint,
        10_000,
    )
    .await;
    setup_lockup(
        &mut context,
        &lockup,
        Lockup {
            discriminator: Lockup::SPL_DISCRIMINATOR.into(),
            amount: 10_000,
            authority: authority.pubkey(),
            lockup_start_timestamp: clock.unix_timestamp as u64,
            lockup_end_timestamp: NonZeroU64::new(clock.unix_timestamp as u64), // Unlocked.
            mint,
            pool,
            metadata: Pubkey::new_unique().to_bytes(),
        },
    )
    .await;

    // Don't advance clock, still locked
    let instruction = paladin_lockup_program::instruction::withdraw(
        &authority.pubkey(),
        &token_account,
        &token_account,
        &lockup,
        &mint,
        &spl_token_2022::id(),
    );

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
            InstructionError::Custom(PaladinLockupError::LockupActive as u32)
        )
    );
}

#[tokio::test]
async fn fail_incorrect_lockup_authority() {
    let mint = Pubkey::new_unique();

    let authority = Keypair::new();
    let token_account = get_associated_token_address_with_program_id(
        &authority.pubkey(),
        &mint,
        &spl_token_2022::id(),
    );

    let lockup = Pubkey::new_unique();

    let mut context = setup().start_with_context().await;

    let clock = context.banks_client.get_sysvar::<Clock>().await.unwrap();

    // Create the lockup pool account.
    let pool = Pubkey::new_unique();
    setup_lockup_pool(&mut context, &pool).await;

    setup_token_account(
        &mut context,
        &token_account,
        &authority.pubkey(),
        &mint,
        10_000,
    )
    .await;
    setup_lockup(
        &mut context,
        &lockup,
        Lockup {
            discriminator: Lockup::SPL_DISCRIMINATOR.into(),
            amount: 10_000,
            authority: Pubkey::new_unique(), // Incorrect authority.
            lockup_start_timestamp: clock.unix_timestamp as u64,
            lockup_end_timestamp: NonZeroU64::new(clock.unix_timestamp as u64), // Unlocked.
            mint,
            pool,
            metadata: Pubkey::new_unique().to_bytes(),
        },
    )
    .await;
    add_seconds_to_clock(&mut context, LOCKUP_COOLDOWN_SECONDS).await;

    let instruction = paladin_lockup_program::instruction::withdraw(
        &authority.pubkey(),
        &token_account,
        &token_account,
        &lockup,
        &mint,
        &spl_token_2022::id(),
    );

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
async fn fail_incorrect_mint() {
    let mint = Pubkey::new_unique();

    let authority = Keypair::new();
    let token_account = get_associated_token_address_with_program_id(
        &authority.pubkey(),
        &mint,
        &spl_token_2022::id(),
    );

    let lockup = Pubkey::new_unique();

    let mut context = setup().start_with_context().await;

    let clock = context.banks_client.get_sysvar::<Clock>().await.unwrap();

    // Create the lockup pool account.
    let pool = Pubkey::new_unique();
    setup_lockup_pool(&mut context, &pool).await;

    setup_token_account(
        &mut context,
        &token_account,
        &authority.pubkey(),
        &mint,
        10_000,
    )
    .await;
    setup_lockup(
        &mut context,
        &lockup,
        Lockup {
            discriminator: Lockup::SPL_DISCRIMINATOR.into(),
            amount: 10_000,
            authority: authority.pubkey(),
            lockup_start_timestamp: clock.unix_timestamp as u64,
            lockup_end_timestamp: NonZeroU64::new(clock.unix_timestamp as u64), // Unlocked.
            mint: Pubkey::new_unique(),                                         // Incorrect mint.
            pool,
            metadata: Pubkey::new_unique().to_bytes(),
        },
    )
    .await;
    add_seconds_to_clock(&mut context, LOCKUP_COOLDOWN_SECONDS).await;

    let instruction = paladin_lockup_program::instruction::withdraw(
        &authority.pubkey(),
        &token_account,
        &token_account,
        &lockup,
        &mint,
        &spl_token_2022::id(),
    );

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
            InstructionError::Custom(PaladinLockupError::IncorrectMint as u32)
        )
    );
}

fn get_token_account_balance(token_account: &Account) -> u64 {
    StateWithExtensions::<TokenAccount>::unpack(&token_account.data)
        .unwrap()
        .base
        .amount
}

#[tokio::test]
async fn success() {
    let mint = Pubkey::new_unique();

    let authority = Keypair::new();
    let token_account = get_associated_token_address_with_program_id(
        &authority.pubkey(),
        &mint,
        &spl_token_2022::id(),
    );

    // Just posterity.
    let lamport_destination = Pubkey::new_unique();

    let escrow_authority = get_escrow_authority_address(&paladin_lockup_program::id());
    let escrow_token_account = get_associated_token_address_with_program_id(
        &escrow_authority,
        &mint,
        &spl_token_2022::id(),
    );

    let lockup = Pubkey::new_unique();

    let lockup_amount = 10_000;

    let mut context = setup().start_with_context().await;
    let clock = context.banks_client.get_sysvar::<Clock>().await.unwrap();

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
            lockup_start_timestamp: clock.unix_timestamp as u64,
            lockup_end_timestamp: NonZeroU64::new(clock.unix_timestamp as u64), // Unlocked.
            mint,
            pool,
            metadata: Pubkey::new_unique().to_bytes(),
        },
    )
    .await;
    add_seconds_to_clock(&mut context, LOCKUP_COOLDOWN_SECONDS).await;
    setup_token_account(
        &mut context,
        &token_account,
        &authority.pubkey(),
        &mint,
        10_000,
    )
    .await;
    setup_token_account(
        &mut context,
        &escrow_token_account,
        &escrow_authority,
        &mint,
        10_000,
    )
    .await;
    setup_mint(&mut context, &mint, &Pubkey::new_unique(), 1_000_000).await;

    // For checks later.
    let lockup_account_start_lamports = context
        .banks_client
        .get_account(lockup)
        .await
        .unwrap()
        .unwrap()
        .lamports;
    let token_account_start_balance = get_token_account_balance(
        &context
            .banks_client
            .get_account(token_account)
            .await
            .unwrap()
            .unwrap(),
    );
    let escrow_token_account_start_balance = get_token_account_balance(
        &context
            .banks_client
            .get_account(escrow_token_account)
            .await
            .unwrap()
            .unwrap(),
    );

    let instruction = paladin_lockup_program::instruction::withdraw(
        &authority.pubkey(),
        &lamport_destination,
        &token_account,
        &lockup,
        &mint,
        &spl_token_2022::id(),
    );

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

    // Check the resulting destination lamport balance.
    let lamport_destination_end_balance = context
        .banks_client
        .get_account(lamport_destination)
        .await
        .unwrap()
        .unwrap()
        .lamports;
    assert_eq!(
        lamport_destination_end_balance,
        lockup_account_start_lamports
    );

    // Check the resulting token account balances.
    let token_account_end_balance = get_token_account_balance(
        &context
            .banks_client
            .get_account(token_account)
            .await
            .unwrap()
            .unwrap(),
    );
    let escrow_token_account_end_balance = get_token_account_balance(
        &context
            .banks_client
            .get_account(escrow_token_account)
            .await
            .unwrap()
            .unwrap(),
    );

    assert_eq!(
        token_account_end_balance,
        token_account_start_balance.saturating_add(lockup_amount)
    );
    assert_eq!(
        escrow_token_account_end_balance,
        escrow_token_account_start_balance.saturating_sub(lockup_amount)
    );

    // Assert the lockup account was closed.
    assert!(context
        .banks_client
        .get_account(lockup)
        .await
        .unwrap()
        .is_none());
}
