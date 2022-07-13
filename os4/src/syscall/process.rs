//! Process management syscalls

use crate::config::{MAX_SYSCALL_NUM, PAGE_SIZE, MEMORY_END};
use crate::mm::{VirtAddr, KERNEL_SPACE, MapArea, MapType, MapPermission, VirtPageNum};
use crate::task::{exit_current_and_run_next, suspend_current_and_run_next, TaskStatus, TASK_MANAGER, translate_vpn, set_task_info, mmap, contains_key, m_unmap};
use crate::timer::get_time_us;

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

#[derive(Clone, Copy)]
pub struct TaskInfo {
    pub status: TaskStatus,
    pub syscall_times: [u32; MAX_SYSCALL_NUM],
    pub time: usize,
}

pub fn sys_exit(exit_code: i32) -> ! {
    info!("[kernel] Application exited with code {}", exit_code);
    exit_current_and_run_next();
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    suspend_current_and_run_next();
    0
}

// YOUR JOB: 引入虚地址后重写 sys_get_time
pub fn sys_get_time(_ts: *mut TimeVal, _tz: usize) -> isize {
    let _us = get_time_us();
    let vaddr = _ts as usize;
    let vaddr_obj = VirtAddr(vaddr);
    let page_off = vaddr_obj.page_offset();

    let vpn = vaddr_obj.floor();
    
    let ppn = translate_vpn(vpn);

    let paddr : usize = ppn.0 << 12 | page_off;
    let ts  = paddr as *mut TimeVal;
    unsafe {
        *ts = TimeVal {
            sec: _us / 1_000_000,
            usec: _us % 1_000_000,
        };
    }
    0
}

// CLUE: 从 ch4 开始不再对调度算法进行测试~
pub fn sys_set_priority(_prio: isize) -> isize {
    -1
}

// YOUR JOB: 扩展内核以实现 sys_mmap 和 sys_munmap
pub fn sys_mmap(_start: usize, _len: usize, _port: usize) -> isize{
    if _port & !0x7 != 0 || _port & 0x7 == 0 || _len <= 0 || _start % PAGE_SIZE !=0{
        return -1;
    }
    let _end = _start + _len;

    let start_vpn: VirtPageNum = VirtAddr(_start).floor();
    let end_vpn: VirtPageNum = VirtAddr(_end).ceil();
    
    let mut res = false;
    for i in start_vpn.0..end_vpn.0{
        res = res | contains_key(&VirtPageNum(i));
    }
    if res{
        return -1;
    }

    let p = (_port << 1) | 16;
    let permission = MapPermission::from_bits(p as u8).unwrap();
    mmap(VirtAddr(_start) , VirtAddr(_end), permission);
    0
}

pub fn sys_munmap(_start: usize, _len: usize) -> isize {
    if _len <= 0 || _start % PAGE_SIZE !=0{
        return -1;
    }
    let _end = _start + _len;

    let start_vpn: VirtPageNum = VirtAddr(_start).floor();
    let end_vpn: VirtPageNum = VirtAddr(_end).ceil();
    let mut res = true;
    for i in start_vpn.0..end_vpn.0{
        res = res & contains_key(&VirtPageNum(i));
    }
    if !res{
        return -1;
    }
    m_unmap(start_vpn, end_vpn)
    

    
}

// YOUR JOB: 引入虚地址后重写 sys_task_info
pub fn sys_task_info(ti: *mut TaskInfo) -> isize {
    let vaddr = ti as usize;
    let vaddr_obj = VirtAddr(vaddr);
    let page_off = vaddr_obj.page_offset();

    let vpn = vaddr_obj.floor();
    
    let ppn = translate_vpn(vpn);

    let paddr : usize = ppn.0 << 12 | page_off;
    let ts  = paddr as *mut TaskInfo;
    set_task_info(ts);
    0
    
}
