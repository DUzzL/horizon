//! Frame generation: FSR 3.1's frame interpolation will show a frame halfway between each
//! two real ones. So far this holds its optical flow, its interpolation and the motion
//! vectors of what moves on its own; the setting that runs them comes next.

// Unused until the renderer runs them.
#[allow(dead_code)]
mod interpolation;
#[allow(dead_code)]
mod motion;
#[allow(dead_code)]
mod optical_flow;
