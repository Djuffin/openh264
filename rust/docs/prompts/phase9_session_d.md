# Phase 9 — Session D: the macroblock-cache family (SMbCache / SMB), part one

## What this project is, in one paragraph

This repository ports the openh264 video codec from C++ (under `codec/`) to Rust (the
crate at `rust/crates/openh264-rs/`). The Rust crate is a drop-in replacement for
`libopenh264`: same C API, same output bytes, same error codes. It began as a literal
translation full of raw pointers and is being made safe piece by piece while staying
byte-identical. The decoder is done. The encoder already carries `#![deny(unsafe_code)]`
on every file and compiles only because each remaining unsafe item is marked
`#[allow(unsafe_code)]` with a comment tag naming the family of work that will remove it.
**Phase 9 removes those tags by making the code actually safe.** You are session D.

## Why this session is next, and what it is about

Two sessions ago the plan was "convert the picture-plane pointers first". The session
that tried it built a census of *who calls what with what* and found the plan's order
was wrong: every encoder cost kernel (SAD/SATD), the motion-compensation kernels and the
DCT are reached through **