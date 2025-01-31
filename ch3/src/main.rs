#![no_std]
#![no_main]
#![feature(naked_functions, asm_sym, asm_const)]
#![deny(warnings)]

mod task;

#[macro_use]
extern crate console;

use console::log;
use impls::{Console, SyscallContext};
use riscv::register::*;
use sbi_rt::*;
use task::TaskControlBlock;

// 应用程序内联进来。
core::arch::global_asm!(include_str!(env!("APP_ASM")));

// 应用程序数量。
const APP_CAPACITY: usize = 32;

/// Supervisor 汇编入口。
///
/// 设置栈并跳转到 Rust。
#[naked]
#[no_mangle]
#[link_section = ".text.entry"]
unsafe extern "C" fn _start() -> ! {
    const STACK_SIZE: usize = (APP_CAPACITY + 2) * 4096;

    #[link_section = ".bss.uninit"]
    static mut STACK: [u8; STACK_SIZE] = [0u8; STACK_SIZE];

    core::arch::asm!(
        "la sp, {stack} + {stack_size}",
        "j  {main}",
        stack_size = const STACK_SIZE,
        stack      =   sym STACK,
        main       =   sym rust_main,
        options(noreturn),
    )
}

extern "C" fn rust_main() -> ! {
    // bss 段清零
    utils::zero_bss();
    // 初始化 `console`
    console::init_console(&Console);
    console::set_log_level(option_env!("LOG"));
    console::test_log();
    // 初始化 syscall
    syscall::init_io(&SyscallContext);
    syscall::init_process(&SyscallContext);
    syscall::init_scheduling(&SyscallContext);
    syscall::init_clock(&SyscallContext);
    // 确定应用程序位置
    extern "C" {
        static apps: utils::AppMeta;
    }
    // 任务控制块
    let mut tcbs = [TaskControlBlock::ZERO; APP_CAPACITY];
    let mut index_mod = 0;
    // 初始化
    for (i, entry) in unsafe { apps.iter_static() }.enumerate() {
        log::info!("load app{i} to {entry:#x}");
        tcbs[i].init(entry);
        index_mod += 1;
    }
    println!();
    // 打开中断
    unsafe { sie::set_stimer() };
    // 多道执行
    let mut remain = index_mod;
    let mut i = 0usize;
    while remain > 0 {
        let tcb = &mut tcbs[i];
        if !tcb.finish {
            loop {
                #[cfg(not(feature = "coop"))]
                sbi_rt::set_timer(time::read64() + 12500);
                unsafe { tcb.execute() };

                use scause::*;
                let finish = match scause::read().cause() {
                    Trap::Interrupt(Interrupt::SupervisorTimer) => {
                        sbi_rt::set_timer(u64::MAX);
                        log::trace!("app{i} timeout");
                        false
                    }
                    Trap::Exception(Exception::UserEnvCall) => {
                        use task::SchedulingEvent as Event;
                        match tcb.handle_syscall() {
                            Event::None => continue,
                            Event::Exit(code) => {
                                log::info!("app{i} exit with code {code}");
                                true
                            }
                            Event::Yield => {
                                log::debug!("app{i} yield");
                                false
                            }
                            Event::UnsupportedSyscall(id) => {
                                log::error!("app{i} call an unsupported syscall {}", id.0);
                                true
                            }
                        }
                    }
                    Trap::Exception(e) => {
                        log::error!("app{i} was killed by {e:?}");
                        true
                    }
                    Trap::Interrupt(ir) => {
                        log::error!("app{i} was killed by an unexpected interrupt {ir:?}");
                        true
                    }
                };
                if finish {
                    tcb.finish = true;
                    remain -= 1;
                }
                break;
            }
        }
        i = (i + 1) % index_mod;
    }
    system_reset(RESET_TYPE_SHUTDOWN, RESET_REASON_NO_REASON);
    unreachable!()
}

/// Rust 异常处理函数，以异常方式关机。
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    println!("{info}");
    system_reset(RESET_TYPE_SHUTDOWN, RESET_REASON_SYSTEM_FAILURE);
    unreachable!()
}

/// 各种接口库的实现
mod impls {
    use syscall::*;

    pub struct Console;

    impl console::Console for Console {
        #[inline]
        fn put_char(&self, c: u8) {
            #[allow(deprecated)]
            sbi_rt::legacy::console_putchar(c as _);
        }
    }

    pub struct SyscallContext;

    impl IO for SyscallContext {
        #[inline]
        fn write(&self, _caller: syscall::Caller, fd: usize, buf: usize, count: usize) -> isize {
            use console::log::*;

            if fd == 0 {
                print!("{}", unsafe {
                    core::str::from_utf8_unchecked(core::slice::from_raw_parts(
                        buf as *const u8,
                        count,
                    ))
                });
                count as _
            } else {
                error!("unsupported fd: {fd}");
                -1
            }
        }
    }

    impl Process for SyscallContext {
        #[inline]
        fn exit(&self, _caller: syscall::Caller, _status: usize) -> isize {
            0
        }
    }

    impl Scheduling for SyscallContext {
        #[inline]
        fn sched_yield(&self, _caller: syscall::Caller) -> isize {
            0
        }
    }

    impl Clock for SyscallContext {
        #[inline]
        fn clock_gettime(&self, _caller: syscall::Caller, clock_id: ClockId, tp: usize) -> isize {
            match clock_id {
                ClockId::CLOCK_MONOTONIC => {
                    let time = riscv::register::time::read() * 10000 / 125;
                    *unsafe { &mut *(tp as *mut TimeSpec) } = TimeSpec {
                        tv_sec: time / 1_000_000_000,
                        tv_nsec: time % 1_000_000_000,
                    };
                    0
                }
                _ => -1,
            }
        }
    }
}
