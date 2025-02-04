use solana_program::account_info::AccountInfo;
use solana_program::clock::Clock;
use solana_program::entrypoint::ProgramResult;
use solana_program::instruction::{AccountMeta, Instruction};
use solana_program::program::{invoke, invoke_signed};
use solana_program::pubkey::Pubkey;
use solana_program::rent::Rent;
use solana_program::system_instruction;
use solana_program::sysvar::Sysvar;

use crate::{Funnel, LeaderState, JITO_TIP_PAYMENT_PROGRAM};

const FUNNEL_ACCOUNTS_LEN: usize = 7;
const _: () = match std::mem::size_of::<BecomeReceiverAccounts>() == FUNNEL_ACCOUNTS_LEN * 8 {
    true => (),
    false => panic!(),
};

struct BecomeReceiverAccounts<'a, 'info> {
    // NB: We just need this present so we can CPI to the system program.
    #[allow(dead_code)]
    system_program: &'a AccountInfo<'info>,
    /// CHECK: Must be a [`crate::Funnel`].
    funnel: &'a AccountInfo<'info>,
    /// CHECK: Must match configured stakers receiver.
    stakers_receiver: &'a AccountInfo<'info>,
    /// CHECK: Must match configured holders receiver.
    holders_receiver: &'a AccountInfo<'info>,
    /// CHECK: Must match validator's Jito distribution account.
    receiver_old: &'a AccountInfo<'info>,
    receiver_new: &'a AccountInfo<'info>,
    /// CHECK: Must match `PDA(receiver_new)`.
    receiver_new_state: &'a AccountInfo<'info>,
}

pub(crate) fn process(
    accounts: &[AccountInfo],
    new_receiver: Pubkey,
    additional_lamports: u64,
) -> ProgramResult {
    let (accounts, jito_accounts) = accounts.split_at(FUNNEL_ACCOUNTS_LEN);
    let mut accounts_iter = accounts.iter();
    let funnel_accounts = BecomeReceiverAccounts {
        system_program: accounts_iter.next().unwrap(),
        funnel: accounts_iter.next().unwrap(),
        stakers_receiver: accounts_iter.next().unwrap(),
        holders_receiver: accounts_iter.next().unwrap(),
        receiver_old: accounts_iter.next().unwrap(),
        receiver_new: accounts_iter.next().unwrap(),
        receiver_new_state: accounts_iter.next().unwrap(),
    };

    // Validate & deserialize the funnel account.
    assert_eq!(funnel_accounts.funnel.owner, &crate::ID);
    let funnel_borrow = funnel_accounts.funnel.data.borrow();
    let funnel = bytemuck::from_bytes::<Funnel>(&funnel_borrow);

    // Validate the remaining accounts.
    assert_eq!(funnel_accounts.stakers_receiver.key, &funnel.config.stakers_receiver);
    assert_eq!(funnel_accounts.holders_receiver.key, &funnel.config.holders_receiver);
    assert_eq!(funnel_accounts.receiver_old.key, &funnel.receiver);
    // NB: Jito needs to be able to take a borrow to funnel.
    drop(funnel_borrow);

    // Construct & execute change tip receiver instruction.
    let jito_account_metas = jito_accounts
        .iter()
        .map(|account| AccountMeta {
            pubkey: *account.key,
            is_signer: account.is_signer,
            is_writable: account.is_writable,
        })
        .collect();
    let ix = Instruction::new_with_bytes(
        JITO_TIP_PAYMENT_PROGRAM,
        &anchor_discriminator("global:change_tip_receiver"),
        jito_account_metas,
    );
    invoke(&ix, jito_accounts)?;

    // Split & push the rewards.
    split_rewards(&funnel_accounts, additional_lamports);

    // Update the funnel receiver.
    let mut funnel = funnel_accounts.funnel.data.borrow_mut();
    let funnel = bytemuck::from_bytes_mut::<Funnel>(&mut funnel);
    funnel.receiver = new_receiver;

    // Initialize the leader state account if necessary.
    let (leader_state, leader_state_bump) =
        crate::find_leader_state(funnel_accounts.receiver_new.key);
    assert_eq!(funnel_accounts.receiver_new_state.key, &leader_state,);
    if funnel_accounts.receiver_new_state.owner != &crate::ID {
        // Transfer lamports for rent if necessary.
        let required = Rent::get()?.minimum_balance(LeaderState::LEN);
        let existing = **funnel_accounts.receiver_new_state.lamports.borrow();
        let additional = required.saturating_sub(existing);
        if additional > 0 {
            invoke(
                &system_instruction::transfer(
                    funnel_accounts.receiver_new.key,
                    funnel_accounts.receiver_new_state.key,
                    additional,
                ),
                &[funnel_accounts.receiver_new.clone(), funnel_accounts.receiver_new_state.clone()],
            )?;
        }

        // Allocate the required space.
        invoke_signed(
            &system_instruction::allocate(
                funnel_accounts.receiver_new_state.key,
                LeaderState::LEN as u64,
            ),
            &[funnel_accounts.receiver_new_state.clone()],
            &[&[&funnel_accounts.receiver_new.key.to_bytes() as &[u8], &[leader_state_bump]]],
        )?;

        // Set the funnel program as the owner.
        invoke_signed(
            &system_instruction::assign(funnel_accounts.receiver_new_state.key, &crate::ID),
            &[funnel_accounts.receiver_new_state.clone()],
            &[&[&funnel_accounts.receiver_new.key.to_bytes() as &[u8], &[leader_state_bump]]],
        )?;
    }

    // Set the last slot to the current slot.
    let mut leader_state = funnel_accounts.receiver_new_state.data.borrow_mut();
    let leader_state = bytemuck::from_bytes_mut::<LeaderState>(&mut leader_state);
    leader_state.last_slot = Clock::get().unwrap().slot;

    Ok(())
}

