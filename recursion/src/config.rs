use sp1_core::stark::StarkGenericConfig;

pub trait RecursiveStarkConfig {
    type SC: StarkGenericConfig;

    type RecursiveChallenger;
}
