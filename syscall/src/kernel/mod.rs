﻿use crate::{ClockId, SyscallId};
use spin::Once;

/// 系统调用的发起者信息。
///
/// 没有办法（也没有必要？）调整发起者的描述，只好先用两个 `usize` 了。
/// 至少在一个类 Linux 的宏内核系统这是够用的。
pub struct Caller {
    /// 发起者拥有的资源集的标记，相当于进程号。
    pub entity: usize,
    /// 发起者的控制流的标记，相当于线程号。
    pub flow: usize,
}

pub trait Process: Sync {
    fn exit(&self, caller: Caller, status: usize) -> isize;
}

pub trait IO: Sync {
    fn write(&self, caller: Caller, fd: usize, buf: usize, count: usize) -> isize;
}

pub trait Scheduling: Sync {
    fn sched_yield(&self, caller: Caller) -> isize;
}

pub trait Clock: Sync {
    fn clock_gettime(&self, caller: Caller, clock_id: ClockId, tp: usize) -> isize;
}

static PROCESS: Container<dyn Process> = Container::new();
static IO: Container<dyn IO> = Container::new();
static SCHEDULING: Container<dyn Scheduling> = Container::new();
static CLOCK: Container<dyn Clock> = Container::new();

#[inline]
pub fn init_process(process: &'static dyn Process) {
    PROCESS.init(process);
}

#[inline]
pub fn init_io(io: &'static dyn IO) {
    IO.init(io);
}

#[inline]
pub fn init_scheduling(scheduling: &'static dyn Scheduling) {
    SCHEDULING.init(scheduling);
}

#[inline]
pub fn init_clock(clock: &'static dyn Clock) {
    CLOCK.init(clock);
}

pub enum SyscallResult {
    Done(isize),
    Unsupported(SyscallId),
}

pub fn handle(caller: Caller, id: SyscallId, args: [usize; 6]) -> SyscallResult {
    use SyscallId as Id;
    match id {
        Id::EXIT => PROCESS.call(id, |proc| proc.exit(caller, args[0])),
        Id::WRITE => IO.call(id, |io| io.write(caller, args[0], args[1], args[2])),
        Id::SCHED_YIELD => SCHEDULING.call(id, |sched| sched.sched_yield(caller)),
        Id::CLOCK_GETTIME => CLOCK.call(id, |clock| {
            clock.clock_gettime(caller, ClockId(args[0]), args[1])
        }),
        _ => SyscallResult::Unsupported(id),
    }
}

struct Container<T: 'static + ?Sized>(spin::Once<&'static T>);

impl<T: 'static + ?Sized> Container<T> {
    #[inline]
    const fn new() -> Self {
        Self(Once::new())
    }

    #[inline]
    fn init(&self, val: &'static T) {
        self.0.call_once(|| val);
    }

    #[inline]
    fn call(&self, id: SyscallId, f: impl FnOnce(&T) -> isize) -> SyscallResult {
        self.0
            .get()
            .map_or(SyscallResult::Unsupported(id), |clock| {
                SyscallResult::Done(f(clock))
            })
    }
}
