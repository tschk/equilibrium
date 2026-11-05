# Changelog

## Unreleased

### Breaking: header generation rejects unverified declarations

Rust binding generation and consumer import generation now return an error when
the header parser skips an unsupported declaration, instead of producing partial
output. This applies to all consumers, including C, C++, and Zig. Examples include
multiline function prototypes, multiple declarations on one line, variadic
functions, and unsupported callback declarations. The error starts with
`Cannot generate bindings from unsupported declarations` and identifies what the
parser skipped. Rewrite the declarations in the supported form or provide a
handwritten header that describes the same ABI.

Rust bindings and imports that reconstruct C types also reject headers containing
the `enum` keyword. The error `C enum representation is not established` means
that Equilibrium cannot establish the enum's ABI representation; it does not mean
the C declaration is invalid. Enum size and signedness can depend on the compiler
and its options. C, C++, and Zig imports use the original header instead and are
not subject to this enum check.

Reconstructed bindings and imports also reject unresolved includes, macros, and
conditional directives because their effects can change declarations and ABI.
An error containing `without preprocessing` identifies a directive whose effect
Equilibrium cannot determine. Simple include guards, `#pragma once`, and integer
literal defines remain supported. Equilibrium is not a C preprocessor: even an
accepted header can depend on prior includes or compiler definitions. Use a
header with supported, explicit declarations and verify it against the actual
compiler configuration.

Synthesized headers are marked `ABI unverified`, and successful compilation or
generation reports a warning to inspect the declarations and compilation context.
They follow the same refusal checks as other headers. Synthesis can omit exports
or fail to represent complex declarations, and a synthesized C or C++ header
can retain system includes that reconstructed bindings reject. A successful
generation or compilation does not verify ABI compatibility. Supply and verify
a handwritten header when the synthesized declarations are insufficient.

There is no opt-in to bypass these refusals for now. No safe verification
contract exists for accepting the resulting bindings; bypassing the checks could
silently produce incomplete declarations or an ABI mismatch.
