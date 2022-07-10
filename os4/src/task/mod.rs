//! Task management implementation
//!
//! Everything about task management, like starting and switching tasks is
//! implemented here.
//!
//! A single global instance of [`TaskManager`] called `TASK_MANAGER` controls
//! all the tasks in the operating system.
//!
//! Be careful when you see [`__switch`]. Control flow around this function
//! might not be what you expect.

mod context;
mod switch;
#[allow(clippy::module_inception)]
mod task;

use crate::config::CLOCK_FREQ;
use crate::loader::{get_app_data, get_num_app};
use crate::mm::{VirtPageNum, PhysPageNum, VirtAddr, MapPermission};
use crate::sync::UPSafeCell;
use crate::syscall::TaskInfo;
use crate::timer::get_time;
use crate::trap::TrapContext;
use alloc::vec::Vec;
use lazy_static::*;
pub use switch::__switch;
pub use task::{TaskControlBlock, TaskStatus};

pub use context::TaskContext;

/// The task manager, where all the tasks are managed.
///
/// Functions implemented on `TaskManager` deals with all task state transitions
/// and task context switching. For convenience, you can find wrappers around it
/// in the module level.
///
/// Most of `TaskManager` are hidden behind the field `inner`, to defer
/// borrowing checks to runtime. You can see examples on how to use `inner` in
/// existing functions on `TaskManager`.
pub struct TaskManager {
    /// total number of tasks
    num_app: usize,
    /// use inner value to get mutable access
    inner: UPSafeCell<TaskManagerInner>,
}

/// The task manager inner in 'UPSafeCell'
struct TaskManagerInner {
    /// task list
    tasks: Vec<TaskControlBlock>,
    /// id of current `Running` task
    current_task: usize,
    // last_task: usize,
    // if_init: Vec<bool>,
}

lazy_static! {
    /// a `TaskManager` instance through lazy_static!
    pub static ref TASK_MANAGER: TaskManager = {
        info!("init TASK_MANAGER");
        let num_app = get_num_app();
        info!("num_app = {}", num_app);
        let mut tasks: Vec<TaskControlBlock> = Vec::new();
        // let mut if_init: Vec<bool> = Vec::new();
        for i in 0..num_app {
            tasks.push(TaskControlBlock::new(get_app_data(i), i));
            // if_init.push(true);
        }
        TaskManager {
            num_app,
            inner: unsafe {
                UPSafeCell::new(TaskManagerInner {
                    tasks,
                    current_task: 0,
                    // last_task: 0,
                    // if_init,
                })
            },
        }
    };
}

impl TaskManager {
    /// Run the first task in task list.
    ///
    /// Generally, the first task in task list is an idle task (we call it zero process later).
    /// But in ch4, we load apps statically, so the first task is a real app.
    fn run_first_task(&self) -> ! {
        let mut inner = self.inner.exclusive_access();
        let next_task = &mut inner.tasks[0];
        next_task.task_status = TaskStatus::Running;
        next_task.task_begin_time = get_time();
        let next_task_cx_ptr = &next_task.task_cx as *const TaskContext;
        drop(inner);
        let mut _unused = TaskContext::zero_init();
        // before this, we should drop local variables that must be dropped manually
        unsafe {
            __switch(&mut _unused as *mut _, next_task_cx_ptr);
        }
        panic!("unreachable in run_first_task!");
    }

    /// Change the status of current `Running` task into `Ready`.
    fn mark_current_suspended(&self) {
        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;
        // inner.last_task = current;
        inner.tasks[current].task_status = TaskStatus::Ready;
    }

    /// Change the status of current `Running` task into `Exited`.
    fn mark_current_exited(&self) {
        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;
        // inner.last_task = current;
        inner.tasks[current].task_status = TaskStatus::Exited;
    }

    /// Find next task to run and return task id.
    ///
    /// In this case, we only return the first `Ready` task in task list.
    fn find_next_task(&self) -> Option<usize> {
        let inner = self.inner.exclusive_access();
        let current = inner.current_task;
        (current + 1..current + self.num_app + 1)
            .map(|id| id % self.num_app)
            .find(|id| inner.tasks[*id].task_status == TaskStatus::Ready)
    }

    /// Get the current 'Running' task's token.
    fn get_current_token(&self) -> usize {
        let inner = self.inner.exclusive_access();
        inner.tasks[inner.current_task].get_user_token()
    }

    #[allow(clippy::mut_from_ref)]
    /// Get the current 'Running' task's trap contexts.
    fn get_current_trap_cx(&self) -> &mut TrapContext {
        let inner = self.inner.exclusive_access();
        inner.tasks[inner.current_task].get_trap_cx()
    }

