use criterion::{criterion_group, criterion_main, Criterion};
use std::sync::OnceLock;

use mylib::{
    bytecode::{make_opcode, make_opcode_a_b_c, make_opcode_a_b_jmp, make_opcode_a_imm, Opcode},
    convert::convert,
    internal_instruction::{vm_loop, InternalProgram},
};

static INTERNAL_PROGRAM: OnceLock<InternalProgram> = OnceLock::new();

fn setup() {
    let program = [
        // Init
        make_opcode_a_imm(Opcode::LOAD, 0, 0),
        make_opcode_a_imm(Opcode::LOAD, 1, 1),
        make_opcode_a_imm(Opcode::LOAD, 2, 0xfffff),
        // Loop
        make_opcode_a_b_c(Opcode::ADD, 0, 0, 1),
        make_opcode_a_b_jmp(Opcode::JMPNE, 0, 2, 3),
        // Finish
        make_opcode(Opcode::RET),
    ];

    INTERNAL_PROGRAM.get_or_init(|| convert(&program));
}

fn direct_call_threading() {
    let internal_program = INTERNAL_PROGRAM.get().unwrap();
    vm_loop(internal_program);
}

fn criterion_benchmark(c: &mut Criterion) {
    setup();
    c.bench_function("direct call threading", |b| {
        b.iter(|| direct_call_threading())
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
