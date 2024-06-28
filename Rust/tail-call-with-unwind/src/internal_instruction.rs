use std::{fmt, marker::PhantomData, mem, ptr::NonNull};

const STACK_DEPTH_LIMIT: usize = 500;

macro_rules! instructions {
    (
        $vis:vis enum $Opcode:ident {
            $(
                #[visit = $visit:ident]
                $Variant:ident {
                    $($field:ident: $field_ty:ty,)*
                },
            )*
        }
    ) => {
        #[derive(Copy, Clone, Eq, PartialEq, Debug)]
        #[repr(u8)]
        $vis enum $Opcode {
            $($Variant,)*
        }

        pub struct VisitArray<T>(PhantomData<T>);

        impl<T: Visitor> VisitArray<T> {
            $(unsafe fn $visit(this: T, pc: Pc<'_>) -> T::Ret<'_> {
                this.$visit(pc, unsafe { &*pc.0.as_ptr().add(1).cast::<$Variant>() })
            })*
            pub const VISIT_ARRAY: [for<'a> unsafe fn(this: T, pc: Pc<'a>) -> T::Ret<'a>; const { [$($Opcode::$Variant,)*].len() }] = {
                [
                    $(Self::$visit,)*
                ]
            };
        }

        $(
            #[derive(Copy, Clone, Debug)]
            #[repr(packed)]
            $vis struct $Variant {
                $($vis $field: $field_ty,)*
            }

            impl $Variant {
                $vis const OPCODE: $Opcode = $Opcode::$Variant;
                $vis fn encode(self, bytes: &mut Vec<u8>) {
                    bytes.push(Self::OPCODE as u8);
                    let encoded_bytes: [u8; mem::size_of::<$Variant>()] = unsafe { mem::transmute(self) };
                    bytes.extend_from_slice(&encoded_bytes);
                }
            }
        )*

        $vis trait Visitor {
            type Ret<'a>: 'a;
            $(fn $visit<'a>(self, pc: Pc<'a>, fields: &'a $Variant) -> Self::Ret<'a>;)*
        }

        /// Safety: must not be used with a `Pc` that's pointing to the last instruction and `Visitor::visit_*` must only be called on the correct opcode
        struct NextPcUnchecked(());

        impl Visitor for NextPcUnchecked {
            type Ret<'a> = Pc<'a>;

            $(fn $visit<'a>(self, mut pc: Pc<'a>, _fields: &'a $Variant) -> Self::Ret<'a> {
                unsafe {
                    pc.0 = NonNull::new_unchecked(pc.0.as_ptr().add(1 + mem::size_of::<$Variant>()));
                }
                pc
            })*
        }

        /// Safety: must only be used with a `Pc` that's pointing to the same `InternalProgram` and `Visitor::visit_*` must only be called on the correct opcode
        struct NextPcChecked {
            one_past_end: *const u8,
        }

        impl Visitor for NextPcChecked {
            type Ret<'a> = Option<Pc<'a>>;

            $(fn $visit<'a>(self, mut pc: Pc<'a>, _fields: &'a $Variant) -> Self::Ret<'a> {
                unsafe {
                    let ptr = NonNull::new_unchecked(pc.0.as_ptr().add(1 + mem::size_of::<$Variant>()));
                    if ptr.as_ptr().cast_const() == self.one_past_end {
                        None
                    } else {
                        pc.0 = ptr;
                        Some(pc)
                    }
                }
            })*
        }

        pub struct PrintVisitor<'a, 'b>(pub &'a mut fmt::Formatter<'b>);

        impl Visitor for PrintVisitor<'_, '_> {
            type Ret<'a> = fmt::Result;
            $(fn $visit<'a>(self, _pc: Pc<'a>, fields: &'a $Variant) -> Self::Ret<'a> {
                write!(self.0, "{fields:?}")
            })*
        }

        /// Safety: `Visitor::visit_*` must only be called on the correct opcode
        struct RunUncheckedVisitor<'state, 'prog>(RunArgs<'state, 'prog>);

        impl<'state, 'prog> Visitor for RunUncheckedVisitor<'state, 'prog> {
            type Ret<'a> = RunRet<'a>;

            $(
                #[inline(always)]
                fn $visit<'a>(self, pc: Pc<'a>, fields: &'a $Variant) -> Self::Ret<'a> {
                    let Self(RunArgs {
                        memory,
                        state,
                    }) = self;
                    let pc = match RunOne(RunArgs { memory, state }).$visit(pc, fields) {
                        RunOneRet::RunRet(retval) => return retval,
                        RunOneRet::Branch(pc) => pc,
                        RunOneRet::Continue => NextPcUnchecked(()).$visit(pc, fields),
                    };
                    let retval = pc.visit(Self(RunArgs { memory, state }));
                    #[cfg(feature = "force_use_stack")]
                    std::hint::black_box(());
                    retval
                }
            )*
        }
    };
}

