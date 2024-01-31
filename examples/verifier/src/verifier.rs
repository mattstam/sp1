#![no_main]

extern crate succinct_zkvm;

use std::fs;
use std::hint::black_box;

use clap::{command, Parser};
use serde_json;
use succinct_core::runtime::Program;
use succinct_core::runtime::Runtime;
use succinct_core::stark::types::SegmentProof;
use succinct_core::utils::BabyBearPoseidon2;
use succinct_core::utils::StarkUtils;

succinct_zkvm::entrypoint!(main);

#[derive(Parser, Debug, Clone)]
#[command(about = "Profile a program.")]
struct VerifierArgs {
    #[arg(long)]
    pub program: String,

    #[arg(long)]
    pub proof_directory: String,
}

#[succinct_derive::cycle_tracker]
fn verify<F, P, const WIDTH: usize>(
    runtime: &mut Runtime,
    config: &BabyBearPoseidon2,
    challenger: &mut DuplexChallenger<BabyBear, P, WIDTH>,
    segment_proofs: &[SegmentProof<BabyBearPoseidon2>],
    global_proof: &GlobalProof<BabyBearPoseidon2>,
) {
    runtime
        .verify::<_, _, BabyBearPoseidon2>(&config, challenger, &segment_proofs, &global_proof)
        .unwrap();
}

fn main() {
    let args = VerifierArgs::parse();

    log::info!("Verifying proof: {}", args.proof_directory.as_str());

    let segment_proofs: Vec<SegmentProof<BabyBearPoseidon2>> = {
        let segment_proofs_file_name = format!("{}/segment_proofs.json", args.proof_directory);
        let segment_proofs_json = fs::read_to_string(segment_proofs_file_name).unwrap();
        serde_json::from_str(&segment_proofs_json).unwrap()
    };

    let global_proof = {
        let global_proof_file_name = format!("{}/global_proof.json", args.proof_directory);
        let global_proof_json = fs::read_to_string(global_proof_file_name).unwrap();
        serde_json::from_str(&global_proof_json).unwrap()
    };

    let config = BabyBearPoseidon2::new();
    let mut challenger = config.challenger();

    let program = Program::from_elf(args.program.as_str());
    let mut runtime = Runtime::new(program);
    verify(
        &mut runtime,
        &config,
        &mut challenger,
        &segment_proofs,
        &global_proof,
    );
}