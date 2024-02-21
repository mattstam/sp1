use core::marker::PhantomData;
use sp1_core::stark::{RiscvChip, StarkGenericConfig};

pub struct RecursiveVerifier<SC>(PhantomData<SC>);

impl<SC: StarkGenericConfig> RecursiveVerifier<SC> {
    pub fn verify_shard(config: &SC, chips: &[&RiscvChip<SC>]) {}
}
