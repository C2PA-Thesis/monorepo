//! Phase marks for memory profiling. Without the `probe` feature a mark
//! compiles to nothing.

#[cfg(feature = "probe")]
static HOOK: std::sync::OnceLock<fn(&'static str)> = std::sync::OnceLock::new();

/// Calls `hook` with the phase name at every mark the prover passes.
#[cfg(feature = "probe")]
pub fn on_mark(hook: fn(&'static str)) {
    let _ = HOOK.set(hook);
}

/// Names the phase the prover enters now.
#[inline(always)]
pub(crate) fn mark(_phase: &'static str) {
    #[cfg(feature = "probe")]
    if let Some(hook) = HOOK.get() {
        hook(_phase);
    }
}
