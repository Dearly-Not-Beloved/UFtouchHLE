/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Objective-C runtime.
//!
//! Apple's [Programming with Objective-C](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/ProgrammingWithObjectiveC/Introduction/Introduction.html)
//! is a useful introduction to the language from a user's perspective.
//! There are further resources in the child modules of this module, but they
//! are more implementation-specific.
//!
//! The strategy for this emulator will be to provide our own implementations of
//! an Objective-C runtime and libraries for it (Foundation etc). These
//! implementations will be "host code": Rust code forming part of the emulator,
//! not emulated code. The runtime will need to be able to handle classes that
//! originate from the guest app, classes defined by the host, and sometimes
//! classes that are both (considering Objective-C's support for inheritance,
//! categories and dynamic class editing).

use crate::dyld::{export_c_func, ConstantExports, FunctionExports, HostConstant, HostDylib};
use crate::objc::messages::ThreadInitializer;
use crate::{Environment, MutexId};

mod classes;
mod messages;
mod methods;
mod objects;
mod properties;
mod selectors;
mod synchronization;

pub use classes::{objc_classes, Class, ClassExports, ClassTemplate};
pub use messages::{
    autorelease, msg, msg_class, msg_send, msg_send_no_initialize, msg_send_no_type_checking,
    msg_send_super2, msg_super, objc_super, release, retain, TERMINATING,
};
pub use methods::{HostIMP, IMP};
pub use objects::{
    id, impl_HostObject_with_superclass, nil, AnyHostObject, HostObject, TrivialHostObject,
};
pub use properties::todo_objc_setter;
pub use selectors::{selector, SEL};

use classes::{
    class_getInstanceSize, class_getMethodImplementation, class_getProperty, class_getSuperclass,
    class_replaceMethod, objc_getClass, ClassHostObject, FakeClass, UnimplementedClass,
};
pub(crate) use messages::objc_msgSend;
use messages::{objc_msgSendSuper2, objc_msgSend_stret, MsgSendSignature, MsgSendSuperSignature};
use methods::method_list_t;
use objects::{objc_object, object_getClass, HostObjectEntry};
use properties::{
    ivar_list_t, objc_copyStruct, objc_getProperty, objc_setProperty, objc_setProperty_atomic,
    objc_setProperty_atomic_copy, objc_setProperty_nonatomic, objc_setProperty_nonatomic_copy,
};
use selectors::sel_registerName;
use synchronization::{objc_sync_enter, objc_sync_exit};

/// Typedef for `NSZone *`. This is a [fossil type] found in the signature of
/// `allocWithZone:` and similar methods. Its value is always ignored.
///
/// [fossil type]: https://en.wiktionary.org/wiki/fossil_word
pub type NSZonePtr = crate::mem::MutVoidPtr;

/// Main type holding Objective-C runtime state.
pub struct ObjC {
    /// Known selectors (interned method name strings).
    selectors: crate::fast_hash::FastHashMap<String, SEL>,

    /// Mapping of known (guest) object pointers to their host objects.
    ///
    /// If an object isn't in this map, we will consider it not to exist.
    objects: crate::fast_hash::FastHashMap<id, HostObjectEntry>,

    /// Known classes.
    ///
    /// Look at the `isa` to get the metaclass for a class.
    classes: crate::fast_hash::FastHashMap<String, Class>,

    /// Mutexes used in @synchronized blocks (objc_sync_enter/exit).
    sync_mutexes: crate::fast_hash::FastHashMap<id, MutexId>,

    /// Mutexes for running the +initialize function.
    initializer_threads: crate::fast_hash::FastHashMap<id, ThreadInitializer>,

    /// Temporary storage for optional type information when sending a message.
    /// Type information isn't part of the `objc_msgSend` ABI, so an alternative
    /// channel is needed.
    message_type_info: Option<(std::any::TypeId, &'static str)>,
}

impl ObjC {
    pub fn new() -> ObjC {
        ObjC {
            selectors: Default::default(),
            objects: Default::default(),
            classes: Default::default(),
            sync_mutexes: Default::default(),
            initializer_threads: Default::default(),
            message_type_info: None,
        }
    }
}

pub const DYLIB: HostDylib = HostDylib {
    path: "/usr/lib/libobjc.A.dylib",
    aliases: &["/usr/lib/libobjc.dylib"],
    class_exports: &[],
    constant_exports: &[CONSTANTS],
    function_exports: &[FUNCTIONS],
};

const CONSTANTS: ConstantExports = &[
    // We don't use these in our Objective-C runtime, but exporting useless
    // symbols for these silences the warning about the unhandled relocation,
    // and avoids a linker error for the integration tests.
    ("__objc_empty_vtable", HostConstant::NullPtr),
    ("__objc_empty_cache", HostConstant::NullPtr),
    // Used by the unwinding tables of code that catches Objective-C
    // exceptions. touchHLE doesn't support unwinding through guest code, so
    // these are only needed for linking.
    ("___objc_personality_v0", HostConstant::NullPtr),
    ("_OBJC_EHTYPE_$_NSException", HostConstant::NullPtr),
];

/// Called by `for (x in y)` loops if the collection changes while looping.
fn objc_enumerationMutation(env: &mut Environment, object: id) {
    let class: Class = msg![env; object class];
    panic!(
        "Collection {:?} (class {:?}) was mutated while being enumerated",
        object,
        env.objc.get_class_name(class)
    );
}

/// `@throw`. touchHLE can't unwind guest code to a `@catch` block, so throwing
/// an exception ends the app, with the exception's details.
fn objc_exception_throw(env: &mut Environment, exception: id) {
    let class: Class = msg![env; exception class];
    let class_name = env.objc.get_class_name(class).to_string();
    let description: id = msg![env; exception description];
    let description = if description == nil {
        "(nil)".to_string()
    } else {
        crate::frameworks::foundation::ns_string::to_rust_string(env, description).to_string()
    };
    panic!("Objective-C exception thrown: {class_name} {exception:?}: {description}");
}
fn objc_exception_rethrow(_env: &mut Environment) {
    panic!("Objective-C exception rethrown");
}

const FUNCTIONS: FunctionExports = &[
    export_c_func!(class_getInstanceSize(_)),
    export_c_func!(class_getSuperclass(_)),
    export_c_func!(class_getProperty(_, _)),
    export_c_func!(class_getMethodImplementation(_, _)),
    export_c_func!(class_replaceMethod(_, _, _, _)),
    export_c_func!(objc_msgSend(_, _)),
    export_c_func!(objc_msgSend_stret(_, _, _)),
    export_c_func!(objc_msgSendSuper2(_, _)),
    export_c_func!(objc_getClass(_)),
    export_c_func!(objc_getProperty(_, _, _, _)),
    export_c_func!(objc_setProperty(_, _, _, _, _, _)),
    export_c_func!(objc_setProperty_atomic(_, _, _, _)),
    export_c_func!(objc_setProperty_nonatomic(_, _, _, _)),
    export_c_func!(objc_setProperty_atomic_copy(_, _, _, _)),
    export_c_func!(objc_setProperty_nonatomic_copy(_, _, _, _)),
    export_c_func!(objc_copyStruct(_, _, _, _, _)),
    export_c_func!(objc_enumerationMutation(_)),
    export_c_func!(objc_exception_throw(_)),
    export_c_func!(objc_exception_rethrow()),
    export_c_func!(objc_sync_enter(_)),
    export_c_func!(objc_sync_exit(_)),
    export_c_func!(object_getClass(_)),
    export_c_func!(sel_registerName(_)),
];