// SAFETY:
//
// - Division by non-zero constant cannot trigger division by zero.
// - Overflowing lamport balance is economically unfeasible. Additionally, would
//   be caught by the solana runtime as an unbalanced transfer.
#[allow(clippy::arithmetic_side_effects)]
fn split_rewards(accounts: &BecomeReceiverAccounts, additional_lamports: u64) {
    // Get balance & rent requirements.
    let balance = accounts.funnel.lamports();
    let rent = Rent::get()
        .unwrap()
        .minimum_balance(accounts.funnel.data_len());
    let total = balance.saturating_sub(rent);

    // Compute reward split.
    let stakers_reward = total / 20; // 5%
    let holders_reward = total / 20; // 5%
    let validator_reward = total // 90%
        .checked_sub(stakers_reward)
        .unwrap()
        .checked_sub(holders_reward)
        .unwrap();
    debug_assert_eq!(stakers_reward + holders_reward + validator_reward, total);

    // Add prepay_lamports.
    let stakers_additional = additional_lamports / 2;
    let holders_additional = additional_lamports - stakers_additional;

    // Process payments.
    **accounts.stakers_receiver.lamports.borrow_mut() += stakers_reward;
    **accounts.holders_receiver.lamports.borrow_mut() += holders_reward;
    **accounts.receiver_old.lamports.borrow_mut() += validator_reward;
    **accounts.funnel.lamports.borrow_mut() -= total;
    if stakers_additional > 0 {
        invoke(
            &system_instruction::transfer(
                accounts.receiver_new.key,
                accounts.stakers_receiver.key,
                stakers_additional,
            ),
            &[accounts.receiver_new.clone(), accounts.stakers_receiver.clone()],
        )
        .unwrap();
    }
    if holders_additional > 0 {
        invoke(
            &system_instruction::transfer(
                accounts.receiver_new.key,
                accounts.holders_receiver.key,
                holders_additional,
            ),
            &[accounts.receiver_new.clone(), accounts.holders_receiver.clone()],
        )
        .unwrap();
    }
}

use sha2_const_stable::Sha256;

const fn anchor_discriminator(preimage: &'static str) -> [u8; 8] {
    let hash: [u8; 32] = Sha256::new().update(preimage.as_bytes()).finalize();

    [hash[0], hash[1], hash[2], hash[3], hash[4], hash[5], hash[6], hash[7]]
}
