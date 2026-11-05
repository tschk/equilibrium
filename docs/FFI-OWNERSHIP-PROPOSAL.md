# Proposed cross-family FFI ownership contract

Status: proposal for review. This document defines requirements for a future
adapter contract, not an implemented ABI or a guarantee of any current project.
It does not claim that equilibrium, rig, eqts, and eqswift interoperate. The
[FFI audit](FFI-AUDIT.md#ownership-and-consumer-failure-boundaries) records
the narrower behavior observed in their source.

The goal is to make each transfer explicit at an adapter boundary. A matching C
pointer type alone does not establish compatible ownership, layout, or lifetime.
The contract applies only to endpoints that explicitly opt in and document the
ABI version, target, and supported value types. An adapter must preserve the
underlying producer's rules even when those rules are stricter than this proposal.

## Memory and ownership

For each pointer or handle, the endpoint's ABI description must identify its
owner, permitted access, valid extent, lifetime, and release operation. A length
is a byte count unless the declaration specifies otherwise. The description must
define null and empty values, alignment, encoding, mutability, and whether an
operation can invalidate existing views. Non-null pointers must designate valid
memory for the specified extent; null checks cannot establish pointer validity.

Borrowed inputs remain owned by the caller. The caller keeps them valid and,
unless mutable access is explicitly allowed, unchanged until the call returns.
The callee must not retain them after return. A longer borrow requires a separate
explicit lifetime and an owner that remains alive throughout that lifetime.
Borrowed outputs remain owned by their producer. The endpoint must identify the
event that ends their validity, such as a subsequent mutation, collection, or
owner destruction. A consumer that needs the data longer must copy it before
that event.

For an owned output, the producer transfers one release obligation to the
consumer on the documented success or error paths. The ABI must name the matching
producer-provided deallocator and every argument it requires, including capacity
or allocator context where applicable. The consumer must release each transferred
value exactly once with that operation, not with its own `free` or another
family's deallocator. The allocation owner and code containing its deallocator
must remain alive until release. An endpoint must define the empty value and
whether it needs release. Ownership on failure must be explicit; initialized
outputs and error payloads must have documented cleanup rules.

An adapter must copy data when the producer's lifetime or deallocator cannot be
represented safely to the consumer. It must not relabel a borrowed view as an
owned allocation. In particular, equilibrium's scriptc buffered results are
archive-owned until collection, while the cited eqts JSON path returns buffers
released by `eqts_buffer_free_v1`. Those mechanisms are not interchangeable.

## Callbacks

Each callback declaration must specify whether invocation is synchronous or may
outlive registration, which threads may invoke it, whether calls may overlap or
reenter the producer, and what operations are allowed during invocation. A
callback and its context must remain alive until the producer can no longer call
them. For retained callbacks, the API must provide an unregister or close
operation and define when in-flight calls have finished. The owner must not
release the context before that point.

Callback arguments are borrowed for the invocation unless the declaration
explicitly transfers ownership. The recipient must copy borrowed data it retains
after return. Callback return values and error paths must also specify ownership.
The callback must not unwind or throw through the foreign ABI boundary. The
adapter must translate recoverable callback failures into a documented result.

## Errors and unwinding

Each operation must define its status values, valid outputs on every status,
and the ownership and lifetime of any error payload. A consumer must be able to
release an error payload using its documented producer operation. The contract
must distinguish a recoverable reported error from a process abort or other
fatal failure. It cannot promise recovery from invalid pointers, undefined
behavior, or a runtime that aborts.

No language exception or panic may unwind across a boundary governed by this
contract. Each side must catch and translate failures before they cross it when
its runtime permits that. If a runtime or build configuration aborts instead,
the endpoint must document that behavior rather than promise translation.
An adapter may opt in only if it can uphold these obligations for its actual
runtime and calling convention.

## Versioning and adoption

Each participating endpoint must publish a contract version and identify the
version before exchanging values. A consumer must reject an unknown incompatible
version before calling operations that transfer ownership. A version covers
calling convention, layouts, ownership, release operations, callback behavior,
and failure semantics. Changing any of those incompatibly requires a new major
ABI version. An additive change is compatible only if older consumers can ignore
it without changing their obligations. Negotiation, symbol naming, and the
version encoding remain design decisions, not existing features.

Before an adapter opts in, its maintainers must document each mapping to the
underlying runtime and test transfer, release, error cleanup, callback lifetime,
and version mismatch on supported targets. Tests must cover invalidation events
and callback shutdown with calls in flight. They cannot prove arbitrary pointers
valid or establish compatibility for untested versions and targets.

## Evidence and limits

The [audit](FFI-AUDIT.md#ownership-and-consumer-failure-boundaries) links the
reviewed sources: rig's call-scoped borrowed spans at `018f6d8`, eqts's JSON
buffer and scriptc callback paths at `0a2e307`, and equilibrium's generated
scriptc header. These are examples motivating the proposal, not evidence that
the proposal is implemented. Other paths may have different contracts.

eqswift delegates to UniFFI. Its cited source identifies UniFFI types and a
`0.32` dependency, but does not establish the ownership, callback, or failure
semantics of a particular generated binding. Verify the resolved UniFFI version,
generated code, target runtime, and relevant UniFFI documentation before
designing or claiming a conforming adapter. No common ownership contract or
cross-family interoperability has been established by this document.
