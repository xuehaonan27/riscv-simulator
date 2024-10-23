//! Fetch phase.
//! Actually, to solve `load-use` hazard by data forwarding, we need to
//! do `decode` here to get `rs1`, `rs2` (maybe `rs3`) and `rd` immediately
//! and leave Decode phase just register file reading.

use log::{error, info, trace};

use crate::{
    core::{insts::*, reg::ProgramCounter, vm::VirtualMemory},
    error::{Error, Result},
    multi_stage::debug::f_pinst,
};

use super::{
    ctrl_flags::{BranchFlags, DecodeFlags, ExecFlags, MemFlags, SextType, WbFlags},
    phases::InternalFetchDecode,
};

/// Fetch instruction
pub fn fetch(pc: &ProgramCounter, vm: &VirtualMemory) -> Result<(InternalFetchDecode, bool)> {
    let pc = pc.read();
    let inst = vm.fetch_inst(pc as usize);
    inst_interpret(pc, inst).map(|x| {
        trace!("IF : {}", f_pinst(&x));
        (x, x.exec_flags.alu_op != Inst64::ebreak)
    })
}

/// Decode phase.
/// ```
/// R:  OP_IMM_32  AMO  OP  OP_32  OP_FP
/// R4: MADD  MSUB  NMSUB  NMADD
/// I:  LOAD  LOAD_FP  MISC_MEM  OP_IMM  JALR  SYSTEM
/// U:  AUIPC  LUI
/// UJ: JAL
/// S:  STORE STORE_FP
/// SB: BRANCH
/// ```
fn inst_interpret(pc: u64, inst: u32) -> Result<InternalFetchDecode> {
    use crate::core::insts::inst_64_opcode::*;
    // Format
    let opcode = opcode(inst);

    let mut itl_f_d = match opcode {
        LOAD => decode_load(inst),
        LOAD_FP => decode_load_fp(inst),
        MISC_MEM => decode_misc_mem(inst),
        OP_IMM => decode_op_imm(inst),
        AUIPC => decode_op_auipc(inst),
        OP_IMM_32 => decode_op_imm_32(inst),
        STORE => decode_store(inst),
        STORE_FP => decode_store_fp(inst),
        AMO => decode_amo(inst),
        OP => decode_op(inst),
        LUI => decode_lui(inst),
        OP_32 => decode_op_32(inst),
        MADD => decode_madd(inst),
        MSUB => decode_msub(inst),
        NMSUB => decode_nmsub(inst),
        NMADD => decode_nmadd(inst),
        OP_FP => decode_op_fp(inst),
        BRANCH => decode_branch(inst),
        JALR => decode_jalr(inst),
        JAL => decode_jal(inst),
        SYSTEM => decode_system(inst),
        _ => todo!(),
    };

    if let Ok(ref mut itl_f_d) = itl_f_d {
        itl_f_d.pc = pc;
    } else {
        error!("ERROR DECODING: {:#x}", inst);
    }

    itl_f_d
}

/// 0000011 LOAD: I type
fn decode_load(inst: u32) -> Result<InternalFetchDecode> {
    let funct3 = funct3(inst);
    let alu_op = match funct3 {
        0b000 => Inst64::lb,
        0b001 => Inst64::lh,
        0b010 => Inst64::lw,
        0b011 => Inst64::ld,
        0b100 => Inst64::lbu,
        0b101 => Inst64::lhu,
        0b110 => Inst64::lwu,
        _ => {
            let msg = format!("Unknown LOAD instruction funct3={funct3}");
            error!("{msg}");
            return Err(Error::Decode(msg));
        }
    };

    let rd = rd(inst);
    let rs1 = rs1(inst);
    let imm = imm_I(inst);

    let itl_f_d = InternalFetchDecode {
        decode_flags: DecodeFlags { sext: SextType::I },
        exec_flags: ExecFlags {
            alu_op,
            alu_src: true, // using imm
        },
        mem_flags: MemFlags {
            mem_read: true,
            mem_write: false,
        },
        branch_flags: BranchFlags {
            branch: false,
            pc_src: false, // not set until exec phase
        },
        wb_flags: WbFlags { mem_to_reg: true },
        pc: 0,
        rs1,
        rs2: 0,
        rs3: 0,
        rd,
        imm,
    };

    Ok(itl_f_d)
}

