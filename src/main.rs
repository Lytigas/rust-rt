#![feature(alloc_system, allocator_api)]

extern crate alloc_system;

#[global_allocator]
static A: alloc_system::System = alloc_system::System;

extern crate libc;
use libc::*;

const PRE_ALLOCATION_SIZE: usize = (100 * 1024 * 1024) as usize; /* 100MB pagefault free buffer */
// const PRE_ALLOCATION_SIZE: usize = (4096) as usize; /* 4096 bytes for testing */
const MY_STACK_SIZE: usize = (100 * 1024); /* 100 kB is enough for now. */

fn setprio(prio: i32, sched: i32) {
    // Set realtime priority for this thread
    let param = sched_param {
        sched_priority: prio,
    };
    unsafe {
        if sched_setscheduler(0, sched, &param) < 0 {
            perror(b"sched_setscheduler\0".as_ptr() as *const c_char);
        }
    }
}

static mut last_majflt: c_long = 0;
static mut last_minflt: c_long = 0;

fn show_new_pagefault_count(
    logtext: *const c_char,
    allowed_maj: *const c_char,
    allowed_min: *const c_char,
) {
    let mut usage = rusage {
        ru_utime: timeval {
            tv_sec: 0,
            tv_usec: 0,
        },
        ru_stime: timeval {
            tv_sec: 0,
            tv_usec: 0,
        },
        ru_maxrss: 0,
        ru_ixrss: 0,
        ru_idrss: 0,
        ru_isrss: 0,
        ru_minflt: 0,
        ru_majflt: 0,
        ru_nswap: 0,
        ru_inblock: 0,
        ru_oublock: 0,
        ru_msgsnd: 0,
        ru_msgrcv: 0,
        ru_nsignals: 0,
        ru_nvcsw: 0,
        ru_nivcsw: 0,
    };
    unsafe {
        getrusage(RUSAGE_SELF, &mut usage as *mut rusage);
    }
    unsafe {
        printf(
            b"%-30.30s: Pagefaults, Major:%ld (Allowed %s), Minor:%ld (Allowed %s)\n\0".as_ptr()
                as *const c_char,
            logtext,
            usage.ru_majflt - last_majflt,
            allowed_maj,
            usage.ru_minflt - last_minflt,
            allowed_min,
        );
    }
    unsafe {
        last_majflt = usage.ru_majflt;
        last_minflt = usage.ru_minflt;
    }
}

fn prove_thread_stack_use_is_safe() {
    let mut buffer: [u8; MY_STACK_SIZE] = unsafe { ::std::mem::uninitialized() };

    /* Prove that this thread is behaving well */
    for i in (0..MY_STACK_SIZE).step_by(unsafe { sysconf(_SC_PAGESIZE) } as usize) {
        /* Each write to this buffer shall NOT generate a
        pagefault. */
        buffer[i] = i as u8;
    }

    show_new_pagefault_count(
        b"Caused by using thread stack\0".as_ptr() as *const c_char,
        b"0\0".as_ptr() as *const c_char,
        b"0\0".as_ptr() as *const c_char,
    );
}

fn thread_task() {
    let ts = timespec {
        tv_sec: 30,
        tv_nsec: 0,
    };

    unsafe {
        setprio(sched_get_priority_max(SCHED_RR), SCHED_RR);
    }
    println!("I am an RT-thread with a stack that does not generate page-faults during use, stacksize={}", MY_STACK_SIZE);

    //<do your RT-thing here>

    show_new_pagefault_count(
        b"Caused by creating thread\0".as_ptr() as *const c_char,
        b">=0\0".as_ptr() as *const c_char,
        b">=0\0".as_ptr() as *const c_char,
    );

    prove_thread_stack_use_is_safe();

    /* wait 30 seconds before thread terminates */
    unsafe {
        clock_nanosleep(CLOCK_REALTIME, 0, &ts, ::std::ptr::null_mut());
    }
}
// /*************************************************************/
// /* The thread to start */
// static void *my_rt_thread(void *args)
// {
// struct timespec ts;
// ts.tv_sec = 30;
// ts.tv_nsec = 0;

// setprio(sched_get_priority_max(SCHED_RR), SCHED_RR);

// printf("I am an RT-thread with a stack that does not generate " \
//         "page-faults during use, stacksize=%i\n", MY_STACK_SIZE);

