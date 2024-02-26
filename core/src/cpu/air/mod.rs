pub mod branch;
pub mod memory;

use core::borrow::Borrow;
use p3_air::Air;
use p3_air::AirBuilder;
use p3_air::BaseAir;
use p3_field::AbstractField;
use p3_matrix::MatrixRowSlices;

use crate::air::{SP1AirBuilder, WordAirBuilder};
use crate::cpu::columns::OpcodeSelectorCols;
use crate::cpu::columns::{CpuCols, NUM_CPU_COLS};
use crate::cpu::CpuChip;
use crate::memory::MemoryCols;
use crate::runtime::{AccessPosition, Opcode};

impl<AB> Air<AB> for CpuChip
where
    AB: SP1AirBuilder,
{
    #[inline(never)]
    fn eval(&self, builder: &mut AB) {
        let main = builder.main();
        let local: &CpuCols<AB::Var> = main.row_slice(0).borrow();
        let next: &CpuCols<AB::Var> = main.row_slice(1).borrow();

        // Compute some flags for which type of instruction we are dealing with.
        let is_memory_instruction: AB::Expr = self.is_memory_instruction::<AB>(&local.selectors);
        let is_branch_instruction: AB::Expr = self.is_branch_instruction::<AB>(&local.selectors);
        let is_alu_instruction: AB::Expr = self.is_alu_instruction::<AB>(&local.selectors);

        // Clock constraints.
        // TODO: Handle dynamic clock jumps based on precompiles.
        // builder
        //     .when_transition()
        //     .assert_eq(local.clk + AB::F::from_canonical_u32(4), next.clk);

        // Program constraints.
        builder.send_program(local.pc, local.instruction, local.selectors, local.is_real);

        // Load immediates into b and c, if the immediate flags are on.
        builder
            .when(local.selectors.imm_b)
            .assert_word_eq(local.op_b_val(), local.instruction.op_b);
        builder
            .when(local.selectors.imm_c)
            .assert_word_eq(local.op_c_val(), local.instruction.op_c);

        // If they are not immediates, read `b` and `c` from memory.
        builder.constraint_memory_access(
            local.shard,
            local.clk + AB::F::from_canonical_u32(AccessPosition::B as u32),
            local.instruction.op_b[0],
            &local.op_b_access,
            AB::Expr::one() - local.selectors.imm_b,
        );
        builder
            .when(AB::Expr::one() - local.selectors.imm_b)
            .assert_word_eq(local.op_b_val(), *local.op_b_access.prev_value());

        builder.constraint_memory_access(
            local.shard,
            local.clk + AB::F::from_canonical_u32(AccessPosition::C as u32),
            local.instruction.op_c[0],
            &local.op_c_access,
            AB::Expr::one() - local.selectors.imm_c,
        );
        builder
            .when(AB::Expr::one() - local.selectors.imm_c)
            .assert_word_eq(local.op_c_val(), *local.op_c_access.prev_value());

        // Write the `a` or the result to the first register described in the instruction unless
        // we are performing a branch or a store.
        builder.constraint_memory_access(
            local.shard,
            local.clk + AB::F::from_canonical_u32(AccessPosition::A as u32),
            local.instruction.op_a[0],
            &local.op_a_access,
            AB::Expr::one() - local.selectors.is_noop - local.selectors.reg_0_write,
        );

        // If we are performing a branch or a store, then the value of `a` is the previous value.
        builder
            .when(is_branch_instruction.clone() + self.is_store_instruction::<AB>(&local.selectors))
            .assert_word_eq(local.op_a_val(), local.op_a_access.prev_value);

        // For operations that require reading from memory (not registers), we need to read the
        // value into the memory columns.
        let memory_columns = local.opcode_specific_columns.memory();
        builder.constraint_memory_access(
            local.shard,
            local.clk + AB::F::from_canonical_u32(AccessPosition::Memory as u32),
            memory_columns.addr_aligned,
            &memory_columns.memory_access,
            is_memory_instruction.clone(),
        );

        // Check that reduce(addr_word) == addr_aligned + addr_offset.
        builder
            .when(is_memory_instruction.clone())
            .assert_eq::<AB::Expr, AB::Expr>(
                memory_columns.addr_aligned + memory_columns.addr_offset,
                memory_columns.addr_word.reduce::<AB>(),
            );

        // Check that each addr_word element is a byte.
        builder.slice_range_check_u8(&memory_columns.addr_word.0, is_memory_instruction.clone());

        // Send to the ALU table to verify correct calculation of addr_word.
        builder.send_alu(
            AB::Expr::from_canonical_u32(Opcode::ADD as u32),
            memory_columns.addr_word,
            local.op_b_val(),
            local.op_c_val(),
            is_memory_instruction.clone(),
        );

        // Memory handling.
        self.eval_memory_load::<AB>(builder, local);
        self.eval_memory_store::<AB>(builder, local);

        // Branch instructions.
        self.branch_ops_eval::<AB>(builder, is_branch_instruction.clone(), local, next);

        // Jump instructions.
        self.jump_ops_eval::<AB>(builder, local, next);

        // AUIPC instruction.
        self.auipc_eval(builder, local);

        // ALU instructions.
        builder.send_alu(
            local.instruction.opcode,
            local.op_a_val(),
            local.op_b_val(),
            local.op_c_val(),
            is_alu_instruction,
        );

        // ECALL instructions.
        // TODO:  Need to handle HALT ecall
        // For all non branch or jump instructions, verify that next.pc == pc + 4
        // builder
        //     .when_not(is_branch_instruction + local.selectors.is_jal + local.selectors.is_jalr)
        //     .assert_eq(local.pc + AB::Expr::from_canonical_u8(4), next.pc);

        // Range checks.
        builder.assert_bool(local.is_real);

        // Dummy constraint of degree 3.
        builder.assert_eq(
            local.pc * local.pc * local.pc,
            local.pc * local.pc * local.pc,
        );
    }
}

