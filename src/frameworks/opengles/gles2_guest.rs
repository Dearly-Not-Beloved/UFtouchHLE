/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! ES 2.0 shader and vertex attribute entry points.

use super::gles_guest::with_ctx_and_mem;
use crate::dyld::{export_c_func, FunctionExports};
use crate::gles::gles11_raw::types::*;
use crate::gles::programmable::Programmable;
use crate::mem::{ConstPtr, ConstVoidPtr, GuestUSize, Mem, MutPtr};
use crate::Environment;

fn with_program<U: Default>(
    env: &mut Environment,
    f: impl FnOnce(&mut dyn Programmable, &mut Mem) -> U,
) -> U {
    with_ctx_and_mem(env, |gles, mem| {
        let program = gles
            .programmable()
            .expect("ES 2.0 call without an ES 2.0 context");
        f(program, mem)
    })
}

fn glCreateShader(env: &mut Environment, type_: GLenum) -> GLuint {
    with_program(env, |gl, _| unsafe { gl.CreateShader(type_) })
}

fn glCreateProgram(env: &mut Environment) -> GLuint {
    with_program(env, |gl, _| unsafe { gl.CreateProgram() })
}

fn glCompileShader(env: &mut Environment, shader: GLuint) {
    with_program(env, |gl, _| unsafe { gl.CompileShader(shader) })
}

fn glAttachShader(env: &mut Environment, program: GLuint, shader: GLuint) {
    with_program(env, |gl, _| unsafe { gl.AttachShader(program, shader) })
}

fn glLinkProgram(env: &mut Environment, program: GLuint) {
    with_program(env, |gl, _| unsafe { gl.LinkProgram(program) })
}

fn glUseProgram(env: &mut Environment, program: GLuint) {
    with_program(env, |gl, _| unsafe { gl.UseProgram(program) })
}

fn glDeleteShader(env: &mut Environment, shader: GLuint) {
    with_program(env, |gl, _| unsafe { gl.DeleteShader(shader) })
}

fn glDeleteProgram(env: &mut Environment, program: GLuint) {
    with_program(env, |gl, _| unsafe { gl.DeleteProgram(program) })
}

fn glEnableVertexAttribArray(env: &mut Environment, index: GLuint) {
    with_program(env, |gl, _| unsafe { gl.EnableVertexAttribArray(index) })
}

fn glDisableVertexAttribArray(env: &mut Environment, index: GLuint) {
    with_program(env, |gl, _| unsafe { gl.DisableVertexAttribArray(index) })
}

fn glUniform1i(env: &mut Environment, location: GLint, value: GLint) {
    with_program(env, |gl, _| unsafe { gl.Uniform1i(location, value) })
}

fn glGetUniformLocation(env: &mut Environment, program: GLuint, name: ConstPtr<u8>) -> GLint {
    let name = env.mem.cstr_at(name).to_vec();
    let mut name = name;
    name.push(0);
    with_program(env, |gl, _| unsafe {
        gl.GetUniformLocation(program, name.as_ptr().cast())
    })
}

fn glGetAttribLocation(env: &mut Environment, program: GLuint, name: ConstPtr<u8>) -> GLint {
    let name = env.mem.cstr_at(name).to_vec();
    let mut name = name;
    name.push(0);
    with_program(env, |gl, _| unsafe {
        gl.GetAttribLocation(program, name.as_ptr().cast())
    })
}

fn glBindAttribLocation(env: &mut Environment, program: GLuint, index: GLuint, name: ConstPtr<u8>) {
    let name = env.mem.cstr_at(name).to_vec();
    let mut name = name;
    name.push(0);
    with_program(env, |gl, _| unsafe {
        gl.BindAttribLocation(program, index, name.as_ptr().cast())
    })
}

fn glGetShaderiv(env: &mut Environment, shader: GLuint, pname: GLenum, params: MutPtr<GLint>) {
    with_program(env, |gl, mem| unsafe {
        let params = mem.ptr_at_mut(params, 1);
        gl.GetShaderiv(shader, pname, params)
    })
}