// //<do your RT-thing here>

// show_new_pagefault_count("Caused by creating thread", ">=0", ">=0");

// prove_thread_stack_use_is_safe(MY_STACK_SIZE);

// /* wait 30 seconds before thread terminates */
// clock_nanosleep(CLOCK_REALTIME, 0, &ts, NULL);

// return NULL;
// }

// /*************************************************************/

// fn error(at: i32) {
//     /* Just exit on error */
//     // fprintf(
//     //     stderr,
//     //     b"Some error occured at %d\0".as_ptr() as *const c_char,
//     //     at,
//     // );
//     ::std::process::exit(1);
// }

use std::thread;
fn start_rt_thread() {
    thread::Builder::new()
        .stack_size(PTHREAD_STACK_MIN + MY_STACK_SIZE)
        .spawn(move || thread_task())
        .unwrap();
}

fn configure_malloc_behavior() {
    /* Now lock all current and future pages
    from preventing of being paged */
    unsafe {
        if mlockall(MCL_CURRENT | MCL_FUTURE) != 0 {
            perror(b"mlockall failed:\0".as_ptr() as *const c_char);
        }
        /* Turn off malloc trimming.*/
        mallopt(M_TRIM_THRESHOLD, -1);

        /* Turn off mmap usage. */
        mallopt(M_MMAP_MAX, 0);
    }
}

fn reserve_process_memory(size: usize) {
    unsafe {
        let buffer = malloc(size);
        // println!("a {}", *(buffer as *mut c_char));
        *(buffer as *mut c_char) = 0;
        // println!("a");

        let slice = ::std::slice::from_raw_parts_mut(buffer as *mut c_char, size);

        /* Touch each page in this piece of memory to get it mapped into RAM */
        // for (i = 0; i < size; i += sysconf(_SC_PAGESIZE)) {
        for i in (0..size).step_by(sysconf(_SC_PAGESIZE) as usize) {
            /* Each write to this buffer will generate a pagefault.
            Once the pagefault is handled a page will be locked in
            memory and never given back to the system. */
            // println!("{}", i);
            // *buffer.offset(i as isize) = 0;
            slice[i] = 0;
        }
        // println!("a");

        /* buffer will now be released. As Glibc is configured such that it
        never gives back memory to the kernel, the memory allocated above is
        locked for this process. All malloc() and new() calls come from
        the memory pool reserved and locked above. Issuing free() and
        delete() does NOT make this locking undone. So, with this locking
        mechanism we can build C++ applications that will never run into
        a major/minor pagefault, even with swapping enabled. */
        free(buffer as *mut c_void);
    }
}

fn main() {
    show_new_pagefault_count(
        b"Initial count\0".as_ptr() as *const c_char,
        b">=0\0".as_ptr() as *const c_char,
        b">=0\0".as_ptr() as *const c_char,
    );

    configure_malloc_behavior();

    show_new_pagefault_count(
        b"mlockall() generated\0".as_ptr() as *const c_char,
        b">=0\0".as_ptr() as *const c_char,
        b">=0\0".as_ptr() as *const c_char,
    );

    reserve_process_memory(PRE_ALLOCATION_SIZE);

    show_new_pagefault_count(
        b"malloc() and touch generated\0".as_ptr() as *const c_char,
        b">=0\0".as_ptr() as *const c_char,
        b">=0\0".as_ptr() as *const c_char,
    );

    /* Now allocate the memory for the 2nd time and prove the number of
        pagefaults are zero */
    reserve_process_memory(PRE_ALLOCATION_SIZE);
    show_new_pagefault_count(
        b"2nd malloc() and use generated\0".as_ptr() as *const c_char,
        b"0\0".as_ptr() as *const c_char,
        b"0\0".as_ptr() as *const c_char,
    );

    unsafe {
        printf(
            b"\n\nLook at the output of ps -leyf, and see that the RSS is now about %d [MB]\n\0"
                .as_ptr() as *const c_char,
            PRE_ALLOCATION_SIZE / (1024 * 1024),
        );
    }

    start_rt_thread();

    // //<do your RT-thing>

    unsafe {
        printf(b"Press <ENTER> to exit\n\0".as_ptr() as *const c_char);
        getchar();
    }
}
