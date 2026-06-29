/**
 * Equilibrium FFI helpers for D
 *
 * This module provides mixins and templates for exporting D functions
 * to C FFI, compatible with equilibrium's automatic binding generation.
 *
 * Example:
 *   import equilibrium;
 *   
 *   mixin FFI;
 *   @ffi int add(int a, int b) {
 *       return a + b;
 *   }
 *
 * The @ffi attribute expands to extern(C) with proper linkage.
 */
module equilibrium;

/**
 * UDA (User Defined Attribute) for marking functions for FFI export
 */
struct ffi {}

// Example usage
version(Demo)
{
    @ffi
    extern(C) export int add(int a, int b)
    {
        return a + b;
    }
    
    @ffi
    extern(C) export int multiply(int a, int b)
    {
        return a * b;
    }
}

/**
 * Type conversion helpers
 */
struct FFIHelpers
{
    static int toInt(T)(T value) if (is(T : long))
    {
        return cast(int)value;
    }
    
    static const(char)* toCString(string s)
    {
        import std.string : toStringz;
        return s.toStringz;
    }
}
