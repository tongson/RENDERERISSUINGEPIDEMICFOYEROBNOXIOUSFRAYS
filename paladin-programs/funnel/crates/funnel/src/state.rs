use borsh::{BorshDeserialize, BorshSerialize};
use bytemuck::{Pod, PodCastError, Zeroable};
use solana_program::pubkey::Pubkey;

#[derive(Debug, Clone, Copy, Zeroable, Pod, BorshSerialize, BorshDeserialize)]
#[repr(C)]
pub struct Funnel {
    /// Dynamic state that tracks the current recipient of funnel rewards
    /// (expected to be the current leader).
    pub receiver: Pubkey,
    /// Static state that controls the static reward recipients.
    pub config: FunnelConfig,
}

impl Funnel {
    pub const LEN: usize = 32 + FunnelConfig::LEN;
    const _LEN_CHECK: () = match std::mem::size_of::<Self>() == Self::LEN {
        true => (),
        false => panic!(),
    };

    pub fn try_from_bytes(bytes: &[u8]) -> Result<&Self, PodCastError> {
        bytemuck::try_from_bytes(bytes)
    }

    pub fn as_bytes(&self) -> &[u8] {
        bytemuck::bytes_of(self)
    }
}

#[derive(Debug, Clone, Copy, Zeroable, Pod, BorshSerialize, BorshDeserialize)]
#[repr(C)]
pub struct FunnelConfig {
    pub stakers_receiver: Pubkey,
    pub holders_receiver: Pubkey,
}

impl FunnelConfig {
    pub const LEN: usize = 64;
    const _LEN_CHECK: () = match std::mem::size_of::<Self>() == Self::LEN {
        true => (),
        false => panic!(),
    };

    pub fn try_from_bytes(bytes: &[u8]) -> Result<&Self, PodCastError> {
        bytemuck::try_from_bytes(bytes)
    }

    pub fn as_bytes(&self) -> &[u8] {
        bytemuck::bytes_of(self)
    }
}

pub fn find_leader_state(leader: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[&leader.to_bytes()], &crate::id())
}

#[derive(Debug, Clone, Copy, Zeroable, Pod, BorshSerialize, BorshDeserialize)]
#[repr(C)]
pub struct LeaderState {
    /// The last slot where the leader successfully called this program.
    pub last_slot: u64,
}

impl LeaderState {
    pub const LEN: usize = std::mem::size_of::<LeaderState>();

    pub fn try_from_bytes(bytes: &[u8]) -> Result<&Self, PodCastError> {
        bytemuck::try_from_bytes(bytes)
    }

    pub fn as_bytes(&self) -> &[u8] {
        bytemuck::bytes_of(self)
    }
}
