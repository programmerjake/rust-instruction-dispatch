use crate::{
    bytecode::{
        get_opcode, get_operand_a, get_operand_b, get_operand_c, get_operand_imm, get_operand_jmp,
        Bytecode, Opcode,
    },
    internal_instruction::{
        InternalProgram, OpAdd, OpJmpNE, OpLoad, OpPrint, OpRet, OpUnwindIfNeeded,
    },
};

pub fn convert(bytecode: &[Bytecode]) -> InternalProgram {
    let mut bytes = vec![];
    let mut byte_offsets = vec![];
    assert_eq!(
        get_opcode(*bytecode.last().unwrap()),
        Opcode::RET,
        "last instruction must be RET so we don't run past the end of the code"
    );
    for (index, &op) in bytecode.iter().enumerate() {
        byte_offsets.push(bytes.len());
        match get_opcode(op) {
            Opcode::LOAD => {
                OpLoad {
                    result: get_operand_a(op) as u8,
                    imm: get_operand_imm(op),
                }
                .encode(&mut bytes);
            }
            Opcode::ADD => {
                OpAdd {
                    result: get_operand_a(op) as u8,
                    inputs: [get_operand_b(op) as u8, get_operand_c(op) as u8],
                }
                .encode(&mut bytes);
            }
            Opcode::JMPNE => {
                let depth = index + 1; // an overestimation, but good enough
                OpUnwindIfNeeded {
                    depth: depth.try_into().unwrap(),
                }
                .encode(&mut bytes);
                let target = usize::try_from(get_operand_jmp(op)).unwrap();
                assert!(target < bytecode.len(), "can't branch out-of-bounds");
                if target > index {
                    todo!(
                        "implement forward branches -- they need to write \
                        the target byte offset after we know the target address, \
                        which we don't know yet"
                    );
                }
                let target = i128::try_from(byte_offsets[target]).unwrap()
                    - i128::try_from(bytes.len()).unwrap();
                let target = i16::try_from(target).expect("branch target is out of range");
                OpJmpNE {
                    inputs: [get_operand_a(op) as u8, get_operand_b(op) as u8],
                    target,
                }
                .encode(&mut bytes);
            }
            Opcode::PRINT => {
                OpPrint {
                    input: get_operand_a(op) as u8,
                }
                .encode(&mut bytes);
            }
            Opcode::RET => {
                OpRet {}.encode(&mut bytes);
            }
        }
    }
    unsafe { InternalProgram::new_unchecked(bytes.into_boxed_slice()) }
}