/// 0000111 LOAD_FP: I type
#[allow(unused)]
fn decode_load_fp(_inst: u32) -> Result<InternalFetchDecode> {
    todo!()
}

/// 0001111 MISC_MEM: I type
#[allow(unused)]
fn decode_misc_mem(_inst: u32) -> Result<InternalFetchDecode> {
    todo!()
}

/// 0010011 OP_IMM: I type
fn decode_op_imm(inst: u32) -> Result<InternalFetchDecode> {
    let funct3 = funct3(inst);
    let funct7 = funct7(inst);
    let funct6 = funct6(inst);
    let alu_op = match funct3 {
        0b000 => Inst64::addi,
        0b010 => Inst64::slti,
        0b011 => Inst64::sltiu,
        0b100 => Inst64::xori,
        0b110 => Inst64::ori,
        0b111 => Inst64::andi,

        /* // RV32
        0b001 => Inst64::slli,
        0b101 => match funct7 {
            0b0000000 => Inst64::srli,
            0b0100000 => Inst64::srai,
            _ => {
                let msg = format!("Unknown OP_IMM instruction NOT srli or srai funct7={funct7}");
                error!("{msg}");
                return Err(Error::Decode(msg));
            }
        },
        */
        // RV64
        0b001 => Inst64::slli,
        0b101 => match funct6 {
            0b000000 => Inst64::srli,
            0b010000 => Inst64::srai,
            _ => {
                let msg = format!("Unknown OP_IMM instruction NOT srli or srai funct7={funct7}");
                error!("{msg}");
                return Err(Error::Decode(msg));
            }
        },
        _ => {
            let msg = format!("Unknown OP_IMM instruction funct3={funct3}");
            error!("{msg}");
            return Err(Error::Decode(msg));
        }
    };

    let rd = rd(inst);
    let rs1 = rs1(inst);
    let imm = match funct3 {
        0b000 | 0b010 | 0b011 | 0b100 | 0b110 | 0b111 => imm_I(inst),
        // 0b001 | 0b101 => rs2.into(), // RV32
        0b001 | 0b101 => shift64_I(inst), // RV64
        _ => unreachable!("Should return error before control flow reaches here"),
    };

    let itl_f_d = InternalFetchDecode {
        decode_flags: DecodeFlags { sext: SextType::I },
        exec_flags: ExecFlags {
            alu_op,
            alu_src: true,
        },
        mem_flags: MemFlags {
            mem_read: false,
            mem_write: false,
        },
        wb_flags: WbFlags { mem_to_reg: true },
        branch_flags: BranchFlags {
            branch: false,
            pc_src: false, // not set until exec phase
        },
        pc: 0,
        rs1,
        rs2: 0,
        rs3: 0,
        rd,
        imm,
    };

    Ok(itl_f_d)
}

/// 0010111 AUIPC: U type
fn decode_op_auipc(inst: u32) -> Result<InternalFetchDecode> {
    let alu_op = Inst64::auipc;
    let rd = rd(inst);
    let imm = imm_U(inst);

    let itl_f_d = InternalFetchDecode {
        decode_flags: DecodeFlags { sext: SextType::U },
        exec_flags: ExecFlags {
            alu_op,
            alu_src: false,
        },
        mem_flags: MemFlags {
            mem_read: false,
            mem_write: false,
        },
        wb_flags: WbFlags { mem_to_reg: true },
        branch_flags: BranchFlags {
            branch: false,
            pc_src: false,
        },
        pc: 0,
        rs1: 0,
        rs2: 0,
        rs3: 0,
        rd,
        imm,
    };

    Ok(itl_f_d)
}

