//! Prints the servant seam's protocol, as one JSON document.
//!
//! The seam's definition has to be **readable by a program that is not the
//! emitter**, or "adding C or Java costs an emitter and a small runtime" is a
//! claim about Rust readers only. `orbweaver_gen::seam::protocol()` is the
//! value; this is the way to it from a shell, a Makefile, or a test written in
//! the language being added.
//!
//! ```text
//! cargo run -q -p orbweaver-gen --bin seam-protocol
//! ```
//!
//! It prints and decides nothing. The thing a new binding then owes is one
//! function of its own that answers the same document — see
//! `crates/orbweaver-gen/tests/the_seam_is_one_protocol.rs`, which is where a
//! new language enrols and is the only place it has to.

fn main() {
    println!("{}", orbweaver_gen::seam::protocol());
}