fn glGetProgramiv(env: &mut Environment, program: GLuint, pname: GLenum, params: MutPtr<GLint>) {
    with_program(env, |gl, mem| unsafe {
        let params = mem.ptr_at_mut(params, 1);
        gl.GetProgramiv(program, pname, params)
    })
}

fn glGetShaderInfoLog(
    env: &mut Environment,
    shader: GLuint,
    capacity: GLsizei,
    length: MutPtr<GLsizei>,
    log: MutPtr<u8>,
) {
    with_program(env, |gl, mem| unsafe {
        let log = if capacity == 0 {
            std::ptr::null_mut()
        } else {
            mem.ptr_at_mut(log, capacity.try_into().unwrap()).cast()
        };
        let mut written = 0;
        gl.GetShaderInfoLog(shader, capacity, &mut written, log);
        if !length.is_null() {
            mem.write(length, written);
        }
    })
}

fn glGetProgramInfoLog(
    env: &mut Environment,
    program: GLuint,
    capacity: GLsizei,
    length: MutPtr<GLsizei>,
    log: MutPtr<u8>,
) {
    with_program(env, |gl, mem| unsafe {
        let log = if capacity == 0 {
            std::ptr::null_mut()
        } else {
            mem.ptr_at_mut(log, capacity.try_into().unwrap()).cast()
        };
        let mut written = 0;
        gl.GetProgramInfoLog(program, capacity, &mut written, log);
        if !length.is_null() {
            mem.write(length, written);
        }
    })
}

fn glShaderSource(
    env: &mut Environment,
    shader: GLuint,
    count: GLsizei,
    strings: ConstPtr<ConstPtr<u8>>,
    lengths: ConstPtr<GLint>,
) {
    let mut source = Vec::new();
    for i in 0..GuestUSize::try_from(count).unwrap() {
        let string = env.mem.read(strings + i);
        let length = if lengths.is_null() {
            -1
        } else {
            env.mem.read(lengths + i)
        };
        let part = if length < 0 {
            env.mem.cstr_at(string)
        } else {
            env.mem.bytes_at(string, length as u32)
        };
        source.extend_from_slice(part);
    }
    with_program(env, |gl, _| unsafe { gl.ShaderSource(shader, &source) });
}

fn glVertexAttribPointer(
    env: &mut Environment,
    index: GLuint,
    size: GLint,
    type_: GLenum,
    normalized: GLboolean,
    stride: GLsizei,
    pointer: ConstVoidPtr,
) {
    with_ctx_and_mem(env, |gl, mem| unsafe {
        let mut buffer = 0;
        gl.GetIntegerv(0x8894, &mut buffer);
        let pointer = if buffer != 0 {
            pointer.to_bits() as usize as *const GLvoid
        } else if pointer.is_null() {
            std::ptr::null()
        } else {
            mem.ptr_at(pointer.cast::<u8>(), 0).cast()
        };
        gl.programmable()
            .unwrap()
            .VertexAttribPointer(index, size, type_, normalized, stride, pointer);
    })
}

fn glVertexAttrib4fv(env: &mut Environment, index: GLuint, value: ConstPtr<GLfloat>) {
    with_program(env, |gl, mem| unsafe {
        gl.VertexAttrib4fv(index, mem.ptr_at(value, 4))
    })
}

fn glUniform1fv(env: &mut Environment, location: GLint, count: GLsizei, value: ConstPtr<GLfloat>) {
    with_program(env, |gl, mem| unsafe {
        let size = GuestUSize::try_from(count).unwrap().checked_mul(1).unwrap();
        gl.Uniform1fv(location, count, mem.ptr_at(value, size));
    })
}

fn glUniform2fv(env: &mut Environment, location: GLint, count: GLsizei, value: ConstPtr<GLfloat>) {
    with_program(env, |gl, mem| unsafe {
        let size = GuestUSize::try_from(count).unwrap().checked_mul(2).unwrap();
        gl.Uniform2fv(location, count, mem.ptr_at(value, size));
    })
}