/// 0011011 OP_IMM_32: R type
fn decode_op_imm_32(inst: u32) -> Result<InternalFetchDecode> {
    let funct3 = funct3(inst);
    let funct7 = funct7(inst);
    let rs2 = rs2(inst);
    let alu_op = match funct3 {
        0b000 => Inst64::addiw,
        0b001 => Inst64::slliw,
        0b101 => match funct7 {
            0b0000000 => Inst64::srliw,
            0b0100000 => Inst64::sraiw,
            _ => {
                let msg = format!("Unknown OP_IMM_32 instruction funct7={funct7}");
                error!("{msg}");
                return Err(Error::Decode(msg));
            }
        },
        _ => {
            let msg = format!("Unknown OP_IMM_32 instruction funct3={funct3}");
            error!("{msg}");
            return Err(Error::Decode(msg));
        }
    };

    let imm = match funct3 {
        0b000 => imm_I(inst),
        0b001 | 0b101 => rs2.into(),
        _ => unreachable!("Should return error before control flow reaches here"),
    };

    let rd = rd(inst);
    let rs1 = rs1(inst);

    let itl_f_d = InternalFetchDecode {
        decode_flags: DecodeFlags { sext: SextType::I },
        exec_flags: ExecFlags {
            alu_op,
            alu_src: true,
        },
        mem_flags: MemFlags {
            mem_read: false,
            mem_write: false,
        },
        wb_flags: WbFlags { mem_to_reg: true },
        branch_flags: BranchFlags {
            branch: false,
            pc_src: false, // not set until exec phase
        },
        pc: 0,
        rs1,
        rs2: 0,
        rs3: 0,
        rd,
        imm,
    };

    Ok(itl_f_d)
}

/// 0100011 STORE: S type
fn decode_store(inst: u32) -> Result<InternalFetchDecode> {
    let funct3 = funct3(inst);
    let alu_op = match funct3 {
        0b000 => Inst64::sb,
        0b001 => Inst64::sh,
        0b010 => Inst64::sw,
        0b011 => Inst64::sd,
        _ => {
            let msg = format!("Unknown STORE instruction funct3={funct3}");
            error!("{msg}");
            return Err(Error::Decode(msg));
        }
    };

    let rs1 = rs1(inst);
    let rs2 = rs2(inst);
    let imm = imm_S(inst);

    let itl_f_d = InternalFetchDecode {
        decode_flags: DecodeFlags { sext: SextType::S },
        exec_flags: ExecFlags {
            alu_op,
            alu_src: true,
        },
        mem_flags: MemFlags {
            mem_read: false,
            mem_write: true,
        },
        wb_flags: WbFlags { mem_to_reg: false },
        branch_flags: BranchFlags {
            branch: false,
            pc_src: false, // not set until exec phase
        },
        pc: 0,
        rs1,
        rs2,
        rs3: 0,
        rd: 0,
        imm,
    };

    Ok(itl_f_d)
}

/// 0100111 STORE_FP: S type
#[allow(unused)]
fn decode_store_fp(_inst: u32) -> Result<InternalFetchDecode> {
    todo!()
}

/// 0101111 AMO: R type
#[allow(unused)]
fn decode_amo(_inst: u32) -> Result<InternalFetchDecode> {
    todo!()
}

