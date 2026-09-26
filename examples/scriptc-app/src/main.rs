//! Calls TypeScript through scriptc, using bindings equilibrium generated.
//!
//! `build.rs` compiles `foreign-code/math.ts` into a scriptc library archive
//! and writes the bindings to `$OUT_DIR`; this file only calls the generated
//! `extern "C"` declarations.

use std::os::raw::c_void;

mod ffi {
    include!(concat!(env!("OUT_DIR"), "/math_bindings.rs"));
}

/// scriptc delivers traps here. Register a sink before the first call: an
/// unregistered trap aborts the process.
unsafe extern "C" fn panic_sink(_ctx: *mut c_void, msg: *const u8, len: usize, _address: u64) {
    let message = std::slice::from_raw_parts(msg, len);
    eprintln!("scriptc trap: {}", String::from_utf8_lossy(message));
}

fn read_buffer(ptr: *const u8, len: usize) -> String {
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    String::from_utf8_lossy(bytes).into_owned()
}

fn main() {
    println!("=== TypeScript (scriptc) FFI demo ===\n");

    unsafe {
        ffi::math_set_panic_sink(panic_sink as *mut c_void, std::ptr::null_mut());
        ffi::math_init();

        let sum = ffi::math_add(0.1, 0.2);
        println!("math_add(0.1, 0.2)         = {sum}");
        assert_eq!(sum, 0.1 + 0.2);

        // `string` parameters are (ptr, len) pairs, and so are `string`
        // results: scriptc writes them through the trailing out parameters.
        let who = "world";
        let (mut ptr, mut len) = (std::ptr::null(), 0usize);
        ffi::math_greet(who.as_ptr(), who.len(), 1, &mut ptr, &mut len);
        let greeting = read_buffer(ptr, len);
        println!("math_greet(\"world\", true)  = {greeting:?}");
        assert_eq!(greeting, "WORLD");

        // `Uint8Array` arrives the same way.
        let data = [1u8, 2, 3, 4];
        let total = ffi::math_sum(data.as_ptr(), data.len());
        println!("math_sum([1, 2, 3, 4])     = {total}");
        assert_eq!(total, 10.0);

        // Marshalling classes from equilibrium.toml: u32 parameters ...
        let mixed = ffi::math_mix(7u32, 12u32);
        println!("math_mix(7, 12)            = {mixed}");
        assert_eq!(mixed, 7012.0);

        // ... and an i64 return.
        let truncated = ffi::math_truncate(-12.75);
        println!("math_truncate(-12.75)      = {truncated}");
        assert_eq!(truncated, -12);

        // Buffered string/bytes results stay owned by the archive until this.
        ffi::math_collect();
    }

    println!("\n✓ TypeScript (scriptc) FFI round-trip OK");
}