impl CpuChip {
    /// Whether the instruction is a memory instruction.
    pub(crate) fn is_alu_instruction<AB: SP1AirBuilder>(
        &self,
        opcode_selectors: &OpcodeSelectorCols<AB::Var>,
    ) -> AB::Expr {
        opcode_selectors.is_alu.into()
    }

    /// Constraints related to jump operations.
    pub(crate) fn jump_ops_eval<AB: SP1AirBuilder>(
        &self,
        builder: &mut AB,
        local: &CpuCols<AB::Var>,
        next: &CpuCols<AB::Var>,
    ) {
        // Get the jump specific columns
        let jump_columns = local.opcode_specific_columns.jump();

        // Verify that the local.pc + 4 is saved in op_a for both jump instructions.
        builder
            .when(local.selectors.is_jal + local.selectors.is_jalr)
            .assert_eq(
                local.op_a_val().reduce::<AB>(),
                local.pc + AB::F::from_canonical_u8(4),
            );

        // Verify that the word form of local.pc is correct for JAL instructions.
        builder
            .when(local.selectors.is_jal)
            .assert_eq(jump_columns.pc.reduce::<AB>(), local.pc);

        // Verify that the word form of next.pc is correct for both jump instructions.
        builder
            .when_transition()
            .when(local.selectors.is_jal + local.selectors.is_jalr)
            .assert_eq(jump_columns.next_pc.reduce::<AB>(), next.pc);

        // Verify that the new pc is calculated correctly for JAL instructions.
        builder.send_alu(
            AB::Expr::from_canonical_u32(Opcode::ADD as u32),
            jump_columns.next_pc,
            jump_columns.pc,
            local.op_b_val(),
            local.selectors.is_jal,
        );

        // Verify that the new pc is calculated correctly for JALR instructions.
        builder.send_alu(
            AB::Expr::from_canonical_u32(Opcode::ADD as u32),
            jump_columns.next_pc,
            local.op_b_val(),
            local.op_c_val(),
            local.selectors.is_jalr,
        );
    }

    /// Constraints related to the AUIPC opcode.
    pub(crate) fn auipc_eval<AB: SP1AirBuilder>(&self, builder: &mut AB, local: &CpuCols<AB::Var>) {
        // Get the auipc specific columns.
        let auipc_columns = local.opcode_specific_columns.auipc();

        // Verify that the word form of local.pc is correct.
        builder
            .when(local.selectors.is_auipc)
            .assert_eq(auipc_columns.pc.reduce::<AB>(), local.pc);

        // Verify that op_a == pc + op_b.
        builder.send_alu(
            AB::Expr::from_canonical_u32(Opcode::ADD as u32),
            local.op_a_val(),
            auipc_columns.pc,
            local.op_b_val(),
            local.selectors.is_auipc,
        );
    }
}

impl<F> BaseAir<F> for CpuChip {
    fn width(&self) -> usize {
        NUM_CPU_COLS
    }
}