/// 0110011 OP: R type
fn decode_op(inst: u32) -> Result<InternalFetchDecode> {
    let funct3 = funct3(inst);
    let funct7 = funct7(inst);
    let alu_op = match funct3 {
        0b000 => match funct7 {
            0b0000000 => Inst64::add,
            0b0100000 => Inst64::sub,
            0b0000001 => Inst64::mul,
            _ => {
                let msg = format!("Unknown OP instruction funct3={funct3} funct7={funct7}");
                error!("{msg}");
                return Err(Error::Decode(msg));
            }
        },
        0b001 => match funct7 {
            0b0000000 => Inst64::sll,
            0b0000001 => Inst64::mulh,
            _ => {
                let msg = format!("Unknown OP instruction funct3={funct3} funct7={funct7}");
                error!("{msg}");
                return Err(Error::Decode(msg));
            }
        },
        0b010 => match funct7 {
            0b0000000 => Inst64::slt,
            0b0000001 => Inst64::mulhsu,
            _ => {
                let msg = format!("Unknown OP instruction funct3={funct3} funct7={funct7}");
                error!("{msg}");
                return Err(Error::Decode(msg));
            }
        },
        0b011 => match funct7 {
            0b0000000 => Inst64::sltu,
            0b0000001 => Inst64::mulhu,
            _ => {
                let msg = format!("Unknown OP instruction funct3={funct3} funct7={funct7}");
                error!("{msg}");
                return Err(Error::Decode(msg));
            }
        },
        0b100 => match funct7 {
            0b0000000 => Inst64::xor,
            0b0000001 => Inst64::div,
            _ => {
                let msg = format!("Unknown OP instruction funct3={funct3} funct7={funct7}");
                error!("{msg}");
                return Err(Error::Decode(msg));
            }
        },
        0b101 => match funct7 {
            0b0000000 => Inst64::srl,
            0b0100000 => Inst64::sra,
            0b0000001 => Inst64::divu,
            _ => {
                let msg = format!("Unknown OP instruction funct3={funct3} funct7={funct7}");
                error!("{msg}");
                return Err(Error::Decode(msg));
            }
        },
        0b110 => match funct7 {
            0b0000000 => Inst64::or,
            0b0000001 => Inst64::rem,
            _ => {
                let msg = format!("Unknown OP instruction funct3={funct3} funct7={funct7}");
                error!("{msg}");
                return Err(Error::Decode(msg));
            }
        },
        0b111 => match funct7 {
            0b0000000 => Inst64::and,
            0b0000001 => Inst64::remu,
            _ => {
                let msg = format!("Unknown OP instruction funct3={funct3} funct7={funct7}");
                error!("{msg}");
                return Err(Error::Decode(msg));
            }
        },
        _ => {
            let msg = format!("Unknown OP instruction funct3={funct3}");
            error!("{msg}");
            return Err(Error::Decode(msg));
        }
    };

    let rd = rd(inst);
    let rs1 = rs1(inst);
    let rs2 = rs2(inst);

    let itl_f_d = InternalFetchDecode {
        decode_flags: DecodeFlags {
            sext: SextType::None,
        },
        exec_flags: ExecFlags {
            alu_op,
            alu_src: false,
        },
        mem_flags: MemFlags {
            mem_read: false,
            mem_write: false,
        },
        wb_flags: WbFlags { mem_to_reg: true },
        branch_flags: BranchFlags {
            branch: false,
            pc_src: false, // not set until exec phase
        },
        pc: 0,
        rs1,
        rs2,
        rs3: 0,
        rd,
        imm: 0,
    };

    Ok(itl_f_d)
}

/// 0110111 LUI: U type
fn decode_lui(inst: u32) -> Result<InternalFetchDecode> {
    let alu_op = Inst64::lui;
    let rd = rd(inst);
    let imm = imm_U(inst);

    let itl_f_d = InternalFetchDecode {
        decode_flags: DecodeFlags { sext: SextType::U },
        exec_flags: ExecFlags {
            alu_op,
            alu_src: true,
        },
        mem_flags: MemFlags {
            mem_read: false,
            mem_write: false,
        },
        wb_flags: WbFlags { mem_to_reg: true },
        branch_flags: BranchFlags {
            branch: false,
            pc_src: false, // not set until exec phase
        },
        pc: 0,
        rs1: 0,
        rs2: 0,
        rs3: 0,
        rd,
        imm,
    };

    Ok(itl_f_d)
}

