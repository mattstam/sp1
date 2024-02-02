use std::time::Instant;

use crate::{
    runtime::{Program, Runtime},
    stark::StarkConfig,
};

pub trait StarkUtils: StarkConfig {
    type UniConfig: p3_uni_stark::StarkConfig<
        Val = Self::Val,
        PackedVal = Self::PackedVal,
        Challenge = Self::Challenge,
        PackedChallenge = Self::PackedChallenge,
        Pcs = Self::Pcs,
        Challenger = Self::Challenger,
    >;
    fn challenger(&self) -> Self::Challenger;

    fn uni_stark_config(&self) -> &Self::UniConfig;
}

#[cfg(not(feature = "perf"))]
use crate::lookup::{debug_interactions_with_all_chips, InteractionKind};
