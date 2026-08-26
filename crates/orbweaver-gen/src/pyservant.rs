//! The Python binding of the servant seam — which is now **only a name**.
//!
//! Every line that used to be here is in [`crate::seam`], and nothing moved
//! except the language out of it. This module is kept because
//! `orbweaver-py-bridge`, four tests and one harness group name `PyServant`,
//! and because the rename is the finding rather than the change: for one day
//! this file *was* the seam, and a second language would have inherited a
//! dispatch binding whose type name, module name and documentation all said
//! Python — over a mechanism that never had a Python decision in it.
//!
//! What was genuinely per-language was smaller than the file: `Answerer::ask`,
//! and a runtime that speaks AnyJSON v1. That is D032 §3's third row and it is
//! the only row a language may own.
//!
//! *언어를 걷어내고 나니 언어별인 것은 함수 하나였다.*

/// A servant written in Python, dispatched into by our Rust ORB.
///
/// An alias for [`crate::seam::ForeignServant`]: there is no Python in it, and
/// naming the language here would make a second binding either copy the type or
/// inherit the first one's name.
pub type PyServant<A> = crate::seam::ForeignServant<A>;

pub use crate::seam::{Answerer, SEAM_FAILURE};