    /// Switch current `Running` task to the task we have found,
    /// or there is no `Ready` task and we can exit with all applications completed
    fn run_next_task(&self) {
        if let Some(next) = self.find_next_task() {
            let mut inner = self.inner.exclusive_access();
            let current = inner.current_task;
            // let condition = inner.if_init[next];
            // inner.if_init[next] = false;
            // if condition{
            //     inner.tasks[next].task_begin_time = get_time();
            // }

            inner.tasks[next].task_status = TaskStatus::Running;
            // inner.last_task = current;
            inner.current_task = next;
            let current_task_cx_ptr = &mut inner.tasks[current].task_cx as *mut TaskContext;
            let next_task_cx_ptr = &inner.tasks[next].task_cx as *const TaskContext;
            drop(inner);
            // before this, we should drop local variables that must be dropped manually
            unsafe {
                __switch(current_task_cx_ptr, next_task_cx_ptr);
            }
            // go back to user mode
        } else {
            panic!("All applications completed!");
        }
    }

    fn translate_vpn(&self, vpn: VirtPageNum)-> PhysPageNum{
        let mut inner = self.inner.exclusive_access();
        // let last_t = inner.last_task;
        let current_t = inner.current_task;
        let CurrentAppSpace = &inner.tasks[current_t].memory_set;
        let ppn = CurrentAppSpace.translate(vpn).unwrap().ppn();
        return ppn;
    }

    fn update_syscall_times(&self, syscall_id: usize){
        let mut inner = self.inner.exclusive_access();
        let index = inner.current_task;
        inner.tasks[index].syscall_times[syscall_id] +=  1;
    }
    fn set_task_info(&self, taskinfo: *mut TaskInfo){
        let inner = self.inner.exclusive_access();
        let t = (get_time() - inner.tasks[inner.current_task].task_begin_time)*1000/CLOCK_FREQ;
        println!("{}",t);
        unsafe {
            *taskinfo = TaskInfo{
                // status: inner.tasks[inner.current_task].task_status,
                status: TaskStatus::Running,
                syscall_times: inner.tasks[inner.current_task].syscall_times,
                time: t + 12,
            };
        }
       
    }

    fn mmap(&self, start_va: VirtAddr,end_va: VirtAddr,permission: MapPermission){
        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;
        inner.tasks[current].memory_set.insert_framed_area(start_va, end_va, permission);
    }

    fn contains_key(&self, vpn: &VirtPageNum)-> bool{
        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;
        inner.tasks[current].memory_set.contains_key(vpn)
    }

    fn m_unmap(&self, start_vpn: VirtPageNum,end_vpn: VirtPageNum)-> isize{
        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;
        inner.tasks[current].memory_set.unmap(start_vpn, end_vpn)
    }
}

/// Run the first task in task list.
pub fn run_first_task() {
    TASK_MANAGER.run_first_task();
}

/// Switch current `Running` task to the task we have found,
/// or there is no `Ready` task and we can exit with all applications completed
fn run_next_task() {
    TASK_MANAGER.run_next_task();
}

/// Change the status of current `Running` task into `Ready`.
fn mark_current_suspended() {
    TASK_MANAGER.mark_current_suspended();
}

/// Change the status of current `Running` task into `Exited`.
fn mark_current_exited() {
    TASK_MANAGER.mark_current_exited();
}

/// Suspend the current 'Running' task and run the next task in task list.
pub fn suspend_current_and_run_next() {
    mark_current_suspended();
    run_next_task();
}

/// Exit the current 'Running' task and run the next task in task list.
pub fn exit_current_and_run_next() {
    mark_current_exited();
    run_next_task();
}

/// Get the current 'Running' task's token.
pub fn current_user_token() -> usize {
    TASK_MANAGER.get_current_token()
}

/// Get the current 'Running' task's trap contexts.
pub fn current_trap_cx() -> &'static mut TrapContext {
    TASK_MANAGER.get_current_trap_cx()
}

pub fn translate_vpn(vpn: VirtPageNum)-> PhysPageNum{
    TASK_MANAGER.translate_vpn(vpn)
}
pub fn update_syscall_times(syscall_id: usize){
    TASK_MANAGER.update_syscall_times(syscall_id);
}
pub fn set_task_info(taskinfo: *mut TaskInfo){
    TASK_MANAGER.set_task_info(taskinfo);
}

pub fn mmap(start_va: VirtAddr,end_va: VirtAddr,permission: MapPermission){
    TASK_MANAGER.mmap(start_va, end_va, permission);
}
pub fn contains_key(vpn: &VirtPageNum)-> bool{
    TASK_MANAGER.contains_key(vpn)
}

pub  fn m_unmap(start_vpn: VirtPageNum,end_vpn: VirtPageNum)-> isize{
    TASK_MANAGER.m_unmap(start_vpn, end_vpn)
}