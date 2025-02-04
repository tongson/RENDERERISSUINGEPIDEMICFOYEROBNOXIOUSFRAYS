mod constants;
#[cfg(feature = "bpf-entrypoint")]
mod entrypoint;
pub mod instructions;
mod processor;
mod state;

pub use constants::*;
pub use instructions::FunnelInstruction;
pub use processor::process;
pub use state::*;

solana_program::declare_id!("5rdqfu3FwVWEgY7R7pQSA52cocbWnAEvnoi4bej9EAT5");
