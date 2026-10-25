//! Debugger state for a DAP client of the probe-rs debug adapter, without I/O or UI.
//!
//! The Zed fork prototype (A) showed what a TRACE32 style debugger needs beyond the DAP views
//! Zed has: a disassembly window around the program counter, breakpoints on instructions,
//! global variables read while the target runs (Live Watch) and the target's RTT output. This
//! crate keeps the state and the rules of those views, so that the standalone debugger (B) and
//! any other client use the same logic. A client sends the requests this crate builds and hands
//! it the responses and events it receives.
//!
//! - [`reference`]: addresses in memory references, and the `@core` suffix of probe-rs.
//! - [`protocol`]: the probe-rs requests and events outside DAP (`liveWatch`,
//!   `rttWindowOpened`, the RTT events) and `disassemble` with signed offsets.
//! - [`disassembly`]: the instructions around a program counter, read further on either side.
//! - [`breakpoints`]: breakpoints on instructions.
//! - [`source_breakpoints`]: breakpoints on source lines.
//! - [`variables`]: the scopes and variables of a stack frame, as a tree read on demand.
//! - [`live_watch`]: expressions read on every core, and what changed.
//! - [`memory`]: a memory window read again and again, and what changed.
//! - [`rtt`]: the lines of the RTT channels of every core.
//! - [`orti`]: ORTI files and the OS view (tasks, ISRs) built on them.
//! - [`path_expr`]: the first check of the C expressions the server reads (paths, `&`, casts,
//!   operators).
//! - [`gtm`]: the GTM's MCS channels and memory, their instructions as text, the GTM view.
//! - [`sw_trace`]: the software trace of `aurix_trace.h` records on a binary RTT channel.
//! - [`cache`]: the cores' caches, the segments they cache, their hit and miss counts.

pub mod breakpoints;
pub mod cache;
pub mod disassembly;
pub mod gtm;
pub mod live_watch;
pub mod memory;
pub mod orti;
pub mod path_expr;
pub mod protocol;
pub mod reference;
pub mod rtt;
pub mod source_breakpoints;
pub mod sw_trace;
pub mod variables;
