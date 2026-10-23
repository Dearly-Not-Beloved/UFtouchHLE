/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Main dispatch queue and the Blocks runtime used to enqueue callbacks.
//! Block layout: <https://clang.llvm.org/docs/Block-ABI-Apple.html>

use crate::abi::{CallFromHost, GuestFunction};
use crate::dyld::{export_c_func, ConstantExports, FunctionExports, HostConstant};
use crate::mem::{ConstPtr, ConstVoidPtr, MutPtr, MutVoidPtr};
use crate::Environment;
use std::collections::{HashMap, VecDeque};

const HAS_COPY_DISPOSE: u32 = 1 << 25;
const IS_GLOBAL: u32 = 1 << 28;

#[derive(Default)]
pub struct State {
    main_queue: MutVoidPtr,
    pending: VecDeque<MutVoidPtr>,
    copies: HashMap<MutVoidPtr, u32>,
    draining: bool,
}

fn main_queue(env: &mut Environment) -> ConstVoidPtr {
    if env.libc_state.dispatch.main_queue.is_null() {
        env.libc_state.dispatch.main_queue = env.mem.calloc(64);
    }
    env.libc_state.dispatch.main_queue.cast_const()
}

fn block_class(env: &mut Environment) -> ConstVoidPtr {
    env.mem.calloc(64).cast_const()
}

fn block_word(env: &Environment, block: ConstVoidPtr, word: u32) -> u32 {
    env.mem.read(block.cast::<u32>() + word)
}

fn _Block_copy(env: &mut Environment, block: ConstVoidPtr) -> MutVoidPtr {
    if block.is_null() || block_word(env, block, 1) & IS_GLOBAL != 0 {
        return block.cast_mut();
    }
    if let Some(count) = env.libc_state.dispatch.copies.get_mut(&block.cast_mut()) {
        *count = count.checked_add(1).unwrap();
        return block.cast_mut();
    }
    let descriptor = ConstPtr::<u32>::from_bits(block_word(env, block, 4));
    let size = env.mem.read(descriptor + 1);
    assert!(size >= 20);
    let copy = env.mem.alloc(size);
    env.mem.memmove(copy, block, size);
    env.libc_state.dispatch.copies.insert(copy, 1);
    if block_word(env, block, 1) & HAS_COPY_DISPOSE != 0 {
        let helper = GuestFunction::from_addr_with_thumb_bit(env.mem.read(descriptor + 2));
        let _: () = helper.call_from_host(env, (copy, block));
    }
    copy
}

fn _Block_release(env: &mut Environment, block: MutVoidPtr) {
    let Some(count) = env.libc_state.dispatch.copies.get_mut(&block) else {
        return;
    };
    *count -= 1;
    if *count != 0 {
        return;
    }
    env.libc_state.dispatch.copies.remove(&block);
    if block_word(env, block.cast_const(), 1) & HAS_COPY_DISPOSE != 0 {
        let descriptor = ConstPtr::<u32>::from_bits(block_word(env, block.cast_const(), 4));
        let helper = GuestFunction::from_addr_with_thumb_bit(env.mem.read(descriptor + 3));
        let _: () = helper.call_from_host(env, (block,));
    }
    env.mem.free(block);
}

fn _Block_object_assign(
    env: &mut Environment,
    dest: MutPtr<MutVoidPtr>,
    object: MutVoidPtr,
    flags: i32,
) {
    let object = match flags & 0xf {
        3 => {
            if flags & 16 == 0 {
                crate::objc::retain(env, object.cast());
            }
            object
        }
        7 => _Block_copy(env, object.cast_const()),
        _ => unimplemented!("Block capture flags {flags:#x}"),
    };
    env.mem.write(dest, object);
}

fn _Block_object_dispose(env: &mut Environment, object: MutVoidPtr, flags: i32) {
    match flags & 0xf {
        3 => {
            if flags & 16 == 0 {
                crate::objc::release(env, object.cast());
            }
        }
        7 => _Block_release(env, object),
        // Preserve the earlier runtime's handling of stack-only __block
        // variables. Copying by-reference captures is not implemented yet.
        8 => {
            log!(
                "Warning: Ignoring disposal of stack-only __block variable {:?}",
                object
            );
        }
        _ => unimplemented!("Block capture flags {flags:#x}"),
    }
}

fn dispatch_async(env: &mut Environment, queue: ConstVoidPtr, block: ConstVoidPtr) {
    assert_eq!(
        queue,
        main_queue(env),
        "Only the main dispatch queue is implemented"
    );
    let block = _Block_copy(env, block);
    env.libc_state.dispatch.pending.push_back(block);
}

/// Run the work submitted before this main run-loop iteration. Work enqueued
/// by a callback waits until the next iteration, preserving asynchronous use.
pub fn drain_main_queue(env: &mut Environment) {
    assert_eq!(env.current_thread, 0);
    if env.libc_state.dispatch.draining {
        return;
    }
    env.libc_state.dispatch.draining = true;
    let pending = std::mem::take(&mut env.libc_state.dispatch.pending);
    for block in pending {
        let invoke =
            GuestFunction::from_addr_with_thumb_bit(block_word(env, block.cast_const(), 3));
        let _: () = invoke.call_from_host(env, (block,));
        _Block_release(env, block);
    }
    env.libc_state.dispatch.draining = false;
}

pub const CONSTANTS: ConstantExports = &[
    ("__dispatch_main_q", HostConstant::Custom(main_queue)),
    ("__NSConcreteStackBlock", HostConstant::Custom(block_class)),
    ("__NSConcreteGlobalBlock", HostConstant::Custom(block_class)),
    ("__NSConcreteMallocBlock", HostConstant::Custom(block_class)),
];
pub const FUNCTIONS: FunctionExports = &[
    export_c_func!(dispatch_async(_, _)),
    export_c_func!(_Block_copy(_)),
    export_c_func!(_Block_release(_)),
    export_c_func!(_Block_object_assign(_, _, _)),
    export_c_func!(_Block_object_dispose(_, _)),
];