/// 0111011 OP_32: R type
fn decode_op_32(inst: u32) -> Result<InternalFetchDecode> {
    let funct3 = funct3(inst);
    let funct7 = funct7(inst);
    let alu_op = match funct3 {
        0b000 => match funct7 {
            0b0000000 => Inst64::addw,
            0b0100000 => Inst64::subw,
            0b0000001 => Inst64::mulw,
            _ => {
                let msg = format!("Unknown OP_32 instruction funct3={funct3} funct7={funct7}");
                error!("{msg}");
                return Err(Error::Decode(msg));
            }
        },
        0b001 => Inst64::sllw,
        0b100 => match funct7 {
            0b0000001 => Inst64::divw,
            _ => {
                let msg = format!("Unknown OP_32 instruction funct3={funct3} funct7={funct7}");
                error!("{msg}");
                return Err(Error::Decode(msg));
            }
        },
        0b101 => match funct7 {
            0b0000000 => Inst64::srlw,
            0b0100000 => Inst64::sraw,
            0b0000001 => Inst64::divuw,
            _ => {
                let msg = format!("Unknown OP_32 instruction funct3={funct3} funct7={funct7}");
                error!("{msg}");
                return Err(Error::Decode(msg));
            }
        },
        0b110 => match funct7 {
            0b0000001 => Inst64::remw,
            _ => {
                let msg = format!("Unknown OP_32 instruction funct3={funct3} funct7={funct7}");
                error!("{msg}");
                return Err(Error::Decode(msg));
            }
        },
        0b111 => match funct7 {
            0b0000001 => Inst64::remuw,
            _ => {
                let msg = format!("Unknown OP_32 instruction funct3={funct3} funct7={funct7}");
                error!("{msg}");
                return Err(Error::Decode(msg));
            }
        },
        _ => {
            let msg = format!("Unknown OP_32 instruction funct3={funct3}");
            error!("{msg}");
            return Err(Error::Decode(msg));
        }
    };

    let rd = rd(inst);
    let rs1 = rs1(inst);
    let rs2 = rs2(inst);

    let itl_f_d = InternalFetchDecode {
        decode_flags: DecodeFlags {
            sext: SextType::None,
        },
        exec_flags: ExecFlags {
            alu_op,
            alu_src: false,
        },
        mem_flags: MemFlags {
            mem_read: false,
            mem_write: false,
        },
        wb_flags: WbFlags { mem_to_reg: true },
        branch_flags: BranchFlags {
            branch: false,
            pc_src: false, // not set until exec phase
        },
        pc: 0,
        rs1,
        rs2,
        rs3: 0,
        rd,
        imm: 0,
    };

    Ok(itl_f_d)
}

/// 1000011 MADD: R4 type
#[allow(unused)]
fn decode_madd(_inst: u32) -> Result<InternalFetchDecode> {
    todo!()
}

/// 1000111 MSUB: R4 type
#[allow(unused)]
fn decode_msub(_inst: u32) -> Result<InternalFetchDecode> {
    todo!()
}

/// 1001011 NMSUB: R4 type
#[allow(unused)]
fn decode_nmsub(_inst: u32) -> Result<InternalFetchDecode> {
    todo!()
}

/// 1001111 NMADD: R4 type
#[allow(unused)]
fn decode_nmadd(_inst: u32) -> Result<InternalFetchDecode> {
    todo!()
}

/// 1010011 OP_FP: R type
#[allow(unused)]
fn decode_op_fp(_inst: u32) -> Result<InternalFetchDecode> {
    todo!()
}

/// 1100011 BRANCH: SB type
fn decode_branch(inst: u32) -> Result<InternalFetchDecode> {
    let funct3 = funct3(inst);
    let alu_op = match funct3 {
        0b000 => Inst64::beq,
        0b001 => Inst64::bne,

        0b100 => Inst64::blt,
        0b101 => Inst64::bge,
        0b110 => Inst64::bltu,
        0b111 => Inst64::bgeu,

        _ => {
            let msg = format!("Unknown BRANCH instruction funct3={funct3}");
            error!("{msg}");
            return Err(Error::Decode(msg));
        }
    };

    let rs1 = rs1(inst);
    let rs2 = rs2(inst);
    let imm = imm_SB(inst);

    let itl_f_d = InternalFetchDecode {
        decode_flags: DecodeFlags { sext: SextType::B },
        exec_flags: ExecFlags {
            alu_op,
            alu_src: false,
        },
        mem_flags: MemFlags {
            mem_read: false,
            mem_write: false,
        },
        wb_flags: WbFlags { mem_to_reg: false },
        branch_flags: BranchFlags {
            branch: true,
            pc_src: false, // not set until exec phase
        },
        pc: 0,
        rs1,
        rs2,
        rs3: 0,
        rd: 0,
        imm,
    };

    Ok(itl_f_d)
}