fn glUniform3fv(env: &mut Environment, location: GLint, count: GLsizei, value: ConstPtr<GLfloat>) {
    with_program(env, |gl, mem| unsafe {
        let size = GuestUSize::try_from(count).unwrap().checked_mul(3).unwrap();
        gl.Uniform3fv(location, count, mem.ptr_at(value, size));
    })
}

fn glUniform4fv(env: &mut Environment, location: GLint, count: GLsizei, value: ConstPtr<GLfloat>) {
    with_program(env, |gl, mem| unsafe {
        let size = GuestUSize::try_from(count).unwrap().checked_mul(4).unwrap();
        gl.Uniform4fv(location, count, mem.ptr_at(value, size));
    })
}

fn glUniformMatrix2fv(
    env: &mut Environment,
    location: GLint,
    count: GLsizei,
    transpose: GLboolean,
    value: ConstPtr<GLfloat>,
) {
    with_program(env, |gl, mem| unsafe {
        let size = GuestUSize::try_from(count).unwrap().checked_mul(4).unwrap();
        gl.UniformMatrix2fv(location, count, transpose, mem.ptr_at(value, size));
    })
}

fn glUniformMatrix3fv(
    env: &mut Environment,
    location: GLint,
    count: GLsizei,
    transpose: GLboolean,
    value: ConstPtr<GLfloat>,
) {
    with_program(env, |gl, mem| unsafe {
        let size = GuestUSize::try_from(count).unwrap().checked_mul(9).unwrap();
        gl.UniformMatrix3fv(location, count, transpose, mem.ptr_at(value, size));
    })
}

fn glUniformMatrix4fv(
    env: &mut Environment,
    location: GLint,
    count: GLsizei,
    transpose: GLboolean,
    value: ConstPtr<GLfloat>,
) {
    with_program(env, |gl, mem| unsafe {
        let size = GuestUSize::try_from(count)
            .unwrap()
            .checked_mul(16)
            .unwrap();
        gl.UniformMatrix4fv(location, count, transpose, mem.ptr_at(value, size));
    })
}

fn glDiscardFramebufferEXT(
    env: &mut Environment,
    target: GLenum,
    count: GLsizei,
    attachments: ConstPtr<GLenum>,
) {
    let _ = (env, target, count, attachments);
    // A hint that attachments no longer need to be preserved. Keeping them is
    // correct and works on drivers without EXT_discard_framebuffer.
}

pub const FUNCTIONS: FunctionExports = &[
    export_c_func!(glCreateShader(_)),
    export_c_func!(glCreateProgram()),
    export_c_func!(glCompileShader(_)),
    export_c_func!(glAttachShader(_, _)),
    export_c_func!(glLinkProgram(_)),
    export_c_func!(glUseProgram(_)),
    export_c_func!(glDeleteShader(_)),
    export_c_func!(glDeleteProgram(_)),
    export_c_func!(glEnableVertexAttribArray(_)),
    export_c_func!(glDisableVertexAttribArray(_)),
    export_c_func!(glUniform1i(_, _)),
    export_c_func!(glGetUniformLocation(_, _)),
    export_c_func!(glGetAttribLocation(_, _)),
    export_c_func!(glBindAttribLocation(_, _, _)),
    export_c_func!(glGetShaderiv(_, _, _)),
    export_c_func!(glGetProgramiv(_, _, _)),
    export_c_func!(glGetShaderInfoLog(_, _, _, _)),
    export_c_func!(glGetProgramInfoLog(_, _, _, _)),
    export_c_func!(glShaderSource(_, _, _, _)),
    export_c_func!(glVertexAttribPointer(_, _, _, _, _, _)),
    export_c_func!(glVertexAttrib4fv(_, _)),
    export_c_func!(glUniform1fv(_, _, _)),
    export_c_func!(glUniform2fv(_, _, _)),
    export_c_func!(glUniform3fv(_, _, _)),
    export_c_func!(glUniform4fv(_, _, _)),
    export_c_func!(glUniformMatrix2fv(_, _, _, _)),
    export_c_func!(glUniformMatrix3fv(_, _, _, _)),
    export_c_func!(glUniformMatrix4fv(_, _, _, _)),
    export_c_func!(glDiscardFramebufferEXT(_, _, _)),
];