instructions! {
    pub enum Opcode {
        #[visit = visit_load]
        OpLoad {
            result: u8,
            imm: u32,
        },
        #[visit = visit_add]
        OpAdd {
            result: u8,
            inputs: [u8; 2],
        },
        #[visit = visit_jmp_ne]
        OpJmpNE {
            inputs: [u8; 2],
            target: i16,
        },
        #[visit = visit_print]
        OpPrint {
            input: u8,
        },
        #[visit = visit_ret]
        OpRet {},
        #[visit = visit_unwind_if_needed]
        OpUnwindIfNeeded {
            depth: u16,
        },
    }
}

#[derive(Copy, Clone)]
#[repr(transparent)]
pub struct Pc<'a>(NonNull<u8>, PhantomData<&'a [u8]>);

impl<'a> Pc<'a> {
    #[inline(always)]
    pub fn opcode(self) -> Opcode {
        unsafe { self.0.as_ptr().cast::<Opcode>().read() }
    }
    #[inline(always)]
    pub fn as_ptr(self) -> *const u8 {
        self.0.as_ptr().cast_const()
    }
    #[inline(always)]
    pub fn visit<T: Visitor>(self, visitor: T) -> T::Ret<'a> {
        unsafe {
            const { &VisitArray::<T>::VISIT_ARRAY }.get_unchecked(self.opcode() as u8 as usize)(
                visitor, self,
            )
        }
    }
    /// Safety: must not be used with a `Pc` that's pointing to the last instruction
    #[inline(always)]
    pub unsafe fn next_pc_unchecked(self) -> Pc<'a> {
        self.visit(NextPcUnchecked(()))
    }
    /// Safety: must only be used with a `Pc` that's pointing to the same `InternalProgram`
    #[inline(always)]
    pub unsafe fn next_pc_checked(self, internal_program: &'a InternalProgram) -> Option<Pc<'a>> {
        self.visit(NextPcChecked {
            one_past_end: internal_program.0.as_ptr_range().end,
        })
    }
}

pub struct InternalProgram(Box<[u8]>);

impl fmt::Display for InternalProgram {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        unsafe {
            let mut pc = self.start();
            loop {
                print!("{}: ", self.display_pc(pc));
                pc.visit(PrintVisitor(f))?;
                let Some(next_pc) = pc.next_pc_checked(self) else {
                    break;
                };
                pc = next_pc;
                writeln!(f)?;
            }
        }
        Ok(())
    }
}

#[derive(Copy, Clone, Debug)]
pub struct DisplayPc(usize);

impl fmt::Display for DisplayPc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:#04X}", self.0)
    }
}

impl InternalProgram {
    pub unsafe fn new_unchecked(bytes: Box<[u8]>) -> Self {
        Self(bytes)
    }
    pub fn start(&self) -> Pc<'_> {
        unsafe {
            Pc(
                NonNull::new_unchecked(self.0.as_ptr().cast_mut()),
                PhantomData,
            )
        }
    }
    // pc doesn't have to be from this program for safety, it'll just print garbage
    pub fn display_pc(&self, pc: Pc<'_>) -> DisplayPc {
        DisplayPc((pc.0.as_ptr() as usize).wrapping_sub(self.start().0.as_ptr() as usize))
    }
}

pub struct RunState<'prog> {
    pub current_stack_depth: usize,
    pub internal_program: &'prog InternalProgram,
}

pub struct RunArgs<'state, 'prog> {
    pub memory: &'state mut [u32; 256],
    pub state: &'state mut RunState<'prog>,
}

pub enum RunRet<'a> {
    UnwindAndContinue(Pc<'a>),
    Done,
}

