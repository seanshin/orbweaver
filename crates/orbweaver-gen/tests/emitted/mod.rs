//! Checked-in generator output, so the wire test can compile against it.
//!
//! `skeleton_wire.rs` regenerates this from
//! `corpus/golden/24-skeleton-surface.idl` and asserts byte equality, so it
//! cannot drift from the generator: change the templates and this file has to
//! be re-blessed in the same commit (`ORBWEAVER_BLESS=1 cargo test -p
//! orbweaver-gen`). Reviewing the diff of a generated file is the point —
//! a template change whose output nobody looked at is a template change
//! nobody reviewed.
//!
//! The directory is `emitted/` rather than the obvious `generated/` because
//! the repository's `.gitignore` excludes `generated/` — a checked-in artifact
//! under an ignored name is one that vanishes on a fresh clone and takes the
//! test binary with it.

pub mod f_24_skeleton_surface;
pub mod f_25_servant_faults;
pub mod f_26_object_identity;
pub mod f_27_bounds;
pub mod f_ir_subset;
