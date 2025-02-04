use solana_program::account_info::AccountInfo;
use solana_program::entrypoint::ProgramResult;
use solana_program::program::invoke;
use solana_program::rent::Rent;
use solana_program::sysvar::Sysvar;
use solana_program::{system_instruction, system_program};

use crate::{Funnel, FunnelConfig};

struct InitializeFunnelAccounts<'a, 'info> {
    /// We need the system_program loaded for CPIs.
    #[allow(dead_code)]
    system_program: &'a AccountInfo<'info>,
    /// CHECK: Must be writeable.
    payer: &'a AccountInfo<'info>,
    /// CHECK: Must be signer & writeable. Must not already be initialized.
    funnel: &'a AccountInfo<'info>,
}

pub(crate) fn process(accounts: &[AccountInfo], config: FunnelConfig) -> ProgramResult {
    // Pull out all the required accounts.
    let mut accounts = accounts.iter();
    let accounts = InitializeFunnelAccounts {
        system_program: accounts.next().unwrap(),
        payer: accounts.next().unwrap(),
        funnel: accounts.next().unwrap(),
    };

    // Check accounts.
    assert!(accounts.funnel.data_is_empty());
    assert_eq!(accounts.funnel.owner, &system_program::ID);

    // Transfer lamports for rent if necessary.
    let required = Rent::get()?.minimum_balance(Funnel::LEN);
    let existing = **accounts.funnel.lamports.borrow();
    let additional = required.saturating_sub(existing);
    if additional > 0 {
        invoke(
            &system_instruction::transfer(accounts.payer.key, accounts.funnel.key, additional),
            &[accounts.payer.clone(), accounts.funnel.clone()],
        )?;
    }

    // Allocate the required space.
    invoke(
        &system_instruction::allocate(accounts.funnel.key, Funnel::LEN as u64),
        &[accounts.funnel.clone()],
    )?;

    // Set funnel program as the owner.
    invoke(
        &system_instruction::assign(accounts.funnel.key, &crate::ID),
        &[accounts.funnel.clone()],
    )?;

    // Set the funnel config.
    let mut funnel = accounts.funnel.data.borrow_mut();
    let funnel = bytemuck::from_bytes_mut::<Funnel>(&mut funnel);
    funnel.receiver = *accounts.payer.key;
    funnel.config = config;

    Ok(())
}
