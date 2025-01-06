//! Program processor.

use {
    crate::{
        error::PaladinLockupError,
        instruction::PaladinLockupInstruction,
        state::{
            collect_escrow_authority_signer_seeds, get_escrow_authority_address,
            get_escrow_authority_address_and_bump_seed, Lockup, LockupPool, LockupPoolEntry,
        },
        LOCKUP_COOLDOWN_SECONDS,
    },
    solana_program::{
        account_info::{next_account_info, AccountInfo},
        clock::Clock,
        entrypoint::ProgramResult,
        msg,
        program_error::ProgramError,
        pubkey::Pubkey,
        system_program,
        sysvar::Sysvar,
    },
    spl_associated_token_account::get_associated_token_address_with_program_id,
    spl_discriminator::{ArrayDiscriminator, SplDiscriminate},
    spl_token_2022::{extension::StateWithExtensions, state::Mint},
    std::{cmp::Reverse, num::NonZeroU64},
};

/// Processes a
/// [InitializeLockupPool](enum.PaladinInitializeLockupPoolInstruction.html)
/// instruction.
fn process_initialize_lockup_pool(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let accounts_iter = &mut accounts.iter();
    let lockup_pool_info = next_account_info(accounts_iter)?;

    // Validate the provided account.
    assert_eq!(lockup_pool_info.owner, program_id);
    assert_eq!(lockup_pool_info.data_len(), LockupPool::LEN);

    // Write the discriminator.
    let mut lockup_pool_data = lockup_pool_info.data.borrow_mut();
    let lockup_pool_state = bytemuck::from_bytes_mut::<LockupPool>(&mut lockup_pool_data);
    lockup_pool_state.discriminator = LockupPool::SPL_DISCRIMINATOR.into();

    Ok(())
}

/// Processes a
/// [Lockup](enum.PaladinLockupInstruction.html)
/// instruction.
fn process_lockup(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    metadata: [u8; 32],
    amount: u64,
) -> ProgramResult {
    let accounts_iter = &mut accounts.iter();

    let lockup_authority_info = next_account_info(accounts_iter)?;
    let token_owner_info = next_account_info(accounts_iter)?;
    let token_account_info = next_account_info(accounts_iter)?;
    let lockup_pool_info = next_account_info(accounts_iter)?;
    let lockup_info = next_account_info(accounts_iter)?;
    let escrow_authority_info = next_account_info(accounts_iter)?;
    let escrow_token_account_info = next_account_info(accounts_iter)?;
    let mint_info = next_account_info(accounts_iter)?;
    let token_program_info = next_account_info(accounts_iter)?;

    // Validate & deserialize the lockup pool.
    assert_eq!(
        lockup_pool_info.owner, program_id,
        "lockup_pool invalid owner"
    );
    assert_eq!(
        lockup_pool_info.data_len(),
        LockupPool::LEN,
        "lockup_pool uninitialized"
    );
    let mut lockup_pool_data = lockup_pool_info.data.borrow_mut();
    let lockup_pool_state = bytemuck::from_bytes_mut::<LockupPool>(&mut lockup_pool_data);

    // Ensure the lockup account is owned by the Paladin Lockup program.
    if lockup_info.owner != program_id {
        return Err(ProgramError::InvalidAccountOwner);
    }

    // Ensure the lockup account has enough space.
    if lockup_info.data_len() != std::mem::size_of::<Lockup>() {
        return Err(ProgramError::InvalidAccountData);
    }

    // Ensure the lockup account is not initialized.
    if &lockup_info.try_borrow_data()?[0..8] != ArrayDiscriminator::UNINITIALIZED.as_slice() {
        return Err(ProgramError::AccountAlreadyInitialized);
    }

    // Ensure the provided escrow authority address is correct.
    if !escrow_authority_info
        .key
        .eq(&get_escrow_authority_address(program_id))
    {
        return Err(PaladinLockupError::IncorrectEscrowAuthorityAddress.into());
    }

    // Ensure the provided escrow token account address is correct.
    if !escrow_token_account_info
        .key
        .eq(&get_associated_token_address_with_program_id(
            escrow_authority_info.key,
            mint_info.key,
            token_program_info.key,
        ))
    {
        return Err(PaladinLockupError::IncorrectEscrowTokenAccount.into());
    }

    // Write the data.
    let mut data = lockup_info.try_borrow_mut_data()?;
    *bytemuck::try_from_bytes_mut(&mut data).map_err(|_| ProgramError::InvalidAccountData)? =
        Lockup {
            discriminator: Lockup::SPL_DISCRIMINATOR.into(),
            amount,
            authority: *lockup_authority_info.key,
            lockup_start_timestamp: Clock::get()?.unix_timestamp as u64,
            lockup_end_timestamp: None,
            mint: *mint_info.key,
            pool: *lockup_pool_info.key,
            metadata,
        };

    // Evict the smallest lock if necessary.
    let last_index = std::cmp::min(
        lockup_pool_state.entries_len,
        LockupPool::LOCKUP_CAPACITY - 1,
    );
    let last_amount = lockup_pool_state.entries[last_index].amount;
    match (
        lockup_pool_state.entries_len == lockup_pool_state.entries.len(),
        amount > last_amount,
    ) {
        (true, true) => {}
        (true, false) => return Err(PaladinLockupError::AmountTooLow.into()),
        (false, _) => {
            lockup_pool_state.entries_len = lockup_pool_state.entries_len.checked_add(1).unwrap()
        }
    }

    // Binary search & insert the entry.
    let index = match lockup_pool_state
        .entries
        .binary_search_by_key(&Reverse(amount), |entry| Reverse(entry.amount))
    {
        Ok(index) => index,
        Err(index) => index,
    };
    *lockup_pool_state.entries.last_mut().unwrap() = LockupPoolEntry {
        lockup: *lockup_info.key,
        amount,
        metadata,
    };
    lockup_pool_state.entries[index..].rotate_right(1);

    // Transfer the tokens to the escrow token account.
    {
        let decimals = {
            let mint_data = mint_info.try_borrow_data()?;
            let mint = StateWithExtensions::<Mint>::unpack(&mint_data)?;
            mint.base.decimals
        };

        spl_token_2022::onchain::invoke_transfer_checked(
            &spl_token_2022::id(),
            token_account_info.clone(),
            mint_info.clone(),
            escrow_token_account_info.clone(),
            token_owner_info.clone(),
            accounts_iter.as_slice(),
            amount,
            decimals,
            &[],
        )?;
    }

    Ok(())
}