pub enum RunOneRet<'a> {
    RunRet(RunRet<'a>),
    Branch(Pc<'a>),
    Continue,
}

struct RunOne<'state, 'prog>(pub RunArgs<'state, 'prog>);

impl<'state, 'prog> Visitor for RunOne<'state, 'prog> {
    type Ret<'a> = RunOneRet<'a>;

    #[inline(always)]
    fn visit_load<'a>(self, pc: Pc<'a>, fields: &'a OpLoad) -> Self::Ret<'a> {
        let memory = self.0.memory;
        memory[fields.result as usize] = fields.imm;
        #[cfg(debug_assertions)]
        {
            println!(
                "{}: memory[{}] = {}; memory[{}]:{}",
                self.0.state.internal_program.display_pc(pc),
                { fields.result },
                { fields.imm },
                { fields.result },
                memory[fields.result as usize],
            );
        }
        RunOneRet::Continue
    }

    #[inline(always)]
    fn visit_add<'a>(self, pc: Pc<'a>, fields: &'a OpAdd) -> Self::Ret<'a> {
        let memory = self.0.memory;
        #[cfg(debug_assertions)]
        {
            print!(
                "{}: memory[{}]:{} = memory[{}]:{} + memory[{}]:{}",
                self.0.state.internal_program.display_pc(pc),
                fields.result,
                memory[fields.result as usize],
                fields.inputs[0],
                memory[fields.inputs[0] as usize],
                fields.inputs[1],
                memory[fields.inputs[1] as usize],
            );
        }
        memory[fields.result as usize] =
            memory[fields.inputs[0] as usize] + memory[fields.inputs[1] as usize];
        #[cfg(debug_assertions)]
        {
            println!(
                "; memory[{}]:{}",
                fields.result, memory[fields.result as usize],
            );
        }
        RunOneRet::Continue
    }

    #[inline(always)]
    fn visit_jmp_ne<'a>(self, pc: Pc<'a>, fields: &'a OpJmpNE) -> Self::Ret<'a> {
        let memory = self.0.memory;
        let target_pc = unsafe {
            Pc(
                NonNull::new_unchecked(pc.0.as_ptr().offset(fields.target as isize)),
                PhantomData,
            )
        };
        #[cfg(debug_assertions)]
        {
            print!(
                "{}: if memory[{}]:{} != memory[{}]:{} pc = {}",
                self.0.state.internal_program.display_pc(pc),
                fields.inputs[0],
                memory[fields.inputs[0] as usize],
                fields.inputs[1],
                memory[fields.inputs[1] as usize],
                self.0.state.internal_program.display_pc(target_pc),
            );
        }
        if memory[fields.inputs[0] as usize] != memory[fields.inputs[1] as usize] {
            #[cfg(debug_assertions)]
            {
                println!("; branched");
            }
            RunOneRet::Branch(target_pc)
        } else {
            #[cfg(debug_assertions)]
            {
                println!("; didn't branch");
            }
            RunOneRet::Continue
        }
    }

    #[inline(always)]
    fn visit_print<'a>(self, pc: Pc<'a>, fields: &'a OpPrint) -> Self::Ret<'a> {
        let memory = self.0.memory;
        #[cfg(debug_assertions)]
        {
            println!(
                "{}: print memory[{}]",
                self.0.state.internal_program.display_pc(pc),
                fields.input,
            );
        }
        println!("{}", memory[fields.input as usize]);
        RunOneRet::Continue
    }

    #[inline(always)]
    fn visit_ret<'a>(self, pc: Pc<'a>, _fields: &'a OpRet) -> Self::Ret<'a> {
        #[cfg(debug_assertions)]
        {
            println!("{}: ret", self.0.state.internal_program.display_pc(pc),);
        }
        RunOneRet::RunRet(RunRet::Done)
    }

    #[inline(always)]
    fn visit_unwind_if_needed<'a>(self, pc: Pc<'a>, fields: &'a OpUnwindIfNeeded) -> Self::Ret<'a> {
        #[cfg(debug_assertions)]
        {
            println!(
                "{}: unwind_if_needed",
                self.0.state.internal_program.display_pc(pc),
            );
        }
        #[inline(never)]
        #[cold]
        unsafe fn do_unwind<'a>(insn_pc: Pc<'a>) -> RunRet<'a> {
            unsafe { RunRet::UnwindAndContinue(insn_pc.next_pc_unchecked()) }
        }
        self.0.state.current_stack_depth += usize::from(fields.depth);
        if self.0.state.current_stack_depth > STACK_DEPTH_LIMIT {
            unsafe { RunOneRet::RunRet(do_unwind(pc)) }
        } else {
            RunOneRet::Continue
        }
    }
}

pub fn print_internal_program(prog: &InternalProgram) {
    println!("{prog}");
}

pub fn vm_loop(prog: &InternalProgram) {
    let mut memory = [0; 256];
    let mut pc = prog.start();
    loop {
        match pc.visit(RunUncheckedVisitor(RunArgs {
            memory: &mut memory,
            state: &mut RunState {
                current_stack_depth: 0,
                internal_program: prog,
            },
        })) {
            RunRet::UnwindAndContinue(continue_pc) => pc = continue_pc,
            RunRet::Done => break,
        }
    }
}