/// 1100111 JALR: I type
fn decode_jalr(inst: u32) -> Result<InternalFetchDecode> {
    let funct3 = funct3(inst);
    let alu_op = match funct3 {
        0b000 => Inst64::jalr,
        _ => {
            let msg = format!("Unknown JALR instruction funct3={funct3}");
            error!("{msg}");
            return Err(Error::Decode(msg));
        }
    };

    let rd = rd(inst);
    let rs1 = rs1(inst);
    let imm = imm_I(inst);

    let itl_f_d = InternalFetchDecode {
        decode_flags: DecodeFlags { sext: SextType::I },
        exec_flags: ExecFlags {
            alu_op,
            alu_src: true,
        },
        mem_flags: MemFlags {
            mem_read: false,
            mem_write: false,
        },
        wb_flags: WbFlags { mem_to_reg: true },
        branch_flags: BranchFlags {
            branch: true,
            pc_src: true, // always jump
        },
        pc: 0,
        rs1,
        rs2: 0,
        rs3: 0,
        rd,
        imm,
    };

    Ok(itl_f_d)
}

/// 1101111 JAL: UJ type
fn decode_jal(inst: u32) -> Result<InternalFetchDecode> {
    let alu_op = Inst64::jal;
    let rd = rd(inst);
    let imm = imm_UJ(inst);

    let itl_f_d = InternalFetchDecode {
        decode_flags: DecodeFlags { sext: SextType::J },
        exec_flags: ExecFlags {
            alu_op,
            alu_src: true,
        },
        mem_flags: MemFlags {
            mem_read: false,
            mem_write: false,
        },
        wb_flags: WbFlags { mem_to_reg: true },
        branch_flags: BranchFlags {
            branch: true,
            pc_src: true, // not set until exec phase
        },
        pc: 0,
        rs1: 0,
        rs2: 0,
        rs3: 0,
        rd,
        imm,
    };

    Ok(itl_f_d)
}

/// 1110011 SYSTEM: I type
fn decode_system(inst: u32) -> Result<InternalFetchDecode> {
    let funct3 = funct3(inst);
    let csr = imm_I(inst);
    let alu_op = match funct3 {
        0b000 => match csr {
            0 => Inst64::ecall,
            1 => Inst64::ebreak,
            _ => {
                let msg = format!("Unknown SYSTEM E- instruction csr={csr}");
                error!("{msg}");
                return Err(Error::Decode(msg));
            }
        },
        0b001 => Inst64::csrrw,
        0b010 => Inst64::csrrs,
        0b011 => Inst64::csrrc,
        0b101 => Inst64::csrrwi,
        0b110 => Inst64::csrrsi,
        0b111 => Inst64::csrrci,
        _ => {
            let msg = format!("Unknown SYSTEM instruction funct3={funct3}");
            error!("{msg}");
            return Err(Error::Decode(msg));
        }
    };

    let rd = rd(inst);
    let rs1 = rs1(inst); // zimm for csrrwi, csrrsi, csrrci

    let itl_f_d = InternalFetchDecode {
        decode_flags: DecodeFlags {
            sext: SextType::None,
        },
        exec_flags: ExecFlags {
            alu_op,
            alu_src: false,
        },
        mem_flags: MemFlags {
            mem_read: false,
            mem_write: false,
        },
        wb_flags: WbFlags { mem_to_reg: false },
        branch_flags: BranchFlags {
            branch: false,
            pc_src: false, // not set until exec phase
        },
        pc: 0,
        rs1,
        rs2: 0,
        rs3: 0,
        rd,
        imm: 0,
    };

    Ok(itl_f_d)
}