/// Processes an
/// [Unlock](enum.PaladinLockupInstruction.html)
/// instruction.
fn process_unlock(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let accounts_iter = &mut accounts.iter();

    let lockup_authority_info = next_account_info(accounts_iter)?;
    let lockup_pool_info = next_account_info(accounts_iter)?;
    let lockup_info = next_account_info(accounts_iter)?;

    // Validate & deserialize the lockup pool.
    assert_eq!(
        lockup_pool_info.owner, program_id,
        "lockup_pool invalid owner"
    );
    assert_eq!(
        lockup_pool_info.data_len(),
        LockupPool::LEN,
        "lockup_pool uninitialized"
    );
    let mut lockup_pool_data = lockup_pool_info.data.borrow_mut();
    let lockup_pool_state = bytemuck::from_bytes_mut::<LockupPool>(&mut lockup_pool_data);

    // Ensure the lockup authority is a signer.
    if !lockup_authority_info.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    // Ensure the lockup account is owned by the Paladin Lockup program.
    if lockup_info.owner != program_id {
        return Err(ProgramError::InvalidAccountOwner);
    }

    // Ensure the lockup account is initialized.
    if !(lockup_info.data_len() == std::mem::size_of::<Lockup>()
        && &lockup_info.try_borrow_data()?[0..8] == Lockup::SPL_DISCRIMINATOR_SLICE)
    {
        return Err(ProgramError::UninitializedAccount);
    }

    let mut data = lockup_info.try_borrow_mut_data()?;
    let state = bytemuck::try_from_bytes_mut::<Lockup>(&mut data)
        .map_err(|_| ProgramError::InvalidAccountData)?;

    // Ensure the provided authority is the same as the lockup's authority.
    if state.authority != *lockup_authority_info.key {
        return Err(ProgramError::IncorrectAuthority);
    }

    // Ensure the lockup account has not already been unlocked.
    if state.lockup_end_timestamp.is_some() {
        return Err(PaladinLockupError::LockupAlreadyUnlocked.into());
    }

    // Get the timestamp from the clock sysvar, and use it to set the end
    // timestamp of the lockup, effectively unlocking the funds.
    let clock = <Clock as Sysvar>::get()?;
    state.lockup_end_timestamp = NonZeroU64::new(clock.unix_timestamp as u64);

    // Ensure the lockup matches the pool.
    if lockup_pool_info.key != &state.pool {
        return Err(PaladinLockupError::IncorrectPool.into());
    }

    // Remove the entry from the pool (if it exists).
    let partition_point = lockup_pool_state
        .entries
        .partition_point(|entry| entry.amount > state.amount);
    if partition_point != lockup_pool_state.entries_len {
        let offset = lockup_pool_state.entries[partition_point..]
            .iter()
            .take_while(|entry| entry.amount == state.amount)
            .position(|entry| &entry.lockup == lockup_info.key)
            .unwrap();
        #[allow(clippy::arithmetic_side_effects)]
        let index = partition_point + offset;

        lockup_pool_state.entries[index] = LockupPoolEntry::default();
        lockup_pool_state.entries[index..].rotate_left(1);
        lockup_pool_state.entries_len = lockup_pool_state.entries_len.checked_sub(1).unwrap();
    }

    Ok(())
}

/// Processes a
/// [Withdraw](enum.PaladinLockupInstruction.html)
/// instruction.
fn process_withdraw(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let accounts_iter = &mut accounts.iter();

    let lockup_authority_info = next_account_info(accounts_iter)?;
    let lamport_destination_info = next_account_info(accounts_iter)?;
    let token_destination_info = next_account_info(accounts_iter)?;
    let lockup_info = next_account_info(accounts_iter)?;
    let escrow_authority_info = next_account_info(accounts_iter)?;
    let escrow_token_account_info = next_account_info(accounts_iter)?;
    let mint_info = next_account_info(accounts_iter)?;
    let token_program_info = next_account_info(accounts_iter)?;

    // Note that Token-2022's `TransferChecked` processor will assert the
    // provided token account is for the provided mint.

    // Ensure the lockup authority is a signer.
    if !lockup_authority_info.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    // Ensure the lockup account is owned by the Paladin Lockup program.
    if lockup_info.owner != program_id {
        return Err(ProgramError::InvalidAccountOwner);
    }

    // Ensure the lockup account is initialized.
    if !(lockup_info.data_len() == std::mem::size_of::<Lockup>()
        && &lockup_info.try_borrow_data()?[0..8] == Lockup::SPL_DISCRIMINATOR_SLICE)
    {
        return Err(ProgramError::UninitializedAccount);
    }

    // Ensure the provided escrow authority address is correct.
    let (escrow_authority_address, bump_seed) =
        get_escrow_authority_address_and_bump_seed(program_id);
    if !escrow_authority_info.key.eq(&escrow_authority_address) {
        return Err(PaladinLockupError::IncorrectEscrowAuthorityAddress.into());
    }

    // Ensure the provided escrow token account address is correct.
    if !escrow_token_account_info
        .key
        .eq(&get_associated_token_address_with_program_id(
            escrow_authority_info.key,
            mint_info.key,
            token_program_info.key,
        ))
    {
        return Err(PaladinLockupError::IncorrectEscrowTokenAccount.into());
    }

    let withdraw_amount = {
        let data = lockup_info.try_borrow_data()?;
        let state = bytemuck::try_from_bytes::<Lockup>(&data)
            .map_err(|_| ProgramError::InvalidAccountData)?;

        // Ensure the provided authority is the same as the lockup's authority.
        if state.authority != *lockup_authority_info.key {
            return Err(ProgramError::IncorrectAuthority);
        }

        // Ensure the provided mint is the same as the lockup's mint.
        if state.mint != *mint_info.key {
            return Err(PaladinLockupError::IncorrectMint.into());
        }

        // Ensure the lockup has ended.
        let clock = <Clock as Sysvar>::get()?;
        let timestamp = clock.unix_timestamp as u64;
        let unlock_timestamp = state
            .lockup_end_timestamp
            .ok_or(PaladinLockupError::LockupActive)?
            .get()
            .saturating_add(LOCKUP_COOLDOWN_SECONDS);
        if unlock_timestamp > timestamp {
            msg!(
                "Lockup has not ended yet. {} seconds remaining.",
                unlock_timestamp.saturating_sub(timestamp)
            );
            return Err(PaladinLockupError::LockupActive.into());
        }

        state.amount
    };

    // Transfer the tokens to the depositor.
    {
        let bump_seed = [bump_seed];
        let escrow_authority_signer_seeds = collect_escrow_authority_signer_seeds(&bump_seed);
        let decimals = {
            let mint_data = mint_info.try_borrow_data()?;
            let mint = StateWithExtensions::<Mint>::unpack(&mint_data)?;
            mint.base.decimals
        };

        spl_token_2022::onchain::invoke_transfer_checked(
            &spl_token_2022::id(),
            escrow_token_account_info.clone(),
            mint_info.clone(),
            token_destination_info.clone(),
            escrow_authority_info.clone(),
            accounts_iter.as_slice(),
            withdraw_amount,
            decimals,
            &[&escrow_authority_signer_seeds],
        )?;
    }

    let new_destination_lamports = lockup_info
        .lamports()
        .checked_add(lamport_destination_info.lamports())
        .ok_or(ProgramError::ArithmeticOverflow)?;

    **lockup_info.try_borrow_mut_lamports()? = 0;
    **lamport_destination_info.try_borrow_mut_lamports()? = new_destination_lamports;

    lockup_info.realloc(0, true)?;
    lockup_info.assign(&system_program::id());

    Ok(())
}

/// Processes a
/// [PaladinLockupInstruction](enum.PaladinLockupInstruction.html).
pub fn process(program_id: &Pubkey, accounts: &[AccountInfo], input: &[u8]) -> ProgramResult {
    let instruction = PaladinLockupInstruction::unpack(input)?;
    match instruction {
        PaladinLockupInstruction::InitializeLockupPool => {
            msg!("Instruction: InitializeLockupPool");
            process_initialize_lockup_pool(program_id, accounts)
        }
        PaladinLockupInstruction::Lockup { metadata, amount } => {
            msg!("Instruction: Lockup");
            process_lockup(program_id, accounts, metadata, amount)
        }
        PaladinLockupInstruction::Unlock => {
            msg!("Instruction: Unlock");
            process_unlock(program_id, accounts)
        }
        PaladinLockupInstruction::Withdraw => {
            msg!("Instruction: Withdraw");
            process_withdraw(program_id, accounts)
        }
    }
}
